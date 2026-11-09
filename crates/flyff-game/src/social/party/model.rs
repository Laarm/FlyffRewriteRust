//! The party model (Plan/21 §4.1): [`Party`], [`PartySlot`], the share modes and the pure membership operations of
//! [`PartyService`]. Nothing here sends a message or reads a clock: the handlers (`handlers.rs`, `skills.rs`, ...)
//! call these operations and produce the traffic.
//!
//! Index 0 of [`Party::members`] is the leader; removing a member keeps the order of the others (the original's
//! compaction), changing the leader swaps index 0 with the new leader (`SwapPartyMember`). Parties are never
//! persisted (KQ-21): a restart dissolves them all, only the stored advanced-party names survive.

use flyff_common::math::Vec3;
use flyff_common::{CharacterId, ObjId, PartyId, UnixTime, WorldId};
pub use flyff_rules::social::PartyKind;
use flyff_rules::social::{MAX_PARTY_MEMBERS, PARTY_START_LEVEL};
use std::collections::BTreeMap;

/// `MAX_PARTYMODE`: the party modes 0..=4.
pub const MAX_PARTYMODE: usize = 5;
/// `PARTY_LINKATTACK_MODE`.
pub const MODE_LINKATTACK: usize = 0;
/// `PARTY_FORTUNECIRCLE_MODE`.
pub const MODE_FORTUNECIRCLE: usize = 1;
/// `PARTY_STRETCHING_MODE`.
pub const MODE_STRETCHING: usize = 2;
/// `PARTY_GIFTBOX_MODE`.
pub const MODE_GIFTBOX: usize = 3;
/// `PARTY_PARSKILL_MODE` (a flag, never timed out).
pub const MODE_PARSKILL: usize = 4;

/// `m_nTroupsShareExp`: how the member shares of a kill are computed (advanced parties only; 2 is refused).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExpShareMode {
    /// 0: by level (the default; normal parties always use it).
    #[default]
    Level = 0,
    /// 1: by contribution.
    Contribution = 1,
}

impl ExpShareMode {
    /// The wire value.
    pub const fn wire(self) -> i32 {
        self as i32
    }

    /// The mode of a request; `None` for 2 ("nobody gets EXP", PL-086) and every other value (VR-004).
    pub const fn from_wire(v: i32) -> Option<ExpShareMode> {
        match v {
            0 => Some(ExpShareMode::Level),
            1 => Some(ExpShareMode::Contribution),
            _ => None,
        }
    }
}

/// `m_nTroupeShareItem`: who receives a monster item picked up by a member.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ItemShareMode {
    /// 0: the finder keeps it.
    #[default]
    Finder = 0,
    /// 1: round robin over the members in range.
    RoundRobin = 1,
    /// 2: the leader when in range.
    Leader = 2,
    /// 3: a random member in range.
    Random = 3,
}

impl ItemShareMode {
    /// The wire value.
    pub const fn wire(self) -> i32 {
        self as i32
    }

    /// The mode of a request; `None` outside `0..=3` (VR-004).
    pub const fn from_wire(v: i32) -> Option<ItemShareMode> {
        match v {
            0 => Some(ItemShareMode::Finder),
            1 => Some(ItemShareMode::RoundRobin),
            2 => Some(ItemShareMode::Leader),
            3 => Some(ItemShareMode::Random),
            _ => None,
        }
    }
}

/// One member of a party (`PartyMember`): `offline_since` is `m_bRemove` + `m_tTime` (seconds of the wall
/// clock), `obj` the avatar while the member is online.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartySlot {
    /// The character.
    pub id: CharacterId,
    /// The avatar while the character is in the world.
    pub obj: Option<ObjId>,
    /// When the character went offline (`None` = online).
    pub offline_since: Option<UnixTime>,
    /// `m_vPos`: the position the party map info last sent for this member (21 §4.9).
    pub map_pos: Vec3,
}

impl PartySlot {
    /// An online member.
    pub const fn online(id: CharacterId, obj: ObjId) -> PartySlot {
        PartySlot {
            id,
            obj: Some(obj),
            offline_since: None,
            map_pos: Vec3::ZERO,
        }
    }

    /// `true` for a member that is in the world (`!m_bRemove`).
    pub const fn is_online(&self) -> bool {
        self.offline_since.is_none()
    }
}

