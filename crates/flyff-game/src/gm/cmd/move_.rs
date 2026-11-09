//! Teleport and presence commands (Plan/23 §4.3, §4.9): `/teleport` (row 28), `/summon` (row 31), `/where` (N02),
//! `/goto` (N03). Moves go through `Worlds::replace` (VR-082 bounds, 15 §10); the `_replace` phase executes them
//! at the end of the tick. The instance-dungeon restriction of row 28 asks `Services.gm.instances` (doc 22 installs the
//! registry; until then no world is an instance).

use crate::ctx::PlayerRef;
use crate::gm::target::{self, Target};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::world::ReplaceKind;
use flyff_common::math::Vec3;
use flyff_common::{LayerId, WorldId};
use flyff_script::cnum::c_atoi;

fn here(cx: &CmdCtx<'_, '_>, p: PlayerRef) -> Option<(WorldId, LayerId, Vec3)> {
    cx.ctx.worlds.core(p.0).map(|c| (c.world, c.layer, c.pos))
}

/// Queues the move; a refused move (outside the world, missing world or layer, flying) is silent like the legacy
/// command.
fn go(
    cx: &mut CmdCtx<'_, '_>,
    who: PlayerRef,
    world: WorldId,
    pos: Vec3,
    layer: LayerId,
    kind: ReplaceKind,
) {
    if let Err(e) = cx.ctx.worlds.replace(who.0, world, pos, layer, kind) {
        tracing::debug!(error = %e, "GM move refused");
    }
}

/// Row 28 `/teleport (te)`: `<player>`, `<worldId> <x> <z>` or `<worldId> <regionKey>`.
pub fn teleport(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let first = args.token().to_owned();
    if first.is_empty() {
        return Ok(CmdDone::default());
    }
    if !args.token_is_number() {
        // (a) to a player: unknown or not in a world is the same reply
        let Target::Online { player, name, .. } = target::find(cx.svc, &first) else {
            return Err(CmdError::NotFound(first));
        };
        if let Some((w, l, p)) = here(cx, player) {
            // a target inside an instance dungeon: only from the same world and layer (the dungeon's own entrance
            // rules decide everything else)
            let outside = here(cx, me).is_none_or(|(mw, ml, _)| (mw, ml) != (w, l));
            if cx.svc.gm.instances.is_instance_world(w) && outside {
                return Ok(CmdDone::on(name));
            }
            go(cx, me, w, p, l, ReplaceKind::Normal);
        }
        return Ok(CmdDone::on(name));
    }
    // (b) to a world: `<worldId> <x> <z>` or `<worldId> <regionKey>`
    let world = WorldId(u32::try_from(c_atoi(first.as_bytes())).unwrap_or(0));
    if cx.ctx.worlds.world(world).is_none() {
        return Ok(CmdDone::default());
    }
    let my_layer = here(cx, me).map(|(w, l, _)| (w, l));
    // an instance-dungeon world can only be entered from inside it
    if cx.svc.gm.instances.is_instance_world(world) && my_layer.is_none_or(|(w, _)| w != world) {
        return Ok(CmdDone::default());
    }
    let second = args.token().to_owned();
    if args.token_is_number() {
        let x = c_atoi(second.as_bytes());
        let z = args.number();
        if x <= 0 || z <= 0 {
            return Ok(CmdDone::default());
        }
        let layer = match my_layer {
            Some((w, l)) if w == world => l,
            _ => LayerId(0),
        };
        go(
            cx,
            me,
            world,
            Vec3::new(x as f32, 0.0, z as f32),
            layer,
            ReplaceKind::Normal,
        );
    } else if !second.is_empty()
        && let Some(pt) = cx.ctx.worlds.revival.revival_pos(world, &second)
    {
        go(cx, me, world, pt.pos, LayerId(0), ReplaceKind::Normal);
    }
    Ok(CmdDone::default())
}

/// Row 31 `/summon (su)`: replaces the target to the GM's world, position and layer (`REPLACE_FORCE`).
pub fn summon(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let name = args.token().to_owned();
    if name.is_empty() {
        return Ok(CmdDone::default());
    }
    let (player, name) = match target::online(cx, &name, true) {
        Ok(t) => t,
        // an existing but offline character: nothing (legacy)
        Err(CmdError::Offline(_)) => return Ok(CmdDone::default()),
        Err(e) => return Err(e),
    };
    if let Some((w, l, p)) = here(cx, me) {
        go(cx, player, w, p, l, ReplaceKind::Force);
    }
    Ok(CmdDone::on(name))
}

/// N02 `/where <name>`: world, layer and position of an online character.
pub fn where_cmd(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    if name.is_empty() {
        return Err(CmdError::Usage);
    }
    match target::find(cx.svc, &name) {
        Target::Unknown => Err(CmdError::NotFound(name)),
        Target::Offline { name, .. } => {
            cx.out.text(format!("{name} is not connected."));
            Ok(CmdDone::on(name))
        }
        Target::Online { name, player, .. } => {
            if let Some((w, l, p)) = here(cx, player) {
                cx.out.text(format!(
                    "{name}: world {} layer {} pos ({:.1}, {:.1}, {:.1})",
                    w.0, l.0, p.x, p.y, p.z
                ));
            }
            Ok(CmdDone::on(name))
        }
    }
}

/// N03 `/goto <x> <z>` or `<x> <y> <z>`: a move inside the current world and layer.
pub fn goto(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let mut nums = Vec::new();
    for _ in 0..3 {
        let t = args.token().to_owned();
        if t.is_empty() {
            break;
        }
        if !args.token_is_number() {
            return Err(CmdError::Usage);
        }
        nums.push(c_atoi(t.as_bytes()));
    }
    let (x, y, z) = match nums.as_slice() {
        [x, z] => (*x, 0, *z),
        [x, y, z] => (*x, *y, *z),
        _ => return Err(CmdError::Usage),
    };
    if x <= 0 || z <= 0 {
        return Err(CmdError::Usage);
    }
    let Some((w, l, _)) = here(cx, me) else {
        return Err(CmdError::NeedsSelf);
    };
    go(
        cx,
        me,
        w,
        Vec3::new(x as f32, y as f32, z as f32),
        l,
        ReplaceKind::Normal,
    );
    Ok(CmdDone::default())
}
