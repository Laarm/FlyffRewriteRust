//! The weekly change of the tax rates and owners (Plan/22 §10.4; legacy `CTax::CheckChangeTime`, `SetChangeNextTax`,
//! `Tax.cpp:199-268`, `LuaFunc/TaxFunc.lua`).
//!
//! Three callers, one body:
//!
//! * [`try_change`], the wall job `tax_change` (`Weekly{1, 0, 0}` from `ChangeTaxTime`, `CatchUp::Skip`), at the boundary;
//! * `catch_up`, P5 `tax::minute` every minute: the legacy check "the next application time has passed" (after a
//!   restart across the boundary and after the `Vote` phase of the lord election);
//! * `gm::apply_now` (`/TaxApplyNow`, `/content tax next`): forced.
//!
//! Every caller first looks at the lord election: while it votes nothing changes (KEEP, `/tan` included; the legacy
//! tested the vote before the GM flag, `Tax.cpp:205-208`). A change is due when it is forced or `now >=`
//! [`TaxService::next_change`]. The legacy Lua `CheckChangeTime` also changed unconditionally in the minute whose
//! `"%a %H:%M"` equals `ChangeTaxTime`; that differs from "due" only after a GM change earlier on the target weekday
//! before `hh:mm`, which the shipped `Mon 00:00` cannot have: documented deviation.
//!
//! **The change** (one `TaxPayout` batch, all or nothing):
//!
//! 1. the payout of the old round first (`payout::collect`), unless today's payout already ran and nothing was booked
//!    since (decision: avoids empty duplicate mails after the daily job, a restart or the vote);
//! 2. `All`: `next_owner = lord`, `rate_set`, the next rates 0 (`SetNextLord`);
//! 3. every entry: `owner = next_owner`, `next_owner = None`; `rate_set` takes the next rates of Sales and Purchase
//!    (not for `All`), otherwise `rate_set = true` and both rates are `MinTaxRate`; the next rates and the counters are
//!    zero. KEEP-QUIRK: an entry that is `rate_set` with no new rates (a week without a secret-room winner) gets 0/0;
//! 4. `changed_at = now`, `round += 1`;
//! 5. the batch: the new owners (3), the paid old-round details (reset) and the 8 new-round details, the mails, the
//!    date when the payout ran, `tax.last_changed_at`;
//! 6. `TaxAllInfo` to everyone, the audit line `tax.payout`.

use super::{ALL, TaxService, broadcast_all_info, payout, submit};
use crate::ctx::{GameCtx, Services};
use crate::econ::mail::MailEntry;
use crate::econ::tax::TaxKind;
use crate::sched::wall::{WallSpec, WallZone};
use crate::social::guild::salary::date_of;
use crate::tick::LocalStamp;
use flyff_common::UnixMs;
use flyff_db::AtomicBatch;
use flyff_db::audit::content as audit;

/// What a check did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    /// The election is voting: nothing changed.
    InVote,
    /// Not due yet.
    NotDue,
    /// The change ran.
    Changed,
}

/// Wall job `tax_change` (22 §2.4, `Weekly{1,0,0}`, `Skip`): the weekly change at the boundary.
pub fn try_change(s: &mut Services, ctx: &mut GameCtx<'_>, _at: &LocalStamp) {
    let _ = check(s, ctx, false);
}

/// P5: the catch-up of the weekly change.
pub(super) fn catch_up(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let _ = check(s, ctx, false);
}

impl TaxService {
    /// The next application time of the weekly change after `changed_at` (22 §10.4; TaxFunc `GetNextDate`): the first
    /// `spec` occurrence strictly after `changed_at`; when `changed_at` itself lies on the target weekday and before
    /// `hh:mm` (only after a GM change), the occurrence of the FOLLOWING week. `None` for a spec without an occurrence.
    pub fn next_change(&self, zone: &WallZone, spec: &WallSpec) -> Option<UnixMs> {
        let t1 = zone.next_occurrence(spec, self.changed_at)?;
        if let WallSpec::Weekly {
            weekday,
            hour,
            minute,
        } = *spec
        {
            let l = zone.local(self.changed_at);
            if l.weekday == weekday && (l.hour, l.minute) < (hour, minute) {
                return zone.next_occurrence(spec, t1);
            }
        }
        Some(t1)
    }
}

