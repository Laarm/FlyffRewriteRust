//! `event::tax`: tax collection, payout, rate changes and the Heaven Tower (Plan/22 §10; WP-EVENT-12, WP-ECON-08).
//!
//! | file | what |
//! |---|---|
//! | `mod.rs` | [`TaxService`] (the whole state), [`TaxService::load`], the [`TaxHooks`] the shops read, the queries other tasks ask, [`flush_due`] / [`flush_all`], [`broadcast_all_info`], [`open_rate_window`], P5 [`minute`] |
//! | `payout.rs` | the daily payout ([`pay_daily`], wall job `tax_pay`) and the payout body the weekly change and `gm::pay_now` share |
//! | `change.rs` | the weekly change ([`try_change`], wall job `tax_change`), `next_change` |
//! | `rate.rs` | `TaxSetTaxRate` |
//! | `heaven.rs` | `HeavenTowerTeleport`: the one-way admission teleport into the worlds 21..25 |
//! | `join.rs` | [`TaxAllInfoStep`]: `TaxAllInfo` in every join batch, the rate window for the master of the next owner |
//! | `gm.rs` | the pure GM / admin calls (`apply_now`, `pay_now`, `state_line`) |
//!
//! **State.** Memory owned by the game thread (13 §11: memory is the truth, the database is written behind). Three
//! entries in the legacy `std::map` order `East 0x0F < West 0xF0 < All 0xFF` (every iteration uses it): the guild
//! that owns the east continent, the guild that owns the west continent and the lord (`All`, whose rates are never
//! used; it only takes a share of every booking). An entry has an owner (a guild id, for `All` a character id), the
//! next owner, the flag whether the next owner has chosen the rates, and one detail per tax kind (East and All:
//! sales, purchase and admission; West: sales and purchase) with the rate in percent, the rate from the next change
//! on, and the counters of the round (`count`, `gold` below one Perin, `perin`).
//!
//! **Rows.** The rows of `tax_owners` / `tax_details` carry the round (`meta.tax_round`). A weekly change starts a new
//! round (the previous rows stay as history); a restart loads the current one. A fresh database has no rows: the
//! state is made up (round 1, every rate 0, no owner, `rate_set`) and the first P5 run writes it as a batch of its own
//! (`fresh_unsaved`), so that the start time survives a restart across a Monday 00:00. There is no batch at boot.
//!
//! **Persistence.** [`flush_due`] writes the dirty counters of a detail at most every 10 s (game time) and runs after
//! every booking and in every P5 run; [`flush_all`] writes every dirty detail (the shutdown); the payout, change and
//! rate-set batches carry their rows themselves.
//!
//! **Seams.** The secret room calls [`TaxService::set_next_guild`], [`TaxService::rows_of`] and [`open_rate_window`];
//! gm-content-control calls `gm::{apply_now, pay_now, state_line}`; the shops book through [`TaxHooks`].

pub mod change;
pub mod gm;
pub mod heaven;
pub mod join;
pub mod payout;
pub mod rate;

pub use change::try_change;
pub use join::TaxAllInfoStep;
pub use payout::pay_daily;

use crate::ctx::{GameCtx, Services};
use crate::econ::tax::{Continent, TaxHooks, TaxKind};
use crate::event::scripts::TaxConfig;
use crate::outbox::Audience;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use crate::world::continent::{CONT_ALL, CONT_EAST, CONT_WEST, Continents};
use flyff_common::math::Vec3;
use flyff_common::{GameMs, GuildId, ObjId, UnixMs, WorldId};
use flyff_data::types::ItemId;
use flyff_db::repo::meta::kv;
use flyff_db::{AtomicBatch, DbOp, TaxContinent, TaxDetailRecord, TaxLoad, TaxOwnerRecord};
use flyff_proto::pb;
use serde_json::Value;
use std::collections::BTreeMap;

