//! The bank (Plan/19 §10, WP-ECON-05; ref/13 §16, ref/07a §14; src:WORLDSERVER/DPSrvr.cpp:3234-4077; VR-012, VR-014,
//! VR-015, VR-016, PL-060, PL-061, PL-156): the sessions, the PIN lockout and the comm bank.
//!
//! * **Model.** Every character owns one bank compartment (42 slots + a penya balance) per account slot; the
//!   banks of the account's active characters are loaded at enter-world into `PlayerItems.banks`
//!   ([`crate::item::PlayerItems::bank`]) and every snapshot of the online character rewrites all of them, so
//!   a move between the bag and a bank, or between two banks, is one snapshot and needs no `AtomicBatch`
//!   (13 §17.3; the handlers mark the player urgent). The own compartment is `ident.slot`; another character's
//!   compartment needs an active remote-bank mode (`SM_BANK15 / 30 / 001`, [`crate::stats::sm::is_comm_bank`]).
//! * **Session** ([`BankSession`] in `EconState.bank`). `OpenBankWnd` creates it (a banker NPC within
//!   `gameplay.npc_menu_range`, VR-012, or the instant-bank item `II_SYS_SYS_SCR_CUSTODY`, VR-014);
//!   `ConfirmBank` with the right PIN confirms it (and consumes the instant-bank item); every put, get and
//!   bank-to-bank request needs a confirmed session (**FIX**, the original checked nothing but the NPC
//!   distance). `CloseBankWnd`, death, teleport and world change drop it ([`super::EconState::end_bank_session`]),
//!   logout destroys the avatar with it.
//! * **PIN** (`banks.pin`, four ASCII digits). The lockout of PL-061 lives in [`BankPinGuard`] (memory only, per
//!   character, so a re-login does not clear it): with `gameplay.bank_pin_max_failures` (default 3, `0` =
//!   unlimited like the original) wrong PINs in a row the bank is locked for `gameplay.bank_pin_lock_s` (900 s)
//!   of wall time (`TickClock.unix_ms`, a fake clock in tests); while locked `ConfirmBank` and `ChangeBankPassReq`
//!   answer failure plus `TID_GAME_STILLNOTUSE` and the right PIN is refused too. A right PIN resets the count.
//!   PINs are compared through a salted digest ([`BankService::verify`]); the typed PIN is never stored, logged
//!   or printed ([`Pin`] has a redacting `Debug`).

mod handlers;

pub use handlers::{
    on_bank_to_bank, on_change_pin, on_close, on_confirm, on_get_gold, on_get_item, on_open,
    on_put_gold, on_put_item,
};

use crate::item::container::StorageId;
use flyff_common::rng::GameRng;
use flyff_common::{CharacterId, UnixTime};
use std::collections::BTreeMap;
use std::hash::Hasher;

/// How a bank session was opened (`OpenBankWnd.obj_id`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BankAccess {
    /// A banker NPC (`MMI_BANKING`, `obj_id == NULL_ID`).
    Npc,
    /// The instant-bank item `II_SYS_SYS_SCR_CUSTODY` in this inventory slot.
    InstantItem {
        /// The storage id of the item.
        obj_id: StorageId,
    },
}

/// The bank session of a player (Plan/19 §10.1): `m_bBank` and `m_bInstantBank` of the original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BankSession {
    /// How it was opened.
    pub via: BankAccess,
    /// The PIN was confirmed (`m_bBank`).
    pub pin_confirmed: bool,
    /// The instant-bank item was consumed by the confirmation (`m_bInstantBank`): no banker NPC needed.
    pub instant: bool,
}

impl BankSession {
    /// A fresh, unconfirmed session.
    pub fn opened(via: BankAccess) -> BankSession {
        BankSession {
            via,
            pin_confirmed: false,
            instant: false,
        }
    }
}

/// The PIN failure counter of a character (PL-061), memory only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BankPinGuard {
    /// Wrong PINs since the last right one (or the last expired lock).
    pub failures: u32,
    /// The bank is locked until this wall time.
    pub locked_until: Option<UnixTime>,
}

impl BankPinGuard {
    /// `true` while the lock holds at `now`. An expired lock is cleared (and the count with it): after the
    /// 900 s the player has the full number of tries again.
    pub fn is_locked(&mut self, now: UnixTime) -> bool {
        match self.locked_until {
            Some(until) if now.0 < until.0 => true,
            Some(_) => {
                self.locked_until = None;
                self.failures = 0;
                false
            }
            None => false,
        }
    }

    /// A wrong PIN at `now`; `true` when it locked the bank. `max_failures == 0` never locks.
    pub fn record_failure(&mut self, now: UnixTime, max_failures: u32, lock_s: u32) -> bool {
        self.failures = self.failures.saturating_add(1);
        if max_failures > 0 && self.failures >= max_failures {
            self.locked_until = Some(UnixTime(now.0.saturating_add(i64::from(lock_s))));
            return true;
        }
        false
    }