/// `CParty` (21 §4.1).
#[derive(Clone, Debug, PartialEq)]
pub struct Party {
    /// `m_uPartyId`.
    pub id: PartyId,
    /// `m_nKindTroup`.
    pub kind: PartyKind,
    /// `m_sParty`: meaningful for an advanced party only.
    pub name: String,
    /// The members, index 0 the leader.
    pub members: Vec<PartySlot>,
    /// `m_nLevel`.
    pub level: i32,
    /// `m_nExp`.
    pub exp: i32,
    /// `m_nPoint`: the party skill points.
    pub point: i32,
    /// `m_nTroupsShareExp`.
    pub exp_share: ExpShareMode,
    /// `m_nTroupeShareItem`.
    pub item_share: ItemShareMode,
    /// `m_nModeTime[0..=3]`: remaining milliseconds; `[4]` is the PARSKILL flag (0 or 1).
    pub mode_ms: [i32; MAX_PARTYMODE],
    /// `m_nGetItemPlayerId`: the round-robin cursor, the leader at creation. The hot copy that pickups move lives
    /// in the members' [`PartyLink`](super::PartyLink)s (the world phase has no services); this is the copy the
    /// links are rebuilt from.
    pub loot_cursor: CharacterId,
    /// `m_dwWorldId`: set by the party summon (21 §4.11).
    pub summon_world: Option<WorldId>,
}

impl Party {
    /// A new normal party of `leader` and `member`: level 1, exp 0, point 0, level share, finder share, the
    /// cursor on the leader (`NewParty`).
    pub fn new(id: PartyId, leader: PartySlot, member: PartySlot) -> Party {
        Party {
            id,
            kind: PartyKind::Normal,
            name: String::new(),
            loot_cursor: leader.id,
            members: vec![leader, member],
            level: PARTY_START_LEVEL,
            exp: 0,
            point: 0,
            exp_share: ExpShareMode::Level,
            item_share: ItemShareMode::Finder,
            mode_ms: [0; MAX_PARTYMODE],
            summon_world: None,
        }
    }

    /// The leader (index 0).
    pub fn leader(&self) -> Option<&PartySlot> {
        self.members.first()
    }

    /// The character id of the leader.
    pub fn leader_id(&self) -> Option<CharacterId> {
        self.members.first().map(|m| m.id)
    }

    /// Whether `c` is a member.
    pub fn is_member(&self, c: CharacterId) -> bool {
        self.members.iter().any(|m| m.id == c)
    }

    /// The slot of `c`.
    pub fn slot(&self, c: CharacterId) -> Option<&PartySlot> {
        self.members.iter().find(|m| m.id == c)
    }

    /// The slot of `c`, mutably.
    pub fn slot_mut(&mut self, c: CharacterId) -> Option<&mut PartySlot> {
        self.members.iter_mut().find(|m| m.id == c)
    }

    /// Number of members (`m_nSizeofMember`).
    pub fn size(&self) -> usize {
        self.members.len()
    }

    /// `true` when the party is at the cap of eight.
    pub fn is_full(&self) -> bool {
        self.members.len() >= MAX_PARTY_MEMBERS
    }

    /// The online members' avatars in party order.
    pub fn online_objs(&self) -> impl Iterator<Item = ObjId> + '_ {
        self.members.iter().filter_map(|m| m.obj)
    }

    /// The online members in party order.
    pub fn online(&self) -> impl Iterator<Item = &PartySlot> + '_ {
        self.members.iter().filter(|m| m.is_online())
    }

    /// `true` while the mode `m` runs (the timers of 0..=3 are positive, mode 4 is a flag).
    pub fn mode_running(&self, m: usize) -> bool {
        self.mode_ms.get(m).is_some_and(|&t| t > 0)
    }

    /// `m_nModeTime` as the five wire values.
    pub fn mode_time_wire(&self) -> Vec<i32> {
        self.mode_ms.to_vec()
    }
}

/// The advanced-party names: global, case-insensitive (PL-058), one name per leader. `names` maps the ASCII-folded
/// key to the owner; `by_owner` holds the name as stored (the original's two maps playerId <-> name).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartyNames {
    by_key: BTreeMap<String, CharacterId>,
    by_owner: BTreeMap<CharacterId, String>,
}

impl PartyNames {
    /// The folded key of a name (SQLite `NOCASE`: ASCII only).
    pub fn key(name: &str) -> String {
        flyff_rules::names::ascii_name_key(name)
    }

