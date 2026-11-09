//! `spevent.txt`: the hourly weights of the Christmas event drop (`CXMasEvent::LoadScript`, Plan/14 §6.14; ref/17
//! §8.5; src:_Common/spevent.cpp:25-70).
//!
//! ```text
//! MAX  W0 W1 ... W23          drops a day, then the weight of each hour of the day
//! ```
//!
//! The original turns them into `m_adwInterval[hour] = (DWORD)( (float)MIN( 60 ) / ( nMax * (float)an[i] /
//! (float)nTotal ) )` milliseconds ([`XmasEventTable::interval_ms`]).

use serde::{Deserialize, Serialize};

/// The Christmas event table (`GameData::xmas_event`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmasEventTable {
    /// Drops a day (4200).
    pub max: i32,
    /// The weight of each hour (505, 409, ... as in the event items' table).
    pub hour_weights: Vec<i32>,
}

impl XmasEventTable {
    /// `m_adwInterval[hour]` in ms; 0 when `max` or the total weight is not positive or the hour is out of range
    /// (the original divides by zero).
    pub fn interval_ms(&self, hour: usize) -> u32 {
        let total: i64 = self.hour_weights.iter().map(|w| i64::from(*w)).sum();
        let Some(w) = self.hour_weights.get(hour) else {
            return 0;
        };
        if self.max <= 0 || total <= 0 || *w <= 0 {
            return 0;
        }
        let minute = 60.0_f32 * 60.0 * 1000.0; // MIN( 60 )
        let per_ms = self.max as f32 * *w as f32 / total as f32;
        (minute / per_ms) as u32
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_spevent;

#[cfg(feature = "load")]
mod parse {
    use super::XmasEventTable;
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::sync::Arc;

    /// `CXMasEvent::LoadScript` over decoded `text`: the first number and the next 24.
    pub(crate) fn parse_spevent(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> XmasEventTable {
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let max = sc.get_number_i32();
        let hour_weights = (0..24).map(|_| sc.get_number_i32()).collect();
        XmasEventTable { max, hour_weights }
    }
}
