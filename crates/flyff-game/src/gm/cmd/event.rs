//! The event commands (Plan/23 §4.5 rows 75, 76, §4.7 rows 118, 153, 163; WP-GM-25): `/eventlist` (75), `/eventinfo`
//! (76), `/localevent` (118), `/CallTheRoll` (153) and `/COUPON` (163).
//!
//! Every handler is a thin wrapper over the entry point of doc 22: the Event.lua listing of
//! [`EventServices`](crate::event::EventServices), the flag store of `event::flags`, the attendance bits of
//! `event::generic` and the coupon counter of `event::lua_events`. Numbers are `args.number()` (`atoi`; a missing
//! argument reads as 0). The legacy rows answer nothing on bad input and so do these, except the one new line of
//! `/le 1` (`CmdError::Refused`, nothing is sent or stored).

use crate::event::flags::{FlagOrigin, MAX_EVENT};
use crate::event::generic::gm_call_the_roll;
use crate::event::lua_events::set_coupons;
use crate::event::set_flag;
use crate::gm::audit::issuer_label;
use crate::gm::cmd::admin::issuer_set_by;
use crate::gm::requests::GmRequest;
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult, Issuer};
use crate::overrides::{MAX_FLAG_ID, RuntimeKey, canonical_key};
use flyff_common::config::EventId;

/// The refusal of `/le 1` (`EVE_18` is the 18+ server marker, `TextCmd_LocalEvent` ignored it silently).
pub const EVENT_1_PROTECTED: &str = "event 1 cannot be changed";

/// Row 75 `/EVENTLIST` (`TextCmd_LuaEventList`): one line `ID : %d,  Title : %s,  State : %d` per event of
/// `GetAllEventList`, each a line of the chat box (BFIX1: list output is chat text). Nothing while the `Event` family
/// is not `Ready` or when the script answers nil (`IsPossible`).
pub fn event_list(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    for line in cx.svc.event.list_lines(cx.ctx.lua, cx.ctx.data) {
        cx.out.chat_text(line);
    }
    Ok(CmdDone::default())
}

/// Row 76 `/EVENTINFO <id>` (`TextCmd_LuaEventInfo`): the lines of `GetEventInfo(id)`, each a line of the chat box.
/// A negative id prints nothing (no Lua call).
pub fn event_info(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let id = args.number();
    if let Ok(id) = u32::try_from(id) {
        for line in cx.svc.event.info_lines(cx.ctx.lua, cx.ctx.data, id) {
            cx.out.chat_text(line);
        }
    }
    Ok(CmdDone::default())
}

/// The flag origin of an operator change of an id from 500: `gm:<account id>` for a player or item issuer,
/// `admin:<issuer label>` for the console and the admin API (the origin adds its own prefix).
fn flag_origin(cx: &CmdCtx<'_, '_>) -> FlagOrigin {
    match cx.issuer {
        Issuer::Player { player, .. } | Issuer::Item { player, .. } => FlagOrigin::Gm {
            by: cx.ctx.entry(player).map_or(0, |e| e.account.0).to_string(),
        },
        Issuer::Console | Issuer::Admin { .. } => FlagOrigin::Admin {
            by: issuer_label(cx),
        },
    }
}

/// Row 118 `/localevent (le) <id> <state>` (`TextCmd_LocalEvent`, 22 §3.2, 23 §8.5): `state` is the legacy `(BYTE)`
/// cast (257 is 1). An id outside `0..1024` is silent (the legacy `SetState` answered FALSE); id 1 answers
/// [`EVENT_1_PROTECTED`] and changes nothing. The ids below 500 are a runtime change of doc 10 (one `LiveConfig`
/// generation in the same tick, `SetLocalEvent` to everybody only when the effective value changed, the
/// `runtime_override` row `events.<name>` / `events.flag.<id>` unless `fidelity.persist_gm_overrides` is off); the
/// ids from 500 belong to the generic-event timer and are set directly, never persisted. No reply on success.
pub fn local_event(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let id = args.number();
    let Some(id) = u16::try_from(id)
        .ok()
        .filter(|i| usize::from(*i) < MAX_EVENT)
    else {
        return Ok(CmdDone::default());
    };
    if id == EventId::Eve18.id() {
        return Err(CmdError::Refused(EVENT_1_PROTECTED.to_owned()));
    }
    let state = args.number() as u8;
    if id <= MAX_FLAG_ID {
        let set_by = issuer_set_by(cx);
        let key = RuntimeKey::Event(id);
        cx.svc.gm.request(GmRequest::SetRuntime {
            key,
            value: Some(serde_json::json!(state)),
            set_by,
        });
        return Ok(CmdDone::on(canonical_key(key)));
    }
    let origin = flag_origin(cx);
    if let Err(e) = set_flag(cx.svc, cx.ctx, id, state, origin) {
        // unreachable after the range and id checks above; kept so a changed rule of the store shows in the log
        tracing::warn!(id, error = %e, "/le: the flag store refused the change");
    }
    Ok(CmdDone::on(format!("event {id}")))
}

/// Row 153 `/CallTheRoll (ctr) <n>` (`TextCmd_CallTheRoll`): the attendance bits of the issuer are cleared, the
/// roll and elapsed counters go to 0 and the bits `62 - i` for `i < n` are set (`n` is capped at 63: the C++
/// shifted negative bit numbers). Sends nothing, as the legacy: the window learns the bits at the next roll.
pub fn call_the_roll(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let n = args.number();
    gm_call_the_roll(cx.svc, cx.ctx, who, n);
    Ok(CmdDone::default())
}

/// Row 163 `/COUPON <n>` (`TextCmd_Coupon`): the coupon counter of the issuer is `n`. Sends nothing; the value rides
/// the normal snapshot.
pub fn coupon(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let n = args.number();
    if let Some(p) = cx.ctx.player_mut(who).and_then(|m| m.player_mut()) {
        set_coupons(&mut p.misc, n);
    }
    Ok(CmdDone::default())
}
