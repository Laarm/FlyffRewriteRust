//! The character testing commands (Plan/23 §4.5, §4.6, §4.9; WP-GM-13): `/aroundkill` (row 51), `/stat` (52),
//! `/jobname` (82), `/getgold` (83), `/heal` (N04) and `/kill` (N05).
//!
//! They act on the issuer's character ("self": a player, an item user, or the `as <name>` character of the
//! console), `/heal` and `/kill` also on a named online character.

use crate::combat::arbiter::{
    ApplyMask, AroundSpec, DamageRequest, send_damage, send_damage_around,
};
use crate::ctx::PlayerRef;
use crate::gm::audit::event;
use crate::gm::target;
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::item::gold;
use crate::stats::Out;
use crate::stats::points::{emit_state, restat_full, stats_changed};
use crate::stats::pool::{fill_pools_silent, send_pools_absolute};
use flyff_common::ObjId;
use flyff_common::defs::neuz::PARTS_RWEAPON;
use flyff_rules::combat::{AtkFlags, DamageParam};
use flyff_rules::consts::job::{
    MAX_EXP_LEVEL, MAX_EXPERT, MAX_JOB_LEVEL, MAX_JOBBASE, MAX_PROFESSIONAL,
};
use serde_json::json;

/// The issuer's character.
fn me(cx: &CmdCtx<'_, '_>) -> Result<PlayerRef, CmdError> {
    cx.self_player.ok_or(CmdError::NeedsSelf)
}

/// Row 51 `/aroundkill (ak)`: with a weapon in the right hand, one `SendDamageAround(AF_MAGICSKILL, self,
/// OBJTYPE_MONSTER, attack id 1, range 64 m, target ratio 0, around ratio 1)`: one hit on every monster within
/// 64 m. It kills only together with `/ok`. Without a weapon, nothing happens.
pub fn aroundkill(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let Some(m) = cx.ctx.player(who) else {
        return Ok(CmdDone::default());
    };
    let armed = m
        .player()
        .is_some_and(|p| p.items.inventory.get_equip(PARTS_RWEAPON).is_some());
    if !armed {
        return Ok(CmdDone::default());
    }
    let spec = AroundSpec {
        center: Some(who.0),
        pos: m.core.pos,
        flags: AtkFlags::MAGICSKILL,
        attacker: who.0,
        apply: ApplyMask::MONSTER,
        attack_id: 1,
        range: 64.0,
        target_ratio: 0.0,
    };
    send_damage_around(cx.svc, cx.ctx, spec);
    Ok(CmdDone::default())
}

/// Row 52 `/stat <str|sta|dex|int|gp|all|restate> <n>` (`TextCmd_stat`): the key is lower-cased and must be 2 to 7
/// characters long. It sets one base stat, the remaining stat points (`gp`) or all four stats (`all`); `restate`
/// is the restat scroll. An unknown key answers `Text("<key>unknown setting target")`. Then `SetState` goes to
/// the owner and the honor titles are re-checked. FIX (PL-115): the derived values follow (the pools are clamped
/// to the new maxima and sent, the sim's parameters are refreshed).
pub fn stat(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let typed = args.token().to_owned();
    let n = args.number();
    let len = typed.chars().count();
    if !(2..=7).contains(&len) {
        cx.out.text(format!("{typed}unknown setting target"));
        return Ok(CmdDone::default());
    }
    let key = typed.to_ascii_lowercase();
    let value = n.max(0);
    let core = cx.svc.stats.handle();
    let mut out = Out::new();
    let known = {
        let Some(m) = cx.ctx.worlds.player_mut(who.0) else {
            return Ok(CmdDone::default());
        };
        match key.as_str() {
            "str" => m.mv.base.str = value,
            "sta" => m.mv.base.sta = value,
            "dex" => m.mv.base.dex = value,
            "int" => m.mv.base.int = value,
            "all" => {
                m.mv.base.str = value;
                m.mv.base.sta = value;
                m.mv.base.dex = value;
                m.mv.base.int = value;
            }
            "gp" => {
                if let Some(p) = m.player_mut() {
                    p.progression.stat_points = value;
                }
            }
            "restate" => {
                // the restat scroll's function: refused (a text, nothing changed) when nothing is above 15
                let _ = restat_full(m, &core, &mut out);
                out.flush(cx.ctx, who.0);
                return Ok(CmdDone::default());
            }
            _ => {
                cx.out.text(format!("{key}unknown setting target"));
                return Ok(CmdDone::default());
            }
        }
        stats_changed(m, &core);
        // the pools stay as they are, but never above the new maxima; every client is told
        send_pools_absolute(m, &core, &mut out);
        emit_state(m, &mut out);
        true
    };
    let _ = known;
    out.flush(cx.ctx, who.0);
    crate::social::hooks::honor_recheck(cx.svc, cx.ctx, who, true);
    Ok(CmdDone::default())
}

