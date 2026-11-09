//! The World & Content area of the console (Plan/25 §7; **WP-ADMIN-30**): the spawn table with "respawn now", the
//! monster census, the Event.lua modifiers with the weather, the weather switch (phase-gated) and the registry of the
//! content panels of Phase C (every panel answers `501 needs_feature` until its system exists; **WP-ADMIN-31**).
//!
//! | route | cap | what |
//! |---|---|---|
//! | `GET /admin/api/spawns?world=` | `view.map` | the respawn lines of a world (layer 0): target, alive, awake, countdown (`SpawnStatus`) |
//! | `POST /admin/api/spawns/{region}/respawn` | `act.world` | make a line due now: `respawn-region` of the action route, same audit and answer |
//! | `GET /admin/api/census?world=` | `view.map` | monsters by rank, asleep and awake, the top 20 types (`Census`) |
//! | `GET /admin/api/events/modifiers` | `view.dashboard` | the Event.lua modifiers with their sources, the running Lua events, the weather (`EventInfo`) |
//! | `PUT /admin/api/weather/{world}` | `act.world` | rain or snow on or off; `501 needs_feature WP-GM-30` until the weather service exists |
//! | `GET /admin/api/content/{panel}`, `POST /admin/api/content/{panel}/{action}` | `view.map` / `act.world` | the content panels: `404` for an unknown panel, `501 needs_feature` with the panel's work package |
//!
//! The rates and the event flags keep their routes of Plan/10 §13.2 (`GET /admin/api/rates`, `PUT /admin/api/rates/{key}`,
//! `GET /admin/api/events`, `PUT /admin/api/events/{id}`); `GET /admin/api/rates` also tells the allowed range of each
//! rate (`min`, `max`, `min_exclusive`).

pub mod dto;

use super::actions::{self, GameLink};
use super::{ApiState, Routes};
use crate::admin::auth::Who;
use crate::admin::dto::{ErrorCode, ErrorDetails, Mutation};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::guard::{AuditTrail, MutationJson, Policy};
use crate::admin::identity::Identity;
use crate::audit::AuditFile;
use axum::Json;
use axum::extract::{Path, Query, State};
use dto::{
    EventInfoDto, LuaEventDto, ModifierRow, MonsterCensusDto, MonsterRankDto, MonsterTypeDto,
    PanelDto, RespawnNowBody, SpawnRowDto, SpawnTableDto, WeatherBody, WeatherDto, WeatherKindDto,
    WeatherResult,
};
use flyff_common::WorldId;
use flyff_game::admin::actions::world::{SetWeather, WEATHER_CLEAR, WEATHER_RAIN, WEATHER_SNOW};
use flyff_game::admin::queries::spawns::{CensusResult, EventInfo, SpawnStatusResult};
use flyff_game::admin::{AdminQuery, AdminQueryResult};
use flyff_game::input::{AdminCmd, AdminOutput};
use serde::Deserialize;
use serde_json::{Value, json};

/// The work package the weather switch waits for.
pub const WEATHER_WP: &str = "WP-GM-30";

/// One content panel: its id, title and the work package it waits for (25 §7.3). `web/src/admin/content/registry.ts`
/// holds the same table (pinned by `tests/golden/admin/content_panels.json`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelSpec {
    /// The id.
    pub id: &'static str,
    /// The title.
    pub title: &'static str,
    /// The work package that has to land first.
    pub wp: &'static str,
}

/// The nine panels of 25 §7.3, in the order of the table. Each shows "Not available yet (needs WP-...)" until its
/// system exists; WP-ADMIN-31 builds the panels themselves (the Event.lua panel only waits for that work package,
/// the Event.lua store and the scheduler already exist).
pub const PANELS: &[PanelSpec] = &[
    PanelSpec {
        id: "events",
        title: "Event.lua & scheduler",
        wp: "WP-ADMIN-31",
    },
    PanelSpec {
        id: "lord",
        title: "Lord & election",
        wp: "WP-EVENT-10",
    },
    PanelSpec {
        id: "tax",
        title: "Tax",
        wp: "WP-EVENT-12",
    },
    PanelSpec {
        id: "secret-room",
        title: "Secret room",
        wp: "WP-EVENT-13",
    },
    PanelSpec {
        id: "guild-siege",
        title: "Guild siege",
        wp: "WP-SOCIAL-16",
    },
    PanelSpec {
        id: "rainbow-race",
        title: "Rainbow race",
        wp: "WP-EVENT-17",
    },
    PanelSpec {
        id: "quiz",
        title: "Quiz",
        wp: "WP-EVENT-19",
    },
    PanelSpec {
        id: "instances",
        title: "Instance dungeons",
        wp: "WP-EVENT-08",
    },
    PanelSpec {
        id: "housing",
        title: "Housing & guild houses",
        wp: "WP-EVENT-14",
    },
];

