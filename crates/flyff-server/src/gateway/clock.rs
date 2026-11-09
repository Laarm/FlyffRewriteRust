//! The `ServerTime` estimate and the per-session clock offset (Plan/10 §7.6, WP-NET-03).
//!
//! The gateway answers `ServerTimeReq` without the game thread. The frame estimate is the number of 60 Hz sim
//! frames completed (`4n`, published at the start of tick `n`) plus the frames elapsed since that moment, capped
//! at +3 (11 §8.1). The server-side offset of a session is the median of `estimate − client_frame` over the last
//! five samples; a change of at least one frame is reported to the game as `GameInput::ClockOffset`.

use flyff_common::rt::Telemetry;
use std::sync::atomic::Ordering;

/// Nanoseconds in one 60 Hz sim frame (16.666… ms, rounded up so the estimate never runs ahead).
const FRAME_NS: u64 = 16_666_667;

/// The frame estimate at "now" (10 §7.6): `frames_completed + min(elapsed since the tick started / 16.67 ms, 3)`.
/// With manually run ticks (tests, `Telemetry::manual_ticks`) no time passes between ticks: `frames_completed`.
pub fn estimate_server_frame(t: &Telemetry) -> u64 {
    // `tick_start_ns` is written before `frames_completed` (Release): read them in the opposite order.
    let base = t.frames_completed.load(Ordering::Acquire);
    if t.manual_ticks.load(Ordering::Relaxed) {
        return base;
    }
    let start = t.tick_start_ns.load(Ordering::Acquire);
    let elapsed = t.now_ns().saturating_sub(start);
    base + (elapsed / FRAME_NS).min(3)
}

/// Median of the last five offset samples with the "report on change" rule.
#[derive(Clone, Debug, Default)]
pub struct OffsetFilter {
    samples: [i64; 5],
    len: usize,
    next: usize,
    last_sent: Option<i64>,
}

impl OffsetFilter {
    /// Adds one sample (`estimate − client_frame`) and returns the median of the last five.
    pub fn push(&mut self, sample: i64) -> i64 {
        if let Some(slot) = self.samples.get_mut(self.next) {
            *slot = sample;
        }
        self.next = (self.next + 1) % self.samples.len();
        self.len = (self.len + 1).min(self.samples.len());
        self.median().unwrap_or(sample)
    }

    /// The median of the samples so far.
    pub fn median(&self) -> Option<i64> {
        if self.len == 0 {
            return None;
        }
        let mut v: Vec<i64> = self.samples.iter().copied().take(self.len).collect();
        v.sort_unstable();
        v.get(v.len() / 2).copied()
    }

    /// `Some(offset)` when `offset` must be (re-)sent to the game: the first time, or when it moved by at least
    /// one frame since the last value sent. Records it as sent.
    pub fn to_send(&mut self, offset: i64) -> Option<i64> {
        match self.last_sent {
            Some(prev) if (offset - prev).abs() < 1 => None,
            _ => {
                self.last_sent = Some(offset);
                Some(offset)
            }
        }
    }

    /// Forgets what was sent (the avatar re-entered the world: the game needs the offset again).
    pub fn reset_sent(&mut self) {
        self.last_sent = None;
    }
}
