//! `TaxAllInfo` in every join batch (Plan/22 §10.5, §19.2; join step `TaxAllInfo`, replayed on a re-attach).
//!
//! The legacy login (`DPDatabaseClient.cpp:1237-1238`) sent the tax tables to every joiner (`AddTaxInfo`) and then,
//! for the master of a guild that will own East or West and has not chosen the rates yet,
//! `TaxSetTaxRateOpenWnd` (`CTax::SendNoSetTaxRateOpenWnd`, `Tax.cpp:494-508`: East first, the first match only).

use super::{CONTINENTS, rate_window};
use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};

/// `TaxAllInfo` (always) and, conditionally, `TaxSetTaxRateOpenWnd` right behind it.
pub struct TaxAllInfoStep;

impl JoinContributor for TaxAllInfoStep {
    fn step(&self) -> JoinStep {
        JoinStep::TaxAllInfo
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let tax = &cx.svc.event.tax;
        out.to_session(cx.session, 0, tax.all_info());
        let Some(me) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return;
        };
        let guilds = &cx.svc.social.guild;
        let Some(guild) = guilds.guild_of(me).filter(|_| guilds.is_master(me)) else {
            return;
        };
        // East, then West; the lord's entry never asks for rates
        if let Some(slot) = (0..2).find(|slot| {
            let e = &tax.entries[*slot];
            e.next_owner == Some(guild.0) && !e.rate_set
        }) {
            out.to_session(cx.session, 0, rate_window(tax, CONTINENTS[slot]));
        }
    }
}