/// The entries in the iteration order of the legacy `std::map`: East, West, All.
pub const CONTINENTS: [Continent; 3] = [CONT_EAST, CONT_WEST, CONT_ALL];
/// The index of the lord's entry.
const ALL: usize = 2;
/// `NULL_ID` on the wire: no owner.
pub const NO_OWNER: u32 = u32::MAX;
/// The least time between two writes of one detail (13 §11, "at most every 10 s").
pub const WRITE_INTERVAL_MS: u64 = 10_000;
/// What one Perin is worth (`PERIN_VALUE`).
const PERIN_VALUE: i32 = 100_000_000;

/// The entry index of a continent id (`CONT_EAST`, `CONT_WEST`, `CONT_ALL`); `None` for everything else.
pub fn slot_of(c: Continent) -> Option<usize> {
    CONTINENTS.iter().position(|x| *x == c)
}

/// The tax kinds an entry has, in kind order: the west entry has no admission.
pub fn kinds_of(slot: usize) -> &'static [TaxKind] {
    const FULL: [TaxKind; 3] = [TaxKind::Sales, TaxKind::Purchase, TaxKind::Admission];
    if CONTINENTS.get(slot) == Some(&CONT_WEST) {
        &FULL[..2]
    } else {
        &FULL
    }
}

/// `true` when entry `slot` has the kind.
fn has_kind(slot: usize, kind: TaxKind) -> bool {
    kinds_of(slot).contains(&kind)
}

fn db_continent(slot: usize) -> TaxContinent {
    match slot {
        0 => TaxContinent::East,
        1 => TaxContinent::West,
        _ => TaxContinent::All,
    }
}

fn db_kind(kind: TaxKind) -> flyff_db::TaxKind {
    match kind {
        TaxKind::Sales => flyff_db::TaxKind::Sales,
        TaxKind::Purchase => flyff_db::TaxKind::Purchase,
        TaxKind::Admission => flyff_db::TaxKind::Admission,
    }
}

fn kind_of_db(kind: flyff_db::TaxKind) -> TaxKind {
    match kind {
        flyff_db::TaxKind::Sales => TaxKind::Sales,
        flyff_db::TaxKind::Purchase => TaxKind::Purchase,
        flyff_db::TaxKind::Admission => TaxKind::Admission,
    }
}

/// `(rate as f32) * 0.1f32 * 0.1f32`: two multiplies, bit for bit the original (`Tax.cpp:101-139`, `:310`).
pub fn percent_factor(percent: i32) -> f32 {
    (percent as f32) * 0.1f32 * 0.1f32
}

/// The rates and counters of one kind of one entry (`__TAXDETAIL`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaxDetail {
    /// The rate in percent that applies now (`nTaxRate`).
    pub rate: i32,
    /// The rate from the next change on (`nNextTaxRate`).
    pub next_rate: i32,
    /// How many taxed bookings (`nTaxCount`).
    pub count: i32,
    /// The collected penya below one Perin (`nTaxGold`).
    pub gold: i32,
    /// The collected Perins (`nTaxPerin`).
    pub perin: i32,
    /// The counters changed since the row was written.
    pub dirty: bool,
    /// When the row was last written (game time); `None` = not since the start.
    pub written_ms: Option<GameMs>,
}

impl TaxDetail {
    fn record(&self, round: u32, slot: usize, kind: TaxKind) -> TaxDetailRecord {
        TaxDetailRecord {
            round,
            continent: db_continent(slot),
            kind: db_kind(kind),
            rate: self.rate,
            next_rate: self.next_rate,
            tx_count: self.count,
            gold: self.gold,
            perin: self.perin,
        }
    }

    /// The row went out (in a batch of its own or in a flush).
    fn written(&mut self, now: GameMs) {
        self.dirty = false;
        self.written_ms = Some(now);
    }
}

/// One entry of the service (`__TAXINFO`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaxEntry {
    /// The owner now: a guild id (East, West) or a character id (All); `None` = no owner.
    pub owner: Option<u32>,
    /// The owner from the next change on.
    pub next_owner: Option<u32>,
    /// `bSetTaxRate`: the next owner has chosen the next rates (or there is none and nothing to choose).
    pub rate_set: bool,
    /// The details by kind (`TaxKind as usize`); the West has no admission.
    details: [TaxDetail; 3],
}

