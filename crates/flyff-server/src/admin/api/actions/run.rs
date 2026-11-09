//! The flow of `POST /admin/api/actions/{action}` (Plan/25 §3.12, §10.1, §11.3, §12; WP-ADMIN-12), over any
//! [`GameLink`]:
//!
//! 1. the finer **capability** of the action (the route and the gate hold the lowest, `act.message.player`);
//! 2. the **body** is parsed and checked, and the command is built (a body that names a mistake is `400`);
//! 3. the **group cap** (`413 too_many_targets`), the **reason** (mandatory for a mute) and the **typed confirmation**
//!    (recomputed from the facts of the request: more than 10 targets, a player among the victims of a kill, an area
//!    clear);
//! 4. the audit record `admin.*` with `state: requested`, then the command on the game thread, then `done` or `failed`
//!    (per target for teleports, edits and mutes: the fields of 25 §10.3);
//! 5. the answer: an [`ActionResult`], or the error when the request had exactly one target and it failed
//!    (`409 stale_target` with `details.current`, `403 target_outranks`, `409 character_online`, ...).
//!
//! Rank protection, the stale checks and the dead/offline checks are the game thread's (it owns the facts at the moment
//! of the change); the route only maps their answers. `summon` and `goto` are GM lines (`RunLine` with `as_character`
//! and the actor's authority, 25 §1.4, §1.5): the game audits them as `gm.command`.

use super::GameLink;
use super::dto::{
    ActionResult, AreaKind, DespawnAreaBody, DespawnBody, DropItemBody, EditCharacterBody,
    GotoBody, HealBody, KillBody, MessageBody, MessageStyle, ModeFlagsBody, MuteBody, ObjectDetail,
    RespawnBody, ResurrectBody, SpawnBody, SummonBody, TargetError, TargetOutcome, TargetRef,
    TeleportBody, TeleportLabel, UnmuteBody, UnstuckBody,
};
use crate::admin::caps::Cap;
use crate::admin::dto::{CharacterEdit as WireEdit, ErrorCode, ErrorDetails, Mutation};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::guard::{CONFIRM_TARGETS, ConfirmFacts, Policy, ReasonText, audit_admin, rank};
use crate::admin::identity::{CapTable, Identity};
use crate::audit::AuditFile;
use flyff_common::gm::Authority;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, LayerId, ObjId, WorldId};
use flyff_game::admin::actions::character::{self as edit, EditCharacter};
use flyff_game::admin::actions::combat::{DespawnArea, DespawnObject, Heal, KillObject, Resurrect};
use flyff_game::admin::actions::moderation::{
    Mute, NotifyCharacter, NotifyStyle, SetModeFlags, Unmute,
};
use flyff_game::admin::actions::world::{
    DropItem, RespawnRegion, SpawnMonsters, TeleportCharacter, TeleportMode,
};
use flyff_game::admin::actions::{TargetId, TargetResult};
use flyff_game::admin::map::EntityKind;
use flyff_game::admin::queries::{AdminQuery, AdminQueryResult};
use flyff_game::admin::{AdminErrorCode, AdminFailure};
use flyff_game::input::{AdminCmd, AdminOutput, OnlineEntry};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// What an action needs from the server.
pub(super) struct Env<'a, G> {
    /// The game thread.
    pub game: &'a G,
    /// The live guard policy.
    pub policy: &'a Policy,
    /// The capability table.
    pub caps: &'a CapTable,
    /// The audit appender.
    pub audit: &'a AuditFile,
    /// Who asks.
    pub who: &'a Identity,
}

/// The actions of the route, in the order of the catalogue (25 §3.12).
pub const ACTIONS: &[&str] = &[
    "teleport",
    "unstuck",
    "summon",
    "goto",
    "heal",
    "resurrect",
    "kill",
    "despawn",
    "despawn-area",
    "spawn",
    "drop-item",
    "respawn-region",
    "mute",
    "unmute",
    "message",
    "mode-flags",
    "edit-character",
];

