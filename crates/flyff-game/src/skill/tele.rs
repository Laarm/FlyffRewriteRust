//! `TeleSkill`, the Blinkpool jump (Plan/17 §6.14; WP-SKILL-10; `CDPSrvr::OnTeleSkill`, `DPSrvr.cpp:4302-4327`).
//!
//! The ordinary `UseSkillReq` of `SI_MAG_MAG_BLINKPOOL` paid the MP and set the cool-down; at its own hit frame
//! the client sends `TeleSkill{pos}`. The server needs the skill at a level above 0 (the stored level, as the
//! original reads `pSkill->dwLevel`), and then replaces the player at `pos` in the same world and layer.
//!
//! * **VR-082**: a position that is not finite or lies outside the world is refused (the replace pipeline's
//!   bounds test, `Worlds::replace`).
//! * A guild-quest region on either end (the file is empty in this distribution) demands that both ends lie on
//!   the ground: `|land_height(p) - p.y| <= 1.0`.
//! * No range and no line-of-sight check (NA-05, NA-04): a client can jump anywhere inside the world, exactly as
//!   the original allowed.

use super::use_skill::slot_of;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::world::ReplaceKind;
use flyff_common::defs::skill::SI_MAG_MAG_BLINKPOOL;
use flyff_common::math::Vec3;
use flyff_proto::pb;

/// Why a `TeleSkill` was ignored (for the tests; the client is told nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeleRefusal {
    /// The player does not know Blinkpool (level 0 or no slot).
    NoSkill,
    /// `pos` is missing, not finite, or outside the world (VR-082).
    OutOfWorld,
    /// A guild-quest region is involved and an end is off the ground.
    GuildQuestGround,
    /// The replace pipeline refused (a flying player, a missing layer).
    Replace,
}

/// `OnTeleSkill`: see the module documentation.
pub fn on_tele_skill(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::TeleSkill) {
    let _ = tele(s, ctx, who, m);
}

/// [`on_tele_skill`] with the outcome.
pub fn tele(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::TeleSkill,
) -> Result<(), TeleRefusal> {
    let me = who.0;
    let Some(pl) = ctx.worlds.player(me) else {
        return Err(TeleRefusal::NoSkill);
    };
    let known = slot_of(pl, SI_MAG_MAG_BLINKPOOL)
        .and_then(|slot| pl.player()?.progression.job_skills.get(slot))
        .is_some_and(|sk| sk.level > 0);
    if !known {
        return Err(TeleRefusal::NoSkill);
    }
    let Some(p) = m.pos else {
        return Err(TeleRefusal::OutOfWorld);
    };
    let pos = Vec3::new(p.x, p.y, p.z);
    if !(pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite()) {
        return Err(TeleRefusal::OutOfWorld);
    }
    let (here, world, layer) = (pl.core.pos, pl.core.world, pl.core.layer);
    // the guild-quest ground rule
    if s.skill.is_guild_quest_region(here) || s.skill.is_guild_quest_region(pos) {
        let Some(st) = ctx.worlds.statics_of(me) else {
            return Err(TeleRefusal::NoSkill);
        };
        let off = |v: Vec3| (st.land_height(v.x, v.z) - v.y).abs() > 1.0;
        if off(here) || off(pos) {
            return Err(TeleRefusal::GuildQuestGround);
        }
    }
    ctx.worlds
        .replace(me, world, pos, layer, ReplaceKind::Normal)
        .map_err(|e| match e {
            crate::world::ReplaceError::OutOfWorld => TeleRefusal::OutOfWorld,
            _ => TeleRefusal::Replace,
        })
}
