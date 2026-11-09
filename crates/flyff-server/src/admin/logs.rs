//! The log ring buffer of the admin console (Plan/25 §9.8; WP-ADMIN-26): a `tracing` layer that keeps the last
//! [`CAPACITY`] lines in memory, and the cursor API the admin WebSocket streams them with (`LogSubscribe` /
//! `LogLine`, capability `act.server`).
//!
//! * [`LogRing`] holds the lines (capacity 10,000 and a byte budget, so a flood of long lines cannot grow it past
//!   [`BYTE_BUDGET`]; the whole of history and logs stays under the 16 MB of Plan/25 §13). Every line gets a
//!   sequence number; a reader keeps the number of the next line it wants ([`LogRing::since`]) and is told when the
//!   ring overtook it (`lost`).
//! * [`LogRing::layer`] is the `tracing` layer: it sits behind the reloadable filter of `logging::init`, so the ring
//!   shows what `log.level` shows. It never logs itself and never blocks for long (one short mutex).
//! * [`global`] is the ring of the process: `logging::init` installs its layer, `AdminShared` serves it. Tests build
//!   their own rings and feed them with [`LogRing::push`].

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// Lines kept (Plan/25 §9.8).
pub const CAPACITY: usize = 10_000;
/// Bytes of text kept at most; the oldest lines go first.
pub const BYTE_BUDGET: usize = 6 << 20;
/// Longest stored text of one line (characters); the rest is cut with `…`.
pub const MAX_TEXT_CHARS: usize = 1024;
/// Most lines one [`LogRing::since`] call returns.
pub const BATCH: usize = 200;

/// The level numbers of the protocol (`LogSubscribe.min_level`: 1 error .. 5 trace).
pub fn level_number(level: &Level) -> u8 {
    match *level {
        Level::ERROR => 1,
        Level::WARN => 2,
        Level::INFO => 3,
        Level::DEBUG => 4,
        Level::TRACE => 5,
    }
}

/// The name of a level number.
pub fn level_name(n: u8) -> &'static str {
    match n {
        1 => "ERROR",
        2 => "WARN",
        3 => "INFO",
        4 => "DEBUG",
        _ => "TRACE",
    }
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// Sequence number (1, 2, ...; never reused).
    pub seq: u64,
    /// Unix milliseconds.
    pub ts_ms: i64,
    /// 1 error .. 5 trace.
    pub level: u8,
    /// The `tracing` target (`flyff_server::admin::ws`).
    pub target: String,
    /// The message and the fields (`message key=value ...`).
    pub text: String,
}

/// What a subscriber wants to see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogFilter {
    /// Lines up to this level (1 error .. 5 trace).
    pub max_level: u8,
    /// The target starts with this (empty = all).
    pub target_prefix: String,
}

impl Default for LogFilter {
    fn default() -> Self {
        LogFilter {
            max_level: 5,
            target_prefix: String::new(),
        }
    }
}

impl LogFilter {
    /// `true` when `e` passes.
    pub fn matches(&self, e: &LogEntry) -> bool {
        e.level <= self.max_level && e.target.starts_with(&self.target_prefix)
    }
}

/// What [`LogRing::since`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogBatch {
    /// The matching lines, oldest first (at most [`BATCH`]).
    pub lines: Vec<LogEntry>,
    /// The sequence number to ask for next.
    pub next: u64,
    /// Lines between the asked-for number and the oldest line still in the ring were overwritten.
    pub lost: bool,
}

#[derive(Default)]
struct Inner {
    lines: VecDeque<LogEntry>,
    /// The sequence number of the next line.
    next_seq: u64,
    bytes: usize,
}

/// The ring buffer (see the module documentation).
pub struct LogRing {
    inner: Mutex<Inner>,
    capacity: usize,
    byte_budget: usize,
}

impl std::fmt::Debug for LogRing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogRing")
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

impl Default for LogRing {
    fn default() -> Self {
        LogRing::new(CAPACITY, BYTE_BUDGET)
    }
}

impl LogRing {
    /// A ring of `capacity` lines and `byte_budget` bytes of text.
    pub fn new(capacity: usize, byte_budget: usize) -> LogRing {
        LogRing {
            inner: Mutex::new(Inner {
                next_seq: 1,
                ..Inner::default()
            }),
            capacity: capacity.max(1),
            byte_budget: byte_budget.max(1024),
        }
    }

