//! `event::generic`: the `propEvent.inc` generic events and the attendance roll (Plan/22 §6, WP-EVENT-07; ref/15 §5).
//!
//! * [`GenericEventService`] ([`EventServices::generic`](super::EventServices)) is built lazily from
//!   `GameData.generic_events` on first use ([`GenericEventService::ensure`]): the time windows in file order
//!   ([`GenericWindow`]), the item pacers of the kill drops (`items.rs`), the item spawns (`spawn.rs`).
//! * [`minute`] is the first step of the P9 job `event_minute`: the **flag timer**. It keeps the legacy edge
//!   semantics of the database server's `CEventGeneric::Run` and the world server's `OnEventFlag`
//!   (src:_Common/spevent.cpp:467-492, src:WORLDSERVER/DPDatabaseClient.cpp:2931-2992): the flags change only when
//!   the *set of open windows* changes, so a GM `set_flag(5xx, n, Gm)` (the body of `/le`) lasts until the next
//!   window edge (Plan §6.2's per-minute compare would undo it within a minute: superseded, README ruling 30).
//! * [`GenericEventService::factors`] are the EXP and item-drop factors of the windows whose flag is on, f32
//!   products in file order; [`EventServices::recompute_modifiers`](super::EventServices::recompute_modifiers) stores
//!   them in the cached [`EventModifiers`](super::EventModifiers).
//! * [`due_items`] is the paced item list of a monster kill (reached through
//!   [`EventDrops`](super::drops::EventDrops)), [`spawn_tick`] the P16 item spawns, [`call_the_roll`] the P3.1
//!   attendance roll of event 531.
//!
//! With the shipped data every window ended in 2009, so none of this is active in 2026; it is proven on fixtures.

mod attendance;
mod items;
mod spawn;

pub use attendance::{
    ATTENDANCE_DAILY, ATTENDANCE_REWARDS, attendance_bit, call_the_roll, gm_call_the_roll, rewards,
    set_bit_count,
};
pub use items::{ItemPacer, due_items};
pub use spawn::{
    SpawnPacer, SpawnRegion, max_spawn_probability, spawn_index, spawn_regions, spawn_tick,
};

use super::flags::{EventFlags, FlagOrigin, MAX_EVENT};
use crate::ctx::{GameCtx, Services};
use crate::tick::LocalStamp;
use flyff_data::GameData;
use flyff_data::events::generic_event::GenericEvent;
use std::collections::BTreeMap;

/// One time window of `propEvent.inc` (`EVENT_GENERIC` after the load): the event id, its bit in the database's flag
/// word and the factors it applies while its flag is on.
#[derive(Clone, Debug, PartialEq)]
pub struct GenericWindow {
    /// The event id (`500..=531`, below the flag store's 1024).
    pub id: u16,
    /// `1 << (id - 500)`, the bit of the event in the flag word.
    pub flag: u32,
    /// The English title.
    pub title: String,
    /// `[year, month, day, hour]` of the begin (local calendar, minute 0).
    pub begin: [i32; 4],
    /// `[year, month, day, hour]` of the end (exclusive).
    pub end: [i32; 4],
    /// `fExpFactor`.
    pub exp_factor: f32,
    /// `fItemDropRate`.
    pub item_drop_rate: f32,
}

impl GenericWindow {
    /// `begin <= (year, month, day, hour) < end` as calendar tuples (22 §2.4 rule 3: begin and end at minute 0 of the
    /// given hour in the server zone, no zone arithmetic).
    pub fn active(&self, local: &LocalStamp) -> bool {
        let now = [
            i32::from(local.year),
            i32::from(local.month),
            i32::from(local.day),
            i32::from(local.hour),
        ];
        self.begin <= now && now < self.end
    }
}

/// A calendar tuple the legacy `CTime( y, m, d, h, 0, 0 )` would accept.
fn valid_stamp(t: &[i32; 4]) -> bool {
    (1970..=9999).contains(&t[0])
        && (1..=12).contains(&t[1])
        && (1..=31).contains(&t[2])
        // mktime accepts hour 24 as the next day's midnight; the tuple order treats it the same way
        && (0..=24).contains(&t[3])
}

/// One row of [`GenericEventService::status`].
#[derive(Clone, Debug, PartialEq)]
pub struct GenericEventStatus {
    /// The event id.
    pub id: u16,
    /// The English title.
    pub title: String,
    /// `[year, month, day, hour]` of the begin.
    pub begin: [i32; 4],
    /// `[year, month, day, hour]` of the end.
    pub end: [i32; 4],
    /// Whether the window was open at the last timer run ([`minute`]).
    pub window_active: bool,
    /// The flag store's state of the event now (a GM may have set it to anything).
    pub flag: u8,
}

/// The generic events (`Services.event.generic`, 22 §2.2). `Default` is an unbuilt service; every entry point that
/// reads the data runs [`GenericEventService::ensure`] first.
#[derive(Debug, Default)]
pub struct GenericEventService {
    built: bool,
    /// The windows in file order.
    windows: Vec<GenericWindow>,
    /// The item pacers by event id (`std::map<int, list<CEventItem*>>`: ascending id, file order inside).
    pub(crate) items: BTreeMap<u16, Vec<ItemPacer>>,
    /// The item spawn per event (`std::map::insert` keeps the FIRST `Spawn` row of an event).
    pub(crate) spawns: BTreeMap<u16, SpawnPacer>,
    /// The spawn regions, built at the first need ([`spawn_regions`]).
    pub(crate) regions: Option<Vec<SpawnRegion>>,
    /// The flag word of the open windows at the last run (`None` before the first run).
    word: Option<u32>,
}

