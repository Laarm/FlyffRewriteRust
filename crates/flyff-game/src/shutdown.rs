//! The shutdown countdown on the game thread (Plan/10 §15.2 steps 2-4, 01 §6.10, WP-OPS-12).
//!
//! `GameInput::Shutdown{countdown_s, reason, message}` starts (or shortens) the countdown:
//!
//! * the lobby already stopped accepting logins (`Health::accepting_logins`) and joins that still arrive are
//!   refused with `JoinFailed{ShuttingDown}`;
//! * every in-world session gets a `SystemNotice` when the countdown starts and when the remaining time crosses
//!   60, 30, 10, 5, 4, 3, 2 and 1 s: `"Server shutting down in N seconds."` plus the operator's message;
//! * a cancel broadcasts `"Server shutdown cancelled."` and resumes logins;
//! * at the deadline (phase 3.7, `Game::shutdown_step` in `tick.rs`) everyone leaves **immediately** with the `Shutdown`
//!   trigger (final snapshots with the reason `Shutdown`), every sink receives `Kick{SHUTDOWN, 30000}` and is
//!   closed with `ServerShutdown`, then a `DbCmd::Flush` ticket completes the exit (`GameExit::Shutdown`).
//!
//! The countdown counts ticks (`deadline_tick`), never wall time, so a replayed run counts down identically.

use crate::input::AdminOutput;
use crate::persist::save::save_player;
use crate::tick::{Game, RunState};
use crate::tickets::{FlushPurpose, Pending};
use flyff_common::rt::{HealthState, Reply};
use flyff_common::{ObjId, Tick};
use flyff_db::SaveReason;
use flyff_proto::pb;
use std::sync::atomic::Ordering;

/// The remaining seconds at which a notice is sent, besides the one at the start of the countdown.
pub const NOTICE_MARKS_S: [u32; 8] = [60, 30, 10, 5, 4, 3, 2, 1];

/// The text sent when a shutdown is cancelled.
pub const CANCEL_TEXT: &str = "Server shutdown cancelled.";

/// A running countdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Countdown {
    /// The tick at which everyone is logged out.
    pub deadline_tick: u64,
    /// The operator's message appended to every notice.
    pub message: Option<String>,
    /// The remaining seconds of the last notice (a notice is sent when a smaller mark is reached).
    pub last_notice_s: u32,
}

/// `"Server shutting down in N seconds."` + the operator's message.
pub fn notice_text(remaining_s: u32, message: Option<&str>) -> String {
    let unit = if remaining_s == 1 {
        "second"
    } else {
        "seconds"
    };
    let mut text = format!("Server shutting down in {remaining_s} {unit}.");
    if let Some(m) = message.map(str::trim).filter(|m| !m.is_empty()) {
        text.push(' ');
        text.push_str(m);
    }
    text
}

/// Seconds until `deadline_tick` as the players count them: rounded up, so the notice for 5 s is sent when
/// 5 s are left and the last one when 1 s is left.
fn remaining_seconds(deadline_tick: u64, now: u64) -> u32 {
    let ticks = deadline_tick.saturating_sub(now);
    u32::try_from(ticks.div_ceil(Tick::PER_SECOND)).unwrap_or(u32::MAX)
}

impl Game {
    /// Sends `text` as a `SystemNotice` to every in-world session; returns how many got it.
    pub(crate) fn notice_all(&mut self, text: &str) -> usize {
        let sessions: Vec<_> = self.sessions.bound().map(|(s, _)| s).collect();
        for sid in &sessions {
            self.outbox.to_session(
                *sid,
                0,
                pb::SystemNotice {
                    text: text.to_owned(),
                },
            );
        }
        sessions.len()
    }

