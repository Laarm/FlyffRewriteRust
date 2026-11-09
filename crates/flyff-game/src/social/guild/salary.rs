//! The guild salary (Plan/21 §5.10; WP-SOCIAL-14; KQ-29, PL-094; ref/14 §5.6; `CGuildMng::Process`,
//! `_Common/guild.cpp:1092-1229`).
//!
//! **The job.** [`register`] adds the wall job `guild_salary` (`WallSpec::Window{from 21:00, to 22:00}`, no catch-up)
//! to the scheduler. The engine fires it for every crossed wall minute of `server.timezone` inside the window; its
//! first evaluation fires the current minute, so a (re)start at 21:30 pays at once and a start at 22:00 pays
//! nothing that day. [`run`] is the body: gate `events.guild_bank` (off: return WITHOUT touching the guard), then the
//! pay-once guard [`SalaryGuard`] (the ISO date of the stamp in the server zone, persisted as `server_kv`
//! `guild.salary_last_paid_on`).
//!
//! **The body.** For each guild in id order: `pay` = the sum of `salaries[rank]` over ALL members (online or not);
//! `0 < pay <= guild.gold` pays, otherwise nobody of that guild is paid. A paying guild is ONE `GuildSalary` batch: the
//! pool loses `pay`, the log row `P`, the date, and the members' money:
//!
//! * a member who is online (session not tainted) and whose purse can take it is credited at once
//!   (`add_gold(.., send = true)`) and its snapshot rides in the batch;
//! * every other paid member (offline, tainted, or with a purse that would pass `GOLD_MAX`) gets a pending grant
//!   `guild_salary:<guild id>` (delivered at the next login or by the grant poll).
//!
//! Every online member of a paying guild gets `GuildRealPenya{gold, rank}` (the client prints
//! `TID_GAME_GUILD_PAYTAKE` itself when its rank pays); members with salary 0 get no money. The date rides in EVERY
//! paying guild's batch (ruling 5 of the B8 review, differs from 21 §5.10 step 3): a crash between two guild
//! batches can under-pay a guild for that day, never pay one twice. After the loop the in-memory guard is set and,
//! when no guild was paid, the date is written as its own op.

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch};
use crate::item::gold::{add_gold, can_add};
use crate::sched::wall::{CatchUp, WallJob, WallSpec};
use crate::sched::{DocId, Scheduler};
use crate::social::guild::commit::{self, Origin};
use crate::social::guild::model::GuildRank;
use crate::social::guild::wire;
use crate::tick::LocalStamp;
use flyff_common::config::EventId;
use flyff_common::{CharacterId, GuildId};
use flyff_db::audit::social as audit;
use flyff_db::repo::meta::kv::GUILD_SALARY_LAST_PAID_ON;
use flyff_db::{
    AtomicBatch, CharacterSnapshot, DbOp, GuildBankLogKind, GuildBankLogRecord, PendingGrantRecord,
};
use flyff_proto::pb;
use serde_json::Value;
use std::collections::BTreeMap;

/// The name of the wall job (the suffix of its `schedule.last_fired` key).
pub const JOB: &str = "guild_salary";

/// `HHMM` of the first minute of the pay window (21:00).
pub const WINDOW_FROM_HM: u16 = 2100;
/// `HHMM` of the first minute after the pay window (22:00).
pub const WINDOW_TO_HM: u16 = 2200;

/// The pay-once guard of PL-094: the local date (`YYYY-MM-DD` in the server zone) on which the salary was last
/// paid. It is loaded from `server_kv` at start and set after every run that got past the gate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SalaryGuard {
    last_paid_on: Option<String>,
}

impl SalaryGuard {
    /// The guard of the stored `server_kv` (a missing or malformed value is "never paid").
    pub fn load(kv: &BTreeMap<String, Value>) -> SalaryGuard {
        SalaryGuard {
            last_paid_on: kv
                .get(GUILD_SALARY_LAST_PAID_ON)
                .and_then(Value::as_str)
                .map(str::to_owned),
        }
    }

    /// The date of the last run, if any.
    pub fn last_paid_on(&self) -> Option<&str> {
        self.last_paid_on.as_deref()
    }

    /// `true` when the salary was already handled on `date`.
    pub fn is_paid_on(&self, date: &str) -> bool {
        self.last_paid_on.as_deref() == Some(date)
    }

    fn set(&mut self, date: &str) {
        self.last_paid_on = Some(date.to_owned());
    }
}

/// The ISO date of a stamp in the server zone (`2026-10-24`).
pub fn date_of(stamp: &LocalStamp) -> String {
    format!("{:04}-{:02}-{:02}", stamp.year, stamp.month, stamp.day)
}

/// Registers the wall job `guild_salary` (21 §2.2): every crossed wall minute 21:00-21:59, no catch-up.
pub fn register(sched: &mut Scheduler) {
    sched.register_wall(WallJob {
        name: JOB,
        owner: DocId(21),
        specs: [WallSpec::Window {
            weekday: None,
            from_hm: WINDOW_FROM_HM,
            to_hm: WINDOW_TO_HM,
        }]
        .into_iter()
        .collect(),
        catch_up: CatchUp::Skip,
        run,
    });
}

