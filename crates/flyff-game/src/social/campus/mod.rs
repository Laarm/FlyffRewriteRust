//! The campus: master and pupils (Plan/21 §12, WP-SOCIAL-22; `src:WORLDSERVER/CampusHelper.cpp`,
//! `src:_Common/Campus.cpp`, `src:WORLDSERVER/User.cpp:8880-8945`, `Resource/Campus.lua`).
//!
//! * [`CampusService`] holds the campuses, the member index and the campus POINTS of every character (the sole
//!   writer, 13 §9: an offline master is rewarded through `SetCampusPoints`, never through his snapshot);
//! * [`invite`]: `invite_able`, `CampusInviteReq`, `CampusAccept`, `CampusRefuse` and the `/campusinvite` command;
//! * [`members`]: removal (the master removes a pupil, a pupil leaves), graduation at level 75, dissolution and the
//!   deletion of a character;
//! * [`points`]: `add_points`, the level reward, the recovery of negative points and the exchange seam;
//! * [`buffs`]: the cadence step that applies `II_TS_BUFF_POWER_LOVE0n`;
//! * [`join`]: `CampusUpdate` in the join batch.
//!
//! `PlayerSocial.campus` and `.campus_point` are mirrors of the service refreshed on every change (and set at join,
//! they ride in `SpawnSelf`). Without `GameData.lua.campus` (`Campus.lua`) the whole system is inert: every request
//! is ignored and nobody gets a campus.

pub mod buffs;
pub mod invite;
pub mod join;
pub mod members;
pub mod points;

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::party::wire::to_one;
use crate::social::relation::avatar_of;
use flyff_common::{CharacterId, UnixTime};
use flyff_db::{CampusRole, StartupState};
use flyff_proto::pb;
use std::collections::{BTreeMap, BTreeSet};

pub use buffs::process;
pub use invite::cmd_invite;
pub use members::remove_member;

/// One campus (`CCampus`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Campus {
    /// `campuses.id`.
    pub id: u32,
    /// The master (`m_idMaster`).
    pub master: CharacterId,
    /// The pupils.
    pub pupils: BTreeSet<CharacterId>,
    /// When the campus was created.
    pub created_at: UnixTime,
    /// `m_nPreBuffLevel`: the buff level the master's buff was last applied for (per campus, KEEP).
    pub last_buff_level: i32,
}

impl Campus {
    /// The members, the master and the pupils, in character id order (the wire order of `CCampus::Serialize`).
    pub fn members(&self) -> Vec<(CharacterId, CampusRole)> {
        let mut v: Vec<(CharacterId, CampusRole)> = self
            .pupils
            .iter()
            .map(|p| (*p, CampusRole::Pupil))
            .collect();
        v.push((self.master, CampusRole::Master));
        v.sort_by_key(|(c, _)| *c);
        v
    }

    /// The character ids of the members in id order.
    pub fn member_ids(&self) -> Vec<CharacterId> {
        self.members().into_iter().map(|(c, _)| c).collect()
    }

    /// Number of members.
    pub fn len(&self) -> usize {
        self.pupils.len() + 1
    }

    /// Never empty (a campus has its master).
    pub fn is_empty(&self) -> bool {
        false
    }

    /// `IsMember`.
    pub fn is_member(&self, c: CharacterId) -> bool {
        self.master == c || self.pupils.contains(&c)
    }

    /// The role of `c` in this campus.
    pub fn role_of(&self, c: CharacterId) -> Option<CampusRole> {
        if self.master == c {
            Some(CampusRole::Master)
        } else if self.pupils.contains(&c) {
            Some(CampusRole::Pupil)
        } else {
            None
        }
    }

    /// `CCampus::Serialize` as the wire record: members in id order, level 1 = master, 2 = pupil.
    pub fn wire(&self) -> pb::Campus {
        pb::Campus {
            id_campus: self.id,
            id_master: self.master.0,
            members: self
                .members()
                .into_iter()
                .map(|(c, role)| pb::CampusMember {
                    id_player: c.0,
                    member_level: i32::from(role.code()),
                })
                .collect(),
        }
    }
}

/// What the purge changed in the campuses (the caller tells the online members).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CampusPurge {
    /// Campuses that vanished (the master or all but one member purged): the id and the members that are left.
    pub dissolved: Vec<(u32, Vec<CharacterId>)>,
    /// Campuses that lost a pupil and go on: their members get a fresh `CampusUpdate`.
    pub updated: Vec<u32>,
}

