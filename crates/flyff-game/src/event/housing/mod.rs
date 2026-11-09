//! `event::housing`: personal housing, the mini room (Plan/22 §13, WP-EVENT-16; F-290).
//!
//! A character's room is the layer `LayerId(character id)` of world 209 ([`ROOM_WORLD`], `LayerKind::MiniRoom`),
//! created when the owner joins and `invalidate`d when he leaves. The list of furniture and papering items
//! ([`Housing::items`]) is the client's list (its order is the client's index); an entry is *listed* by using the item
//! (a `HousingFurnitureAdd` batch), *installed* by `HousingSetupFurnitureReq` (a ctrl at the position, or a wall or floor
//! paper for everybody in the room, and the item's buff on the owner) and *expires* at its `keep_until`.
//!
//! | module | content |
//! |---|---|
//! | `mod.rs` | [`HousingService`] (`Services.event.housing`), [`Housing`], [`HousingItem`], the constants, the read API, the P7 [`minute`] and [`remove_entry`] |
//! | `room.rs` | the load normalisation and the layer ([`on_join`]), the buffs after the join, the logout, the drive-out, the ctrls and the papering |
//! | `requests.rs` | the item use ([`use_row`], [`list_up`]), the five client messages, the messenger and deletion seams |
//! | `join.rs` | [`HousingAllInfoStep`] (`JoinStep::HousingAllInfo`) |
//! | `gm.rs` | the seam of gm-content-control: [`gm_remove_all`], `room_line`, `state_line` |
//!
//! State is memory plus the tables `housing_furniture` and `housing_visitors` (the `0007` group of `flyff-db`); all
//! containers are ordered (determinism), the wall clock is read from `ctx.clock` only.
//!
//! **Deviations from the original** (all in `crates/flyff-game/NOTES.md`, "Personal housing"): the room is built from
//! `CharacterLoad.housing` at the join (a `LoadHook` has no `Services`); the list-up waits for its ticket (`pending`,
//! the legacy `m_bSetting`) and a failed batch re-creates the item (the legacy `OnHousingDBFailed`); a re-login while the
//! previous layer still drains revalidates it; unfriending writes the visitor delete for offline owners too;
//! `HousingGoOut` works in world 209 only (VR-062); `HousingAllInfo` is sent for a non-empty room only.

pub mod gm;
pub mod join;
pub mod requests;
pub mod room;

pub use gm::gm_remove_all;
pub use join::HousingAllInfoStep;
pub use requests::{
    friendship_ended, list_up, on_character_deleted, on_go_out, on_req_visitable_list,
    on_set_visit_allow, on_setup_furniture, on_visit_room, use_row, visit,
};
pub use room::{
    after_join, after_reattach, drive_out, go_out, on_join, on_logout, on_room_entered,
};

use crate::ctx::{GameCtx, Services};
use flyff_common::defs::world::WI_WORLD_MINIROOM;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, LayerId, ObjId, UnixTime, WorldId};
use flyff_db::{DbOp, HousingFurnitureRecord};
use flyff_proto::pb;
use std::collections::{BTreeMap, BTreeSet};

/// The world of the rooms (`WI_WORLD_MINIROOM`, 209).
pub const ROOM_WORLD: WorldId = WorldId(WI_WORLD_MINIROOM);
/// `CHousing::IsListUpAble`: a list holds at most 99 entries.
pub const MAX_LIST: usize = 99;
/// Where a visitor lands in a room (`CHousingMng::SetVisitRoom`).
pub const ROOM_ENTRY: Vec3 = Vec3::new(253.0, 105.0, 243.0);
/// The angle a visitor faces on entering.
pub const ROOM_ENTRY_ANGLE: f32 = 175.0;
/// Where `GoOut` puts a player (Madrigal, layer 0, next to the room NPC).
pub const GO_OUT: Vec3 = Vec3::new(6985.0, 0.0, 3234.0);
/// The angle a player faces after `GoOut`.
pub const GO_OUT_ANGLE: f32 = 50.0;
/// `HousingPaperingInfo.item_id` of the reset (`NULL_ID`): wall and floor back to the default.
pub const PAPERING_RESET: u32 = 0xFFFF_FFFF;

