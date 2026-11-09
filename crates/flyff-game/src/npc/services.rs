//! The NPC service messages owned by doc 20 (Plan/20 §8.5, §8.6): `Teleporter`, `NpcBuff` and `Tele16`.
//!
//! This document owns the handlers and the NPC checks, the rules belong to others (D20-16):
//!
//! * `Teleporter`: implemented here (VR-004, VR-012, free replace to Madrigal),
//! * `NpcBuff`: the key length, the NPC definition and the VR-012 check run here, then the call goes to doc 17's
//!   `buff::npc::on_npc_buff` through the [`NpcService::on_npc_buff`] hook,
//! * `Tele16`: forwarded unchanged to doc 15's `world::tele16::handle` through the [`NpcService::on_tele16`] hook
//!   (WP-WORLD-19); this module runs no check of its own.
//!
//! The two hooks default to the real rules (`buff::npc::on_npc_buff`, `world::tele16::handle`); a test replaces one
//! with a probe (`svc.npc.on_npc_buff = probe`). [`no_buff_rules`] and [`no_tele16_rules`] are the log-only stand-ins.

use super::NpcService;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::defs::neuz::{MMI_NPC_BUFF, MMI_TELEPORTER};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::{LayerId, WorldId};
use flyff_proto::pb;

/// Longest `character_key` of `NpcBuff` and `Teleporter` (the legacy buffer is 64).
pub const MAX_KEY_LEN: usize = 63;

/// The body of doc 17's `buff::npc::on_npc_buff(player, character_key)`.
pub type NpcBuffHook = fn(&mut Services, &mut GameCtx<'_>, PlayerRef, &str);

/// The body of doc 15's `world::tele16::handle(player, index)`.
pub type Tele16Hook = fn(&mut Services, &mut GameCtx<'_>, PlayerRef, u32);

/// Doc 19's eat-pet deactivation as the Tele16 handler needs it (`InactivateEatPet`).
pub type EatPetHook = fn(&mut Services, &mut GameCtx<'_>, PlayerRef);

/// The default of [`NpcService::deactivate_eat_pet`]: doc 19's `InactivateEatPet` (the owner's pet effects go and
/// the pet mover is deleted, `item::pets::eat::dismiss`).
pub fn dismiss_eat_pet(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    crate::item::pets::eat::dismiss(svc, ctx, who);
}

/// The default of [`NpcService::on_npc_buff`] before the service is built: the buff rules (doc 17, WP-SKILL-03)
/// are not installed. Kept for tests that replace the hook.
pub fn no_buff_rules(_svc: &mut Services, _ctx: &mut GameCtx<'_>, who: PlayerRef, key: &str) {
    tracing::debug!(
        player = who.0.0,
        key,
        "NpcBuff: no buff rules are installed"
    );
}

/// The default of [`NpcService::on_tele16`]: the teleporter window rules (doc 15, WP-WORLD-19) are not installed.
pub fn no_tele16_rules(_svc: &mut Services, _ctx: &mut GameCtx<'_>, who: PlayerRef, index: u32) {
    tracing::debug!(
        player = who.0.0,
        index,
        "Tele16: no teleporter rules are installed"
    );
}

/// `NpcBuff{character_key}` (Plan/20 §8.5): key length, NPC definition, **VR-012** (an `MMI_NPC_BUFF` NPC within
/// range in the player's world and layer: the original applied the class buffs without any check), then the
/// buff rules. Everything else is dropped silently.
pub fn on_npc_buff(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::NpcBuff) {
    if m.character_key.len() > MAX_KEY_LEN || ctx.data.npc(&m.character_key).is_none() {
        return;
    }
    let Some(p) = ctx.worlds.player(who.0) else {
        return;
    };
    let (world, layer, pos) = (p.core.world, p.core.layer, p.core.pos);
    if ctx
        .worlds
        .close_npc(MMI_NPC_BUFF, world, layer, pos)
        .is_none()
    {
        return;
    }
    let hook = svc.npc.on_npc_buff;
    hook(svc, ctx, who, &m.character_key);
}

/// `Teleporter{character_key, index}` (Plan/20 §8.6): the NPC must exist, an `MMI_TELEPORTER` NPC must be within
/// range (VR-012), `0 <= index < teleports.len()` (VR-004: a negative index is rejected); then a free replace to
/// Madrigal at `(x, 0, z)` of `AddTeleport(x, z)`, layer 0, normal.
pub fn on_teleporter(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::Teleporter) {
    if m.character_key.len() > MAX_KEY_LEN {
        return;
    }
    let Some(def) = ctx.data.npc(&m.character_key) else {
        return;
    };
    let Ok(index) = usize::try_from(m.index) else {
        return;
    };
    let Some(&[x, z]) = def.teleports.get(index) else {
        return;
    };
    let Some(p) = ctx.worlds.player(who.0) else {
        return;
    };
    let (world, layer, pos) = (p.core.world, p.core.layer, p.core.pos);
    if ctx
        .worlds
        .close_npc(MMI_TELEPORTER, world, layer, pos)
        .is_none()
    {
        return;
    }
    // y = 0 as the original; the replace puts the mover on the ground of the target
    if let Err(e) = ctx.worlds.replace(
        who.0,
        WorldId(WI_WORLD_MADRIGAL),
        Vec3::new(x, 0.0, z),
        LayerId(0),
        crate::world::ReplaceKind::Normal,
    ) {
        tracing::debug!(player = who.0.0, error = %e, "Teleporter: the replace was refused");
    }
    let _ = svc;
}

/// `Tele16{index}` (Plan/20 §8.6): dispatch only, see the module documentation.
pub fn on_tele16(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::Tele16) {
    let hook = svc.npc.on_tele16;
    hook(svc, ctx, who, m.index);
}

impl NpcService {
    /// Installs doc 17's `NpcBuff` rules.
    pub fn with_buff_rules(mut self, f: NpcBuffHook) -> Self {
        self.on_npc_buff = f;
        self
    }

    /// Installs doc 15's `Tele16` rules.
    pub fn with_tele16_rules(mut self, f: Tele16Hook) -> Self {
        self.on_tele16 = f;
        self
    }
}