/// The campuses and the campus points (`Services.social.campus`).
#[derive(Clone, Debug, Default)]
pub struct CampusService {
    campuses: BTreeMap<u32, Campus>,
    by_member: BTreeMap<CharacterId, u32>,
    /// The campus points of every character with a non-zero balance (missing = 0).
    points: BTreeMap<CharacterId, i32>,
    next_id: u32,
    /// Stored campuses the load could not keep (no master, fewer than two members): the ops that remove them,
    /// queued by the first pass of the game.
    repairs: Vec<flyff_db::DbOp>,
    /// The warning about a missing `Campus.lua` was logged.
    inert_noted: bool,
}

impl CampusService {
    /// The campuses and points of `StartupState`. A campus without a master or with fewer than two members is not
    /// kept (the dissolution rule of 21 §12.3) and its removal is queued as a repair.
    pub fn load(startup: &StartupState) -> CampusService {
        let mut s = CampusService {
            next_id: startup.counters.next_campus_id.max(1),
            ..CampusService::default()
        };
        for load in &startup.campuses {
            let master = load.master().map(|m| m.character_id);
            let pupils: BTreeSet<CharacterId> = load
                .members
                .iter()
                .filter(|m| m.role == CampusRole::Pupil)
                .map(|m| m.character_id)
                .collect();
            let taken = load
                .members
                .iter()
                .any(|m| s.by_member.contains_key(&m.character_id));
            match master {
                Some(master) if !pupils.is_empty() && !taken => {
                    s.insert(Campus {
                        id: load.id,
                        master,
                        pupils,
                        created_at: load.created_at,
                        last_buff_level: 0,
                    });
                }
                _ => {
                    tracing::warn!(
                        campus = load.id,
                        "campus dropped at load: no master, no pupil or a member twice"
                    );
                    s.repairs
                        .push(flyff_db::DbOp::DeleteCampus { campus_id: load.id });
                    s.next_id = s.next_id.max(load.id.saturating_add(1));
                }
            }
        }
        for &(c, p) in &startup.campus_points {
            if p != 0 {
                s.points.insert(c, p);
            }
        }
        s
    }

    fn insert(&mut self, c: Campus) {
        self.next_id = self.next_id.max(c.id.saturating_add(1));
        for m in c.member_ids() {
            self.by_member.insert(m, c.id);
        }
        self.campuses.insert(c.id, c);
    }

    /// The repair ops of the load, once.
    pub(crate) fn take_repairs(&mut self) -> Vec<flyff_db::DbOp> {
        std::mem::take(&mut self.repairs)
    }

    /// `true` the first time it is called: the caller logs that the system is inert (no `Campus.lua`).
    pub(crate) fn note_inert(&mut self) -> bool {
        !std::mem::replace(&mut self.inert_noted, true)
    }

