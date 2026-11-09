//! Regions: attributes, portals, `ReplaceInspection`, dynamic regions (Plan/15 §11, WP-WORLD-11; ref/06 §13;
//! F-086, F-101).
//!
//! * **Evaluation** (§11.2, `CMover::UpdateRegionAttr`, players only, in the per-mover tick tail after the
//!   tick's position write-backs and only when the position changed): the attributes of every region whose
//!   rect contains `((int)x, (int)z)` (half-open: `left <= x < right`, `top <= z < bottom`) are OR-ed; the
//!   **last** containing portal (`tele_world != 0`) wins. If the OR contains `SAFETY`, `PENALTY_PK`, `PK` or
//!   `FIGHT` it is the player's region attribute, otherwise the world's `pkmode`.
//! * **Portals and `ReplaceInspection`** (§11.3): a portal found in this evaluation is taken unless the player
//!   is flying. A target in an instance dungeon goes through doc 22's hooks, any other target needs
//!   [`replace_inspection`] and then `replace(tele_world, tele_pos, Normal, layer 0)`. A player standing still
//!   in a portal region is not re-teleported (no evaluation without a position change).
//! * **Region-class change** (§11.4): [`is_region_move`] -> `WorldHooks::region_class_changed` (doc 17 cancels
//!   duels).
//! * **Dynamic regions** (§11.5): [`Worlds::add_dynamic_region`] / [`Worlds::remove_dynamic_region`]; they take
//!   part in the evaluation after the static regions.

use super::Worlds;
use super::object::MoverKey;
use super::phases::PhaseEnv;
use super::replace::ReplaceKind;
use crate::mover::Mover;
use flyff_common::defs::define::{RA_FIGHT, RA_PENALTY_PK, RA_PK, RA_SAFETY};
use flyff_common::math::Vec3;
use flyff_common::{LayerId, WorldId};
use flyff_data::worlds::RegionDef;

/// `RA_SAFETY | RA_PENALTY_PK | RA_PK | RA_FIGHT`: the attributes that become the player's region attribute.
pub const REGION_CHECK_MASK: u32 = RA_SAFETY | RA_PENALTY_PK | RA_PK | RA_FIGHT;

/// The outcome of one evaluation: the new attribute and the portal found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionEval<'a> {
    /// The attribute the player gets.
    pub attr: u32,
    /// The last containing region with a teleport target.
    pub portal: Option<&'a RegionDef>,
}

/// `UpdateRegionAttr`'s computation over the static regions then the dynamic ones.
pub fn evaluate<'a>(
    statics: &'a [RegionDef],
    dynamic: &'a [RegionDef],
    x: f32,
    z: f32,
    pk_mode: u32,
) -> RegionEval<'a> {
    let mut attr = 0;
    let mut portal = None;
    for r in statics.iter().chain(dynamic.iter()) {
        if r.contains(x, z) {
            attr |= r.attribute;
            if r.tele_world != 0 {
                portal = Some(r);
            }
        }
    }
    RegionEval {
        attr: if attr & REGION_CHECK_MASK != 0 {
            attr
        } else {
            pk_mode
        },
        portal,
    }
}

/// `IsRegionAttr(flags)` on a stored attribute: all of `flags` are set.
pub fn is_region_attr(attr: u32, flags: u32) -> bool {
    attr & flags == flags
}

/// `GetPKPVPRegionAttr`: FIGHT > PENALTY_PK > PK > SAFETY (the default).
pub fn pkpvp_region_attr(attr: u32) -> u32 {
    if is_region_attr(attr, RA_FIGHT) {
        RA_FIGHT
    } else if is_region_attr(attr, RA_PENALTY_PK) {
        RA_PENALTY_PK
    } else if is_region_attr(attr, RA_PK) {
        RA_PK
    } else {
        RA_SAFETY
    }
}

/// `IsRegionMove(old, new)` (`Mover.cpp:2768-2800`): checks, in this order, whether `old` had SAFETY, else
/// PENALTY_PK, else PK, else FIGHT, and the new attribute lacks that same flag.
pub fn is_region_move(old: u32, new: u32) -> bool {
    for flag in [RA_SAFETY, RA_PENALTY_PK, RA_PK, RA_FIGHT] {
        if is_region_attr(old, flag) {
            return !is_region_attr(new, flag);
        }
    }
    false
}

/// The player's region attribute (`m_dwRegionAttr`); 0 for a non-player.
pub fn region_attr_of(m: &Mover) -> u32 {
    m.player().map_or(0, |p| p.region_attr)
}

/// `GetItemNum(item)`: the stack counts of every inventory entry of the item (equipment included).
fn item_count(m: &Mover, item: u32) -> u32 {
    m.player().map_or(0, |p| p.items.inventory.count_of(item))
}