/// The layer of `owner`'s room: the character id (legacy `static_cast<int>(m_dwMasterId)`). An id that does not fit a
/// positive `i32` has no room (layer 0 is the world's own and is never created).
pub fn room_layer(owner: CharacterId) -> Option<LayerId> {
    i32::try_from(owner.0).ok().filter(|&l| l > 0).map(LayerId)
}

/// The owner of the room on `layer` (the inverse of [`room_layer`]).
pub fn owner_of_layer(layer: LayerId) -> Option<CharacterId> {
    u32::try_from(layer.0)
        .ok()
        .filter(|&c| c > 0)
        .map(CharacterId)
}

/// One entry of a room's list (`HOUSINGINFO` plus the server's bookkeeping).
#[derive(Clone, Debug, PartialEq)]
pub struct HousingItem {
    /// The item id (`propItem`).
    pub item: u32,
    /// When the entry expires (absolute unix seconds; the server's value, never the client's).
    pub keep_until: UnixTime,
    /// Standing in the room (a ctrl at `pos`) or, for a paper, shown on the walls and the floor.
    pub installed: bool,
    /// The position the owner installed it at (the stored one while it is not installed).
    pub pos: Vec3,
    /// The angle it was installed with, in degrees.
    pub angle: f32,
    /// When the entry was listed (never rewritten).
    pub listed_at: UnixTime,
    /// The ctrl of an installed piece of furniture (`m_vecHousingInfo[i].objId`).
    pub obj: Option<ObjId>,
    /// `true` while the `HousingFurnitureAdd` batch of the list-up has not answered: the entry is not announced,
    /// installed, expired or removed.
    pub pending: bool,
    /// The item is a wall or floor paper (`IK2_PAPERING`): installing it shows a paper, it never makes a ctrl.
    /// Cached from the prop so that [`HousingService::papering_of`] needs no game data.
    pub paper: bool,
}

impl HousingItem {
    /// The wire form (`HOUSINGINFO`, `expire_at` absolute).
    pub fn info(&self) -> pb::HousingInfo {
        pb::HousingInfo {
            item_id: self.item,
            expire_at: self.keep_until.0,
            setup: self.installed,
            pos: Some(crate::movement::intents::pb_vec(self.pos)),
            angle: self.angle,
        }
    }

    /// The row of the table (`housing_furniture`) of `owner`.
    pub fn record(&self, owner: CharacterId) -> HousingFurnitureRecord {
        HousingFurnitureRecord {
            character_id: owner,
            item_id: self.item,
            keep_until: self.keep_until,
            installed: self.installed,
            pos: [self.pos.x, self.pos.y, self.pos.z],
            angle: self.angle,
            listed_at: self.listed_at,
        }
    }
}

/// One loaded room (`CHousing`): the list in the client's order, who may visit, and the list-up lock.
#[derive(Clone, Debug, PartialEq)]
pub struct Housing {
    /// The owner (`m_dwMasterId`).
    pub owner: CharacterId,
    /// The list; its order is the client's index.
    pub items: Vec<HousingItem>,
    /// The messenger friends who may enter.
    pub visitors: BTreeSet<CharacterId>,
    /// A list-up is in flight (the legacy `m_bSetting`): further list-ups and setups are ignored.
    pub pending: bool,
}

impl Housing {
    /// An empty room of `owner`.
    pub fn new(owner: CharacterId) -> Housing {
        Housing {
            owner,
            items: Vec::new(),
            visitors: BTreeSet::new(),
            pending: false,
        }
    }

    /// The index of the entry of `item` (`GetIndexFromList`).
    pub fn index_of(&self, item: u32) -> Option<usize> {
        self.items.iter().position(|i| i.item == item)
    }

    /// The entry of `item`.
    pub fn item(&self, item: u32) -> Option<&HousingItem> {
        self.items.iter().find(|i| i.item == item)
    }

    /// `IsAllowVisit`: the owner always, otherwise the characters on the visitor list.
    pub fn is_allowed(&self, visitor: CharacterId) -> bool {
        visitor == self.owner || self.visitors.contains(&visitor)
    }

    /// How many entries stand in the room (installed).
    pub fn installed_count(&self) -> usize {
        self.items.iter().filter(|i| i.installed).count()
    }

