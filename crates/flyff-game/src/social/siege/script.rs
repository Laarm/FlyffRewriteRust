//! The siege script (Plan/21 §7.2, `CGuildCombat::ProcessCommand`, `src:_Common/eveschool.cpp:2853-2963`): the `OPEN`
//! block of `GuildCombat.txt` as a table with the jump targets of its `STATE` rows, and the interpreter that
//! [`super::process`] runs once per second.
//!
//! The interpreter runs **at most one row per call**: a row sets `step_due = now + time_ms`, so a row with wait 0 is
//! followed one second later (KEEP). The rows of the other two blocks (`MAINTENANCE`, `WAR`) are never run, in the
//! original as here.
//!
//! | row | what [`step`] does |
//! |---|---|
//! | `WORLDMSG` | `WorldMsg` (the text of TID `command`, `%d` = `time_ms / 1000`) to the siege world |
//! | `ALLMSG` | a system notice with the same text; TID 2710 (`TID_GAME_GUILDCOMBAT_OPEN_MSG`) sends `GC_ENTERTIME{time_ms}` instead |
//! | `STATE n` | the siege state becomes `n`, `GC_GCSTATE` to everybody |
//! | `WARSTATE n` | the phase action of `n` first (101 kicks the siege world to Madrigal, 102..=107 are [`PhaseHooks`]), then the war state becomes `n` and `GC_GCSTATE` goes out |
//! | `GUILDMSG`, `JOINMSG`, `WAIT` | nothing |
//!
//! After every row: `GC_NEXTTIMESTATE{time_ms, param}` to everybody when `param != 0`. `GC_NEXTTIMESTATE` and
//! `GC_ENTERTIME` go to EVERY online player (the original loops over all users; Plan/21 §7.2 says "world 202", the
//! client ignores them elsewhere: deviation noted in the crate NOTES).

use super::{PhaseHooks, SiegeService, SiegeState, SiegeWarState, StopWar};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, Services};
use crate::gm::text::format_positional;
use crate::outbox::Audience;
use flyff_common::defs::text::TID_GAME_GUILDCOMBAT_OPEN_MSG;
use flyff_common::defs::world::WI_WORLD_GUILDWAR;
use flyff_common::{GameMs, ObjId, WorldId};
use flyff_data::social::guild::{GcProcessRow, GcRowKind, GuildCombatSettings};
use flyff_proto::pb;
use std::sync::Arc;

/// The `OPEN` block of `GuildCombat.txt`: its rows, `open_count` (the number after `OPEN`: 14 with the 13 shipped
/// rows) and the jump target of every siege state.
///
/// FIX: the jump targets live in a map of their own. The original stored the index of a `STATE n` row in
/// `m_nProcessCount[n]`, the same array that holds the block counts, so a `STATE 0` row overwrote `OPEN`'s count; and a
/// missing `STATE 3` / `STATE 4` row left a zero, which jumped to row 0 (here the stop request is ignored with a
/// warning). The target is the index of the LAST such row (2 -> 4, 3 -> 7, 4 -> 11 in the shipped file).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptTable {
    rows: Vec<GcProcessRow>,
    open_count: usize,
    jumps: [Option<usize>; 5],
}

impl ScriptTable {
    /// The table of `process[0]` and `process_count[0]`.
    pub fn build(combat: &GuildCombatSettings) -> ScriptTable {
        let rows = combat.process[0].clone();
        let open_count = usize::try_from(combat.process_count[0]).unwrap_or(0);
        let mut jumps = [None; 5];
        for (i, row) in rows.iter().enumerate() {
            if row.kind != GcRowKind::State {
                continue;
            }
            match usize::try_from(row.command)
                .ok()
                .and_then(|c| jumps.get_mut(c))
            {
                Some(slot) => *slot = Some(i),
                None => tracing::warn!(
                    row = i,
                    state = row.command,
                    "GuildCombat.txt: a STATE row names a state outside 0..=4: no jump target"
                ),
            }
        }
        ScriptTable {
            rows,
            open_count,
            jumps,
        }
    }

    /// How many rows the interpreter may run (`m_nProcessCount[0]`).
    pub fn open_count(&self) -> usize {
        self.open_count
    }

    /// The loaded rows.
    pub fn rows(&self) -> &[GcProcessRow] {
        &self.rows
    }

    /// Row `i`; `None` beyond the loaded rows (the interpreter treats that like the zeroed C++ row: a no-op).
    pub fn row(&self, i: usize) -> Option<&GcProcessRow> {
        self.rows.get(i)
    }

    /// The row index to continue at when the siege is forced into `state` (the index of the last `STATE state` row).
    pub fn jump_target(&self, state: SiegeState) -> Option<usize> {
        usize::try_from(state.code())
            .ok()
            .and_then(|i| self.jumps.get(i))
            .copied()
            .flatten()
    }
}

/// The text of `tid` with `%d` = `seconds` (`CString::Format(prj.GetText(tid), time / 1000)`); `None` for a missing
/// text.
fn row_text(ctx: &GameCtx<'_>, tid: u32, seconds: u32) -> Option<String> {
    let template = ctx.data.texts.get(tid);
    if template.is_empty() || template == "error" {
        tracing::debug!(tid, "a siege script row names a missing text");
        return None;
    }
    Some(format_positional(template, &[seconds.to_string()]))
}