/// The panels as the golden file lists them.
pub fn panels() -> Vec<PanelDto> {
    PANELS
        .iter()
        .map(|p| PanelDto {
            id: p.id.to_owned(),
            title: p.title.to_owned(),
            wp: p.wp.to_owned(),
        })
        .collect()
}

/// Mounts the routes of this area.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/spawns", spawns)
        .post("/admin/api/spawns/{region}/respawn", respawn)
        .get("/admin/api/census", census)
        .get("/admin/api/events/modifiers", modifiers)
        .put("/admin/api/weather/{world}", weather)
        .get("/admin/api/content/{panel}", panel_read)
        .post("/admin/api/content/{panel}/{action}", panel_action)
}

// ---- spawns and census ----------------------------------------------------------------------------------------------------

/// `?world=`.
#[derive(Debug, Deserialize)]
pub struct WorldQuery {
    world: Option<u32>,
}

fn world_of(q: &WorldQuery) -> ApiResult<WorldId> {
    match q.world {
        Some(w) if w != 0 && w != u32::MAX => Ok(WorldId(w)),
        _ => Err(ApiError::invalid("world", "name a world: ?world=<id>")),
    }
}

fn spawn_table(s: SpawnStatusResult) -> SpawnTableDto {
    SpawnTableDto {
        world: s.world.0,
        layer: s.layer.0,
        respawn_on: s.respawn_on,
        game_day: s.game_day,
        game_hour: s.game_hour,
        lines: s
            .lines
            .into_iter()
            .map(|l| SpawnRowDto {
                region: l.region,
                kind: l.kind.name().to_owned(),
                prop: l.prop,
                name: l.name,
                level: l.level,
                rank: l.rank,
                rect: l.rect,
                target: l.target,
                alive: l.alive,
                awake: l.awake,
                aggressive: l.aggressive,
                aggressive_alive: l.aggressive_alive,
                countdown_s: l.countdown_s,
                half: l.half,
                interval_s: l.interval_s,
                fixed: l.fixed,
                flying: l.flying,
                in_window: l.in_window,
            })
            .collect(),
    }
}

fn census_dto(c: CensusResult) -> MonsterCensusDto {
    MonsterCensusDto {
        world: c.world.0,
        layers: c.layers,
        alive: c.alive,
        asleep: c.asleep,
        awake: c.awake,
        by_rank: c
            .by_rank
            .into_iter()
            .map(|r| MonsterRankDto {
                rank: r.rank,
                alive: r.alive,
                asleep: r.asleep,
                awake: r.awake,
            })
            .collect(),
        top: c
            .top
            .into_iter()
            .map(|t| MonsterTypeDto {
                prop: t.prop,
                name: t.name,
                level: t.level,
                rank: t.rank,
                alive: t.alive,
                awake: t.awake,
            })
            .collect(),
        players: c.players,
        npcs: c.npcs,
    }
}

/// `GET /admin/api/spawns?world=`.
async fn spawns(
    State(st): State<ApiState>,
    Who(_who): Who,
    Query(q): Query<WorldQuery>,
) -> ApiResult<Json<SpawnTableDto>> {
    let world = world_of(&q)?;
    match st.ops.query(AdminQuery::SpawnStatus(world)).await? {
        AdminQueryResult::Spawns(s) => Ok(Json(spawn_table(*s))),
        AdminQueryResult::Failed(f) => Err(ApiError::from(&f)),
        other => Err(ApiError::internal(&format!(
            "SpawnStatus answered {other:?}"
        ))),
    }
}

/// `GET /admin/api/census?world=`.
async fn census(
    State(st): State<ApiState>,
    Who(_who): Who,
    Query(q): Query<WorldQuery>,
) -> ApiResult<Json<MonsterCensusDto>> {
    let world = world_of(&q)?;
    match st.ops.query(AdminQuery::Census(world)).await? {
        AdminQueryResult::Census(c) => Ok(Json(census_dto(*c))),
        AdminQueryResult::Failed(f) => Err(ApiError::from(&f)),
        other => Err(ApiError::internal(&format!("Census answered {other:?}"))),
    }
}