/// Row 82 `/jobname (jn)`: the expert and professional job names with their level ranges (the text of the legacy
/// client command, `TextCmd_JobName`, now printed by the server).
pub fn jobname(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let data = std::sync::Arc::clone(cx.ctx.data);
    let list = |from: u32, to: u32| -> String {
        let mut s = String::new();
        for i in from..to {
            let name = data
                .jobs
                .info
                .get(i as usize)
                .and_then(Option::as_ref)
                .map_or("", |j| j.name.as_str());
            if name.len() < 15 {
                s.push_str(name);
                if i + 1 != to {
                    s.push_str(", ");
                }
            }
        }
        s
    };
    cx.out
        .text(format!("Expert Job : {}", list(MAX_JOBBASE, MAX_EXPERT)));
    cx.out.text(format!(
        "Expert Level : {} ~ {}",
        MAX_JOB_LEVEL + 1,
        MAX_JOB_LEVEL + MAX_EXP_LEVEL
    ));
    cx.out.text(format!(
        "Professional Job : {}",
        list(MAX_EXPERT, MAX_PROFESSIONAL)
    ));
    cx.out.text(format!(
        "Professional Level : {} ~~~ ",
        MAX_JOB_LEVEL + MAX_EXP_LEVEL
    ));
    Ok(CmdDone::default())
}

/// Row 83 `/getgold (gg) <n>` (`AddGold(n)`): `n` gold are added; a negative `n` is allowed and the wallet is
/// clamped at 0 (FIX, PL-115). Audit `item.create` with `source = "gm"`.
pub fn getgold(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let n = args.number();
    gold::add_gold(cx.svc, cx.ctx, who, i64::from(n), true);
    event(
        cx,
        "item.create",
        json!({"source": "gm", "gold": n, "item": null}),
    );
    Ok(CmdDone::default())
}

/// The online character a command with an optional name acts on: the name, else the issuer.
fn subject(
    cx: &CmdCtx<'_, '_>,
    args: &mut CmdArgs<'_>,
) -> Result<(PlayerRef, Option<String>), CmdError> {
    let name = args.token().to_owned();
    if name.is_empty() {
        return Ok((me(cx)?, None));
    }
    let (p, stored) = target::online(cx, &name, false)?;
    Ok((p, Some(stored)))
}

/// N04 `/heal [name]`: HP, MP and FP of self or of an online character are set to their maxima and sent
/// (`SetPointParam` to the near set). A dead character is not revived (`Text`).
pub fn heal(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let (who, named) = subject(cx, args)?;
    let core = cx.svc.stats.handle();
    let mut out = Out::new();
    let alive = match cx.ctx.worlds.player_mut(who.0) {
        Some(m) if m.mv.hp > 0 => {
            fill_pools_silent(m, &core);
            send_pools_absolute(m, &core, &mut out);
            true
        }
        _ => false,
    };
    if alive {
        out.flush(cx.ctx, who.0);
    } else {
        cx.out.text("the target is dead");
    }
    Ok(named.map_or_else(CmdDone::default, CmdDone::on))
}

/// The mover the GM has focused (`SetTargetReq` with `clear = 2` remembers it, 17 §3.10): a live, not deleted
/// mover.
fn focus_of(cx: &CmdCtx<'_, '_>, who: PlayerRef) -> Option<ObjId> {
    let id = cx.ctx.worlds.mover(who.0)?.combat.id_set_target?;
    cx.ctx
        .worlds
        .mover(id)
        .filter(|m| !m.core.deleted && m.mv.hp > 0)
        .map(|_| id)
}

/// N05 `/kill [name]`: without an argument the GM's focused monster or player, with a name that online
/// character. One `AF_FORCE` hit of the current HP through the damage pipeline, so the normal death rules run
/// (EXP, drops, the death penalty); a player victim hits itself (no PvP rules, no killer). Without a target: `Text`.
pub fn kill(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    let (victim, label) = if name.is_empty() {
        let who = me(cx)?;
        match focus_of(cx, who) {
            Some(id) => (id, None),
            None => {
                cx.out
                    .text("no target: focus a monster or player, or /kill <name>");
                return Ok(CmdDone::default());
            }
        }
    } else {
        let (p, stored) = target::online(cx, &name, false)?;
        (p.0, Some(stored))
    };
    let hp = cx.ctx.worlds.mover(victim).map_or(0, |m| m.mv.hp);
    if hp <= 0 {
        return Ok(CmdDone::default());
    }
    // a monster is hit by the GM (the kill EXP and the drops are the GM's); a player hits itself, so that the
    // PvP rules neither refuse the hit nor flag the GM as a killer
    let victim_is_player = cx.ctx.worlds.mover(victim).is_some_and(|m| m.is_player());
    let attacker = match cx.self_player {
        Some(p) if !victim_is_player => p.0,
        _ => victim,
    };
    let req = DamageRequest::new(victim, attacker, AtkFlags::FORCE, DamageParam::force(hp));
    send_damage(cx.svc, cx.ctx, req);
    Ok(label.map_or_else(CmdDone::default, CmdDone::on))
}
