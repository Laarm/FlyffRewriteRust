//! `SerialAllocator`: unique item serials from reserved blocks (Plan/13 §15.1, WP-DB-06; PL-050).
//!
//! The legacy server used a random 32-bit `xRand()` serial per item (ref/05 §12), so two items could share one.
//! Here the game thread holds blocks of `[database] serial_block_size` serials that the writer reserved and
//! committed **before** they are handed out (`UPDATE meta SET value = value + size RETURNING ...`), so a crash
//! can only waste serials, never repeat one.
//!
//! * Two blocks exist at start (`GameInit.serial_blocks`): the current one and a spare.
//! * When the current block runs out the spare becomes current and `DbCmd::ReserveSerialBlock` goes out
//!   ([`BlockSource::request_block`]); the answer is [`SerialAllocator::on_block`] (`DbReply::SerialBlock`).
//! * If both are empty (more serials than a block within one database round trip) the allocator reserves a
//!   block **synchronously** ([`BlockSource::reserve_blocking`], at most [`BLOCKING_TIMEOUT`]) and logs a
//!   warning; if even that fails [`SerialError`] is returned and the caller refuses the operation.
//! * Serial `0` is never returned (it means "assign one" in grants and templates).
//!
//! The allocator knows nothing about channels or tickets: callers pass a [`BlockSource`]. In the game that is
//! [`GamePersist`] (the persistence handle and the ticket registry of the `GameCtx`); tests pass a fake.

use crate::tickets::{Pending, TicketRegistry};
use flyff_db::{DbError, PersistHandle};
use std::collections::VecDeque;
use std::ops::Range;
use std::time::Duration;

/// How long the blocking fallback waits for the writer (13 §15.1 "≤ 50 ms"; a slower writer is an error).
pub const BLOCKING_TIMEOUT: Duration = Duration::from_millis(50);

/// Where new blocks come from.
pub trait BlockSource {
    /// Asks for one more block; the answer comes back through [`SerialAllocator::on_block`].
    fn request_block(&mut self);

    /// Reserves one block and waits for the commit, at most `timeout`.
    fn reserve_blocking(&mut self, timeout: Duration) -> Result<Range<u64>, DbError>;
}

/// The production [`BlockSource`]: the persistence handle plus the ticket registry that routes the
/// `DbReply::SerialBlock` back (`Pending::SerialBlock`).
pub struct GamePersist<'a> {
    /// `GameCtx.persist`.
    pub persist: &'a PersistHandle,
    /// `GameCtx.tickets`.
    pub tickets: &'a mut TicketRegistry,
}

impl BlockSource for GamePersist<'_> {
    fn request_block(&mut self) {
        let ticket = self.tickets.issue(Pending::SerialBlock);
        self.persist.reserve_serial_block(ticket);
    }

    fn reserve_blocking(&mut self, timeout: Duration) -> Result<Range<u64>, DbError> {
        self.persist.reserve_serial_block_blocking(timeout)
    }
}

/// No serial could be produced.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("no item serial available: {0}")]
pub struct SerialError(pub String);

/// Counters (tests, `/metrics` later).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SerialStats {
    /// Serials handed out.
    pub issued: u64,
    /// Blocks that became the current block.
    pub blocks_adopted: u64,
    /// Asynchronous refills requested.
    pub refills_requested: u64,
    /// Blocking reservations (the spare was empty; each one is logged as a warning).
    pub blocking_fallbacks: u64,
}

/// The game thread's serial source (13 §15.1).
#[derive(Debug, Default)]
pub struct SerialAllocator {
    current: Range<u64>,
    /// Reserved blocks waiting to become current, oldest first.
    spare: VecDeque<Range<u64>>,
    /// Asynchronous requests whose answer has not arrived.
    in_flight: u32,
    stats: SerialStats,
}

/// A block without serial `0`.
fn without_zero(block: Range<u64>) -> Range<u64> {
    if block.start == 0 {
        1..block.end.max(1)
    } else {
        block
    }
}

impl SerialAllocator {
    /// The allocator over the two blocks reserved at start (`GameInit.serial_blocks`; empty ranges are
    /// ignored, so a test world without a database starts empty and falls back to blocking reservations).
    pub fn new(blocks: [Range<u64>; 2]) -> SerialAllocator {
        let mut a = SerialAllocator::default();
        for block in blocks {
            a.add_block(block);
        }
        a
    }

    fn add_block(&mut self, block: Range<u64>) {
        let block = without_zero(block);
        if block.is_empty() {
            return;
        }
        if self.current.is_empty() {
            self.current = block;
            self.stats.blocks_adopted += 1;
        } else {
            self.spare.push_back(block);
        }
    }

    /// The answer of a `ReserveSerialBlock` request (`DbReply::SerialBlock`).
    pub fn on_block(&mut self, block: Range<u64>) {
        self.in_flight = self.in_flight.saturating_sub(1);
        self.add_block(block);
    }

    /// Serials left in the current block and the spares.
    pub fn remaining(&self) -> u64 {
        let spare: u64 = self.spare.iter().map(|b| b.end - b.start).sum();
        (self.current.end - self.current.start) + spare
    }

    /// The counters.
    pub fn stats(&self) -> SerialStats {
        self.stats
    }

    /// The next serial, never 0 and never handed out before in this database.
    pub fn next(&mut self, src: &mut dyn BlockSource) -> Result<u64, SerialError> {
        loop {
            if let Some(serial) = self.current.next() {
                self.stats.issued += 1;
                if self.current.is_empty() {
                    // The block just ran out: the spare takes over at the next call and a new spare is ordered
                    // now, so the writer has a whole block of serials to answer in.
                    self.rotate();
                    self.top_up(src);
                }
                return Ok(serial);
            }
            if !self.rotate() {
                self.blocking_fallback(src)?;
            }
            self.top_up(src);
        }
    }

    /// Makes the oldest spare the current block; `false` when there is none.
    fn rotate(&mut self) -> bool {
        match self.spare.pop_front() {
            Some(block) => {
                self.current = block;
                self.stats.blocks_adopted += 1;
                true
            }
            None => false,
        }
    }

    /// Keeps one block in reserve: requests one when no spare exists and no request is outstanding.
    fn top_up(&mut self, src: &mut dyn BlockSource) {
        if self.spare.is_empty() && self.in_flight == 0 {
            self.in_flight += 1;
            self.stats.refills_requested += 1;
            src.request_block();
        }
    }

    fn blocking_fallback(&mut self, src: &mut dyn BlockSource) -> Result<(), SerialError> {
        self.stats.blocking_fallbacks += 1;
        tracing::warn!(
            in_flight = self.in_flight,
            "the item serial blocks ran out before the writer answered: reserving one synchronously"
        );
        let block = src
            .reserve_blocking(BLOCKING_TIMEOUT)
            .map_err(|e| SerialError(e.to_string()))?;
        self.add_block(block);
        if self.current.is_empty() {
            return Err(SerialError("the writer reserved an empty block".to_owned()));
        }
        Ok(())
    }
}