/// The common entry: the vote check, the due check, the change. `force` skips the due check only.
pub(super) fn check(s: &mut Services, ctx: &mut GameCtx<'_>, force: bool) -> Outcome {
    if s.event.lord.in_vote() {
        return Outcome::InVote;
    }
    if !force {
        let spec = s.event.configs.schedule.tax_change;
        match s.event.tax.next_change(ctx.sched.zone(), &spec) {
            Some(due) if ctx.clock.unix_ms >= due => {}
            _ => return Outcome::NotDue,
        }
    }
    apply(s, ctx);
    Outcome::Changed
}

/// The change itself (see the module documentation).
fn apply(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.unix_ms;
    let game_now = ctx.clock.game_ms;
    let today = date_of(&ctx.clock.local);

    // (1) the old round is paid first
    let skip_payout =
        s.event.tax.last_paid_on.as_deref() == Some(today.as_str()) && s.event.tax.nothing_booked();
    let paid = if skip_payout {
        payout::Paid::default()
    } else {
        payout::collect(s, ctx)
    };
    if !skip_payout {
        s.event.tax.last_paid_on = Some(today.clone());
    }

    // (2) the lord of the election is the next owner of the lord's share
    let lord = s.event.lord.lord().map(|c| c.0);
    let tax = &mut s.event.tax;
    {
        let e = &mut tax.entries[ALL];
        e.next_owner = lord;
        e.rate_set = true;
        for d in &mut e.details {
            d.next_rate = 0;
        }
    }

    // (3) every entry moves to its next owner and rates
    let min = tax.cfg.min_tax_rate;
    for slot in 0..3 {
        let e = &mut tax.entries[slot];
        e.owner = e.next_owner.take();
        let lord_entry = slot == ALL;
        if e.rate_set {
            if !lord_entry {
                for kind in [TaxKind::Sales, TaxKind::Purchase] {
                    let d = &mut e.details[kind as usize];
                    d.rate = d.next_rate;
                }
            }
        } else {
            e.rate_set = true;
            if !lord_entry {
                for kind in [TaxKind::Sales, TaxKind::Purchase] {
                    e.details[kind as usize].rate = min;
                }
            }
        }
        for d in &mut e.details {
            d.next_rate = 0;
            d.count = 0;
            d.gold = 0;
            d.perin = 0;
        }
    }

    // (4) a new round
    tax.changed_at = now;
    tax.round = tax.round.saturating_add(1);
    let round = tax.round;

    // (5) one batch
    let (owners, new_details) = tax.all_rows();
    tax.mark_all_written(game_now);
    tax.fresh_unsaved = false;
    let mut details = paid.details;
    details.extend(new_details);
    let records: Vec<_> = paid
        .mails
        .iter()
        .filter_map(|id| s.econ.mail.get(*id).map(MailEntry::to_record))
        .collect();
    let batch = AtomicBatch::tax_payout(
        owners,
        details,
        records,
        (!skip_payout).then(|| today.clone()),
        Some(now.to_unix_time()),
    );
    ctx.audit.emit(audit::tax_payout(
        round,
        (!skip_payout).then_some(today.as_str()),
        u32::try_from(paid.mails.len()).unwrap_or(u32::MAX),
        paid.gold,
        paid.perin,
    ));
    submit(ctx, "tax_change", batch, paid.mails, true);

    // (6) everybody sees the new owners and rates
    broadcast_all_info(ctx, &s.event.tax);
    // secret-room (WP-EVENT-13): `OnTaxInfo` with `bToAllClient`: the caption and the eviction of the hunting ground
    crate::event::secret_room::rules::on_tax_changed(s, ctx);
}