/// The body of the wall job (see the module documentation).
pub fn run(svc: &mut Services, ctx: &mut GameCtx<'_>, stamp: &LocalStamp) {
    if ctx.live.event_state(EventId::GuildInventory) == 0 {
        return;
    }
    let today = date_of(stamp);
    if svc.social.guild.salary.is_paid_on(&today) {
        return;
    }
    let mut paid_any = false;
    for guild in svc.social.guild.ids() {
        paid_any |= pay_guild(svc, ctx, guild, &today);
    }
    svc.social.guild.salary.set(&today);
    if !paid_any {
        ctx.persist.op(DbOp::SetServerKv {
            key: GUILD_SALARY_LAST_PAID_ON.to_owned(),
            value: Value::from(today),
        });
    }
}

/// One member's salary and how it is paid.
struct Due {
    c: CharacterId,
    rank: GuildRank,
    pay: u32,
}

/// Pays one guild; `true` when it paid.
fn pay_guild(svc: &mut Services, ctx: &mut GameCtx<'_>, guild: GuildId, today: &str) -> bool {
    let Some(g) = svc.social.guild.get(guild) else {
        return false;
    };
    let dues: Vec<Due> = g
        .members
        .values()
        .map(|m| Due {
            c: m.id,
            rank: m.rank,
            pay: g
                .salaries
                .get(usize::from(m.rank.as_u8()))
                .copied()
                .unwrap_or(0),
        })
        .collect();
    let total: u64 = dues.iter().map(|d| u64::from(d.pay)).sum();
    let Ok(total) = u32::try_from(total) else {
        return false;
    };
    if total == 0 || total > g.gold {
        return false;
    }
    // who is credited at once and who gets a pending grant
    let mut online: Vec<(PlayerRef, &Due)> = Vec::new();
    let mut offline: Vec<&Due> = Vec::new();
    for d in dues.iter().filter(|d| d.pay > 0) {
        let avatar = crate::social::relation::avatar_of(svc, ctx, d.c).filter(|p| {
            can_snapshot(ctx, *p)
                && ctx
                    .player(*p)
                    .and_then(|m| m.player())
                    .is_some_and(|pl| can_add(i64::from(pl.items.gold), i64::from(d.pay)))
        });
        match avatar {
            Some(p) => online.push((p, d)),
            None => offline.push(d),
        }
    }
    // ---- the commit block ----
    let at = commit::now(ctx);
    if let Some(g) = svc.social.guild.get_mut(guild) {
        g.gold -= total;
    }
    let mut snapshots: Vec<CharacterSnapshot> = Vec::new();
    let mut saved = Vec::new();
    let mut grants: Vec<PendingGrantRecord> = offline
        .iter()
        .map(|d| PendingGrantRecord::guild_salary(guild, d.c, i64::from(d.pay), at))
        .collect();
    let mut credited = 0_u32;
    for (p, d) in &online {
        add_gold(svc, ctx, *p, i64::from(d.pay), true);
        match snapshot_for_batch(svc, ctx, *p) {
            Some((snap, generation)) => {
                snapshots.push(snap);
                saved.push((*p, generation));
                credited += 1;
            }
            None => {
                // cannot happen after `can_snapshot`; undo the credit and pay through a grant instead
                tracing::error!(
                    guild = guild.0,
                    character = d.c.0,
                    "a salary snapshot failed"
                );
                add_gold(svc, ctx, *p, -i64::from(d.pay), true);
                grants.push(PendingGrantRecord::guild_salary(
                    guild,
                    d.c,
                    i64::from(d.pay),
                    at,
                ));
            }
        }
    }
    let log = GuildBankLogRecord::new(guild, CharacterId(0), GuildBankLogKind::Salary, at)
        .with_penya(i64::from(total));
    let offline_paid = u32::try_from(grants.len()).unwrap_or(u32::MAX);
    let Some(g) = svc.social.guild.get(guild) else {
        return true;
    };
    let (after, record) = (g.gold, g.to_record());
    let batch = AtomicBatch::guild_salary(snapshots, record, grants, log, Some(today));
    commit::submit(svc, ctx, "GuildSalary", batch, &saved, None);
    let members: Vec<(CharacterId, GuildRank)> = dues.iter().map(|d| (d.c, d.rank)).collect();
    for (c, rank) in members {
        if let Some(p) = crate::social::relation::avatar_of(svc, ctx, c) {
            wire::to_one(
                ctx,
                p.0,
                pb::GuildRealPenya {
                    gold: i32::try_from(after).unwrap_or(i32::MAX),
                    rank: i32::from(rank.as_u8()),
                },
            );
        }
    }
    commit::emit(
        ctx,
        Origin::System,
        audit::guild_salary(
            guild,
            i64::from(total),
            credited,
            offline_paid,
            after,
            today,
        ),
    );
    true
}
