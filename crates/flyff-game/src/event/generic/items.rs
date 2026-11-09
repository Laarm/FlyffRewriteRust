//! The paced event items of a monster kill (`CEventItem`, `CEventGeneric::GetItem`; Plan/22 §6.3, ref/15 §5;
//! src:_Common/spevent.cpp:87-121, 359-381).
//!
//! Every `Item { ITEM MAX NUM }` row of an event is an [`ItemPacer`]: `MAX` drops a day, spread over the hours of the
//! day by the fixed weights of [`EVENT_ITEM_HOUR_WEIGHTS`](flyff_data::events::generic_event::EVENT_ITEM_HOUR_WEIGHTS)
//! (the interval of the local hour is [`EventItem::interval_ms`]). [`due_items`] walks the pacers of the events whose
//! flag is on and returns what a kill drops now; delivery (a ground item at the monster, or the inventory of a flying
//! monster's killer) is doc 19's `item::drop::event_items`.

use super::GenericEventService;
use crate::event::flags::EventFlags;
use flyff_common::GameMs;
use flyff_common::rng::GameRng;
use flyff_data::GameData;
use flyff_data::events::generic_event::EventItem;
use flyff_data::types::ItemId;

/// One `CEventItem`: the item, its limits and the pacing state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemPacer {
    /// The item that drops.
    pub item: ItemId,
    /// Drops a day (`m_nMax`).
    pub max: i32,
    /// The most items of one drop (`m_nNum`).
    pub num: i32,
    /// `m_dwTimeout`: the game time from which the next drop is due; 0 at load (the legacy `GetTickCount()` of the
    /// load, so the first kill after the load drops at once).
    pub timeout: GameMs,
    /// `m_lSkip`: the drops still to be swallowed after a multi-item drop.
    pub skip: i32,
    /// `m_adwInterval`: the milliseconds between two drops per local hour.
    pub interval: [u32; 24],
}

impl ItemPacer {
    /// The pacer of a row (`CEventItem::CEventItem`).
    pub fn new(e: &EventItem) -> ItemPacer {
        let mut interval = [0_u32; 24];
        for (h, slot) in interval.iter_mut().enumerate() {
            *slot = e.interval_ms(h);
        }
        ItemPacer {
            item: e.item,
            max: e.max,
            num: e.num,
            timeout: GameMs(0),
            skip: 0,
            interval,
        }
    }

    /// `CEventItem::IsTimeout( hour )`: due when `now >= timeout`; that run moves the timeout to
    /// `now + interval[hour]`, and a pending `skip` swallows the drop. An item with `max <= 0` or an interval of 0
    /// never drops (FIX: the original divided by zero, and an interval of 0 would loop forever).
    pub fn is_timeout(&mut self, now: GameMs, hour: u8) -> bool {
        let interval = self.interval.get(usize::from(hour)).copied().unwrap_or(0);
        if self.max <= 0 || interval == 0 || now < self.timeout {
            return false;
        }
        self.timeout = GameMs(now.0.saturating_add(u64::from(interval)));
        if self.skip > 0 {
            self.skip -= 1;
            return false;
        }
        true
    }
}

/// `CEventGeneric::GetItem`: the first due pacer of the events with a flag on (ascending event id, file order), or
/// `None`. The due item drops `n = xRandom( 1, num + 1 )` items and swallows the next `n - 1` drops.
fn next_due(
    svc: &mut GenericEventService,
    flags: &EventFlags,
    now: GameMs,
    hour: u8,
    rng: &mut dyn GameRng,
) -> Option<(ItemId, i32)> {
    for (id, list) in &mut svc.items {
        if flags.get(*id) == 0 {
            continue;
        }
        for p in list.iter_mut() {
            if p.is_timeout(now, hour) {
                let top = u32::try_from(p.num.max(0)).unwrap_or(0).saturating_add(1);
                let n = rng.random_range(1, top);
                let n = i32::try_from(n).unwrap_or(i32::MAX);
                p.skip = n - 1;
                return Some((p.item, n));
            }
        }
    }
    None
}

/// The generic event items a kill at game time `now` (local hour `hour`) drops: the legacy `while (GetItem(&n))`
/// loop, every due item once with its count, in the order found (22 §6.3). `EventDropHooks::generic_event_items`
/// returns this; doc 19 asks only when its level gate passed.
pub fn due_items(
    svc: &mut GenericEventService,
    flags: &EventFlags,
    data: &GameData,
    now: GameMs,
    hour: u8,
    rng: &mut dyn GameRng,
) -> Vec<(ItemId, i32)> {
    svc.ensure(data);
    let mut out = Vec::new();
    // every success moves a timeout past `now`, so a pacer is due at most once per call: the loop ends
    while let Some(due) = next_due(svc, flags, now, hour, rng) {
        out.push(due);
    }
    out
}