impl TaxEntry {
    /// `CTax::CTax`: a fresh entry has no owner, rate 0 and `bSetTaxRate TRUE` (`Tax.h:44-80`).
    fn fresh() -> TaxEntry {
        TaxEntry {
            owner: None,
            next_owner: None,
            rate_set: true,
            details: [TaxDetail::default(); 3],
        }
    }
}

/// The tax service (`Services.event.tax`, 22 §10).
#[derive(Debug)]
pub struct TaxService {
    /// `meta.tax_round`: the round of the rows in force.
    round: u32,
    /// East, West, All.
    entries: [TaxEntry; 3],
    /// When the round started (the base of `next_change`).
    changed_at: UnixMs,
    /// The local date (`YYYY-MM-DD`) of the last payout (`server_kv tax.last_paid_on`).
    last_paid_on: Option<String>,
    /// The continents of Madrigal (15 §12.4).
    continents: Continents,
    /// The copy of `Tax.lua`: the bounds, the earning rates and the occupation items.
    cfg: TaxConfig,
    /// The rows of the current round are not known to be in the database: the first P5 run writes them.
    fresh_unsaved: bool,
}

impl Default for TaxService {
    /// A fresh state in round 1 that writes nothing (a service nobody loaded).
    fn default() -> Self {
        TaxService {
            round: 1,
            entries: [TaxEntry::fresh(), TaxEntry::fresh(), TaxEntry::fresh()],
            changed_at: UnixMs(0),
            last_paid_on: None,
            continents: Continents::default(),
            cfg: TaxConfig::default(),
            fresh_unsaved: false,
        }
    }
}

impl TaxService {
    /// Builds the service from the start-up state (22 §10.1; legacy `LoadTaxInfo`).
    ///
    /// * No owner row at all (a fresh database): round `TaxLoad.round + 1`, every rate 0, `rate_set`, no owner,
    ///   `changed_at = now`, and the rows are written by the first P5 run (`fresh_unsaved`).
    /// * Otherwise the rows of the round; a missing owner row is made up like a fresh entry, a missing detail row is
    ///   zero (one warning each), and the first P5 run writes the rows again. `changed_at` is the newest `changed_at`
    ///   of the owner rows (the rows win over `server_kv tax.last_changed_at`).
    /// * `last_paid_on` is `server_kv tax.last_paid_on`.
    pub fn load(
        load: &TaxLoad,
        server_kv: &BTreeMap<String, Value>,
        continents: Continents,
        now: UnixMs,
    ) -> TaxService {
        let mut svc = TaxService {
            continents,
            last_paid_on: server_kv
                .get(kv::TAX_LAST_PAID_ON)
                .and_then(Value::as_str)
                .map(str::to_owned),
            ..TaxService::default()
        };
        if load.owners.is_empty() {
            svc.round = load.round.saturating_add(1);
            svc.changed_at = now;
            svc.fresh_unsaved = true;
            return svc;
        }
        svc.round = load
            .owners
            .iter()
            .map(|o| o.round)
            .chain(std::iter::once(load.round))
            .max()
            .unwrap_or(1);
        let mut seen = [false; 3];
        let mut changed = i64::MIN;
        for o in &load.owners {
            let Some(slot) = CONTINENTS.iter().position(|c| *c == o.continent.code()) else {
                continue;
            };
            seen[slot] = true;
            let e = &mut svc.entries[slot];
            e.owner = o.owner_id;
            e.next_owner = o.next_owner_id;
            e.rate_set = o.rate_set;
            changed = changed.max(o.changed_at.0);
        }
        svc.changed_at = if changed == i64::MIN {
            now
        } else {
            UnixMs(changed.saturating_mul(1000))
        };
        let mut missing = 0_u32;
        for slot in 0..3 {
            if !seen[slot] {
                tracing::warn!(
                    continent = CONTINENTS[slot],
                    "tax: the owner row is missing: a fresh entry is made up"
                );
                svc.fresh_unsaved = true;
            }
            for kind in kinds_of(slot) {
                let row = load.details.iter().find(|d| {
                    d.continent.code() == CONTINENTS[slot] && kind_of_db(d.kind) == *kind
                });
                match row {
                    Some(d) => {
                        svc.entries[slot].details[*kind as usize] = TaxDetail {
                            rate: d.rate,
                            next_rate: d.next_rate,
                            count: d.tx_count,
                            gold: d.gold,
                            perin: d.perin,
                            dirty: false,
                            written_ms: None,
                        };
                    }
                    None => missing += 1,
                }
            }
        }
        if missing > 0 {
            tracing::warn!(
                missing,
                "tax: detail rows are missing: their counters start at zero"
            );
        }
        svc
    }