    /// Appends a line stamped `ts_ms`. Returns its sequence number.
    pub fn push_at(&self, ts_ms: i64, level: u8, target: &str, text: &str) -> u64 {
        let text = cut(text);
        let mut g = self.inner.lock();
        let seq = g.next_seq;
        g.next_seq += 1;
        g.bytes += text.len() + target.len() + 48;
        g.lines.push_back(LogEntry {
            seq,
            ts_ms,
            level,
            target: target.to_owned(),
            text,
        });
        while g.lines.len() > self.capacity || (g.bytes > self.byte_budget && g.lines.len() > 1) {
            if let Some(old) = g.lines.pop_front() {
                g.bytes = g
                    .bytes
                    .saturating_sub(old.text.len() + old.target.len() + 48);
            }
        }
        seq
    }

    /// Appends a line stamped now.
    pub fn push(&self, level: u8, target: &str, text: &str) -> u64 {
        self.push_at(now_ms(), level, target, text)
    }

    /// The number the next line will get (what a new subscriber that wants only new lines starts at).
    pub fn next_seq(&self) -> u64 {
        self.inner.lock().next_seq
    }

    /// Lines in the ring.
    pub fn len(&self) -> usize {
        self.inner.lock().lines.len()
    }

    /// `true` when the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes of text held.
    pub fn bytes(&self) -> usize {
        self.inner.lock().bytes
    }

    /// The sequence number from which the last `n` lines matching `filter` start (a new subscriber's first
    /// position); `next_seq` when there are none.
    pub fn tail_start(&self, filter: &LogFilter, n: usize) -> u64 {
        let g = self.inner.lock();
        g.lines
            .iter()
            .rev()
            .filter(|e| filter.matches(e))
            .take(n)
            .last()
            .map_or(g.next_seq, |e| e.seq)
    }

    /// The lines from sequence number `from` on that match `filter`, at most `limit` (and [`BATCH`]).
    pub fn since(&self, from: u64, filter: &LogFilter, limit: usize) -> LogBatch {
        let g = self.inner.lock();
        let oldest = g.lines.front().map_or(g.next_seq, |e| e.seq);
        let lost = from < oldest;
        let start = from.max(oldest);
        let skip = usize::try_from(start - oldest).unwrap_or(usize::MAX);
        let limit = limit.clamp(1, BATCH);
        let mut lines = Vec::new();
        let mut next = start.min(g.next_seq);
        for e in g.lines.iter().skip(skip) {
            next = e.seq + 1;
            if filter.matches(e) {
                lines.push(e.clone());
                if lines.len() >= limit {
                    break;
                }
            }
        }
        LogBatch { lines, next, lost }
    }

    /// The `tracing` layer that feeds this ring.
    pub fn layer(self: &Arc<Self>) -> LogLayer {
        LogLayer {
            ring: Arc::clone(self),
        }
    }
}

/// The ring of the process (`logging::init` installs its layer, `AdminShared` serves it).
pub fn global() -> &'static Arc<LogRing> {
    static RING: OnceLock<Arc<LogRing>> = OnceLock::new();
    RING.get_or_init(|| Arc::new(LogRing::default()))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn cut(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT_CHARS {
        return text.to_owned();
    }
    let mut s: String = text.chars().take(MAX_TEXT_CHARS).collect();
    s.push('…');
    s
}

/// The layer: one event is `message key=value ...`.
pub struct LogLayer {
    ring: Arc<LogRing>,
}

#[derive(Default)]
struct Fields {
    message: String,
    rest: String,
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            if !self.rest.is_empty() {
                self.rest.push(' ');
            }
            let _ = write!(self.rest, "{}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            if !self.rest.is_empty() {
                self.rest.push(' ');
            }
            let _ = write!(self.rest, "{}={value}", field.name());
        }
    }
}

impl<S: Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut f = Fields::default();
        event.record(&mut f);
        let text = if f.rest.is_empty() {
            f.message
        } else if f.message.is_empty() {
            f.rest
        } else {
            format!("{} {}", f.message, f.rest)
        };
        self.ring
            .push(level_number(meta.level()), meta.target(), &text);
    }
}
