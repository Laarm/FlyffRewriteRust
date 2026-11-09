//! `propEvent.inc`: the generic events (`CEventGeneric::LoadScript`, Plan/14 §6.14, ref/17 §8.7, ref/15 §5;
//! src:_Common/spevent.cpp:183-312).
//!
//! ```text
//! Event ID '{' { Title '{' (LANG rest-of-line)* '}' | Time '{' y m d h  y m d h '}' | Item '{' (ITEM MAX NUM)* '}'
//!              | fExpFactor F | fItemDropRate F | Spawn '{' TYPE INDEX MAX RATIO '}' }* '}'
//! ```
//!
//! Three events are active in this data set (501, 502, 531; the block of 500 is commented out), all expired
//! in 2009. The loader keeps everything a world server needs: the id and its flag bit (`1 << (id - 500)`), the
//! title of the server language (`LANG_ENG`, the whole rest of the line with the quotes removed), the begin and
//! end time as `[year, month, day, hour]` (the DB server turns them into `time_t` in local time; that is the
//! event scheduler's job, doc 22), the drop items, the exp and drop-rate factors (`1.0` by default) and the
//! item spawn.

use serde::{Deserialize, Serialize};

/// The server language of this build (`LANG_ENG`).
pub const LANG_ENG: u32 = 1;

/// The first generic event id; the flag of an event is `1 << (id - EVENT_BASE)`.
pub const EVENT_BASE: u32 = 500;

/// The hour weights of `CEventItem`'s drop interval (`s_anHour` in `spevent.cpp`): the share of an item's daily
/// drops in each hour of the day.
pub const EVENT_ITEM_HOUR_WEIGHTS: [u32; 24] = [
    505, 409, 324, 280, 220, 203, 202, 212, 227, 261, 302, 349, 571, 701, 764, 803, 790, 789, 754,
    849, 936, 940, 919, 720,
];

/// One `Item { ITEM MAX NUM }` row of an event (`CEventItem`): a drop that the event adds to monster kills.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventItem {
    /// The item that drops.
    pub item: u32,
    /// How many drops a day (`m_nMax`).
    pub max: i32,
    /// The most items of one drop (`m_nNum`).
    pub num: i32,
}

impl EventItem {
    /// `m_adwInterval[hour]` in milliseconds: `MIN( 60 ) / ( max * weight / total )` in `f32`, truncated to
    /// `DWORD` (`CEventItem::CEventItem`). 0 for `max <= 0` or an hour outside 0..24 (the original divides by
    /// zero; the interval would be infinite).
    pub fn interval_ms(&self, hour: usize) -> u32 {
        let Some(w) = EVENT_ITEM_HOUR_WEIGHTS.get(hour) else {
            return 0;
        };
        if self.max <= 0 {
            return 0;
        }
        let total: u32 = EVENT_ITEM_HOUR_WEIGHTS.iter().sum();
        let minute = 60.0_f32 * 60.0 * 1000.0; // MIN( 60 ) = 60 minutes in ms
        (minute / (self.max as f32 * *w as f32 / total as f32)) as u32
    }
}

/// One `Spawn { TYPE INDEX MAX RATIO }` row of an event (`CSpawn`): a world item that appears in random level
/// regions while the event is active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EventSpawn {
    /// The object type (`OT_ITEM`).
    pub object_type: u32,
    /// The object index (the item id).
    pub index: u32,
    /// How many spawns a day (`m_nMax`).
    pub max: i32,
    /// The weight ratio of the highest to the lowest region level (`m_fRatio`, must be > 0).
    pub ratio: f32,
    /// `86400000 / max` ms between two spawns (`m_dwInterval`; 0 for `max <= 0`).
    pub interval_ms: u32,
}

