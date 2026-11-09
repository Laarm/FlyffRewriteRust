//! The layer commands (Plan/23 §4.5 rows 193-195; WP-GM-26): `/CreateLayer (cl) <worldId> <layer>`, `/DeleteLayer (dl)
//! <worldId> <layer>` and `/Layer (lay) <worldId> <layer> <x> <z>` (`FuncTextCmd.cpp:2209-2268`, `World.cpp`
//! `CreateLayer` / `ReleaseLayer`), over `Worlds::{create_layer, invalidate, has_nobody, replace}` (Plan/15 §4.2-§4.3,
//! §10.1).
//!
//! * `/cl`: a GM layer is a full `.dyo` copy of the world with the event flags as they are now and a fresh respawner, of
//!   kind [`LayerKind::Normal`]. Any loaded world is accepted (legacy): a GM layer in an instance world, 209 or 212-214
//!   occupies the id a party, a room owner or a guild would use later, and that owner's `create_layer` then fails with
//!   `Exists`. Silent on bad input; FIX: ids `<= 0` are refused (layer 0 is the world, negative ids are the ticket layers).
//! * `/dl`: FIX of the legacy `ReleaseLayer(0)` that destroyed the world with its players. Releases a GM layer through
//!   `invalidate` (the layer garbage collection of that world releases it at the end of its tick once nobody is in or
//!   entering it); a layer of another kind (an instance, a mini room, a guild house: their services own them) or with a
//!   player in it, entering it or queued into it prints [`LAYER_IN_USE`].
//! * `/lay` (needs a character): replaces the issuer to `(x, 0, z)` of the layer (`y` is resolved on arrival, as legacy)
//!   with the legacy texts in the legacy order: [`UNDEFINED_WORLD`], [`LAYER_NO_EXISTS`], [`OUT_OF_WORLD`]. No
//!   instance-world restriction (unlike `/teleport`).
//!
//! The audit target of all three is `"world W layer L"`.

use super::{LAYER_IN_USE, LAYER_NO_EXISTS, OUT_OF_WORLD, UNDEFINED_WORLD};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::world::{LayerKind, ReplaceError, ReplaceKind};
use flyff_common::math::Vec3;
use flyff_common::{LayerId, WorldId};

fn target(world: u32, layer: i32) -> CmdDone {
    CmdDone::on(format!("world {world} layer {layer}"))
}

/// Row 193 `/CreateLayer <worldId> <layer>`: creates the live `Normal` layer `layer > 0` of a loaded world; silent.
pub fn create_layer(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let world = args.number() as u32;
    let n = args.number();
    let w = WorldId(world);
    if n <= 0 || cx.ctx.worlds.world(w).is_none() {
        return Ok(target(world, n));
    }
    match cx.ctx.worlds.create_layer(
        w,
        LayerId(n),
        LayerKind::Normal,
        cx.ctx.data,
        &cx.svc.event.flags,
        cx.ctx.rng.world(w),
    ) {
        Ok(()) => {}
        Err(e) => tracing::debug!(world, layer = n, error = %e, "/cl was refused"),
    }
    Ok(target(world, n))
}

/// Row 194 `/DeleteLayer <worldId> <layer>`: releases a GM layer; see the module documentation.
pub fn delete_layer(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let world = args.number() as u32;
    let n = args.number();
    let (w, id) = (WorldId(world), LayerId(n));
    if n <= 0 || cx.ctx.worlds.world(w).is_none() {
        return Ok(target(world, n));
    }
    let Some(kind) = cx.ctx.worlds.layer(w, id).map(|l| l.kind) else {
        return Ok(target(world, n));
    };
    if kind != LayerKind::Normal || !cx.ctx.worlds.has_nobody(w, id) {
        cx.out.text(LAYER_IN_USE);
        return Ok(target(world, n));
    }
    cx.ctx.worlds.invalidate(w, id);
    Ok(target(world, n))
}

/// Row 195 `/Layer <worldId> <layer> <x> <z>` (needs a character): the issuer is replaced into the layer.
pub fn layer(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let world = args.number() as u32;
    let n = args.number();
    let (w, id) = (WorldId(world), LayerId(n));
    if cx.ctx.worlds.world(w).is_none() {
        cx.out.text(UNDEFINED_WORLD);
        return Ok(target(world, n));
    }
    if cx.ctx.worlds.layer(w, id).is_none() {
        cx.out.text(LAYER_NO_EXISTS);
        return Ok(target(world, n));
    }
    let x = args.float();
    let z = args.float();
    // `x > 0 && z > 0` is false for NaN too
    if !(x > 0.0 && z > 0.0) {
        cx.out.text(OUT_OF_WORLD);
        return Ok(target(world, n));
    }
    match cx
        .ctx
        .worlds
        .replace(me.0, w, Vec3::new(x, 0.0, z), id, ReplaceKind::Normal)
    {
        Ok(()) => {}
        Err(ReplaceError::WorldMissing) => cx.out.text(UNDEFINED_WORLD),
        Err(ReplaceError::LayerMissing) => cx.out.text(LAYER_NO_EXISTS),
        Err(ReplaceError::OutOfWorld) => cx.out.text(OUT_OF_WORLD),
        // a GM in a world cannot reach these: nothing to tell the player
        Err(e @ (ReplaceError::NotInWorld | ReplaceError::FlyingNotAllowed)) => {
            tracing::debug!(world, layer = n, error = %e, "/lay was refused");
        }
    }
    Ok(target(world, n))
}
