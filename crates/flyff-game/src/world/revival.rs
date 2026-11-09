//! Revival point selection (Plan/15 §12.1-§12.3, WP-WORLD-15; ref/06 §9.8; F-087).
//!
//! [`RevivalIndex`] is built from `GameData.worlds.revival` (`CWorldMng::LoadRevivalPos`: every region with the
//! index `RI_REVIVAL` or `RI_PLACE` of every `World.inc` world, loaded or not; `target_key == false` regions are
//! the *points*, `target_key == true` the *areas*). Its four queries are exact ports of
//! `src:_Common/worldmng.cpp:560-660`; [`RevivalIndex::select`] is the shared part of `DestroyPlayer`
//! (`src:WORLDSERVER/User.cpp:4100-4138`) and of `OnRevivalLodestar` step 3. The result's layer is always 0
//! (`nRevivalLayer`). Doc 17's lodestar handler and the logout of a dead player (`world::logout`) call it.

use flyff_common::WorldId;
use flyff_common::math::{Vec3, ftoi_x86};
use flyff_data::worlds::{RI_REVIVAL, RegionDef, RevivalTables, WldSettings};

/// `MADRIGAL_START` (15 §12.3): the join fallback and the ticket fallback.
pub const MADRIGAL_START: Vec3 = Vec3::new(6971.984, 100.0, 3336.884);
/// `MADRIGAL_FLARIS_CENTER`: logout from 151 / 209 / instances without a Madrigal mark.
pub const MADRIGAL_FLARIS_CENTER: Vec3 = Vec3::new(6968.0, 100.0, 3328.0);
/// `MADRIGAL_DRIVE_OUT`: layer drive-out, the GC1to1 join fallback.
pub const MADRIGAL_DRIVE_OUT: Vec3 = Vec3::new(6983.0, 0.0, 3330.0);

/// A revival point: the world it belongs to and its position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RevivalPoint {
    /// The world of the point (may differ from the world the player died in).
    pub world: WorldId,
    /// `vPos` of the region.
    pub pos: Vec3,
}

/// The global revival tables (`m_aRevivalPos` / `m_aRevivalRgn`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RevivalIndex {
    points: Vec<(WorldId, RegionDef)>,
    areas: Vec<(WorldId, RegionDef)>,
}

fn pos_of(r: &RegionDef) -> Vec3 {
    Vec3::new(r.pos[0], r.pos[1], r.pos[2])
}

/// `(long)D3DXVec3LengthSq(&(p - q))`.
fn dist_sq_long(p: Vec3, q: Vec3) -> i32 {
    let d = p - q;
    ftoi_x86(d.x * d.x + d.y * d.y + d.z * d.z)
}

impl RevivalIndex {
    /// The index of the loaded tables.
    pub fn new(t: &RevivalTables) -> RevivalIndex {
        RevivalIndex {
            points: t
                .points
                .iter()
                .map(|(w, r)| (WorldId(*w), r.clone()))
                .collect(),
            areas: t
                .areas
                .iter()
                .map(|(w, r)| (WorldId(*w), r.clone()))
                .collect(),
        }
    }

    /// Number of revival points.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// `true` without any point.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    fn point(&self, world: WorldId, key: &str, chao: bool) -> Option<RevivalPoint> {
        self.points
            .iter()
            .find(|(w, r)| *w == world && r.key == key && r.req_or_legacy().chao_key == chao)
            .map(|(w, r)| RevivalPoint {
                world: *w,
                pos: pos_of(r),
            })
    }

    /// `GetRevivalPos(world, key)`: the first point of `world` with that key (case-sensitive `strcmp`) and
    /// `!chao_key`.
    pub fn revival_pos(&self, world: WorldId, key: &str) -> Option<RevivalPoint> {
        self.point(world, key, false)
    }

    /// `GetRevivalPosChao(world, key)`: the same with `chao_key`.
    pub fn revival_pos_chao(&self, world: WorldId, key: &str) -> Option<RevivalPoint> {
        self.point(world, key, true)
    }

    /// `GetNearRevivalPos(world, pos)`: the first **area** of `world` (index `RI_REVIVAL`, `!chao_key`) whose
    /// rect contains `(int x, int z)` answers with its keyed point, which **may be `None`** (no fallback,
    /// KEEP-QUIRK); when no area matches, the point of `world` with `!chao_key` that is nearest (strict `<` on
    /// the `long`-cast squared distance, the first wins), else `None`.
    pub fn near_revival_pos(&self, world: WorldId, pos: Vec3) -> Option<RevivalPoint> {
        for (w, area) in &self.areas {
            if *w == world
                && area.index == RI_REVIVAL
                && !area.req_or_legacy().chao_key
                && area.contains(pos.x, pos.z)
            {
                return self.revival_pos(world, &area.key);
            }
        }
        let mut best: Option<(i32, RevivalPoint)> = None;
        for (w, r) in &self.points {
            if *w == world && !r.req_or_legacy().chao_key {
                let d = dist_sq_long(pos, pos_of(r));
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((
                        d,
                        RevivalPoint {
                            world: *w,
                            pos: pos_of(r),
                        },
                    ));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    /// `GetNearRevivalPosChao(world, pos)`: the nearest chao point of `world`; else the **first** chao point of
    /// any other world; else `None`.
    pub fn near_revival_pos_chao(&self, world: WorldId, pos: Vec3) -> Option<RevivalPoint> {
        let mut best: Option<(i32, RevivalPoint)> = None;
        for (w, r) in &self.points {
            if *w == world && r.req_or_legacy().chao_key {
                let d = dist_sq_long(pos, pos_of(r));
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((
                        d,
                        RevivalPoint {
                            world: *w,
                            pos: pos_of(r),
                        },
                    ));
                }
            }
        }
        if let Some((_, p)) = best {
            return Some(p);
        }
        self.points
            .iter()
            .find(|(w, r)| *w != world && r.req_or_legacy().chao_key)
            .map(|(w, r)| RevivalPoint {
                world: *w,
                pos: pos_of(r),
            })
    }

    /// `select_revival` (15 §12.2): with the world's `revival_world` / `revival_key` (a different, non-zero
    /// revival world uses the keyed lookup first) and the nearest-point fallback. `chaotic` is doc 17's
    /// `is_chaotic`. The layer of the result is always 0.
    pub fn select(
        &self,
        world: WorldId,
        settings: &WldSettings,
        pos: Vec3,
        chaotic: bool,
    ) -> Option<RevivalPoint> {
        let rw = settings.revival_world;
        let keyed = rw != 0 && rw != world.0;
        if chaotic {
            let first = if keyed {
                self.revival_pos_chao(WorldId(rw), &settings.revival_key)
            } else {
                None
            };
            first.or_else(|| self.near_revival_pos_chao(world, pos))
        } else {
            let first = if keyed {
                self.revival_pos(WorldId(rw), &settings.revival_key)
            } else {
                None
            };
            first.or_else(|| self.near_revival_pos(world, pos))
        }
    }
}
