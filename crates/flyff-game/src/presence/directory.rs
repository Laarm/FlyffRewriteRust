//! `PlayerDirectory`: every character of the server by id and by name, with its online reference (Plan/21 §3.1,
//! WP-SOCIAL-01; the single name-to-id resolver of whisper, friends, invitations and the GM commands).
//!
//! * Names are unique case-insensitively among non-deleted characters (`Active` and `Sealed`; PL-057, PL-058):
//!   [`NameKey`] is the ASCII case fold (SQLite `NOCASE`). A soft-deleted character's name is free at once.
//! * A character creation reserves its name and slot while the writer allocates the id
//!   ([`PlayerDirectory::reserve_pending`], 12 §9.5, 21 §3.1): until `Committed` the name is taken
//!   ([`PlayerDirectory::name_taken`], [`PlayerDirectory::rename`]) and the slot counts as used
//!   ([`PlayerDirectory::account_slots`] reports [`PENDING_ID`]). Reservations are memory-only.
//! * `data_version` is the `PlayerData.version` of the client cache (F-267): it starts at 1 and grows by exactly 1
//!   per change (login, logout, level, job, rename).
//!
//! Everything here is plain game-thread state: no clock, no channel, no allocation on the lookup paths.

use flyff_common::{AccountId, CharacterId, LayerId, ObjId, SessionId, UnixTime, WorldId};
use flyff_db::{CharacterState, DirectoryEntry, Ticket};
use flyff_proto::pb;
use std::collections::{BTreeMap, BTreeSet};

/// The placeholder id of a pending slot in [`PlayerDirectory::account_slots`]. Character ids start at 1
/// (`meta.next_character_id`), so 0 is never a character.
pub const PENDING_ID: CharacterId = CharacterId(0);

/// Slots per account (`0..=2`).
pub const SLOTS: usize = 3;

/// A character name as the uniqueness rule compares it: ASCII case-folded UTF-8 (13 §5.3
/// `characters_name_live`, SQLite `NOCASE`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameKey(String);

impl NameKey {
    /// The key of `name` (`flyff_rules::names::ascii_name_key`: Unicode is **not** folded).
    pub fn new(name: &str) -> NameKey {
        NameKey(flyff_rules::names::ascii_name_key(name))
    }

    /// The folded text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where an online character is (set while its avatar exists).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnlineRef {
    /// The session of the socket (the avatar of a lingering character has none: it is still "online").
    pub session: SessionId,
    /// The avatar.
    pub obj: ObjId,
    /// Its world.
    pub world: WorldId,
    /// Its layer.
    pub layer: LayerId,
}

/// One character (21 §3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    /// `characters.id`.
    pub id: CharacterId,
    /// Owning account.
    pub account: AccountId,
    /// Name as stored.
    pub name: String,
    /// `characters.slot` 0..=2.
    pub slot: u8,
    /// 0 male, 1 female.
    pub sex: u8,
    /// Job id (`sPlayerData` width).
    pub job: u8,
    /// Level (`sPlayerData` width).
    pub level: u8,
    /// `Active`, `Sealed` or `Deleted`.
    pub state: CharacterState,
    /// When it was soft-deleted.
    pub deleted_at: Option<UnixTime>,
    /// `PlayerData.version`, starts at 1.
    pub data_version: i32,
    /// Set while the character is in the world.
    pub online: Option<OnlineRef>,
}

impl DirEntry {
    /// A new, offline entry at version 1.
    pub fn new(
        id: CharacterId,
        account: AccountId,
        name: impl Into<String>,
        slot: u8,
        sex: u8,
        job: u8,
        level: u8,
    ) -> DirEntry {
        DirEntry {
            id,
            account,
            name: name.into(),
            slot,
            sex,
            job,
            level,
            state: CharacterState::Active,
            deleted_at: None,
            data_version: 1,
            online: None,
        }
    }

    /// `true` for `Active` and `Sealed` characters: they own their name.
    pub fn owns_name(&self) -> bool {
        self.state != CharacterState::Deleted
    }
}

/// A character creation awaiting `DbReply::Committed` (12 §9.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCreate {
    /// The reserved name.
    pub key: NameKey,
    /// The creating account.
    pub account: AccountId,
    /// The reserved slot.
    pub slot: u8,
}