/// The capability an action needs (25 §3.12, §4.6).
pub fn cap_of(action: &str) -> Option<Cap> {
    Some(match action {
        "teleport" => Cap::ActTeleportOthers,
        "unstuck" => Cap::ActUnstuck,
        "summon" | "goto" => Cap::ActTeleportSelf,
        "heal" | "resurrect" | "kill" | "despawn" | "despawn-area" | "spawn" => Cap::ActCombat,
        "drop-item" => Cap::ActItems,
        "respawn-region" => Cap::ActWorld,
        "mute" | "unmute" => Cap::ActMute,
        "message" => Cap::ActMessagePlayer,
        "mode-flags" | "edit-character" => Cap::ActCharacterEdit,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReasonRule {
    Required,
    Optional,
}

/// A request that passed its body checks.
struct Plan {
    event: &'static str,
    cmd: AdminCmd,
    /// How many targets the guards count.
    targets: usize,
    /// The target of a single-target request (for the stale answer).
    single: Option<TargetRef>,
    reason: ReasonRule,
    irreversible: bool,
    /// What the operator types to confirm a single target.
    confirm_name: Option<String>,
    /// The data of the audit record.
    audit: Value,
    /// The game audits it itself (`gm.command`): no `admin.*` records.
    game_audited: bool,
}

fn parse<T: DeserializeOwned>(body: Value) -> ApiResult<T> {
    serde_json::from_value(body).map_err(|e| ApiError::bad_request(format!("malformed body: {e}")))
}

fn nonempty<T>(field: &str, v: &[T]) -> ApiResult<()> {
    if v.is_empty() {
        Err(ApiError::invalid(
            field,
            format!("{field} must not be empty"),
        ))
    } else {
        Ok(())
    }
}

fn finite(field: &str, v: &[f32]) -> ApiResult<()> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(ApiError::invalid(
            field,
            format!("{field} must be finite numbers"),
        ))
    }
}

fn vec3(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn chars(v: &[u32]) -> Vec<CharacterId> {
    v.iter().copied().map(CharacterId).collect()
}

fn objs(v: &[u32]) -> Vec<ObjId> {
    v.iter().copied().map(ObjId).collect()
}

fn expect_place(world: Option<u32>, layer: Option<i32>) -> Option<(WorldId, LayerId)> {
    world.map(|w| (WorldId(w), LayerId(layer.unwrap_or(0))))
}

fn single_char(v: &[u32]) -> Option<TargetRef> {
    match v {
        [one] => Some(TargetRef::Character(*one)),
        _ => None,
    }
}

fn single_obj(v: &[u32]) -> Option<TargetRef> {
    match v {
        [one] => Some(TargetRef::Object(*one)),
        _ => None,
    }
}

/// Runs one action (see the module documentation).
pub(super) async fn handle<G: GameLink>(
    env: &Env<'_, G>,
    action: &str,
    m: Mutation<Value>,
) -> ApiResult<ActionResult> {
    let Some(cap) = cap_of(action) else {
        return Err(ApiError::not_found(format!(
            "there is no action named '{action}' (known: {})",
            ACTIONS.join(", ")
        )));
    };
    env.who.require(env.caps, cap)?;
    let plan = plan(env, action, m.body.clone()).await?;
    env.policy.check_targets(plan.targets)?;
    let reason = match plan.reason {
        ReasonRule::Required => Some(env.policy.reason_required(m.reason.as_ref())?),
        ReasonRule::Optional => env.policy.reason_optional(m.reason.as_ref())?,
    };
    let facts = ConfirmFacts {
        targets: plan.targets,
        target_name: plan.confirm_name.as_deref(),
        irreversible: plan.irreversible,
        ..ConfirmFacts::default()
    };
    env.policy.confirm(&facts, m.confirm.as_deref())?;
    execute(env, action, plan, reason).await
}

// ---- dispatch -------------------------------------------------------------------------------------------------------

async fn execute<G: GameLink>(
    env: &Env<'_, G>,
    action: &str,
    plan: Plan,
    reason: Option<ReasonText>,
) -> ApiResult<ActionResult> {
    let audited = !plan.game_audited;
    if audited {
        audit_admin(
            env.audit,
            plan.event,
            env.who,
            "requested",
            reason.as_ref(),
            plan.audit.clone(),
        )?;
    }
    let out = match env.game.command(plan.cmd.clone(), env.who.actor()).await {
        Ok(out) => out,
        Err(e) => {
            if audited {
                let _ = audit_admin(
                    env.audit,
                    plan.event,
                    env.who,
                    "failed",
                    reason.as_ref(),
                    merged(&plan.audit, json!({"error": e.to_string()})),
                );
            }
            return Err(e.into());
        }
    };
    let result = finish(env, action, &plan, out).await;
    // the command ran: a record of its outcome that cannot be written must not turn an applied action into an error
    // (the `requested` record, which gates the dispatch, is the one that must be acknowledged, 25 §10.1)
    if audited && let Err(e) = audit_outcome(env, &plan, reason.as_ref(), &result) {
        tracing::error!(event = plan.event, error = %e, "the outcome of an admin action could not be audited");
    }
    result
}

/// The answer of the game as an [`ActionResult`], or the error of a single failed target.
async fn finish<G: GameLink>(
    env: &Env<'_, G>,
    action: &str,
    plan: &Plan,
    out: AdminOutput,
) -> ApiResult<ActionResult> {
    let mut res = ActionResult {
        action: action.to_owned(),
        ..ActionResult::default()
    };
    match out {
        AdminOutput::Report { per_target } => {
            res.results = per_target.iter().map(outcome).collect();
            res.failed = count(res.results.iter().filter(|r| !r.ok).count());
            res.ok = count(res.results.iter().filter(|r| r.ok).count());
            if plan.targets == 1
                && let [one] = per_target.as_slice()
                && let Err(f) = &one.outcome
            {
                return Err(error_of(env, f, plan.single).await);
            }
        }
        AdminOutput::Spawned { ids } => {
            res.ok = count(ids.len());
            res.spawned = ids.iter().map(|i| i.0).collect();
        }
        AdminOutput::Done { affected } => {
            res.ok = 1;
            res.affected = Some(affected);
        }
        AdminOutput::Lines(lines) => {
            res.ok = 1;
            res.lines = lines;
        }
        AdminOutput::Failed(f) => return Err(ApiError::from(&f)),
        AdminOutput::Error(text) => return Err(text_error(&text)),
        other => {
            return Err(ApiError::internal(&format!(
                "unexpected answer to {action}: {other:?}"
            )));
        }
    }
    Ok(res)
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn outcome(t: &TargetResult) -> TargetOutcome {
    TargetOutcome {
        target: match t.target {
            TargetId::Object(o) => TargetRef::Object(o.0),
            TargetId::Character(c) => TargetRef::Character(c.0),
            TargetId::Guild(g) => TargetRef::Guild(g.0),
        },
        ok: t.is_ok(),
        error: t.outcome.as_ref().err().map(|f| TargetError {
            code: ApiError::from(f).code,
            message: f.message.clone(),
        }),
        info: t.info.clone(),
    }
}

/// The error answer of a request whose only target failed. A stale object carries what it is now.
async fn error_of<G: GameLink>(
    env: &Env<'_, G>,
    f: &AdminFailure,
    single: Option<TargetRef>,
) -> ApiError {
    let mut e = ApiError::from(f);
    if f.code == AdminErrorCode::StaleTarget
        && let Some(TargetRef::Object(id)) = single
        && let Ok(AdminQueryResult::Object(d)) =
            env.game.query(AdminQuery::InspectObject(ObjId(id))).await
        && let Ok(current) = serde_json::to_value(ObjectDetail::from(&*d))
    {
        e.details = Some(ErrorDetails {
            current: Some(Box::new(current)),
            ..ErrorDetails::default()
        });
    }
    e
}

/// The text answer of `RunLine` (`X is not connected`, `X does not exist`, ...) as an error.
fn text_error(text: &str) -> ApiError {
    if text.ends_with("is not connected") {
        ApiError::new(ErrorCode::CharacterOnline, text)
    } else if text.ends_with("does not exist") {
        ApiError::not_found(text)
    } else {
        ApiError::new(ErrorCode::Invalid, text)
    }
}

// ---- audit ----------------------------------------------------------------------------------------------------------

/// `base` and `extra` (both objects) in one object; `extra` wins.
fn merged(base: &Value, extra: Value) -> Value {
    let mut m = base.as_object().cloned().unwrap_or_default();
    if let Value::Object(e) = extra {
        m.extend(e);
    }
    Value::Object(m)
}

fn target_id(t: &TargetRef) -> u32 {
    match t {
        TargetRef::Object(o) => *o,
        TargetRef::Character(c) => *c,
        TargetRef::Guild(g) => *g,
    }
}

/// The records of `done` / `failed` (25 §10.1, §10.3): one per target for teleports, character edits and mutes, one
/// summary for the rest.
fn audit_outcome<G>(
    env: &Env<'_, G>,
    plan: &Plan,
    reason: Option<&ReasonText>,
    result: &ApiResult<ActionResult>,
) -> ApiResult<()> {
    let emit =
        |state: &str, data: Value| audit_admin(env.audit, plan.event, env.who, state, reason, data);
    let r = match result {
        Ok(r) => r,
        Err(e) => {
            return emit(
                "failed",
                merged(
                    &plan.audit,
                    json!({"error": e.code.name(), "message": e.message}),
                ),
            );
        }
    };
    let per_target = matches!(
        plan.event,
        "admin.teleport" | "admin.character_edit" | "admin.mute"
    );
    if !per_target {
        return emit(
            "done",
            merged(
                &plan.audit,
                json!({
                    "ok": r.ok,
                    "failed": r.failed,
                    "spawned": r.spawned,
                    "affected": r.affected,
                    "results": r.results.iter().map(compact).collect::<Vec<_>>(),
                }),
            ),
        );
    }
    for t in &r.results {
        let state = if t.ok { "done" } else { "failed" };
        let mut extra = serde_json::Map::new();
        match t.target {
            TargetRef::Character(c) => {
                extra.insert("character".to_owned(), json!(c));
                extra.insert("target".to_owned(), json!(c));
            }
            TargetRef::Object(o) => {
                extra.insert("object".to_owned(), json!(o));
            }
            TargetRef::Guild(g) => {
                extra.insert("guild".to_owned(), json!(g));
            }
        }
        if let Some(Value::Object(info)) = &t.info {
            extra.extend(info.clone());
        }
        if let Some(e) = &t.error {
            extra.insert("error".to_owned(), json!(e.code.name()));
            extra.insert("message".to_owned(), json!(e.message));
        }
        emit(state, merged(&plan.audit, Value::Object(extra)))?;
    }
    Ok(())
}

fn compact(t: &TargetOutcome) -> Value {
    json!({
        "target": target_id(&t.target),
        "ok": t.ok,
        "error": t.error.as_ref().map(|e| e.code.name()),
    })
}

// ---- plans ----------------------------------------------------------------------------------------------------------

async fn plan<G: GameLink>(env: &Env<'_, G>, action: &str, body: Value) -> ApiResult<Plan> {
    match action {
        "teleport" => {
            let b: TeleportBody = parse(body)?;
            nonempty("characters", &b.characters)?;
            finite("pos", &b.pos)?;
            let label = match b.label {
                TeleportLabel::Point => "point",
                TeleportLabel::Location => "location",
            };
            Ok(Plan {
                event: "admin.teleport",
                cmd: AdminCmd::TeleportCharacter(TeleportCharacter {
                    characters: chars(&b.characters),
                    world: WorldId(b.world),
                    layer: LayerId(b.layer),
                    pos: vec3(b.pos),
                    mode: TeleportMode::Point,
                }),
                targets: b.characters.len(),
                single: single_char(&b.characters),
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                audit: json!({
                    "characters": b.characters,
                    "mode": label,
                    "to": {"world": b.world, "layer": b.layer, "pos": b.pos},
                }),
                game_audited: false,
            })
        }
        "unstuck" => {
            let b: UnstuckBody = parse(body)?;
            nonempty("characters", &b.characters)?;
            Ok(Plan {
                event: "admin.teleport",
                cmd: AdminCmd::TeleportCharacter(TeleportCharacter {
                    characters: chars(&b.characters),
                    world: WorldId(0),
                    layer: LayerId(0),
                    pos: Vec3::ZERO,
                    mode: TeleportMode::Unstuck,
                }),
                targets: b.characters.len(),
                single: single_char(&b.characters),
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                audit: json!({"characters": b.characters, "mode": "unstuck"}),
                game_audited: false,
            })
        }
        "summon" => {
            let b: SummonBody = parse(body)?;
            let (acting, line) = gm_line(env, b.as_character, b.character, "su").await?;
            Ok(lines_plan(line, acting, TargetRef::Character(b.character)))
        }
        "goto" => {
            let b: GotoBody = parse(body)?;
            goto_plan(env, b).await
        }
        "heal" => {
            let b: HealBody = parse(body)?;
            nonempty("objs", &b.objs)?;
            Ok(Plan {
                event: "admin.heal",
                cmd: AdminCmd::Heal(Heal {
                    targets: objs(&b.objs),
                    expect: expect_place(b.world, b.layer),
                }),
                targets: b.objs.len(),
                single: single_obj(&b.objs),
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                audit: json!({"targets": b.objs, "world": b.world, "layer": b.layer}),
                game_audited: false,
            })
        }
        "resurrect" => {
            let b: ResurrectBody = parse(body)?;
            Ok(Plan {
                event: "admin.heal",
                cmd: AdminCmd::Resurrect(Resurrect {
                    character: CharacterId(b.character),
                }),
                targets: 1,
                single: Some(TargetRef::Character(b.character)),
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                audit: json!({"targets": [b.character], "resurrect": true}),
                game_audited: false,
            })
        }
        "kill" => {
            let b: KillBody = parse(body)?;
            nonempty("objs", &b.objs)?;
            // the kill of a player is irreversible and asks for its name; looking at the victims costs one query each,
            // so a long list (typed confirmation anyway) is not looked at
            let (players, name) = players_among(env, &b.objs).await?;
            Ok(Plan {
                event: "admin.kill",
                cmd: AdminCmd::KillObject(KillObject {
                    objs: objs(&b.objs),
                    drops: b.drops,
                    expect: expect_place(b.world, b.layer),
                }),
                targets: b.objs.len(),
                single: single_obj(&b.objs),
                reason: ReasonRule::Optional,
                irreversible: players,
                confirm_name: name,
                audit: json!({
                    "targets": b.objs, "drops": b.drops, "world": b.world, "layer": b.layer,
                    "players": players,
                }),
                game_audited: false,
            })
        }
        "despawn" => {
            let b: DespawnBody = parse(body)?;
            nonempty("objs", &b.objs)?;
            Ok(Plan {
                event: "admin.despawn",
                cmd: AdminCmd::DespawnObject(DespawnObject {
                    objs: objs(&b.objs),
                    expect: expect_place(b.world, b.layer),
                }),
                targets: b.objs.len(),
                single: single_obj(&b.objs),
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                audit: json!({"targets": b.objs, "world": b.world, "layer": b.layer}),
                game_audited: false,
            })
        }
        "despawn-area" => {
            let b: DespawnAreaBody = parse(body)?;
            nonempty("kinds", &b.kinds)?;
            finite("rect", &b.rect)?;
            let kinds = b.kinds.iter().fold(0_u32, |acc, k| acc | area_bit(*k));
            Ok(Plan {
                event: "admin.despawn_area",
                cmd: AdminCmd::DespawnArea(DespawnArea {
                    world: WorldId(b.world),
                    layer: LayerId(b.layer),
                    rect: b.rect,
                    kinds,
                    max_objects: env.policy.max_objects_per_action,
                }),
                targets: 0,
                single: None,
                reason: ReasonRule::Optional,
                // always a typed confirmation: the operator types CONFIRM
                irreversible: true,
                confirm_name: None,
                audit: json!({
                    "world": b.world, "layer": b.layer, "rect": b.rect,
                    "kinds": b.kinds.iter().map(|k| area_name(*k)).collect::<Vec<_>>(),
                }),
                game_audited: false,
            })
        }
        "spawn" => {
            let b: SpawnBody = parse(body)?;
            finite("pos", &b.pos)?;
            finite("spread", &[b.spread])?;
            Ok(Plan {
                event: "admin.spawn",
                cmd: AdminCmd::SpawnMonsters(SpawnMonsters {
                    world: WorldId(b.world),
                    layer: LayerId(b.layer),
                    pos: vec3(b.pos),
                    prop: b.prop,
                    count: b.count,
                    spread: b.spread,
                    aggressive: b.aggressive,
                    lifetime_s: b.lifetime_s,
                }),
                targets: 0,
                single: None,
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                audit: json!({
                    "world": b.world, "layer": b.layer, "pos": b.pos, "prop": b.prop,
                    "count": b.count, "spread": b.spread, "aggressive": b.aggressive,
                    "lifetime_s": b.lifetime_s,
                }),
                game_audited: false,
            })
        }
        "drop-item" => {
            let b: DropItemBody = parse(body)?;
            finite("pos", &b.pos)?;
            let (item_id, count) = (b.item.item.item_id, b.item.item.count);
            Ok(Plan {
                event: "admin.drop_item",
                cmd: AdminCmd::DropItem(DropItem {
                    world: WorldId(b.world),
                    layer: LayerId(b.layer),
                    pos: vec3(b.pos),
                    item: Box::new(b.item),
                    owner: b.owner.map(CharacterId),
                }),
                targets: 0,
                single: None,
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                audit: json!({
                    "world": b.world, "layer": b.layer, "pos": b.pos,
                    "item": item_id, "count": count, "owner": b.owner,
                }),
                game_audited: false,
            })
        }
        "respawn-region" => {
            let b: RespawnBody = parse(body)?;
            Ok(Plan {
                event: "admin.respawn",
                cmd: AdminCmd::RespawnRegion(RespawnRegion {
                    world: WorldId(b.world),
                    layer: LayerId(b.layer),
                    region: b.region,
                }),
                targets: 0,
                single: None,
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                audit: json!({"world": b.world, "layer": b.layer, "region": b.region}),
                game_audited: false,
            })
        }
        "mute" => {
            let b: MuteBody = parse(body)?;
            if b.seconds == Some(0) {
                return Err(ApiError::invalid(
                    "seconds",
                    "a mute lasts at least one second: use unmute to lift it",
                ));
            }
            let seconds = b.seconds.unwrap_or(u32::MAX);
            Ok(Plan {
                event: "admin.mute",
                cmd: AdminCmd::Mute(Mute {
                    character: CharacterId(b.character),
                    seconds,
                }),
                targets: 1,
                single: Some(TargetRef::Character(b.character)),
                reason: ReasonRule::Required,
                irreversible: false,
                confirm_name: None,
                audit: json!({"action": "mute", "duration": b.seconds, "permanent": b.seconds.is_none()}),
                game_audited: false,
            })
        }
        "unmute" => {
            let b: UnmuteBody = parse(body)?;
            Ok(Plan {
                event: "admin.mute",
                cmd: AdminCmd::Unmute(Unmute {
                    character: CharacterId(b.character),
                }),
                targets: 1,
                single: Some(TargetRef::Character(b.character)),
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                audit: json!({"action": "unmute", "duration": 0}),
                game_audited: false,
            })
        }
        "message" => {
            let b: MessageBody = parse(body)?;
            nonempty("characters", &b.characters)?;
            let style = match b.style {
                MessageStyle::Notice => "notice",
                MessageStyle::Whisper => "whisper",
            };
            Ok(Plan {
                event: "admin.message",
                cmd: AdminCmd::NotifyCharacter(NotifyCharacter {
                    characters: chars(&b.characters),
                    text: b.text.clone(),
                    style: match b.style {
                        MessageStyle::Notice => NotifyStyle::Notice,
                        MessageStyle::Whisper => NotifyStyle::System,
                    },
                }),
                targets: b.characters.len(),
                single: single_char(&b.characters),
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                // the text itself is not audited, its hash is (25 §10.3)
                audit: json!({
                    "targets": b.characters, "style": style,
                    "text_hash": blake3::hash(b.text.as_bytes()).to_hex().to_string(),
                    "text_len": b.text.len(),
                }),
                game_audited: false,
            })
        }
        "mode-flags" => {
            let b: ModeFlagsBody = parse(body)?;
            let set = b.set.iter().fold(0_u32, |a, f| a | f.bits());
            let clear = b.clear.iter().fold(0_u32, |a, f| a | f.bits());
            Ok(Plan {
                event: "admin.character_edit",
                cmd: AdminCmd::SetModeFlags(SetModeFlags {
                    character: CharacterId(b.character),
                    set,
                    clear,
                }),
                targets: 1,
                single: Some(TargetRef::Character(b.character)),
                reason: ReasonRule::Optional,
                irreversible: false,
                confirm_name: None,
                audit: json!({"field": "mode_flags", "set": set, "clear": clear}),
                game_audited: false,
            })
        }
        "edit-character" => {
            let b: EditCharacterBody = parse(body)?;
            let game_edit = match &b.edit {
                WireEdit::Level { level, job } => edit::CharacterEdit::Level {
                    level: *level,
                    job: *job,
                },
                WireEdit::Job(j) => edit::CharacterEdit::Job(*j),
                WireEdit::Stats { str, sta, dex, int } => edit::CharacterEdit::Stats {
                    str: *str,
                    sta: *sta,
                    dex: *dex,
                    int: *int,
                },
                WireEdit::Restat => edit::CharacterEdit::Restat,
                WireEdit::SkillPoints(n) => edit::CharacterEdit::SkillPoints(*n),
                WireEdit::ResetSkills => edit::CharacterEdit::ResetSkills,
                WireEdit::Rename(name) => {
                    // the identity edits belong to `act.accounts` (25 §1.6), not to the stats editor's capability
                    env.who.require(env.caps, Cap::ActAccounts)?;
                    edit::CharacterEdit::Rename(name.clone())
                }
                // the quest edits are Phase C: the request is understood, the backend is not there yet
                WireEdit::Quest { .. } => return Err(ApiError::needs_feature("WP-ADMIN-31")),
            };
            let field = game_edit.field();
            Ok(Plan {
                event: "admin.character_edit",
                cmd: AdminCmd::EditCharacter(EditCharacter {
                    character: CharacterId(b.character),
                    edit: game_edit,
                }),
                targets: 1,
                single: Some(TargetRef::Character(b.character)),
                reason: ReasonRule::Optional,
                // a rename cannot be undone (25 §2.5): the operator types CONFIRM
                irreversible: matches!(b.edit, WireEdit::Rename(_)),
                confirm_name: None,
                audit: json!({"field": field, "edit": b.edit}),
                game_audited: false,
            })
        }
        other => Err(ApiError::not_found(format!(
            "there is no action named '{other}'"
        ))),
    }
}

fn area_bit(k: AreaKind) -> u32 {
    match k {
        AreaKind::Monster => EntityKind::Monster.bit(),
        AreaKind::Pet => EntityKind::Pet.bit(),
        AreaKind::Npc => EntityKind::Npc.bit(),
        AreaKind::Item => EntityKind::Item.bit(),
        AreaKind::Ctrl => EntityKind::Ctrl.bit(),
    }
}

fn area_name(k: AreaKind) -> &'static str {
    match k {
        AreaKind::Monster => "monster",
        AreaKind::Pet => "pet",
        AreaKind::Npc => "npc",
        AreaKind::Item => "item",
        AreaKind::Ctrl => "ctrl",
    }
}

/// Whether a player is among the objects of a kill and, for exactly one object, the player's name. Only up to
/// [`CONFIRM_TARGETS`] objects are looked at; more need the typed confirmation anyway.
async fn players_among<G: GameLink>(
    env: &Env<'_, G>,
    ids: &[u32],
) -> ApiResult<(bool, Option<String>)> {
    if ids.len() > CONFIRM_TARGETS {
        return Ok((false, None));
    }
    let answers = futures_util::future::join_all(
        ids.iter()
            .map(|o| env.game.query(AdminQuery::InspectObject(ObjId(*o)))),
    )
    .await;
    let mut players = Vec::new();
    for a in answers {
        if let AdminQueryResult::Object(d) = a?
            && d.kind == EntityKind::Player
        {
            players.push(d.name.clone());
        }
    }
    let name = match (ids.len(), players.as_slice()) {
        (1, [one]) => Some(one.clone()),
        _ => None,
    };
    Ok((!players.is_empty(), name))
}

// ---- GM lines (summon, goto) ------------------------------------------------------------------------------------------

/// A name that can stand in a GM line.
fn word(name: &str) -> ApiResult<&str> {
    if name.is_empty()
        || name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"')
    {
        Err(ApiError::invalid(
            "character",
            "that name cannot be used in a GM line",
        ))
    } else {
        Ok(name)
    }
}

fn authority_of(e: &OnlineEntry) -> Authority {
    u8::try_from(u32::from(e.authority))
        .ok()
        .and_then(Authority::try_from_db_byte)
        .unwrap_or(Authority::GENERAL)
}

/// The acting GM character of a staff member or an owner (25 §1.5) as the online row of the game.
async fn acting_row<G: GameLink>(
    env: &Env<'_, G>,
    rows: &[OnlineEntry],
    asked: Option<u32>,
) -> ApiResult<OnlineEntry> {
    let row_of = |id: u32| rows.iter().find(|r| r.character_id.0 == id).cloned();
    match &env.who.account {
        Some(account) => {
            let own = env
                .game
                .in_world_character(account.id)
                .map(|c| c.0)
                .ok_or_else(|| {
                    ApiError::invalid(
                        "as_character",
                        "no acting character: log in a GM character in the game",
                    )
                })?;
            if asked.is_some_and(|a| a != own) {
                return Err(ApiError::forbidden_text(
                    "the acting character must belong to your own account",
                ));
            }
            row_of(own).ok_or_else(|| {
                ApiError::invalid("as_character", "your character is not in the world")
            })
        }
        None => {
            let id = asked
                .ok_or_else(|| ApiError::invalid("as_character", "name the character that acts"))?;
            let row = row_of(id).ok_or_else(|| {
                ApiError::new(
                    ErrorCode::CharacterOnline,
                    "that character is not in the world",
                )
            })?;
            if !authority_of(&row).at_least(Authority::GAMEMASTER) {
                return Err(ApiError::forbidden_text(
                    "the acting character needs an authority of L or above",
                ));
            }
            Ok(row)
        }
    }
}

/// `/<command> <target name>` as the acting character: resolves both rows and applies rank protection to the target.
async fn gm_line<G: GameLink>(
    env: &Env<'_, G>,
    as_character: Option<u32>,
    target: u32,
    command: &str,
) -> ApiResult<(OnlineEntry, String)> {
    let rows = online_rows(env).await?;
    let acting = acting_row(env, &rows, as_character).await?;
    let t = rows
        .iter()
        .find(|r| r.character_id.0 == target)
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::CharacterOnline,
                "that character is not in the world",
            )
        })?;
    rank::check_target(env.who, authority_of(t))?;
    Ok((acting, format!("/{command} {}", word(&t.name)?)))
}