    /// Takes the numbers of `Tax.lua` (`Game::new` and `EventServices::read_configs`): the bounds, the earning rates
    /// and the occupation items.
    pub fn apply_config(&mut self, cfg: &TaxConfig) {
        self.cfg = cfg.clone();
    }

    // ---- queries ------------------------------------------------------------------------------------------------

    /// `meta.tax_round`: the round in force.
    pub fn round(&self) -> u32 {
        self.round
    }

    /// When the round in force started.
    pub fn changed_at(&self) -> UnixMs {
        self.changed_at
    }

    /// The local date (`YYYY-MM-DD`) of the last payout.
    pub fn last_paid_on(&self) -> Option<&str> {
        self.last_paid_on.as_deref()
    }

    /// `MinTaxRate`.
    pub fn min_rate(&self) -> i32 {
        self.cfg.min_tax_rate
    }

    /// `MaxTaxRate`.
    pub fn max_rate(&self) -> i32 {
        self.cfg.max_tax_rate
    }

    /// The entry of `c` (`CONT_EAST`, `CONT_WEST`, `CONT_ALL`).
    pub fn entry(&self, c: Continent) -> Option<&TaxEntry> {
        slot_of(c).map(|i| &self.entries[i])
    }

    /// The owner of `c` now: a guild id (East, West), a character id (All), `None` without one.
    pub fn owner(&self, c: Continent) -> Option<u32> {
        self.entry(c).and_then(|e| e.owner)
    }

    /// The owner of `c` from the next change on.
    pub fn next_owner(&self, c: Continent) -> Option<u32> {
        self.entry(c).and_then(|e| e.next_owner)
    }

    /// `true` when the next owner of `c` has chosen the rates (or there is none).
    pub fn rate_set(&self, c: Continent) -> bool {
        self.entry(c).is_none_or(|e| e.rate_set)
    }

    /// The detail of `kind` on `c`; `None` for a continent without that kind (the West has no admission).
    pub fn detail(&self, c: Continent, kind: TaxKind) -> Option<TaxDetail> {
        let slot = slot_of(c)?;
        has_kind(slot, kind).then(|| self.entries[slot].details[kind as usize])
    }

    /// The rate in percent of `kind` on `c` now (0 where there is none).
    pub fn rate(&self, c: Continent, kind: TaxKind) -> i32 {
        self.detail(c, kind).map_or(0, |d| d.rate)
    }

    /// The rate in percent of `kind` on `c` from the next change on.
    pub fn next_rate(&self, c: Continent, kind: TaxKind) -> i32 {
        self.detail(c, kind).map_or(0, |d| d.next_rate)
    }

    /// The owner row and the detail rows (kind order) of `c` as they stand in memory, in the current round: the tax
    /// rows of the secret room's result batch. A continent that is not one of the two sides (a caller bug) answers
    /// the rows of the east entry and logs an error.
    pub fn rows_of(&self, c: Continent) -> (TaxOwnerRecord, Vec<TaxDetailRecord>) {
        let slot = match slot_of(c) {
            Some(s) => s,
            None => {
                tracing::error!(continent = c, "tax rows_of: not a tax continent");
                0
            }
        };
        (self.owner_record(slot), self.detail_records(slot))
    }

    fn owner_record(&self, slot: usize) -> TaxOwnerRecord {
        self.owner_record_in(slot, self.round, &self.entries[slot])
    }