/// Why a directory change is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DirectoryError {
    /// Another character or a pending creation owns the name.
    #[error("name taken")]
    NameTaken,
    /// The slot is outside `0..=2` or used (also: the account already has three characters).
    #[error("slot taken")]
    SlotTaken,
    /// No such character.
    #[error("no such character")]
    NotFound,
}

/// All characters by id and by name (21 §3.1).
#[derive(Debug, Default)]
pub struct PlayerDirectory {
    by_id: BTreeMap<CharacterId, DirEntry>,
    /// Name key -> id of the character that owns the name (`Active` and `Sealed` only).
    by_name: BTreeMap<NameKey, CharacterId>,
    /// Characters of an account, any state (the slot view filters).
    by_account: BTreeMap<AccountId, BTreeSet<CharacterId>>,
    pending: BTreeMap<Ticket, PendingCreate>,
}

impl PlayerDirectory {
    /// The directory of `StartupState.directory` (13 §16.9): every non-purged character, version 1, offline.
    pub fn load(rows: &[DirectoryEntry]) -> PlayerDirectory {
        let mut d = PlayerDirectory::default();
        for r in rows {
            let mut e = DirEntry::new(
                r.id,
                r.account_id,
                r.name.clone(),
                r.slot,
                r.sex,
                r.job,
                u8::try_from(r.level).unwrap_or(u8::MAX),
            );
            e.state = r.state;
            e.deleted_at = r.deleted_at;
            d.insert(e);
        }
        d
    }

    fn insert(&mut self, e: DirEntry) {
        if let Some(old) = self.by_id.get(&e.id).cloned() {
            self.unindex(&old);
        }
        if e.owns_name() {
            self.by_name.insert(NameKey::new(&e.name), e.id);
        }
        self.by_account.entry(e.account).or_default().insert(e.id);
        self.by_id.insert(e.id, e);
    }

    fn unindex(&mut self, e: &DirEntry) {
        let key = NameKey::new(&e.name);
        if self.by_name.get(&key) == Some(&e.id) {
            self.by_name.remove(&key);
        }
        if let Some(set) = self.by_account.get_mut(&e.account) {
            set.remove(&e.id);
            if set.is_empty() {
                self.by_account.remove(&e.account);
            }
        }
    }

    /// Number of entries (any state).
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// `true` when there is no entry.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The entry of `id` (any state).
    pub fn get(&self, id: CharacterId) -> Option<&DirEntry> {
        self.by_id.get(&id)
    }

    /// The character that owns `name` (`Active` or `Sealed`, case-insensitive). Never a pending reservation:
    /// that character has no id yet.
    pub fn by_name(&self, name: &str) -> Option<&DirEntry> {
        self.by_name
            .get(&NameKey::new(name))
            .and_then(|id| self.by_id.get(id))
    }

    /// `true` when a character or a pending creation owns `name`.
    pub fn name_taken(&self, name: &str) -> bool {
        let key = NameKey::new(name);
        self.by_name.contains_key(&key) || self.pending.values().any(|p| p.key == key)
    }

    /// The slot view of an account: `Active` characters as `Some(id)`, pending creations as
    /// `Some(PENDING_ID)`; `Sealed` and `Deleted` characters occupy no slot.
    pub fn account_slots(&self, account: AccountId) -> [Option<CharacterId>; SLOTS] {
        let mut slots = [None; SLOTS];
        if let Some(ids) = self.by_account.get(&account) {
            for e in ids.iter().filter_map(|id| self.by_id.get(id)) {
                if e.state == CharacterState::Active
                    && let Some(s) = slots.get_mut(usize::from(e.slot))
                    && s.is_none()
                {
                    *s = Some(e.id);
                }
            }
        }
        for p in self.pending.values().filter(|p| p.account == account) {
            if let Some(s) = slots.get_mut(usize::from(p.slot))
                && s.is_none()
            {
                *s = Some(PENDING_ID);
            }
        }
        slots
    }