async fn online_rows<G: GameLink>(env: &Env<'_, G>) -> ApiResult<Vec<OnlineEntry>> {
    match env
        .game
        .command(AdminCmd::ListOnline { limit: None }, env.who.actor())
        .await?
    {
        AdminOutput::Online(rows) => Ok(rows),
        other => Err(ApiError::internal(&format!(
            "ListOnline answered {other:?}"
        ))),
    }
}

fn lines_plan(line: String, acting: OnlineEntry, target: TargetRef) -> Plan {
    Plan {
        event: "gm.command",
        cmd: AdminCmd::RunLine {
            line,
            as_character: Some(acting.name),
        },
        targets: 1,
        single: Some(target),
        reason: ReasonRule::Optional,
        irreversible: false,
        confirm_name: None,
        audit: Value::Null,
        game_audited: true,
    }
}

async fn goto_plan<G: GameLink>(env: &Env<'_, G>, b: GotoBody) -> ApiResult<Plan> {
    match (b.character, b.world, b.pos) {
        (Some(target), None, None) => {
            let (acting, line) = gm_line(env, b.as_character, target, "te").await?;
            Ok(lines_plan(line, acting, TargetRef::Character(target)))
        }
        (None, Some(world), Some(pos)) => {
            finite("pos", &pos)?;
            // the legacy command takes whole metres greater than zero
            let (x, z) = (pos[0].round() as i64, pos[2].round() as i64);
            if x <= 0 || z <= 0 {
                return Err(ApiError::invalid("pos", "x and z must be at least 1 m"));
            }
            let rows = online_rows(env).await?;
            let acting = acting_row(env, &rows, b.as_character).await?;
            Ok(lines_plan(
                format!("/te {world} {x} {z}"),
                acting,
                TargetRef::Character(0),
            ))
        }
        _ => Err(ApiError::invalid(
            "character",
            "give either a character, or a world and a pos",
        )),
    }
}