    /// The names of the startup state (`party_names` rows).
    pub fn load<'a>(rows: impl IntoIterator<Item = (CharacterId, &'a str)>) -> PartyNames {
        let mut n = PartyNames::default();
        for (owner, name) in rows {
            n.set(owner, name);
        }
        n
    }

    /// The stored name of `owner`.
    pub fn of(&self, owner: CharacterId) -> Option<&str> {
        self.by_owner.get(&owner).map(String::as_str)
    }

    /// The owner of `name` (case-insensitive).
    pub fn owner_of(&self, name: &str) -> Option<CharacterId> {
        self.by_key.get(&Self::key(name)).copied()
    }

    /// `true` when `name` is free for `who`: nobody owns it, or `who` does (re-using the own stored name).
    pub fn is_available_to(&self, name: &str, who: CharacterId) -> bool {
        self.owner_of(name).is_none_or(|o| o == who)
    }

    /// Stores `name` for `owner`, replacing the previous name of `owner` (one name per player). The caller has
    /// checked [`PartyNames::is_available_to`].
    pub fn set(&mut self, owner: CharacterId, name: &str) {
        if let Some(old) = self.by_owner.insert(owner, name.to_owned()) {
            self.by_key.remove(&Self::key(&old));
        }
        self.by_key.insert(Self::key(name), owner);
    }

    /// Forgets the name of `owner` (a purged character).
    pub fn remove(&mut self, owner: CharacterId) {
        if let Some(old) = self.by_owner.remove(&owner) {
            self.by_key.remove(&Self::key(&old));
        }
    }

    /// Number of stored names.
    pub fn len(&self) -> usize {
        self.by_owner.len()
    }

    /// `true` when no name is stored.
    pub fn is_empty(&self) -> bool {
        self.by_owner.is_empty()
    }
}

/// What removing a member did to its party.
#[derive(Clone, Debug, PartialEq)]
pub struct Removal {
    /// The removed character.
    pub removed: CharacterId,
    /// The party after the removal had fewer than two members and was deleted.
    pub dissolved: bool,
    /// The removed member was the leader (the next member is now index 0; meaningless when dissolved).
    pub was_leader: bool,
    /// The new leader when `was_leader` and the party survived.
    pub new_leader: Option<CharacterId>,
    /// The members left (empty when dissolved: they were the former members of the deleted party, see
    /// [`Removal::former`]).
    pub remaining: Vec<PartySlot>,
    /// When dissolved: the surviving member(s) the deletion also released.
    pub former: Vec<PartySlot>,
}

/// The party registry: parties by id, membership by character, the advanced-party names.
#[derive(Clone, Debug, PartialEq)]
pub struct PartyService {
    parties: BTreeMap<PartyId, Party>,
    by_member: BTreeMap<CharacterId, PartyId>,
    /// The stored advanced-party names (`party_names`).
    pub names: PartyNames,
    next_id: u32,
    /// `m_nSecCount` of the party map info: the call counter (every 10th call acts).
    pub map_info_calls: u32,
}

impl Default for PartyService {
    fn default() -> Self {
        PartyService {
            parties: BTreeMap::new(),
            by_member: BTreeMap::new(),
            names: PartyNames::default(),
            next_id: 1,
            map_info_calls: 0,
        }
    }
}

impl PartyService {
    /// The party `c` is a member of (`CPartyMng::GetParty( m_idparty )` + `IsMember`).
    pub fn party_of(&self, c: CharacterId) -> Option<PartyId> {
        self.by_member.get(&c).copied()
    }

    /// The party record.
    pub fn get(&self, id: PartyId) -> Option<&Party> {
        self.parties.get(&id)
    }

    /// The party record, mutably.
    pub fn get_mut(&mut self, id: PartyId) -> Option<&mut Party> {
        self.parties.get_mut(&id)
    }

    /// The party of `c`.
    pub fn party_record_of(&self, c: CharacterId) -> Option<&Party> {
        self.parties.get(self.by_member.get(&c)?)
    }