    /// Reserves `name` and `slot` for a creation whose `DbReply::Committed` is awaited under `ticket`
    /// (12 §9.5 step 2): `NameTaken` when the name is taken, `SlotTaken` when the slot is above 2 or used (a
    /// full account has no free slot). A second reservation under the same ticket replaces the first.
    pub fn reserve_pending(
        &mut self,
        name: &str,
        account: AccountId,
        slot: u8,
        ticket: Ticket,
    ) -> Result<(), DirectoryError> {
        self.pending.remove(&ticket);
        if self.name_taken(name) {
            return Err(DirectoryError::NameTaken);
        }
        let used = usize::from(slot) >= SLOTS
            || self
                .account_slots(account)
                .get(usize::from(slot))
                .is_none_or(Option::is_some);
        if used {
            return Err(DirectoryError::SlotTaken);
        }
        self.pending.insert(
            ticket,
            PendingCreate {
                key: NameKey::new(name),
                account,
                slot,
            },
        );
        Ok(())
    }

    /// Drops the reservation of `ticket` (on `Committed`, before [`PlayerDirectory::insert_created`], and on every
    /// failure). An unknown ticket changes nothing.
    pub fn release_pending(&mut self, ticket: Ticket) {
        self.pending.remove(&ticket);
    }

    /// Number of outstanding reservations.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Adds the character a committed `CreateCharacter` produced.
    pub fn insert_created(&mut self, e: DirEntry) {
        self.insert(e);
    }

    /// Soft delete: the entry stays (the purge job removes it after the retention time) but the name is free
    /// from this moment on (PL-057) and the slot too. Returns `false` for an unknown or already deleted id.
    pub fn mark_deleted(&mut self, id: CharacterId, at: UnixTime) -> bool {
        let Some(e) = self.by_id.get(&id).cloned() else {
            return false;
        };
        if e.state == CharacterState::Deleted {
            return false;
        }
        let key = NameKey::new(&e.name);
        if self.by_name.get(&key) == Some(&id) {
            self.by_name.remove(&key);
        }
        if let Some(entry) = self.by_id.get_mut(&id) {
            entry.state = CharacterState::Deleted;
            entry.deleted_at = Some(at);
            entry.online = None;
        }
        true
    }

    /// Undoes [`PlayerDirectory::mark_deleted`] after the database refused the delete: `false` when the entry
    /// is not deleted or another character owns the name by now.
    pub fn restore(&mut self, id: CharacterId) -> bool {
        let Some(e) = self.by_id.get(&id) else {
            return false;
        };
        if e.state != CharacterState::Deleted {
            return false;
        }
        let key = NameKey::new(&e.name);
        if self.by_name.contains_key(&key) {
            return false;
        }
        self.by_name.insert(key, id);
        if let Some(entry) = self.by_id.get_mut(&id) {
            entry.state = CharacterState::Active;
            entry.deleted_at = None;
        }
        true
    }

    /// Undelete into `slot` (the admin console, 25 §4.6; the CLI's checks): the entry must be deleted, `slot` must
    /// be `0..=2` and free for the account (pending creations count), and no other character or pending creation
    /// may own the name by now. The character is `Active` again in `slot`; the data version moves. `Err` changes
    /// nothing. The roll-back of a refused database change is [`PlayerDirectory::mark_deleted`].
    pub fn undelete(&mut self, id: CharacterId, slot: u8) -> Result<(), DirectoryError> {
        let e = self
            .by_id
            .get(&id)
            .cloned()
            .ok_or(DirectoryError::NotFound)?;
        if e.state != CharacterState::Deleted {
            return Err(DirectoryError::NotFound);
        }
        let free = usize::from(slot) < SLOTS
            && self
                .account_slots(e.account)
                .get(usize::from(slot))
                .is_some_and(Option::is_none);
        if !free {
            return Err(DirectoryError::SlotTaken);
        }
        if self.name_taken(&e.name) {
            return Err(DirectoryError::NameTaken);
        }
        self.by_name.insert(NameKey::new(&e.name), id);
        if let Some(entry) = self.by_id.get_mut(&id) {
            entry.state = CharacterState::Active;
            entry.deleted_at = None;
            entry.slot = slot;
            entry.data_version += 1;
        }
        Ok(())
    }

    /// Removes purged characters from both indexes (13 §14.2).
    pub fn purge(&mut self, ids: &[CharacterId]) {
        for id in ids {
            if let Some(e) = self.by_id.remove(id) {
                self.unindex(&e);
            }
        }
    }