    /// Allocates the id of the next campus (`CCampusMng::AddCampus`: `m_idCampus + 1`).
    pub(crate) fn alloc_id(&mut self) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        id
    }

    /// A new campus of `master` with one `pupil`.
    pub(crate) fn create(
        &mut self,
        id: u32,
        master: CharacterId,
        pupil: CharacterId,
        now: UnixTime,
    ) {
        self.insert(Campus {
            id,
            master,
            pupils: BTreeSet::from([pupil]),
            created_at: now,
            last_buff_level: 0,
        });
    }

    /// Adds a pupil to the campus `id`; `false` when the campus is unknown, the character already belongs to a
    /// campus or the campus is full (`AddMember`: `Pupil is full` at three).
    pub(crate) fn add_pupil(&mut self, id: u32, pupil: CharacterId) -> bool {
        if self.by_member.contains_key(&pupil) {
            return false;
        }
        let Some(c) = self.campuses.get_mut(&id) else {
            return false;
        };
        if c.pupils.len() >= flyff_rules::social::CAMPUS_MAX_PUPILS {
            return false;
        }
        c.pupils.insert(pupil);
        self.by_member.insert(pupil, id);
        true
    }

    /// Removes a pupil from his campus; returns the campus id.
    pub(crate) fn remove_pupil(&mut self, pupil: CharacterId) -> Option<u32> {
        let id = self.by_member.remove(&pupil)?;
        if let Some(c) = self.campuses.get_mut(&id) {
            c.pupils.remove(&pupil);
        }
        Some(id)
    }

    /// Dissolves the campus `id` (`RemoveCampus`): every member leaves it; returns the campus.
    pub(crate) fn dissolve(&mut self, id: u32) -> Option<Campus> {
        let c = self.campuses.remove(&id)?;
        for m in c.member_ids() {
            self.by_member.remove(&m);
        }
        Some(c)
    }

    /// Records the buff level of the master's buff; returns whether it differs from the previous one
    /// (`IsChangeBuffLevel`).
    pub(crate) fn change_buff_level(&mut self, id: u32, level: i32) -> bool {
        match self.campuses.get_mut(&id) {
            Some(c) if c.last_buff_level != level => {
                c.last_buff_level = level;
                true
            }
            _ => false,
        }
    }

    /// The campus `id`.
    pub fn get(&self, id: u32) -> Option<&Campus> {
        self.campuses.get(&id)
    }

    /// The campus a character belongs to.
    pub fn campus_of(&self, c: CharacterId) -> Option<&Campus> {
        self.by_member.get(&c).and_then(|id| self.campuses.get(id))
    }

    /// The id of the campus a character belongs to.
    pub fn campus_id_of(&self, c: CharacterId) -> Option<u32> {
        self.by_member.get(&c).copied()
    }

    /// The role of a character in its campus.
    pub fn role_of(&self, c: CharacterId) -> Option<CampusRole> {
        self.campus_of(c).and_then(|k| k.role_of(c))
    }

    /// The campus points of a character (0 when none are stored; may be negative).
    pub fn points(&self, c: CharacterId) -> i32 {
        self.points.get(&c).copied().unwrap_or(0)
    }

    /// Stores the balance; returns it.
    pub(crate) fn set_points(&mut self, c: CharacterId, n: i32) {
        if n == 0 {
            self.points.remove(&c);
        } else {
            self.points.insert(c, n);
        }
    }

    /// Number of campuses.
    pub fn len(&self) -> usize {
        self.campuses.len()
    }

    /// `true` when there is no campus.
    pub fn is_empty(&self) -> bool {
        self.campuses.is_empty()
    }

    /// The campuses in id order.
    pub fn campuses(&self) -> impl Iterator<Item = &Campus> {
        self.campuses.values()
    }

    /// The database purge in memory: purged characters leave their campus and lose their points; a campus left
    /// without its master or with fewer than two members vanishes (the stored rows follow in the daily job).
    pub fn purge(&mut self, ids: &[CharacterId]) -> CampusPurge {
        let mut report = CampusPurge::default();
        for &id in ids {
            self.points.remove(&id);
            let Some(cid) = self.by_member.get(&id).copied() else {
                continue;
            };
            let Some(c) = self.campuses.get_mut(&cid) else {
                continue;
            };
            let was_master = c.master == id;
            c.pupils.remove(&id);
            self.by_member.remove(&id);
            if was_master || c.pupils.is_empty() {
                let left: Vec<CharacterId> = c
                    .member_ids()
                    .into_iter()
                    .filter(|m| *m != id && !ids.contains(m))
                    .collect();
                self.dissolve(cid);
                report.updated.retain(|u| *u != cid);
                report.dissolved.push((cid, left));
            } else if !report.updated.contains(&cid) {
                report.updated.push(cid);
            }
        }
        report
    }
}

/// Tells the online members of campuses that vanished (`CampusRemove`) and resets their mirror.
pub fn announce_dissolved(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    dissolved: Vec<(u32, Vec<CharacterId>)>,
) {
    for (id, members) in dissolved {
        for m in members {
            if let Some(o) = avatar_of(svc, ctx, m) {
                reset_mirror(ctx, o);
                to_one(ctx, o.0, pb::CampusRemove { id_campus: id });
            }
        }
    }
}

/// The campus mirror of an avatar goes back to "none".
pub(crate) fn reset_mirror(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.social.campus = 0;
    }
}

/// `CampusUpdate` of the campus `id` to every online member, in id order.
pub(crate) fn send_update(svc: &Services, ctx: &mut GameCtx<'_>, id: u32) {
    let Some(c) = svc.social.campus.get(id) else {
        return;
    };
    let msg = c.wire();
    for m in c.member_ids() {
        if let Some(o) = avatar_of(svc, ctx, m) {
            to_one(
                ctx,
                o.0,
                pb::CampusUpdate {
                    campus: Some(msg.clone()),
                },
            );
        }
    }
}

/// The campus configuration (`Campus.lua`); `None` makes the system inert.
pub(crate) fn config<'a>(ctx: &'a GameCtx<'_>) -> Option<&'a flyff_data::lua_config::CampusConfig> {
    ctx.data.lua.campus.as_ref()
}