    /// Every party in id order.
    pub fn iter(&self) -> impl Iterator<Item = &Party> + '_ {
        self.parties.values()
    }

    /// The ids of every party, ascending.
    pub fn ids(&self) -> Vec<PartyId> {
        self.parties.keys().copied().collect()
    }

    /// Number of parties.
    pub fn len(&self) -> usize {
        self.parties.len()
    }

    /// `true` when there is no party.
    pub fn is_empty(&self) -> bool {
        self.parties.is_empty()
    }

    /// Whether `a` and `b` are different members of one party (`same_party`, the loot right of 19 §6.6).
    pub fn same_party(&self, a: CharacterId, b: CharacterId) -> bool {
        match (self.party_of(a), self.party_of(b)) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        }
    }

    /// Creates a normal party of `leader` and `member` and returns its id (`NewParty`). Both must be free.
    pub fn create(&mut self, leader: PartySlot, member: PartySlot) -> PartyId {
        let id = PartyId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.by_member.insert(leader.id, id);
        self.by_member.insert(member.id, id);
        self.parties.insert(id, Party::new(id, leader, member));
        id
    }

    /// Appends `slot` to the party (`NewMember`). Returns `false` for an unknown or full party.
    pub fn add_member(&mut self, id: PartyId, slot: PartySlot) -> bool {
        let Some(p) = self.parties.get_mut(&id) else {
            return false;
        };
        if p.is_full() || p.is_member(slot.id) {
            return false;
        }
        p.members.push(slot);
        self.by_member.insert(slot.id, id);
        true
    }

    /// Removes `c` from its party in order (`DeleteMember`). With fewer than two members left the party is
    /// deleted and every remaining member released. Returns `None` when `c` is in no party.
    pub fn remove_member(&mut self, c: CharacterId) -> Option<Removal> {
        let id = self.by_member.remove(&c)?;
        let p = self.parties.get_mut(&id)?;
        let at = p.members.iter().position(|m| m.id == c)?;
        let was_leader = at == 0;
        p.members.remove(at);
        if p.members.len() < 2 {
            let former = p.members.clone();
            for m in &former {
                self.by_member.remove(&m.id);
            }
            self.parties.remove(&id);
            return Some(Removal {
                removed: c,
                dissolved: true,
                was_leader,
                new_leader: None,
                remaining: Vec::new(),
                former,
            });
        }
        let new_leader = was_leader.then(|| p.members[0].id);
        Some(Removal {
            removed: c,
            dissolved: false,
            was_leader,
            new_leader,
            remaining: p.members.clone(),
            former: Vec::new(),
        })
    }

    /// Deletes the whole party and releases its members (a leader logout with nobody online).
    pub fn delete(&mut self, id: PartyId) -> Option<Party> {
        let p = self.parties.remove(&id)?;
        for m in &p.members {
            self.by_member.remove(&m.id);
        }
        Some(p)
    }

    /// `ChangeLeader` / `SwapPartyMember`: swaps index 0 with `new` (a member other than the leader).
    pub fn swap_leader(&mut self, id: PartyId, new: CharacterId) -> bool {
        let Some(p) = self.parties.get_mut(&id) else {
            return false;
        };
        match p.members.iter().position(|m| m.id == new) {
            Some(at) if at > 0 => {
                p.members.swap(0, at);
                true
            }
            _ => false,
        }
    }

    /// Marks `c` offline at `since` (logout). The avatar is forgotten.
    pub fn set_offline(&mut self, c: CharacterId, since: UnixTime) -> bool {
        let Some(slot) = self
            .by_member
            .get(&c)
            .and_then(|id| self.parties.get_mut(id))
            .and_then(|p| p.slot_mut(c))
        else {
            return false;
        };
        slot.offline_since = Some(since);
        slot.obj = None;
        true
    }

    /// Marks `c` online again with its avatar (reconnect). `false` when `c` is in no party.
    pub fn set_online(&mut self, c: CharacterId, obj: ObjId) -> bool {
        let Some(slot) = self
            .by_member
            .get(&c)
            .and_then(|id| self.parties.get_mut(id))
            .and_then(|p| p.slot_mut(c))
        else {
            return false;
        };
        slot.offline_since = None;
        slot.obj = Some(obj);
        true
    }

    /// Records the stored name of the leader (`names.set`) - the persistence op is the caller's.
    pub fn name_available_to(&self, name: &str, who: CharacterId) -> bool {
        self.names.is_available_to(name, who)
    }

    /// Next id the registry will hand out (tests).
    pub fn next_id(&self) -> u32 {
        self.next_id
    }
}
