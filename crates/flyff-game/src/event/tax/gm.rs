//! The GM and admin calls of the tax system (Plan/22 §10.4, §20, Plan/23 row 188 `/tan`, `/content tax`).
//!
//! These are pure calls: `gm/cmd/content/` parses the line, checks the authority and prints; nothing here prints or checks
//! authority (the legacy `/TaxApplyNow` printed nothing).
//!
//! * [`apply_now`] (`/tan`, `/TaxApplyNow`, `/content tax next`): the weekly change now, regardless of the time. Refused
//!   while the lord election votes (KEEP: the legacy tested the vote before the GM flag).
//! * [`pay_now`] (`/content tax pay`): the payout now WITHOUT its date guard; every owner's counters are mailed, today's
//!   date is recorded; no rate or owner changes.
//! * [`state_line`]: ONE status line of the form
//!   `tax: <Voting|Waiting>, <n> s left, round <r>, East <guild|none> <sales>/<purchase> next <guild|none> <set|unset>,
//!   West <same>, All <lord|none>, paid <YYYY-MM-DD|never>` where the guilds are guild ids, the lord is a character id,
//!   `Voting` means the election votes (a due change waits), `<n>` is the whole number of seconds to the next change (0 when
//!   due) and `set` / `unset` tell whether the next owner has chosen the rates.

use super::change::{self, Outcome};
use super::payout;
use crate::ctx::{GameCtx, Services};
use crate::econ::tax::TaxKind;
use crate::social::guild::salary::date_of;
use crate::world::continent::{CONT_ALL, CONT_EAST, CONT_WEST};

/// Why a GM call was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TaxGmError {
    /// The lord election votes: the change waits.
    #[error("the lord election is in its vote")]
    InVote,
}

/// `/TaxApplyNow`: the weekly change now.
pub fn apply_now(svc: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), TaxGmError> {
    match change::check(svc, ctx, true) {
        Outcome::InVote => Err(TaxGmError::InVote),
        Outcome::NotDue | Outcome::Changed => Ok(()),
    }
}

/// The payout now, without its date guard.
pub fn pay_now(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let today = date_of(&ctx.clock.local);
    payout::run(svc, ctx, &today);
}

/// The one status line (see the module documentation).
pub fn state_line(svc: &Services, ctx: &GameCtx<'_>) -> String {
    let tax = &svc.event.tax;
    let voting = svc.event.lord.in_vote();
    let spec = svc.event.configs.schedule.tax_change;
    let left = tax
        .next_change(ctx.sched.zone(), &spec)
        .map_or(0, |due| (due.0 - ctx.clock.unix_ms.0).max(0) / 1000);
    let id = |v: Option<u32>| v.map_or_else(|| "none".to_owned(), |g| g.to_string());
    let side = |name: &str, c| {
        format!(
            "{name} {} {}/{} next {} {}",
            id(tax.owner(c)),
            tax.rate(c, TaxKind::Sales),
            tax.rate(c, TaxKind::Purchase),
            id(tax.next_owner(c)),
            if tax.rate_set(c) { "set" } else { "unset" },
        )
    };
    format!(
        "tax: {}, {left} s left, round {}, {}, {}, All {}, paid {}",
        if voting { "Voting" } else { "Waiting" },
        tax.round(),
        side("East", CONT_EAST),
        side("West", CONT_WEST),
        id(tax.owner(CONT_ALL)),
        tax.last_paid_on().unwrap_or("never"),
    )
}
