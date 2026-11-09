//! Monsters the AI creates: summons, transformations and (later) boss minions (Plan/18 §8.4 `MonsterSpawn`,
//! §9.9, §9.11; WP-AI-11).
//!
//! A handler cannot create a mover (it only holds the acting one): it queues an [`AiCmd::Spawn`](crate::ai::AiCmd::Spawn) and the object-creating step of the phase executes it right after the hook returns
//! ([`crate::world::phases::FrameCtx::spawn`], `ADDOBJ` of the original: the mover is linked by the `_add` of the
//! layer's next tick). A transformation raised by the damage pipeline travels the same way through the effects of
//! the damage call ([`crate::combat::Effects`]).

use crate::mover::{Mover, monster_parts};
use crate::world::Worlds;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{LayerId, ObjId, WorldId};
use flyff_data::GameData;
use flyff_data::types::MoverId;

/// One monster to create (18 §8.4 `MonsterSpawn`).
#[derive(Clone, Debug, PartialEq)]
pub struct MonsterSpawn {
    /// The mover prop (`CreateObj(OT_MOVER, id)`).
    pub mover: MoverId,
    /// The world it appears in.
    pub world: WorldId,
    /// The layer it appears in (`ADDOBJ(.., pMover->GetLayer())`).
    pub layer: LayerId,
    /// Where (`SetPos`).
    pub pos: Vec3,
    /// `m_bActiveAttack`: summons, transformations and boss minions attack on sight.
    pub active_attack: bool,
    /// `SetGold`: `None` leaves the monster without gold (summons carry none, Q17).
    pub gold: Option<i32>,
    /// The summoner whose `summons` list learns the new id (`m_vecIdSummon`).
    pub owner_ai: Option<ObjId>,
}

impl MonsterSpawn {
    /// A spawn of `mover` at `pos` in the layer of the summoner, aggressive, without gold, owned by nobody.
    pub fn aggressive(mover: MoverId, world: WorldId, layer: LayerId, pos: Vec3) -> MonsterSpawn {
        MonsterSpawn {
            mover,
            world,
            layer,
            pos,
            active_attack: true,
            gold: None,
            owner_ai: None,
        }
    }
}

/// `CreateObj(OT_MOVER, id)` + the fields the creator sets: the mover of `s`, or `None` for an unknown prop (the
/// original logs `Error` and gives up).
pub fn build(data: &GameData, rng: &mut dyn GameRng, s: &MonsterSpawn) -> Option<Mover> {
    let prop = data.mover(s.mover)?;
    let (core, sim) = monster_parts(data, prop, s.pos, 0.0);
    let mut m = Mover::new_monster(core, sim, prop, rng);
    if let Some(ext) = m.monster_mut() {
        ext.active_attack = s.active_attack;
        if let Some(g) = s.gold {
            ext.gold = g;
        }
    }
    Some(m)
}

/// Creates the monster of `s` in `worlds` (queued for the `_add` of its layer) and tells its summoner. The new
/// id, `None` when the prop is unknown or the position refused (outside the world, no such layer).
pub fn commit(
    worlds: &mut Worlds,
    data: &GameData,
    rng: &mut dyn GameRng,
    s: &MonsterSpawn,
) -> Option<ObjId> {
    let m = build(data, rng, s)?;
    let id = match worlds.spawn_mover(s.world, s.layer, m) {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(mover = s.mover, ?e, "an AI spawn was refused");
            return None;
        }
    };
    if let Some(owner) = s.owner_ai
        && let Some(brain) = worlds
            .mover_mut(owner)
            .and_then(Mover::monster_mut)
            .and_then(|e| e.ai.monster_mut())
    {
        brain.summons.push(id);
    }
    Some(id)
}