    /// How many entries wait for their list-up batch.
    pub fn pending_count(&self) -> usize {
        self.items.iter().filter(|i| i.pending).count()
    }
}

/// The housing service (`CHousingMng`, `Services.event.housing`): the rooms of the characters who are in the world
/// (the 10 s linger included), by owner.
#[derive(Debug, Default)]
pub struct HousingService {
    rooms: BTreeMap<CharacterId, Housing>,
}

impl HousingService {
    /// The room of `owner` while he is in the world.
    pub fn room(&self, owner: CharacterId) -> Option<&Housing> {
        self.rooms.get(&owner)
    }

    /// The loaded rooms in owner order.
    pub fn rooms(&self) -> impl Iterator<Item = &Housing> + '_ {
        self.rooms.values()
    }

    /// How many rooms are loaded.
    pub fn len(&self) -> usize {
        self.rooms.len()
    }

    /// `true` without a loaded room.
    pub fn is_empty(&self) -> bool {
        self.rooms.is_empty()
    }

    /// `IsAllowVisit` of the room of `owner`: `false` while it is not loaded, `true` for the owner himself.
    pub fn is_allowed(&self, owner: CharacterId, visitor: CharacterId) -> bool {
        self.rooms
            .get(&owner)
            .is_some_and(|r| r.is_allowed(visitor))
    }

    /// The rooms `c` may visit (`m_mapVisitable`): the loaded rooms whose visitor list holds `c`, ascending by owner.
    pub fn visitable_for(&self, c: CharacterId) -> Vec<CharacterId> {
        self.rooms
            .values()
            .filter(|r| r.visitors.contains(&c))
            .map(|r| r.owner)
            .collect()
    }

    /// `GetAllPaperingInfo`: the installed papers of `owner`'s room in list order.
    pub fn papering_of(&self, owner: CharacterId) -> Vec<u32> {
        self.rooms.get(&owner).map_or_else(Vec::new, |r| {
            r.items
                .iter()
                .filter(|i| i.installed && i.paper && !i.pending)
                .map(|i| i.item)
                .collect()
        })
    }

    pub(crate) fn rooms_mut(&mut self) -> &mut BTreeMap<CharacterId, Housing> {
        &mut self.rooms
    }
}

/// P7 `event::housing::minute` (22 §13.3): every minute the entries of the loaded rooms whose `keep_until` has come are
/// removed (owner order, list order). The comparison uses the wall clock, so an entry goes at the first run after its
/// time, at most 60 s late. The rooms of owners who are offline are handled when they load (the same effect as the
/// legacy DB timer, 13 §12).
pub fn minute(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.unix_ms.to_unix_time();
    let due: Vec<(CharacterId, u32)> = s
        .event
        .housing
        .rooms()
        .flat_map(|r| {
            r.items
                .iter()
                .filter(|i| !i.pending && i.keep_until <= now)
                .map(|i| (r.owner, i.item))
        })
        .collect();
    for (owner, item) in due {
        remove_entry(s, ctx, owner, item);
    }
}

/// Removes the entry of `item` from `owner`'s loaded room (`CHousing::RemoveFurnitureList` and the DB delete): an
/// installed piece is taken down first (its ctrl deleted, a paper shown away to the room, the buff removed; no
/// `HousingSetupFurnitureNtf`), the row is deleted and the owner, if he is in the world, is told with
/// `HousingFurnitureList{add: false}`. Returns `false` when the room or the entry is not there.
pub fn remove_entry(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    owner: CharacterId,
    item: u32,
) -> bool {
    let Some(room) = svc.event.housing.rooms.get_mut(&owner) else {
        return false;
    };
    let Some(index) = room.index_of(item) else {
        return false;
    };
    let entry = room.items.remove(index);
    if entry.installed {
        let avatar = room::avatar_of(svc, ctx, owner);
        room::take_down(svc, ctx, owner, avatar, &entry);
    }
    ctx.persist.op(DbOp::DeleteHousingFurniture {
        character_id: owner,
        item_id: item,
    });
    room::tell_owner(
        svc,
        ctx,
        owner,
        pb::HousingFurnitureList {
            info: Some(entry.info()),
            add: false,
        },
    );
    true
}