    fn owner_record_in(&self, slot: usize, round: u32, e: &TaxEntry) -> TaxOwnerRecord {
        TaxOwnerRecord {
            round,
            continent: db_continent(slot),
            owner_id: e.owner,
            next_owner_id: e.next_owner,
            rate_set: e.rate_set,
            changed_at: self.changed_at.to_unix_time(),
        }
    }

    fn detail_records(&self, slot: usize) -> Vec<TaxDetailRecord> {
        kinds_of(slot)
            .iter()
            .map(|k| self.entries[slot].details[*k as usize].record(self.round, slot, *k))
            .collect()
    }

    /// Every owner row and every detail row (the 3 and the 8) of the round in force.
    pub(crate) fn all_rows(&self) -> (Vec<TaxOwnerRecord>, Vec<TaxDetailRecord>) {
        let owners = (0..3).map(|s| self.owner_record(s)).collect();
        let details = (0..3).flat_map(|s| self.detail_records(s)).collect();
        (owners, details)
    }

    /// Marks every detail as written (their rows went out in a batch).
    pub(crate) fn mark_all_written(&mut self, now: GameMs) {
        for e in &mut self.entries {
            for d in &mut e.details {
                d.written(now);
            }
        }
    }

    /// The share of a booking the entries earn: `rate × 0.1 × 0.1` of the `SOURCE` continent's kind
    /// (`CTax::GetEarningRate(nCont, kind)`, `Tax.cpp:290-311`). KEEP-QUIRK: only a booking whose source continent is
    /// `All` takes the lord's rates, which never happens, so the lord's share uses the secret-room rates.
    fn earning(&self, source: Continent, kind: TaxKind) -> f32 {
        let lord = source == CONT_ALL;
        let percent = match kind {
            TaxKind::Admission if lord => self.cfg.admission_lord_rate,
            TaxKind::Admission => self.cfg.admission_secret_room_rate,
            _ if lord => self.cfg.tax_lord_rate,
            _ => self.cfg.tax_secret_room_rate,
        };
        percent_factor(percent)
    }

    /// `CContinent::GetArea(CContinent::GetContinent(pos))` for `world`.
    fn area_at(&self, pos: Vec3, world: WorldId) -> Continent {
        Continents::area_of(self.continents.continent_of(world, pos))
    }

    // ---- the secret room's seam --------------------------------------------------------------------------------------

    /// `CTax::SetNextSecretRoomGuild(cont, guild)` (`Tax.cpp:319-340`), called by the secret room at the close-wait of a
    /// side: `next_owner = guild`, `rate_set = guild.is_none()` (no winner: nothing to choose), the next rates 0.
    /// Memory only; returns the owner row and the detail rows of that continent in the current round (the secret room
    /// builds its own batch with [`TaxService::rows_of`] at the close). `cont` is `CONT_EAST` or `CONT_WEST`; anything
    /// else logs an error, changes nothing and answers the east rows.
    pub fn set_next_guild(
        &mut self,
        cont: Continent,
        guild: Option<GuildId>,
    ) -> (TaxOwnerRecord, Vec<TaxDetailRecord>) {
        match slot_of(cont).filter(|s| *s != ALL) {
            Some(slot) => {
                let e = &mut self.entries[slot];
                e.next_owner = guild.map(|g| g.0);
                e.rate_set = guild.is_none();
                for d in &mut e.details {
                    d.next_rate = 0;
                }
            }
            None => tracing::error!(continent = cont, "tax set_next_guild: not a side"),
        }
        self.rows_of(cont)
    }

    // ---- the wire form -----------------------------------------------------------------------------------------------

