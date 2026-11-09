//! `TaxSetTaxRate` (Plan/22 §10.5; legacy `CDPSrvr::OnSetTaxRate`, `DPSrvr.cpp:11923-11939`, `Tax.cpp:353-366`).
//!
//! The master of the guild that will own a continent from the next change on chooses its sales and purchase rates.
//! Not NPC-gated (22 §2.6). The request is ignored (a debug line; the legacy wrote an error log) unless:
//!
//! * the continent is East or West (FIX: the legacy also took `All` when a guild id equalled the lord's character id),
//! * the sender is in a guild that is the next owner of that continent and its master (no guild or no next owner never
//!   matches),
//! * both rates lie in `[MinTaxRate, MaxTaxRate]`.
//!
//! The commit: both next rates and `rate_set` in memory, ONE `Other("TaxRate")` batch (the two detail rows and the owner row),
//! `TaxAllInfo` to everyone (the client-visible fields do not change here, but the rate window closes on it), the audit
//! line `tax.rate`. Choosing again before the weekly change is allowed (legacy).

use super::{ALL, CONTINENTS, TaxService, broadcast_all_info, slot_of, submit};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::tax::{Continent, TaxKind};
use flyff_common::GuildId;
use flyff_db::audit::content as audit;
use flyff_db::{AtomicBatch, TaxContinent};
use flyff_proto::pb;

/// Why a request was refused (logged at debug level only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The continent is not East or West.
    Continent,
    /// The sender has no guild.
    NoGuild,
    /// The sender's guild is not the next owner of the continent.
    NotNextOwner,
    /// The sender is not the master of the guild.
    NotMaster,
    /// A rate is outside `[MinTaxRate, MaxTaxRate]`.
    Range,
}

/// `TaxSetTaxRate{continent, sales_rate, purchase_rate}`.
pub fn on_tax_set_tax_rate(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::TaxSetTaxRate,
) {
    let Some(character) = ctx
        .player(who)
        .and_then(|mv| mv.player())
        .map(|p| p.ident.character_id)
    else {
        return;
    };
    let cont = u8::try_from(m.continent).unwrap_or(0);
    let guild = s.social.guild.guild_of(character);
    let master = s.social.guild.is_master(character);
    if let Err(why) = s
        .event
        .tax
        .check_rates(cont, guild, master, (m.sales_rate, m.purchase_rate))
    {
        tracing::debug!(
            character = character.0,
            continent = m.continent,
            sales = m.sales_rate,
            purchase = m.purchase_rate,
            ?why,
            "TaxSetTaxRate refused"
        );
        return;
    }
    let (Some(slot), Some(guild)) = (slot_of(cont), guild) else {
        return;
    };
    let now = ctx.clock.game_ms;
    let tax = &mut s.event.tax;
    {
        let e = &mut tax.entries[slot];
        e.details[TaxKind::Sales as usize].next_rate = m.sales_rate;
        e.details[TaxKind::Purchase as usize].next_rate = m.purchase_rate;
        e.rate_set = true;
        // the two rows carry the live counters, so they count as written
        e.details[TaxKind::Sales as usize].written(now);
        e.details[TaxKind::Purchase as usize].written(now);
    }
    let owner = tax.owner_record(slot);
    let details: Vec<_> = tax
        .detail_records(slot)
        .into_iter()
        .filter(|d| d.kind != flyff_db::TaxKind::Admission)
        .collect();
    let round = tax.round;
    let batch = AtomicBatch::tax_rate_set(owner, details);
    submit(ctx, "tax_rate", batch, Vec::new(), false);
    broadcast_all_info(ctx, &s.event.tax);
    let line = audit::tax_rate(
        round,
        db_continent(cont),
        guild,
        m.sales_rate,
        m.purchase_rate,
    );
    ctx.audit.emit(match crate::audit::actor_of(ctx, who) {
        Some(actor) => line.by(actor),
        None => line,
    });
}

fn db_continent(cont: Continent) -> TaxContinent {
    TaxContinent::from_code(cont).unwrap_or(TaxContinent::East)
}

impl TaxService {
    /// The checks of `TaxSetTaxRate` for a sender in `guild` (`master`: he is its master) asking for `rates`
    /// `(sales, purchase)` on `cont`.
    pub(super) fn check_rates(
        &self,
        cont: Continent,
        guild: Option<GuildId>,
        master: bool,
        rates: (i32, i32),
    ) -> Result<(), Refusal> {
        let slot = slot_of(cont)
            .filter(|s| *s != ALL)
            .ok_or(Refusal::Continent)?;
        debug_assert!(CONTINENTS[slot] == cont);
        let guild = guild.ok_or(Refusal::NoGuild)?;
        if self.entries[slot].next_owner != Some(guild.0) {
            return Err(Refusal::NotNextOwner);
        }
        if !master {
            return Err(Refusal::NotMaster);
        }
        let range = self.cfg.min_tax_rate..=self.cfg.max_tax_rate;
        if !range.contains(&rates.0) || !range.contains(&rates.1) {
            return Err(Refusal::Range);
        }
        Ok(())
    }
}