/// `ProcessCommand`: carries out a pending stop request, then runs the row `step` when it is due. Nothing once the
/// script is through (`step >= open_count`).
pub fn step(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let table = Arc::clone(&svc.social.siege.script);
    let now = ctx.clock.game_ms;
    let at = {
        let s = &mut svc.social.siege;
        if s.step >= table.open_count() {
            return;
        }
        apply_stop(s, &table);
        if now < s.step_due {
            return;
        }
        s.step
    };
    let row = table.row(at).copied().unwrap_or_default();
    run_row(svc, ctx, &row);
    let s = &mut svc.social.siege;
    s.step_due = GameMs(now.0.saturating_add(u64::from(row.time_ms)));
    if row.param != 0 {
        super::send_all(
            svc,
            ctx,
            pb::guild_combat::Sub::GcNextTimeState(pb::guild_combat::GcNextTimeState {
                time_ms: row.time_ms,
                state: row.param,
            }),
        );
    }
    svc.social.siege.step += 1;
}

/// `m_nStopWar` 1 and 2: jump to the `STATE 3` / `STATE 4` row with the timer cleared. A missing target ignores the
/// request with a warning (the original jumped to row 0).
fn apply_stop(s: &mut SiegeService, table: &ScriptTable) {
    let (state, gm) = match s.stop_war {
        StopWar::None => return,
        StopWar::Early => (SiegeState::CompleteClose, false),
        StopWar::GmCancel => (SiegeState::GmClose, true),
    };
    s.stop_war = StopWar::None;
    match table.jump_target(state) {
        Some(target) => {
            s.step = target;
            s.step_due = GameMs(0);
            if gm {
                s.gm_cancelled = true;
            }
        }
        None => tracing::warn!(
            ?state,
            "the siege script has no STATE row for the stop request: ignored"
        ),
    }
}

/// One row (`switch ( GuildCombatProcess[m_nProcessGo].nState )`).
fn run_row(svc: &mut Services, ctx: &mut GameCtx<'_>, row: &GcProcessRow) {
    let seconds = row.time_ms / 1000;
    match row.kind {
        GcRowKind::WorldMsg => {
            if let Some(text) = row_text(ctx, row.command, seconds) {
                ctx.broadcast(
                    Some(svc),
                    ObjId::NULL,
                    Audience::World(WorldId(WI_WORLD_GUILDWAR)),
                    pb::WorldMsg { text },
                );
            }
        }
        GcRowKind::AllMsg if row.command == TID_GAME_GUILDCOMBAT_OPEN_MSG => {
            super::send_all(
                svc,
                ctx,
                pb::guild_combat::Sub::GcEnterTime(pb::guild_combat::GcEnterTime {
                    time_ms: row.time_ms,
                }),
            );
        }
        GcRowKind::AllMsg => {
            if let Some(text) = row_text(ctx, row.command, seconds) {
                notice::system(svc, ctx, &text);
            }
        }
        GcRowKind::State => match i32::try_from(row.command)
            .ok()
            .and_then(SiegeState::from_code)
        {
            Some(state) => super::set_state(svc, ctx, state),
            None => tracing::warn!(
                state = row.command,
                "a siege script STATE row names an unknown state: ignored"
            ),
        },
        GcRowKind::WarState => {
            let Some(war) = i32::try_from(row.command)
                .ok()
                .and_then(SiegeWarState::from_code)
            else {
                tracing::warn!(
                    war_state = row.command,
                    "a siege script WARSTATE row names an unknown war state: ignored"
                );
                return;
            };
            // the action FIRST, then the state and its broadcast (the original order: `close_teleport` has set the
            // siege state to `Close` before row 12 announces `(1, 107)`)
            phase_action(svc, ctx, war);
            super::set_war_state(svc, ctx, war);
        }
        GcRowKind::GuildMsg | GcRowKind::JoinMsg | GcRowKind::Wait | GcRowKind::None => {}
    }
}

/// What entering the war state `war` does (`ProcessCommand`, the `WARSTATE` branch).
fn phase_action(svc: &mut Services, ctx: &mut GameCtx<'_>, war: SiegeWarState) {
    let hooks: PhaseHooks = svc.social.siege.hooks;
    match war {
        SiegeWarState::NotEnterCount => super::kick_to_madrigal(ctx),
        SiegeWarState::Enter => (hooks.set_enter)(svc, ctx),
        SiegeWarState::Maintenance => (hooks.set_maintenance)(svc, ctx),
        SiegeWarState::WarWar => (hooks.start_fight)(svc, ctx),
        SiegeWarState::WarClose => (hooks.close_fight)(svc, ctx),
        SiegeWarState::WarCloseWait => (hooks.results)(svc, ctx),
        SiegeWarState::WarTeleport => (hooks.close_teleport)(svc, ctx),
        SiegeWarState::NotEnter => {}
    }
}