    /// `TaxAllInfo` in the client form (`CTax::Serialize` without the world-server fields): East, West, All, each
    /// with its owner (`NULL_ID` = none, kept) and the rates in kind order.
    pub fn all_info(&self) -> pb::TaxAllInfo {
        pb::TaxAllInfo {
            continents: (0..3)
                .map(|slot| pb::TaxContinent {
                    continent: u32::from(CONTINENTS[slot]),
                    owner_id: self.entries[slot].owner.unwrap_or(NO_OWNER),
                    rates: kinds_of(slot)
                        .iter()
                        .map(|k| pb::TaxRate {
                            kind: *k as u32,
                            rate: self.entries[slot].details[*k as usize].rate,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

impl TaxHooks for TaxService {
    fn continent_of(&self, pos: Vec3, world: WorldId) -> Continent {
        self.area_at(pos, world)
    }

    fn sales_rate(&self, c: Continent) -> f32 {
        self.side_factor(c, TaxKind::Sales)
    }

    fn purchase_rate(&self, c: Continent) -> f32 {
        self.side_factor(c, TaxKind::Purchase)
    }

    /// `CTax::AddTax` (`Tax.cpp:443-480`): for the entries `All` and `c` that have an owner and the kind, `count += 1`
    /// and `gold += (i32)(amount as f32 × earning)`; a total of 1e8 moves whole Perins. Nothing for `amount <= 0` and for
    /// a continent that is none of East, West, All. The rows are written by [`flush_due`].
    fn add_tax(&mut self, c: Continent, amount: i64, kind: TaxKind) {
        if amount <= 0 || slot_of(c).is_none() {
            return;
        }
        let earning = self.earning(c, kind);
        // the legacy argument is an `int`
        let nominal = i32::try_from(amount).unwrap_or(i32::MAX);
        for (slot, entry) in self.entries.iter_mut().enumerate() {
            if slot != ALL && CONTINENTS[slot] != c {
                continue;
            }
            if entry.owner.is_none() || !has_kind(slot, kind) {
                continue;
            }
            let d = &mut entry.details[kind as usize];
            d.count = d.count.saturating_add(1);
            d.gold = d.gold.saturating_add((nominal as f32 * earning) as i32);
            if d.gold >= PERIN_VALUE {
                d.perin = d.perin.saturating_add(d.gold / PERIN_VALUE);
                d.gold %= PERIN_VALUE;
            }
            d.dirty = true;
        }
    }

    fn is_occupation_shop_item(&self, item: ItemId) -> bool {
        self.cfg.occupation_shop_items.contains(&item)
    }

    /// `CTax::IsOccupationGuildMember` (`Tax.cpp:516-526`): the owner of the continent is the buyer's guild. No
    /// continent, no owner or no guild never match.
    fn is_occupation_guild_member(&self, c: Continent, guild: Option<GuildId>) -> bool {
        match (slot_of(c).filter(|s| *s != ALL), guild) {
            (Some(slot), Some(g)) => self.entries[slot].owner == Some(g.0),
            _ => false,
        }
    }
}

impl TaxService {
    /// `GetSalesTaxRate` / `GetPurchaseTaxRate`: `rate × 0.1 × 0.1` of the east or west entry, 0.0 for anything else.
    fn side_factor(&self, c: Continent, kind: TaxKind) -> f32 {
        match slot_of(c).filter(|s| *s != ALL) {
            Some(slot) => percent_factor(self.entries[slot].details[kind as usize].rate),
            None => 0.0,
        }
    }
}

// ---- persistence ---------------------------------------------------------------------------------------------------------

/// Writes the dirty counters whose last write lies at least [`WRITE_INTERVAL_MS`] of game time back, one
/// `UpsertTaxDetail` each (13 §11). Runs after every booking and in every P5 run.
pub fn flush_due(tax: &mut TaxService, ctx: &mut GameCtx<'_>) {
    flush(tax, ctx, false);
}

/// Writes every dirty counter (the clean stop, `Game::begin_final_shutdown`).
pub fn flush_all(tax: &mut TaxService, ctx: &mut GameCtx<'_>) {
    flush(tax, ctx, true);
}

fn flush(tax: &mut TaxService, ctx: &mut GameCtx<'_>, all: bool) {
    let now = ctx.clock.game_ms;
    let round = tax.round;
    for slot in 0..3 {
        for kind in kinds_of(slot) {
            let d = &mut tax.entries[slot].details[*kind as usize];
            if !d.dirty {
                continue;
            }
            let due = all
                || d.written_ms
                    .is_none_or(|w| now.0.saturating_sub(w.0) >= WRITE_INTERVAL_MS);
            if due {
                ctx.persist
                    .op(DbOp::UpsertTaxDetail(d.record(round, slot, *kind)));
                d.written(now);
            }
        }
    }
}

/// `TaxAllInfo` to everyone who is in a world (after a rate set and after a change; never after a payout).
pub fn broadcast_all_info(ctx: &mut GameCtx<'_>, tax: &TaxService) {
    ctx.broadcast(None, ObjId(0), Audience::AllOnline, tax.all_info());
}

/// `CTax::SendSetTaxRateOpenWnd`: the window of the rates to the online master of `guild` (the secret room calls it at the
/// close-wait of the winner's side). Nothing when the guild or its master is not online.
pub fn open_rate_window(s: &mut Services, ctx: &mut GameCtx<'_>, cont: Continent, guild: GuildId) {
    let Some(master) = s
        .social
        .guild
        .get(guild)
        .and_then(crate::social::guild::model::Guild::master)
    else {
        return;
    };
    let Some(online) = s.presence.get(master).and_then(|e| e.online) else {
        return;
    };
    let msg = rate_window(&s.event.tax, cont);
    ctx.outbox.to_session(online.session, 0, msg);
}

/// The window message of a continent: the continent and the bounds of the rates.
fn rate_window(tax: &TaxService, cont: Continent) -> pb::TaxSetTaxRateOpenWnd {
    pb::TaxSetTaxRateOpenWnd {
        continent: u32::from(cont),
        min_rate: tax.cfg.min_tax_rate,
        max_rate: tax.cfg.max_tax_rate,
    }
}

/// Sends `batch` with a ticket whose failure is a bug (21 §2.4): the mails it carried leave the memory again and a
/// CRITICAL line names it; `rewrite_all` sets `fresh_unsaved` again so that the next P5 run writes every row.
pub(crate) fn submit(
    ctx: &mut GameCtx<'_>,
    name: &'static str,
    batch: AtomicBatch,
    mails: Vec<flyff_common::MailId>,
    rewrite_all: bool,
) {
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| match result {
            TicketResult::Committed(_) => {}
            TicketResult::Failed(e) => {
                for id in &mails {
                    svc.econ.mail.remove(*id);
                }
                if rewrite_all {
                    svc.event.tax.fresh_unsaved = true;
                }
                tracing::error!(
                    error = %e,
                    batch = name,
                    "CRITICAL: a tax batch failed: the memory is kept (a bug, 21 §2.4)"
                );
            }
            other => tracing::error!(?other, batch = name, "a tax batch completed unexpectedly"),
        },
    )));
    ctx.persist.atomic(batch.with_ticket(ticket));
}

/// P5 `event::tax::minute` (22 §2.3, §10.4; every 900 ticks): writes the rows of a fresh database once, runs the catch-up
/// of the weekly change (`now >= next_change(changed_at)`; skipped while the election votes) and flushes the dirty
/// counters.
pub fn minute(s: &mut Services, ctx: &mut GameCtx<'_>) {
    if s.event.tax.fresh_unsaved {
        write_fresh(s, ctx);
    }
    change::catch_up(s, ctx);
    flush_due(&mut s.event.tax, ctx);
}

/// The first batch of a fresh database: all 3 owner rows and 8 detail rows of the round in force, no mails, no date,
/// `tax.last_changed_at`; its `RaiseCounter(tax_round)` makes a restart load this round. The ticket sets
/// `fresh_unsaved` again when the batch fails.
fn write_fresh(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let tax = &mut s.event.tax;
    tax.fresh_unsaved = false;
    let (owners, details) = tax.all_rows();
    let changed = tax.changed_at.to_unix_time();
    tax.mark_all_written(ctx.clock.game_ms);
    let batch = AtomicBatch::tax_payout(owners, details, Vec::new(), None, Some(changed));
    submit(ctx, "tax_fresh", batch, Vec::new(), true);
}