/// `POST /admin/api/spawns/{region}/respawn`: the `respawn-region` action (capability `act.world`, audit
/// `admin.respawn`), with the line in the path.
async fn respawn(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(region): Path<u32>,
    MutationJson(m): MutationJson<RespawnNowBody>,
) -> ApiResult<Json<actions::dto::ActionResult>> {
    let envelope = Mutation {
        body: json!({"world": m.body.world, "layer": m.body.layer, "region": region}),
        reason: m.reason,
        confirm: m.confirm,
    };
    actions::run_action(&st, &who, "respawn-region", envelope)
        .await
        .map(Json)
}

// ---- Event.lua modifiers and the weather ----------------------------------------------------------------------------------

/// The Event.lua aggregates: field of `EventModifiers`, label, unit, the neutral value, the source.
const MODIFIERS: &[(&str, &str, &str, f64, &str)] = &[
    ("lua_exp", "EXP rate", "x", 1.0, "Event.lua GetExpFactor()"),
    (
        "lua_item_drop",
        "Item drop rate",
        "x",
        1.0,
        "Event.lua GetItemDropRate()",
    ),
    (
        "lua_piece_drop",
        "Piece item drop rate",
        "x",
        1.0,
        "Event.lua GetPieceItemDropRate()",
    ),
    (
        "lua_gold",
        "Penya drop",
        "x",
        1.0,
        "Event.lua GetGoldDropFactor()",
    ),
    (
        "lua_attack",
        "Attack bonus of players",
        "+",
        0.0,
        "Event.lua GetAttackPower()",
    ),
    (
        "lua_defense",
        "Defense bonus of players",
        "+",
        0.0,
        "Event.lua GetDefensePower()",
    ),
    (
        "lua_cheer_exp",
        "Cheer EXP",
        "x",
        1.0,
        "Event.lua GetCheerExpFactor()",
    ),
    (
        "lua_weather_exp",
        "Weather EXP",
        "x",
        1.0,
        "Event.lua GetWeatherEventExpFactor()",
    ),
    (
        "shop_buy",
        "Shop buy price",
        "x",
        1.0,
        "Event.lua GetShopBuyFactor()",
    ),
    (
        "shop_sell",
        "Shop sell price",
        "x",
        1.0,
        "Event.lua GetShopSellFactor()",
    ),
    (
        "coupon_ms",
        "Coupon event",
        "ms",
        0.0,
        "Event.lua GetCouponEvent()",
    ),
    (
        "keep_connect_ms",
        "Keep-connect time",
        "ms",
        0.0,
        "Event.lua GetKeepConnectTime()",
    ),
    (
        "generic_exp",
        "Generic events: EXP",
        "x",
        1.0,
        "propEvent.inc generic events (flags 500 and up)",
    ),
    (
        "generic_item_drop",
        "Generic events: item drop",
        "x",
        1.0,
        "propEvent.inc generic events (flags 500 and up)",
    ),
    (
        "lord_exp",
        "Lord: EXP",
        "x",
        1.0,
        "the lord's events (lord system)",
    ),
    (
        "lord_item",
        "Lord: item drop",
        "x",
        1.0,
        "the lord's events (lord system)",
    ),
];

/// The rows of the modifiers from `EventModifiers::to_json` (a field the JSON lacks is shown at its neutral value).
pub fn modifier_rows(json: &Value) -> Vec<ModifierRow> {
    MODIFIERS
        .iter()
        .map(|(key, label, unit, neutral, source)| {
            let value = json.get(*key).and_then(Value::as_f64).unwrap_or(*neutral);
            ModifierRow {
                key: (*key).to_owned(),
                label: (*label).to_owned(),
                value,
                neutral: *neutral,
                active: (value - neutral).abs() > 1e-6,
                unit: (*unit).to_owned(),
                source: (*source).to_owned(),
            }
        })
        .collect()
}

fn event_info_dto(i: EventInfo) -> EventInfoDto {
    EventInfoDto {
        modifiers: modifier_rows(&i.modifiers),
        running: i
            .running
            .into_iter()
            .map(|r| LuaEventDto {
                id: r.id,
                desc: r.desc,
            })
            .collect(),
        weather: WeatherDto {
            effect: i.weather.effect,
            title: i.weather.title,
            control: i.weather.control,
        },
    }
}