    /// `GameInput::Shutdown`: starts the countdown (immediately final for `0` or when nobody is connected), or
    /// shortens a running one. A longer request than the running countdown is ignored.
    pub(crate) fn on_shutdown_request(&mut self, countdown_s: u32, message: Option<String>) {
        let health = &self.telemetry.health;
        health.set_state(HealthState::ShuttingDown);
        health.accepting_logins.store(false, Ordering::Relaxed);
        let now = self.clock.tick.0;
        let deadline_tick = now.saturating_add(u64::from(countdown_s) * Tick::PER_SECOND);
        match &self.run_state {
            RunState::Running => {
                if countdown_s == 0 || self.outbox.is_empty() {
                    self.begin_final_shutdown();
                    return;
                }
                let unix_s = self.clock.unix_ms.to_unix_time().0 + i64::from(countdown_s);
                self.telemetry
                    .health
                    .shutdown_deadline_unix_s
                    .store(unix_s, Ordering::Relaxed);
                let text = notice_text(countdown_s, message.as_deref());
                self.run_state = RunState::ShuttingDown(Countdown {
                    deadline_tick,
                    message,
                    last_notice_s: countdown_s,
                });
                self.notice_all(&text);
            }
            // A shorter new request shortens the running countdown; a longer one is ignored.
            RunState::ShuttingDown(current) if deadline_tick < current.deadline_tick => {
                if countdown_s == 0 {
                    self.begin_final_shutdown();
                    return;
                }
                let message = message.or_else(|| current.message.clone());
                let text = notice_text(countdown_s, message.as_deref());
                let unix_s = self.clock.unix_ms.to_unix_time().0 + i64::from(countdown_s);
                self.telemetry
                    .health
                    .shutdown_deadline_unix_s
                    .store(unix_s, Ordering::Relaxed);
                self.run_state = RunState::ShuttingDown(Countdown {
                    deadline_tick,
                    message,
                    last_notice_s: countdown_s,
                });
                self.notice_all(&text);
            }
            _ => {}
        }
    }

    /// Phase 3.7, first half: the notice for the mark the countdown just crossed (the lowest one when a shortened
    /// countdown crossed several at once).
    pub(crate) fn countdown_notices(&mut self) {
        let now = self.clock.tick.0;
        let RunState::ShuttingDown(c) = &self.run_state else {
            return;
        };
        let remaining = remaining_seconds(c.deadline_tick, now);
        let Some(mark) = NOTICE_MARKS_S
            .iter()
            .copied()
            .filter(|m| *m < c.last_notice_s && remaining <= *m)
            .min()
        else {
            return;
        };
        let text = notice_text(remaining.max(1), c.message.as_deref());
        if let RunState::ShuttingDown(c) = &mut self.run_state {
            c.last_notice_s = mark;
        }
        self.notice_all(&text);
    }

    /// `AdminCmd::SaveAll` (console `save-all`, Plan/10 §13.1): one `Urgent` snapshot for every character in the
    /// world (a lingering avatar included), then a persistence flush; the answer arrives when everything queued
    /// before the flush committed (`AdminOutput::Done{affected}` = snapshots queued).
    pub(crate) fn admin_save_all(&mut self, reply: Reply<AdminOutput>) {
        let players: Vec<ObjId> = self.sessions.iter().map(|e| e.player).collect();
        let mut affected = 0_u32;
        {
            let (svc, mut ctx) = self.split();
            for player in players {
                if save_player(svc, &mut ctx, player, SaveReason::Urgent, None).is_some() {
                    affected += 1;
                }
            }
        }
        if self.persist.is_detached() {
            // nothing to wait for without a database (replay, tests)
            let _ = reply.send(AdminOutput::Done { affected });
            return;
        }
        let ticket = self
            .tickets
            .issue(Pending::Flush(FlushPurpose::SaveAll { reply, affected }));
        self.persist.flush(ticket);
    }

    /// `AdminCmd::CancelShutdown`: back to `Running` while the countdown runs, with the cancel notice.
    pub(crate) fn cancel_shutdown(&mut self) -> AdminOutput {
        match self.run_state {
            RunState::ShuttingDown(_) => {
                self.run_state = RunState::Running;
                self.telemetry.health.set_state(HealthState::Running);
                self.telemetry
                    .health
                    .accepting_logins
                    .store(true, Ordering::Relaxed);
                self.telemetry
                    .health
                    .shutdown_deadline_unix_s
                    .store(0, Ordering::Relaxed);
                self.notice_all(CANCEL_TEXT);
                AdminOutput::Lines(vec![CANCEL_TEXT.to_owned()])
            }
            _ => AdminOutput::Error("no shutdown countdown is running".to_owned()),
        }
    }
}
