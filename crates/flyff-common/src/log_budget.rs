//! `LogBudget`: the per-session limiter of client-caused WARN lines (Plan/10 §10.4, 02 §12).
//!
//! A client can provoke WARN lines at will (malformed input, out-of-state messages, rejected game rules).
//! Each session (the gateway task and the game's `SessionEntry`) owns one budget: at most
//! `log.client_warn_per_min` lines per rolling 60 s window pass; the rest are counted and summarised in one
//! line (`suppressed N warnings for session S`) when the window ends. Server-caused warnings never go
//! through a budget. The type is clock-free (callers pass milliseconds) so it works on the game thread and
//! in the gateway alike.

/// Length of one budget window.
pub const WINDOW_MS: u64 = 60_000;

/// What [`LogBudget::admit`] decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Admit {
    /// `true`: log this line. `false`: it is suppressed (and counted).
    pub pass: bool,
    /// The previous window ended with this many suppressed lines: log one summary line
    /// (`suppressed N warnings for session S`) before the line itself.
    pub summary: Option<u32>,
}

/// Counter state of one session's client-WARN budget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogBudget {
    window_start: u64,
    used: u32,
    suppressed: u32,
}

impl LogBudget {
    /// A fresh budget; its first window starts with the first line.
    pub const fn new() -> Self {
        Self {
            window_start: 0,
            used: 0,
            suppressed: 0,
        }
    }

    /// Asks whether one more client-caused WARN line may be logged at `now_ms`. `limit_per_min == 0`
    /// means unlimited.
    pub fn admit(&mut self, now_ms: u64, limit_per_min: u32) -> Admit {
        let summary = self.roll(now_ms);
        let pass = if limit_per_min == 0 || self.used < limit_per_min {
            self.used = self.used.saturating_add(1);
            true
        } else {
            self.suppressed = self.suppressed.saturating_add(1);
            false
        };
        Admit { pass, summary }
    }

    /// Ends an elapsed window without a new line: returns the number of lines it suppressed, once. Call it
    /// from a periodic place (the 1 s slot, the gateway tick) so a session that went quiet still gets its
    /// summary.
    pub fn poll(&mut self, now_ms: u64) -> Option<u32> {
        self.roll(now_ms)
    }

    /// Lines suppressed in the current window.
    pub fn suppressed(&self) -> u32 {
        self.suppressed
    }

    fn roll(&mut self, now_ms: u64) -> Option<u32> {
        let started = self.used > 0 || self.suppressed > 0;
        if !started {
            self.window_start = now_ms;
            return None;
        }
        if now_ms.saturating_sub(self.window_start) < WINDOW_MS {
            return None;
        }
        let summary = (self.suppressed > 0).then_some(self.suppressed);
        self.window_start = now_ms;
        self.used = 0;
        self.suppressed = 0;
        summary
    }
}