    /// Marks a character online (`Some`) or offline (`None`); bumps the data version and returns it.
    pub fn set_online(&mut self, id: CharacterId, online: Option<OnlineRef>) -> Option<i32> {
        let e = self.by_id.get_mut(&id)?;
        e.online = online;
        e.data_version += 1;
        Some(e.data_version)
    }

    /// New level and job; bumps the data version and returns it.
    pub fn set_level_job(&mut self, id: CharacterId, level: u8, job: u8) -> Option<i32> {
        let e = self.by_id.get_mut(&id)?;
        e.level = level;
        e.job = job;
        e.data_version += 1;
        Some(e.data_version)
    }

    /// Renames a character (scroll, GM): `NameTaken` when another character or a pending creation owns the new
    /// name (a change of case of its own name is fine); bumps the data version.
    pub fn rename(&mut self, id: CharacterId, new_name: &str) -> Result<(), DirectoryError> {
        let old = self
            .by_id
            .get(&id)
            .cloned()
            .ok_or(DirectoryError::NotFound)?;
        let new_key = NameKey::new(new_name);
        let other_owner = self.by_name.get(&new_key).is_some_and(|owner| *owner != id);
        let pending = self.pending.values().any(|p| p.key == new_key);
        if other_owner || pending {
            return Err(DirectoryError::NameTaken);
        }
        if old.owns_name() {
            self.by_name.remove(&NameKey::new(&old.name));
            self.by_name.insert(new_key, id);
        }
        if let Some(e) = self.by_id.get_mut(&id) {
            e.name = new_name.to_owned();
            e.data_version += 1;
        }
        Ok(())
    }

    /// The `sPlayerData` of a character (`login` = 1 while it is in the world; 21 §3.2).
    pub fn player_data(&self, id: CharacterId) -> Option<pb::PlayerData> {
        let e = self.by_id.get(&id)?;
        Some(pb::PlayerData {
            job: u32::from(e.job),
            level: u32::from(e.level),
            sex: u32::from(e.sex),
            version: e.data_version,
            login: u32::from(e.online.is_some()),
        })
    }

    /// The characters of an account in id order, any state.
    pub fn characters_of(&self, account: AccountId) -> impl Iterator<Item = &DirEntry> + '_ {
        self.by_account
            .get(&account)
            .into_iter()
            .flatten()
            .filter_map(|id| self.by_id.get(id))
    }

    /// Every entry in id order.
    pub fn iter(&self) -> impl Iterator<Item = &DirEntry> + '_ {
        self.by_id.values()
    }

    /// Soft-deleted characters whose retention ran out before `cutoff` (the purge selection of 13 §14.2).
    pub fn purge_candidates(&self, cutoff: UnixTime) -> Vec<CharacterId> {
        self.by_id
            .values()
            .filter(|e| {
                e.state == CharacterState::Deleted && e.deleted_at.is_some_and(|t| t < cutoff)
            })
            .map(|e| e.id)
            .collect()
    }

    /// Characters currently marked online.
    pub fn online_count(&self) -> usize {
        self.by_id.values().filter(|e| e.online.is_some()).count()
    }

    /// Checks that the indexes agree with the entries (tests): every non-deleted name maps to its owner, no
    /// stale key exists, and the account index lists every entry.
    pub fn check_invariants(&self) -> Result<(), String> {
        let mut names = BTreeMap::new();
        for e in self.by_id.values() {
            if !self
                .by_account
                .get(&e.account)
                .is_some_and(|s| s.contains(&e.id))
            {
                return Err(format!("{} is missing from the account index", e.id));
            }
            if e.owns_name()
                && let Some(prev) = names.insert(NameKey::new(&e.name), e.id)
            {
                return Err(format!("name '{}' is owned by {prev} and {}", e.name, e.id));
            }
        }
        if names != self.by_name {
            return Err("the name index differs from the entries".to_owned());
        }
        let indexed: usize = self.by_account.values().map(BTreeSet::len).sum();
        if indexed != self.by_id.len() {
            return Err("the account index has stale ids".to_owned());
        }
        Ok(())
    }
}