    /// The right PIN.
    pub fn record_success(&mut self) {
        self.failures = 0;
        self.locked_until = None;
    }

    /// `true` when the guard holds no information (it can be dropped).
    pub fn is_clear(&self) -> bool {
        self.failures == 0 && self.locked_until.is_none()
    }
}

/// A PIN as typed by a player. Its `Debug` and `Display` never show the digits.
#[derive(Clone, PartialEq, Eq)]
pub struct Pin(Vec<u8>);

impl Pin {
    /// Wraps the typed string.
    pub fn typed(s: &str) -> Pin {
        Pin(s.as_bytes().to_vec())
    }

    /// `true` when the PIN is exactly four ASCII digits (the only form `banks.pin` can hold).
    pub fn is_well_formed(&self) -> bool {
        self.0.len() == 4 && self.0.iter().all(u8::is_ascii_digit)
    }

    /// The four digits when well formed.
    pub fn digits(&self) -> Option<[u8; 4]> {
        <[u8; 4]>::try_from(self.0.as_slice())
            .ok()
            .filter(|d| d.iter().all(u8::is_ascii_digit))
    }
}

impl std::fmt::Debug for Pin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pin(****)")
    }
}

impl std::fmt::Display for Pin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("****")
    }
}

/// More guards than this are pruned (cleared ones go first): the map is keyed by character and a clear guard
/// carries nothing.
const GUARD_PRUNE_AT: usize = 1024;

/// The bank service (`Services.econ.bank`): the PIN guards and the salted PIN comparison.
#[derive(Debug, Default)]
pub struct BankService {
    guards: BTreeMap<CharacterId, BankPinGuard>,
    /// The salt of the PIN digests, drawn from the `service` random stream on first use.
    salt: Option<u64>,
}

impl BankService {
    /// The guard of `c`, when there is one.
    pub fn guard(&self, c: CharacterId) -> Option<&BankPinGuard> {
        self.guards.get(&c)
    }

    /// `true` while the bank of `c` is locked at `now` (an expired lock is cleared).
    pub fn is_locked(&mut self, c: CharacterId, now: UnixTime) -> bool {
        let Some(g) = self.guards.get_mut(&c) else {
            return false;
        };
        let locked = g.is_locked(now);
        if g.is_clear() {
            self.guards.remove(&c);
        }
        locked
    }

    /// A wrong PIN of `c`; `true` when the bank is locked now.
    pub fn record_failure(
        &mut self,
        c: CharacterId,
        now: UnixTime,
        max_failures: u32,
        lock_s: u32,
    ) -> bool {
        if self.guards.len() >= GUARD_PRUNE_AT {
            self.guards.retain(|_, g| !g.is_clear());
        }
        self.guards
            .entry(c)
            .or_default()
            .record_failure(now, max_failures, lock_s)
    }

    /// The right PIN of `c`.
    pub fn record_success(&mut self, c: CharacterId) {
        self.guards.remove(&c);
    }

    /// Forgets the guard of `c` (tests, an administrator unlocking a bank).
    pub fn unlock(&mut self, c: CharacterId) {
        self.guards.remove(&c);
    }

    /// The salt of the digests (drawn once from `rng`).
    fn salt(&mut self, rng: &mut dyn GameRng) -> u64 {
        *self
            .salt
            .get_or_insert_with(|| (u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32()))
    }

    /// The salted digest of a PIN (SipHash-1-3 over `salt || pin`; only ever compared inside one process).
    fn digest(salt: u64, pin: &[u8]) -> u64 {
        let mut h = std::hash::DefaultHasher::new();
        h.write_u64(salt);
        h.write(pin);
        h.finish()
    }

    /// Does `given` match the `stored` PIN? Both go through the salted digest and the digests are compared; a
    /// `given` that is not four digits never matches. The stored PIN is `banks.pin` (the 4-digit column of the
    /// schema; hashing it at rest would need a migration, the lockout is the real protection).
    pub fn verify(&mut self, rng: &mut dyn GameRng, stored: &[u8; 4], given: &Pin) -> bool {
        let salt = self.salt(rng);
        let Some(typed) = given.digits() else {
            return false;
        };
        let a = BankService::digest(salt, stored);
        let b = BankService::digest(salt, &typed);
        // fold the difference instead of branching on the first differing bit
        (a ^ b) == 0
    }
}

/// The audit `kind` of a bank move (`bank.put` / `bank.take`).
pub(crate) mod kind {
    /// An item from the bag into a bank.
    pub const PUT_ITEM: &str = "put_item";
    /// An item from a bank into the bag.
    pub const GET_ITEM: &str = "get_item";
    /// Penya from the wallet into a bank.
    pub const PUT_GOLD: &str = "put_gold";
    /// Penya from a bank into the wallet.
    pub const GET_GOLD: &str = "get_gold";
    /// Between two banks.
    pub const BANK_TO_BANK: &str = "bank_to_bank";
}