/// `CMover::ReplaceInspection` (`Mover.cpp:2164-2240`): every condition that is set must pass; a field equal
/// to `-1` (`0xFFFF_FFFF`) means "no condition".
///
/// | field | test |
/// |---|---|
/// | `item` | `count_of(item) <= item_count` (**KEEP-QUIRK**: "at most", looks inverted; every shipped region3 line has -1) |
/// | `min_level` | `min_level <= level <= max_level` (unsigned compare: `max_level = -1` is no maximum) |
/// | `quest` | the quest is active and (`quest_flag == 0` or its state equals `quest_flag`) |
/// | `job` | `job == player.job` |
/// | `gender` | `gender == player.sex` |
/// | `check_party` | the player is a member of a party (doc 21) |
/// | `check_guild` | the player is a member of a guild (doc 21) |
pub fn replace_inspection(m: &Mover, region: &RegionDef) -> bool {
    let req = region.req_or_legacy();
    if req.item != u32::MAX && item_count(m, req.item) > req.item_count {
        return false;
    }
    if req.min_level != u32::MAX {
        let level = u32::try_from(m.mv.level).unwrap_or(0);
        if !(req.min_level <= level && level <= req.max_level) {
            return false;
        }
    }
    if req.quest != -1 {
        let state = m.player().and_then(|p| {
            p.quests
                .doc
                .active
                .iter()
                .find(|q| i32::from(q.quest_id) == req.quest)
                .map(|q| i32::from(q.state))
        });
        match state {
            Some(s) if req.quest_flag == 0 || req.quest_flag == s => {}
            _ => return false,
        }
    }
    if req.job != -1 && req.job != m.mv.job {
        return false;
    }
    if req.gender != -1 && m.player().map(|p| i32::from(p.appearance.sex)) != Some(req.gender) {
        return false;
    }
    if req.check_party && m.player().is_none_or(|p| p.social.party.is_none()) {
        return false;
    }
    if req.check_guild && m.player().is_none_or(|p| p.social.guild.is_none()) {
        return false;
    }
    true
}

impl Worlds {
    /// F-101: adds a region to a world's dynamic list; it takes part in the attribute evaluation after the
    /// static regions. Returns its index, `None` for an unknown world.
    pub fn add_dynamic_region(&mut self, world: WorldId, region: RegionDef) -> Option<usize> {
        let w = self.world_mut(world)?;
        w.dynamic_regions.push(region);
        Some(w.dynamic_regions.len() - 1)
    }

    /// Removes the dynamic region `idx` (later indices shift down); `false` when there is none.
    pub fn remove_dynamic_region(&mut self, world: WorldId, idx: usize) -> bool {
        match self.world_mut(world) {
            Some(w) if idx < w.dynamic_regions.len() => {
                w.dynamic_regions.remove(idx);
                true
            }
            _ => false,
        }
    }

    /// `CMover::ProcessRegion` for the player `key` (15 §11.2-§11.4), called after the mover's tick tail.
    pub(crate) fn process_region(
        &mut self,
        w: WorldId,
        l: LayerId,
        key: MoverKey,
        env: &mut PhaseEnv<'_>,
    ) {
        let Some(m) = self.store.movers.get(key) else {
            return;
        };
        if !m.is_player() || !m.core.pos_changed || m.core.deleted {
            return;
        }
        let id = m.core.id;
        let pos = m.core.pos;
        let flying = m.sim.act.flags.fly();
        let Some(world) = self.world(w) else {
            return;
        };
        let ev = evaluate(
            &world.statics.regions,
            &world.dynamic_regions,
            pos.x,
            pos.z,
            world.statics.settings.pk_mode,
        );
        let (attr, portal) = (ev.attr, ev.portal.cloned());
        let Some(m) = self.store.movers.get_mut(key) else {
            return;
        };
        m.core.pos_changed = false;
        let Some(p) = m.player_mut() else {
            return;
        };
        let old = p.region_attr;
        p.old_region_attr = old;
        p.region_attr = attr;
        if is_region_move(old, attr) {
            env.hooks
                .region_class_changed(&mut self.store, id, old, attr);
        }
        let Some(portal) = portal else {
            return;
        };
        if flying {
            return;
        }
        if env.hooks.is_instance_world(WorldId(portal.tele_world)) {
            let in_instance = env.hooks.is_instance_world(w);
            if in_instance {
                let _ = self.replace(
                    id,
                    WorldId(portal.tele_world),
                    vec3(portal.tele_pos),
                    l,
                    ReplaceKind::Normal,
                );
            } else {
                env.hooks.instance_enter(&mut self.store, id, &portal);
            }
            return;
        }
        let ok = self
            .store
            .movers
            .get(key)
            .is_some_and(|m| replace_inspection(m, &portal));
        if !ok {
            return;
        }
        if let Err(e) = self.replace(
            id,
            WorldId(portal.tele_world),
            vec3(portal.tele_pos),
            LayerId(0),
            ReplaceKind::Normal,
        ) {
            tracing::debug!(player = id.0, "portal refused: {e}");
        }
    }
}

fn vec3(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}
