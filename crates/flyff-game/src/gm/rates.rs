//! Game settings and runtime rates (Plan/23 §8.5; WP-GM-17): the `GameRate` messages of `/gamesetting` and
//! `/ReloadConstant`, the `/rates` command and [`RatesConsumer`], the [`LiveConsumer`] that tells the clients when a
//! `rates.*` key changed.
//!
//! The values are the live ones: `ctx.live.rates` (override over file over the data default 1.0), the skill
//! point costs of `Constant.inc` (`GameData.constant`) and the shop factors of `Event.lua`. The messages follow
//! `CUser::AddGameSetting` (`User.cpp:6533`): everybody gets the shop cost, the three skill point costs and the
//! two shop factors; a GM (`>= L`) also the drop, gold, EXP and hit rates, the rebirth and hit point rates (1.0)
//! and the aggressive rate.

use super::cmd::admin::issuer_set_by;
use super::requests::GmRequest;
use super::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::ctx::{GameCtx, Services};
use crate::event::game_rate as gr;
use crate::live::{LiveConfig, LiveConsumer};
use crate::overrides::{RATE_KEYS, RateKey, RuntimeKey};
use flyff_common::SessionId;
use flyff_common::gm::Authority;

/// The `GameRate` kind a settable rate is announced under (`None`: the party EXP rate has no client message).
pub fn kind_of(key: &RateKey) -> Option<u8> {
    match key.short {
        "shop" => Some(gr::SHOPCOST),
        "drop" => Some(gr::ITEMDROP),
        "gold" => Some(gr::GOLDDROP),
        "exp" => Some(gr::MONSTEREXP),
        "hit" => Some(gr::MONSTERHIT),
        "aggressive" => Some(gr::AGGRESSIVE),
        _ => None,
    }
}

/// The rate kinds only a GM receives (`IsAuthHigher(AUTH_GAMEMASTER)` in `AddGameSetting`).
pub fn gm_only(kind: u8) -> bool {
    matches!(
        kind,
        gr::ITEMDROP
            | gr::GOLDDROP
            | gr::MONSTEREXP
            | gr::MONSTERHIT
            | gr::REBIRTH
            | gr::HITPOINT
            | gr::AGGRESSIVE
    )
}

/// The `(kind, rate)` list of `AddGameSetting`, in the order of the original: 6 entries, 13 with `gm`. The list is
/// doc 22's [`gr::join_rates`], the same one the join batch sends (`event::game_rate::GameRateStep`).
pub fn settings(svc: &Services, ctx: &GameCtx<'_>, gm: bool) -> Vec<(u8, f32)> {
    gr::join_rates(ctx.live, ctx.data, svc.event.factors(), gm)
}

/// Sends the settings to one session (`CUser::AddGameSetting`).
pub fn send_settings(svc: &Services, ctx: &mut GameCtx<'_>, sid: SessionId, gm: bool) {
    for (kind, rate) in settings(svc, ctx, gm) {
        ctx.outbox.to_session(sid, 0, gr::message(kind, rate));
    }
}

/// `CUserMng::AddGameSetting`: every bound session gets its own list (GMs the long one).
pub fn send_settings_to_all(svc: &Services, ctx: &mut GameCtx<'_>) {
    let targets: Vec<(SessionId, bool)> = ctx
        .sessions
        .bound()
        .map(|(sid, obj)| {
            let gm = ctx
                .sessions
                .get(obj)
                .is_some_and(|e| e.authority.at_least(Authority::GAMEMASTER));
            (sid, gm)
        })
        .collect();
    for (sid, gm) in targets {
        send_settings(svc, ctx, sid, gm);
    }
}

/// Row 77 `/gamesetting (gs)`: the game settings to the issuer (6 `GameRate` kinds, 13 for a GM).
pub fn gamesetting(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let Some(sid) = cx.ctx.session_of(who) else {
        return Ok(CmdDone::default());
    };
    let gm = cx.authority.at_least(Authority::GAMEMASTER);
    send_settings(cx.svc, cx.ctx, sid, gm);
    Ok(CmdDone::default())
}

/// Row 88 `/ReloadConstant (rec)` (SIMPL, 23 §13 Q3): `GameData` is immutable, so `Constant.inc` is not reread;
/// the game settings go to every session again and the reply says a restart is needed.
pub fn reload_constant(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    send_settings_to_all(cx.svc, cx.ctx);
    cx.out.text("Constant.inc reload requires a restart");
    Ok(CmdDone::default())
}

