//! The monster spawn and removal commands (Plan/23 §4.5; WP-GM-13): `/createnpc` (row 74, alias `spawn`) and
//! `/rmvnpc` (row 78).

use crate::ai::spawn::{MonsterSpawn, build};
use crate::combat::dps::mark_instance_spawn;
use crate::gm::lookup;
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use flyff_common::ObjId;
use flyff_common::defs::neuz::{
    AII_BEAR, AII_BIGMUSCLE, AII_CLOCKWORKS, AII_KRRR, AII_METEONYKER, AII_MONSTER,
};

/// Most monsters one `/createnpc` makes.
pub const MAX_SPAWN: u32 = 100;

/// The AI kinds `/createnpc` accepts (`AII_MONSTER`, `CLOCKWORKS`, `BIGMUSCLE`, `KRRR`, `BEAR`, `METEONYKER`);
/// the admin console's spawn (25 §3.12) uses the same rule.
pub(crate) fn spawnable(ai: u32) -> bool {
    matches!(
        ai,
        AII_MONSTER | AII_CLOCKWORKS | AII_BIGMUSCLE | AII_KRRR | AII_BEAR | AII_METEONYKER
    )
}

/// Row 74 `/createnpc (cn, spawn) <moverId or name> [count] [aggressive 0/1] [dps 0/1]`: a NUMBER is the mover id,
/// anything else a name (exact, case-insensitive or the `MI_*` define, PL-099). Only the AI kinds above spawn
/// (anything else is silent). `count` 0 becomes 1, above 100 becomes 100 (a negative number is read as the
/// unsigned value of the legacy, so it becomes 100). The monsters appear at the issuer's position and layer,
/// standing, `active_attack = aggressive`, with `level * 15` gold, and are not members of a respawn region.
/// FIX (PL-115): `dps` is read once for all of them (the legacy read one token per monster); it marks them for
/// the DPS meter and works only with `features.dps_meter`.
pub fn createnpc(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let token = args.token().to_owned();
    let is_number = args.token_is_number();
    let data = std::sync::Arc::clone(cx.ctx.data);
    let Some(id) = lookup::mover(&data, &token, is_number) else {
        return Ok(CmdDone::default());
    };
    let Some(prop) = data.mover(id) else {
        return Ok(CmdDone::default());
    };
    if id == 0 || !spawnable(prop.ai) {
        return Ok(CmdDone::default());
    }
    let count = match args.number() as u32 {
        0 => 1,
        n if n > MAX_SPAWN => MAX_SPAWN,
        n => n,
    };
    let aggressive = args.number() != 0;
    let dps = args.number() != 0 && cx.ctx.cfg.rules.features.dps_meter;
    let Some(core) = cx.ctx.worlds.core(who.0) else {
        return Ok(CmdDone::default());
    };
    let (world, layer, pos) = (core.world, core.layer, core.pos);
    // `SetGold(level * 15)` with the C `int` wrap
    let gold = i32::try_from(prop.level)
        .unwrap_or(i32::MAX)
        .wrapping_mul(15);
    let spec = MonsterSpawn {
        mover: id,
        world,
        layer,
        pos,
        active_attack: aggressive,
        gold: Some(gold),
        owner_ai: None,
    };
    let mut made = 0;
    for _ in 0..count {
        let Some(mut m) = build(&data, cx.ctx.rng.world(world), &spec) else {
            break;
        };
        if dps {
            mark_instance_spawn(&mut m, &[id]);
        }
        match cx.ctx.worlds.spawn_mover(world, layer, m) {
            Ok(_) => made += 1,
            Err(e) => {
                // the legacy command was silent; a refused position (outside the world, no such layer) is worth a line
                tracing::warn!(mover = id, error = %e, "a GM spawn was refused");
                cx.out
                    .text(format!("{}: the spawn was refused ({e})", prop.name));
                break;
            }
        }
    }
    tracing::debug!(mover = id, made, "GM spawned monsters");
    Ok(CmdDone::on(prop.name.clone()))
}

/// Row 78 `/rmvnpc (rn) <objId>`: deletes the mover with that object id when it is an NPC or monster (not a
/// player). Anything else is silent.
pub fn rmvnpc(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let raw = args.number();
    let Ok(id) = u32::try_from(raw) else {
        return Ok(CmdDone::default());
    };
    let id = ObjId(id);
    let removable = cx
        .ctx
        .worlds
        .mover(id)
        .is_some_and(|m| !m.core.deleted && !m.is_player());
    if removable {
        cx.ctx.worlds.delete(id);
    }
    Ok(CmdDone::default())
}