/// One generic event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenericEvent {
    /// The event id (500..=531: bit `id - 500` of the event flag word).
    pub id: u32,
    /// `nFlag`: `1 << (id - 500)` (0 for an id below 500 or above 531).
    pub flag: u32,
    /// The English title (empty when the file has none).
    pub title: String,
    /// `[year, month, day, hour]` of the begin.
    pub begin: [i32; 4],
    /// `[year, month, day, hour]` of the end.
    pub end: [i32; 4],
    /// `fExpFactor` (default 1).
    pub exp_factor: f32,
    /// `fItemDropRate` (default 1; `__ITEMDROPRATE`).
    pub item_drop_rate: f32,
    /// The drops the event adds (`Item`).
    pub items: Vec<EventItem>,
    /// The item spawns of the event (`Spawn`; the original keeps one per event: `std::map::insert` keeps the FIRST,
    /// all are kept here in file order).
    pub spawns: Vec<EventSpawn>,
}

impl Default for GenericEvent {
    fn default() -> Self {
        Self {
            id: 0,
            flag: 0,
            title: String::new(),
            begin: [0; 4],
            end: [0; 4],
            exp_factor: 1.0,
            item_drop_rate: 1.0,
            items: Vec::new(),
            spawns: Vec::new(),
        }
    }
}

/// `1 << (id - 500)`, or 0 when the id has no bit in the 32-bit flag word.
pub fn event_flag(id: u32) -> u32 {
    id.checked_sub(EVENT_BASE)
        .and_then(|b| 1_u32.checked_shl(b))
        .unwrap_or(0)
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_generic_events;

#[cfg(feature = "load")]
mod parse {
    use super::{EventItem, EventSpawn, GenericEvent, LANG_ENG, event_flag};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `CEventGeneric::LoadScript` over decoded `text`.
    pub(crate) fn parse_generic_events(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<GenericEvent> {
        let mut out = Vec::new();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            if sc.token() == "Event" {
                let id = sc.get_number();
                let mut ev = GenericEvent {
                    id,
                    flag: event_flag(id),
                    ..GenericEvent::default()
                };
                if ev.flag == 0 {
                    sc.report(
                        Severity::Warning,
                        DiagCode::ValueOutOfRange,
                        format!("propEvent.inc: event {id} has no bit in the event flag word (500..=531)"),
                    );
                }
                sc.get_token(); // {
                sc.get_token();
                while open(&sc) {
                    match sc.token().to_owned().as_str() {
                        "Title" => {
                            sc.get_token(); // {
                            let mut lang = sc.get_number();
                            while open(&sc) {
                                let line = sc.get_last_full().replace('"', "");
                                if lang == LANG_ENG {
                                    ev.title = line;
                                }
                                lang = sc.get_number();
                            }
                        }
                        "Time" => {
                            sc.get_token(); // {
                            for slot in ev.begin.iter_mut().chain(ev.end.iter_mut()) {
                                *slot = sc.get_number_i32();
                            }
                            sc.get_token(); // }
                        }
                        "Item" => {
                            sc.get_token(); // {
                            let mut item = sc.get_number();
                            while open(&sc) {
                                let max = sc.get_number_i32();
                                let num = sc.get_number_i32();
                                ev.items.push(EventItem { item, max, num });
                                item = sc.get_number();
                            }
                        }
                        "fExpFactor" => ev.exp_factor = sc.get_float(),
                        "fItemDropRate" => ev.item_drop_rate = sc.get_float(),
                        "Spawn" => {
                            sc.get_token(); // {
                            let object_type = sc.get_number();
                            let index = sc.get_number();
                            let max = sc.get_number_i32();
                            let ratio = sc.get_float();
                            sc.get_token(); // }
                            if max <= 0
                                || ratio.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
                            {
                                sc.report(
                                    Severity::Error,
                                    DiagCode::ValueOutOfRange,
                                    format!("propEvent.inc: event {id}: Spawn max {max} / ratio {ratio} must be positive"),
                                );
                            }
                            let interval_ms = u32::try_from(
                                86_400_000_i64.checked_div(i64::from(max)).unwrap_or(0),
                            )
                            .unwrap_or(0);
                            ev.spawns.push(EventSpawn {
                                object_type,
                                index,
                                max,
                                ratio,
                                interval_ms,
                            });
                        }
                        _ => {}
                    }
                    sc.get_token();
                }
                out.push(ev);
            }
            sc.get_token();
        }
        out
    }
}