impl GenericEventService {
    /// Builds the windows, the item pacers and the spawn pacers from `data` on first use. An event without a flag bit
    /// (`id` outside `500..=531`) or with an impossible date is skipped with one warning.
    pub fn ensure(&mut self, data: &GameData) {
        if self.built {
            return;
        }
        self.built = true;
        let mut ids: Vec<&GenericEvent> = Vec::new();
        for e in &data.generic_events {
            let Some(id) = u16::try_from(e.id)
                .ok()
                .filter(|i| usize::from(*i) < MAX_EVENT)
            else {
                tracing::warn!(
                    id = e.id,
                    "propEvent.inc: the event id is outside the flag store; skipped"
                );
                continue;
            };
            if e.flag == 0 {
                tracing::warn!(
                    id,
                    "propEvent.inc: the event has no bit in the flag word; skipped"
                );
                continue;
            }
            if !valid_stamp(&e.begin) || !valid_stamp(&e.end) {
                tracing::warn!(
                    id,
                    begin = ?e.begin,
                    end = ?e.end,
                    "propEvent.inc: the event has an invalid date; skipped"
                );
                continue;
            }
            self.windows.push(GenericWindow {
                id,
                flag: e.flag,
                title: e.title.clone(),
                begin: e.begin,
                end: e.end,
                exp_factor: e.exp_factor,
                item_drop_rate: e.item_drop_rate,
            });
            ids.push(e);
        }
        for (w, e) in self.windows.iter().zip(ids) {
            if !e.items.is_empty() {
                self.items
                    .entry(w.id)
                    .or_default()
                    .extend(e.items.iter().map(ItemPacer::new));
            }
            if let Some(s) = e.spawns.first() {
                // `std::map::insert` keeps the first (flyff-data says "the last wins": wrong)
                self.spawns
                    .entry(w.id)
                    .or_insert_with(|| SpawnPacer::new(s));
            }
        }
    }

    /// The windows in file order (empty until [`GenericEventService::ensure`]).
    pub fn windows(&self) -> &[GenericWindow] {
        &self.windows
    }

    /// The window of event `id`.
    pub fn window(&self, id: u16) -> Option<&GenericWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// `(Π fExpFactor, Π fItemDropRate)` over the windows whose flag is not 0, f32 products in file order
    /// (`CEventGeneric::GetExpFactor` / `GetItemDropRateFactor`, src:_Common/spevent.cpp:383-407).
    pub fn factors(&mut self, data: &GameData, flags: &EventFlags) -> (f32, f32) {
        self.ensure(data);
        let (mut exp, mut drop) = (1.0_f32, 1.0_f32);
        for w in &self.windows {
            if flags.get(w.id) != 0 {
                exp *= w.exp_factor;
                drop *= w.item_drop_rate;
            }
        }
        (exp, drop)
    }

    /// The listing of the generic events (GM and admin views): one row per window in file order.
    pub fn status(&mut self, data: &GameData, flags: &EventFlags) -> Vec<GenericEventStatus> {
        self.ensure(data);
        let word = self.word.unwrap_or(0);
        self.windows
            .iter()
            .map(|w| GenericEventStatus {
                id: w.id,
                title: w.title.clone(),
                begin: w.begin,
                end: w.end,
                window_active: word & w.flag != 0,
                flag: flags.get(w.id),
            })
            .collect()
    }

    /// The flag changes the timer wants at `local`: `(event id, state)` pairs in file order. The first run sets every
    /// open window to 1 (`EVENT_GENERIC` at connect); later runs act only when the word of the open windows differs
    /// from the stored one and then re-level EVERY window: bit set -> 1 but only from state exactly 0, bit clear -> 0
    /// but only from state exactly 1, any other state (a GM's 2) is left alone (`OnEventFlag`).
    pub fn plan(
        &mut self,
        data: &GameData,
        flags: &EventFlags,
        local: &LocalStamp,
    ) -> Vec<(u16, u8)> {
        self.ensure(data);
        let word = self
            .windows
            .iter()
            .filter(|w| w.active(local))
            .fold(0_u32, |acc, w| acc | w.flag);
        let first = self.word.is_none();
        if !first && self.word == Some(word) {
            return Vec::new();
        }
        self.word = Some(word);
        if first {
            return self
                .windows
                .iter()
                .filter(|w| word & w.flag != 0)
                .map(|w| (w.id, 1))
                .collect();
        }
        let mut out = Vec::new();
        for w in &self.windows {
            let state = flags.get(w.id);
            if word & w.flag != 0 {
                if state == 0 {
                    out.push((w.id, 1));
                }
            } else if state == 1 {
                out.push((w.id, 0));
            }
        }
        out
    }
}

/// P9 step 1 `event::generic::minute` (22 §2.3, §6.2): the flag refresh of the `propEvent.inc` windows, before the
/// Event.lua pass of the same minute (the legacy DB minute-timer order, src:databaseserver/DatabaseServer.cpp:549-565).
/// Each change goes through [`EventServices::set_flag`](super::EventServices::set_flag) with
/// [`FlagOrigin::GenericTimer`] (one `SetLocalEvent` to everybody, the modifiers recomputed).
pub fn minute(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let data = std::sync::Arc::clone(ctx.data);
    let changes = {
        let event = &mut s.event;
        event.generic.plan(&data, &event.flags, &ctx.clock.local)
    };
    for (id, state) in changes {
        // `GenericTimer` is never `Protected` and ensure() only keeps ids below the store size
        let _ = s.event.set_flag(ctx, id, state, FlagOrigin::GenericTimer);
    }
}