/// Why a `/rates` value is refused.
pub fn validate(key: &RateKey, value: f64) -> Result<f32, &'static str> {
    let v = value as f32;
    if value.is_finite() && key.allows(v) {
        Ok(v)
    } else {
        Err("value out of range")
    }
}

/// `1`, `0.5`, `2.25`: a rate as the command prints it.
fn show(v: f32) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// N13 `/rates [<key> <value|default>]`: without arguments (`>= L`) the effective rates; setting a value needs
/// `ADMINISTRATOR`. The value must be inside the range of its key (`value out of range` otherwise); `default`
/// removes the override. The change is a new `LiveConfig` generation in the same tick (every formula reads it at
/// once), persisted as `runtime_override` unless `fidelity.persist_gm_overrides` is off, and the clients are told
/// by [`RatesConsumer`]. The list of the no-argument form is `ChatText` (BFIX1 #006): 7 lines belong in the chat box, the
/// centre notices keep only 6.
pub fn rates(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    if name.is_empty() {
        let live = &cx.ctx.live.rates;
        let file = &cx.ctx.cfg.rules.rates;
        for k in &RATE_KEYS {
            let now = k.effective(live);
            let mark = if k.get(live) == k.get(file) {
                ""
            } else {
                "  [override]"
            };
            cx.out.chat_text(format!(
                "{:<10} {:<26} {}  (file: {}, default {}){mark}",
                k.short,
                k.key,
                show(now),
                k.get(file).map_or_else(|| "unset".to_owned(), show),
                show(k.data_default()),
            ));
        }
        return Ok(CmdDone::default());
    }
    if !cx.authority.at_least(Authority::ADMINISTRATOR) {
        return Err(CmdError::Refused("needs administrator".to_owned()));
    }
    let Some(key) = RateKey::find(&name) else {
        return Err(CmdError::Refused(format!(
            "unknown rate '{name}': exp, drop, gold, hit, shop, party, aggressive"
        )));
    };
    let mut value = args.token().to_owned();
    // the tokenizer splits a minus sign off the number (`-2` is `-` then `2`)
    let negative = value == "-";
    if negative {
        value = args.token().to_owned();
    }
    if value.is_empty() {
        return Err(CmdError::Usage);
    }
    let set_by = issuer_set_by(cx);
    if value.eq_ignore_ascii_case("default") {
        cx.svc.gm.request(GmRequest::SetRuntime {
            key: RuntimeKey::Rate(key),
            value: None,
            set_by,
        });
        cx.out.text(format!("{}: back to the default", key.short));
        return Ok(CmdDone::on(key.short));
    }
    let Ok(mut parsed) = value.parse::<f64>() else {
        return Err(CmdError::Usage);
    };
    if negative {
        parsed = -parsed;
    }
    let v = validate(key, parsed).map_err(|e| CmdError::Refused(e.to_owned()))?;
    cx.svc.gm.request(GmRequest::SetRuntime {
        key: RuntimeKey::Rate(key),
        value: Some(serde_json::json!(v)),
        set_by,
    });
    cx.out.text(format!("{} = {}", key.short, show(v)));
    Ok(CmdDone::on(key.short))
}

/// The consumer of 23 §8.5: when a `rates.*` key changed (a GM or admin change, a file reload) every in-world
/// session gets the new `GameRate` of that key. The shop cost goes to everybody, the others to GMs only (the
/// legacy `AddGameSetting` rule).
#[derive(Debug, Default)]
pub struct RatesConsumer;

impl LiveConsumer for RatesConsumer {
    fn on_live_change(&mut self, ctx: &mut GameCtx<'_>, old: &LiveConfig, new: &LiveConfig) {
        let changed: Vec<(u8, f32)> = RATE_KEYS
            .iter()
            .filter(|k| k.effective(&old.rates) != k.effective(&new.rates))
            .filter_map(|k| Some((kind_of(k)?, k.effective(&new.rates))))
            .collect();
        if changed.is_empty() {
            return;
        }
        let targets: Vec<(SessionId, bool)> = ctx
            .sessions
            .bound()
            .map(|(sid, obj)| {
                let gm = ctx
                    .sessions
                    .get(obj)
                    .is_some_and(|e| e.authority.at_least(Authority::GAMEMASTER));
                (sid, gm)
            })
            .collect();
        for (sid, gm) in targets {
            for &(kind, rate) in &changed {
                if gm || !gm_only(kind) {
                    ctx.outbox.to_session(sid, 0, gr::message(kind, rate));
                }
            }
        }
    }
}