async fn event_info<G: GameLink>(game: &G) -> ApiResult<EventInfo> {
    match game.query(AdminQuery::EventInfo).await? {
        AdminQueryResult::EventInfo(i) => Ok(*i),
        AdminQueryResult::Failed(f) => Err(ApiError::from(&f)),
        other => Err(ApiError::internal(&format!("EventInfo answered {other:?}"))),
    }
}

/// `GET /admin/api/events/modifiers`.
async fn modifiers(State(st): State<ApiState>, Who(_who): Who) -> ApiResult<Json<EventInfoDto>> {
    event_info(st.ops.as_ref())
        .await
        .map(|i| Json(event_info_dto(i)))
}

fn kind_code(k: WeatherKindDto) -> u8 {
    match k {
        WeatherKindDto::None => WEATHER_CLEAR,
        WeatherKindDto::Rain => WEATHER_RAIN,
        WeatherKindDto::Snow => WEATHER_SNOW,
    }
}

/// The flow of `PUT /admin/api/weather/{world}` over any [`GameLink`]: until the weather service exists
/// (`weather.control` is false) the answer is `501 needs_feature WP-GM-30` and nothing is audited; afterwards the
/// change is audited (`admin.config{key: "weather"}`, `requested` then `done`) and sent as `SetWeather`.
pub(crate) async fn set_weather<G: GameLink>(
    game: &G,
    policy: &Policy,
    audit: &AuditFile,
    who: &Identity,
    world: u32,
    m: Mutation<WeatherBody>,
) -> ApiResult<WeatherResult> {
    if world == 0 || world == u32::MAX {
        return Err(ApiError::invalid("world", "name a world id"));
    }
    let reason = policy.reason_optional(m.reason.as_ref())?;
    if !event_info(game).await?.weather.control {
        return Err(ApiError::needs_feature(WEATHER_WP));
    }
    let kind = m.body.kind;
    let trail = AuditTrail::begin(
        audit,
        who,
        "admin.config",
        reason,
        json!({"key": "weather", "world": world, "after": kind_code(kind)}),
    )?;
    let cmd = AdminCmd::SetWeather(SetWeather {
        world: WorldId(world),
        kind: kind_code(kind),
    });
    match game.command(cmd, who.actor()).await {
        Ok(AdminOutput::Done { .. }) => {
            trail.done(json!({}));
            Ok(WeatherResult { world, kind })
        }
        Ok(AdminOutput::Failed(f)) => {
            trail.failed(&f.to_string());
            Err(ApiError::from(&f))
        }
        Ok(other) => {
            trail.failed("unexpected answer");
            Err(ApiError::internal(&format!(
                "the game answered the weather with {other:?}"
            )))
        }
        Err(e) => {
            trail.failed(&e.to_string());
            Err(ApiError::from(e))
        }
    }
}

/// `PUT /admin/api/weather/{world}`.
async fn weather(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(world): Path<u32>,
    MutationJson(m): MutationJson<WeatherBody>,
) -> ApiResult<Json<WeatherResult>> {
    let policy = st.policy();
    set_weather(st.ops.as_ref(), &policy, &st.ops.audit, &who, world, m)
        .await
        .map(Json)
}

// ---- the content panels (Phase C) ------------------------------------------------------------------------------------------

fn panel_of(id: &str) -> ApiResult<&'static PanelSpec> {
    PANELS
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| ApiError::not_found(format!("there is no content panel '{id}'")))
}

fn needs(p: &PanelSpec) -> ApiError {
    ApiError::new(
        ErrorCode::NeedsFeature,
        format!("not available yet: the {} panel needs {}", p.title, p.wp),
    )
    .with_details(ErrorDetails {
        wp: Some(p.wp.to_owned()),
        ..ErrorDetails::default()
    })
}

/// `GET /admin/api/content/{panel}`.
async fn panel_read(Who(_who): Who, Path(panel): Path<String>) -> ApiResult<Json<Value>> {
    Err(needs(panel_of(&panel)?))
}

/// `POST /admin/api/content/{panel}/{action}`.
async fn panel_action(
    Who(_who): Who,
    Path((panel, _action)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    Err(needs(panel_of(&panel)?))
}
