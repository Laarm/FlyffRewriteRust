//! The item reads of the console (Plan/25 §5.5, §5.7, §11.6; **WP-ADMIN-20** `ItemSearch` / `DuplicateSerials`,
//! **WP-ADMIN-22** `EconomySummary`), plus the small `RecipientFacts` read of the grant route.
//!
//! All of them are *whole-database passes* built on the chunked scan framework of [`super::scan`]: they hold the
//! server-wide heavy permit, run on the dedicated admin reader (or in 5 ms chunks behind lobby work on the single
//! reader) and can be cancelled between chunks. The shared walker [`ItemWalk`] visits every item that exists in the
//! database with the same coverage as `flyff db check --serials` (`check::for_each_item`): inventories and
//! equipment, the three pockets, banks, guild banks, open mails and undelivered, uncancelled item grants.
//!
//! ```text
//! let mut hits = persist.admin_item_search(ItemSearchQuery::by_item(5033))?;     // ScanStream<ItemHit>
//! let dups = persist.admin_duplicate_serials().await?;                            // Vec<DuplicateSerial>
//! let eco = persist.admin_economy_summary(EconomyQuery::default(), now).await?;   // EconomySummary
//! ```
//!
//! Live containers of online accounts override these DB copies (25 §5.5): the route merges the game thread's
//! `FindItems` result in front of this stream; the rows carry `account_id` so it can drop the covered ones.

use super::model::invalid;
use super::scan::{AdminScan, Budget, Once, Progress};
use crate::error::{DbError, DbErrorKind};
use crate::handle::PersistHandle;
use crate::model::{
    BankDoc, CharacterState, GuildBankDoc, InventoryDoc, ItemRecord, JsonDoc, PocketsDoc,
    StoredItem, character_state_from_db,
};
use flyff_common::{AccountId, CharacterId, GuildId, UnixTime};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The most hits an item search returns (a runaway query is cut here and says so).
pub const HITS_MAX: usize = 10_000;
/// The default cap of a search when the request says nothing.
pub const HITS_DEFAULT: usize = 2_000;
/// The most duplicate serials a report lists.
pub const DUPLICATES_MAX: usize = 5_000;
/// How many richest characters / accounts the economy summary lists (Plan/25 §5.7).
pub const RICHEST_TOP: usize = 50;
/// How many days the grants-per-day table covers.
pub const GRANT_DAYS: i64 = 30;
/// The most recipients one [`PersistHandle::admin_recipient_facts`] call resolves.
pub const RECIPIENTS_MAX: usize = 600;

// ---- where an item is ------------------------------------------------------------------------------------------

/// Where in the database an item was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemSite {
    /// A worn item (the equipment slots of the inventory document).
    Equipment,
    /// The inventory bag.
    Inventory,
    /// A pocket `0..3`.
    Pocket(u8),
    /// The bank of a character.
    Bank,
    /// A guild bank.
    GuildBank,
    /// An open mail's attachment.
    Mail,
    /// An undelivered, uncancelled item grant.
    PendingGrant,
}

impl ItemSite {
    /// A short text for logs and CSV (`inventory`, `pocket 1`, ...).
    pub fn label(self) -> String {
        match self {
            ItemSite::Equipment => "equipment".to_owned(),
            ItemSite::Inventory => "inventory".to_owned(),
            ItemSite::Pocket(i) => format!("pocket {i}"),
            ItemSite::Bank => "bank".to_owned(),
            ItemSite::GuildBank => "guild bank".to_owned(),
            ItemSite::Mail => "mail".to_owned(),
            ItemSite::PendingGrant => "pending grant".to_owned(),
        }
    }
}

/// The owner of the row a scan is looking at: who the item belongs to or where it waits.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemOwner {
    /// The character (owner of the inventory, the bank, the mail box or the grant).
    pub character_id: Option<CharacterId>,
    /// Its name.
    pub character_name: Option<String>,
    /// Its state (a deleted character still holds items until it is purged).
    pub character_state: Option<CharacterState>,
    /// Its account.
    pub account_id: Option<AccountId>,
    /// The account's name.
    pub account_name: Option<String>,
    /// The guild of a guild bank.
    pub guild_id: Option<GuildId>,
    /// The guild's name.
    pub guild_name: Option<String>,
    /// The mail holding the item.
    pub mail_id: Option<i64>,
    /// The pending grant holding the item.
    pub grant_id: Option<i64>,
    /// The character slot of a bank (`0..=2`): banks are account-scoped, this names the compartment.
    pub bank_slot: Option<u8>,
    /// `characters.saved_at` (the DB copy of an online character can be 90 s old), the creation time of a mail or
    /// a grant.
    pub saved_at: Option<UnixTime>,
}

// ---- the walker ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Inventories,
    Banks,
    GuildBanks,
    Mails,
    Grants,
    Done,
}

/// Visits every item of the database in chunks (see the module documentation).
///
/// `prefilter`: a text that every document containing a wanted item must contain (the digits of an item id or a
/// serial); documents without it are skipped without being parsed, which makes a search over 10,000 characters a
/// string scan. A document that contains it is parsed and judged exactly by the caller's visitor.
pub struct ItemWalk {
    phase: Phase,
    cursor: i64,
    prefilter: Option<String>,
    total: Option<u64>,
}

/// What a visitor is told about one item.
pub struct Visit<'a> {
    /// The owner of the row.
    pub owner: &'a ItemOwner,
    /// Where the item is.
    pub site: ItemSite,
    /// The visible slot in its container (`None` for a mail or a grant).
    pub slot: Option<u16>,
    /// The item.
    pub item: &'a ItemRecord,
}

impl ItemWalk {
    /// A walk over everything; `prefilter` as in the type documentation.
    pub fn new(prefilter: Option<String>) -> ItemWalk {
        ItemWalk {
            phase: Phase::Inventories,
            cursor: 0,
            prefilter: prefilter.filter(|p| !p.is_empty()),
            total: None,
        }
    }

    fn count_all(conn: &Connection) -> Result<u64, DbError> {
        let mut total = 0_i64;
        for sql in [
            "SELECT COUNT(*) FROM character_docs",
            "SELECT COUNT(*) FROM banks",
            "SELECT COUNT(*) FROM guild_banks",
            "SELECT COUNT(*) FROM mails WHERE item IS NOT NULL AND deleted_at IS NULL AND item_taken_at IS NULL",
            "SELECT COUNT(*) FROM pending_grants WHERE kind = 'item' AND item IS NOT NULL AND delivered_at IS NULL AND cancelled_at IS NULL",
        ] {
            let n: i64 = conn.query_row(sql, [], |r| r.get(0))?;
            total = total.saturating_add(n);
        }
        Ok(u64::try_from(total).unwrap_or(0))
    }

    fn wanted(&self, text: &str) -> bool {
        self.prefilter.as_ref().is_none_or(|p| text.contains(p))
    }

    /// Does one chunk of work, calling `visit` for every item of the rows it reads.
    pub fn step(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        visit: &mut dyn FnMut(Visit<'_>),
    ) -> Result<Progress, DbError> {
        if self.total.is_none() {
            self.total = Some(Self::count_all(conn)?);
        }
        let total = self.total.unwrap_or(0);
        let read = match self.phase {
            Phase::Inventories => self.inventories(conn, budget, visit)?,
            Phase::Banks => self.banks(conn, budget, visit)?,
            Phase::GuildBanks => self.guild_banks(conn, budget, visit)?,
            Phase::Mails => self.mails(conn, budget, visit)?,
            Phase::Grants => self.grants(conn, budget, visit)?,
            Phase::Done => 0,
        };
        let progress = if self.phase == Phase::Done {
            Progress::finished(read)
        } else {
            Progress::more(read)
        };
        Ok(progress.with_total(total))
    }

    /// Moves to the next phase when the current one ran dry (`exhausted`).
    fn advance(&mut self, exhausted: bool) {
        if !exhausted {
            return;
        }
        self.cursor = 0;
        self.phase = match self.phase {
            Phase::Inventories => Phase::Banks,
            Phase::Banks => Phase::GuildBanks,
            Phase::GuildBanks => Phase::Mails,
            Phase::Mails => Phase::Grants,
            Phase::Grants | Phase::Done => Phase::Done,
        };
    }

    fn inventories(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        visit: &mut dyn FnMut(Visit<'_>),
    ) -> Result<u64, DbError> {
        let cap = budget.max_rows();
        let mut stmt = conn.prepare_cached(
            "SELECT d.character_id, ch.name, ch.account_id, a.name, ch.state, ch.saved_at, d.inventory, d.pockets
             FROM character_docs d
             JOIN characters ch ON ch.id = d.character_id
             JOIN accounts a ON a.id = ch.account_id
             WHERE d.character_id > ?1 ORDER BY d.character_id LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![self.cursor, i64::try_from(cap).unwrap_or(i64::MAX)])?;
        let (mut read, mut cut) = (0_u64, false);
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            self.cursor = id;
            read += 1;
            let inventory: String = row.get(6)?;
            let pockets: String = row.get(7)?;
            if self.wanted(&inventory) || self.wanted(&pockets) {
                let owner = ItemOwner {
                    character_id: Some(CharacterId(u32::try_from(id).unwrap_or(0))),
                    character_name: Some(row.get(1)?),
                    account_id: Some(AccountId(row.get(2)?)),
                    account_name: Some(row.get(3)?),
                    character_state: character_state_from_db(row.get::<_, i64>(4)?).ok(),
                    saved_at: row.get::<_, Option<i64>>(5)?.map(UnixTime),
                    ..ItemOwner::default()
                };
                match InventoryDoc::from_json(&inventory) {
                    Ok(doc) => {
                        let c = &doc.container;
                        let bag = c.capacity.saturating_sub(crate::model::EQUIP_SLOTS);
                        walk_container(&owner, c, visit, |slot| {
                            if slot >= bag {
                                ItemSite::Equipment
                            } else {
                                ItemSite::Inventory
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(character = id, error = %e, "item scan: unreadable inventory")
                    }
                }
                match PocketsDoc::from_json(&pockets) {
                    Ok(doc) => {
                        for (i, pocket) in doc.pockets.iter().enumerate() {
                            let n = u8::try_from(i).unwrap_or(0);
                            walk_container(&owner, &pocket.container, visit, |_| {
                                ItemSite::Pocket(n)
                            });
                        }
                    }
                    Err(e) => {
                        tracing::warn!(character = id, error = %e, "item scan: unreadable pockets")
                    }
                }
            }
            if budget.expired() {
                cut = true;
                break;
            }
        }
        self.advance(!cut && read < u64::try_from(cap).unwrap_or(u64::MAX));
        Ok(read)
    }

    fn banks(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        visit: &mut dyn FnMut(Visit<'_>),
    ) -> Result<u64, DbError> {
        let cap = budget.max_rows();
        let mut stmt = conn.prepare_cached(
            "SELECT b.character_id, ch.name, ch.account_id, a.name, ch.state, ch.slot, ch.saved_at, b.container
             FROM banks b
             JOIN characters ch ON ch.id = b.character_id
             JOIN accounts a ON a.id = ch.account_id
             WHERE b.character_id > ?1 ORDER BY b.character_id LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![self.cursor, i64::try_from(cap).unwrap_or(i64::MAX)])?;
        let (mut read, mut cut) = (0_u64, false);
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            self.cursor = id;
            read += 1;
            let text: String = row.get(7)?;
            if self.wanted(&text) {
                let owner = ItemOwner {
                    character_id: Some(CharacterId(u32::try_from(id).unwrap_or(0))),
                    character_name: Some(row.get(1)?),
                    account_id: Some(AccountId(row.get(2)?)),
                    account_name: Some(row.get(3)?),
                    character_state: character_state_from_db(row.get::<_, i64>(4)?).ok(),
                    bank_slot: u8::try_from(row.get::<_, i64>(5)?).ok(),
                    saved_at: row.get::<_, Option<i64>>(6)?.map(UnixTime),
                    ..ItemOwner::default()
                };
                match BankDoc::from_json(&text) {
                    Ok(doc) => walk_container(&owner, &doc.container, visit, |_| ItemSite::Bank),
                    Err(e) => {
                        tracing::warn!(character = id, error = %e, "item scan: unreadable bank")
                    }
                }
            }
            if budget.expired() {
                cut = true;
                break;
            }
        }
        self.advance(!cut && read < u64::try_from(cap).unwrap_or(u64::MAX));
        Ok(read)
    }

    fn guild_banks(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        visit: &mut dyn FnMut(Visit<'_>),
    ) -> Result<u64, DbError> {
        let cap = budget.max_rows();
        let mut stmt = conn.prepare_cached(
            "SELECT gb.guild_id, g.name, gb.container
             FROM guild_banks gb JOIN guilds g ON g.id = gb.guild_id
             WHERE gb.guild_id > ?1 ORDER BY gb.guild_id LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![self.cursor, i64::try_from(cap).unwrap_or(i64::MAX)])?;
        let (mut read, mut cut) = (0_u64, false);
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            self.cursor = id;
            read += 1;
            let text: String = row.get(2)?;
            if self.wanted(&text) {
                let owner = ItemOwner {
                    guild_id: Some(GuildId(u32::try_from(id).unwrap_or(0))),
                    guild_name: Some(row.get(1)?),
                    ..ItemOwner::default()
                };
                match GuildBankDoc::from_json(&text) {
                    Ok(doc) => {
                        walk_container(&owner, &doc.container, visit, |_| ItemSite::GuildBank);
                    }
                    Err(e) => {
                        tracing::warn!(guild = id, error = %e, "item scan: unreadable guild bank")
                    }
                }
            }
            if budget.expired() {
                cut = true;
                break;
            }
        }
        self.advance(!cut && read < u64::try_from(cap).unwrap_or(u64::MAX));
        Ok(read)
    }

    fn mails(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        visit: &mut dyn FnMut(Visit<'_>),
    ) -> Result<u64, DbError> {
        let cap = budget.max_rows();
        let mut stmt = conn.prepare_cached(
            "SELECT m.id, m.receiver_id, ch.name, ch.account_id, a.name, ch.state, m.created_at, m.item
             FROM mails m
             JOIN characters ch ON ch.id = m.receiver_id
             JOIN accounts a ON a.id = ch.account_id
             WHERE m.id > ?1 AND m.item IS NOT NULL AND m.deleted_at IS NULL AND m.item_taken_at IS NULL
             ORDER BY m.id LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![self.cursor, i64::try_from(cap).unwrap_or(i64::MAX)])?;
        let (mut read, mut cut) = (0_u64, false);
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            self.cursor = id;
            read += 1;
            let text: String = row.get(7)?;
            if self.wanted(&text) {
                let owner = ItemOwner {
                    mail_id: Some(id),
                    character_id: Some(CharacterId(row.get(1)?)),
                    character_name: Some(row.get(2)?),
                    account_id: Some(AccountId(row.get(3)?)),
                    account_name: Some(row.get(4)?),
                    character_state: character_state_from_db(row.get::<_, i64>(5)?).ok(),
                    saved_at: Some(UnixTime(row.get(6)?)),
                    ..ItemOwner::default()
                };
                match StoredItem::from_json(&text) {
                    Ok(stored) => visit(Visit {
                        owner: &owner,
                        site: ItemSite::Mail,
                        slot: None,
                        item: &stored.item,
                    }),
                    Err(e) => {
                        tracing::warn!(mail = id, error = %e, "item scan: unreadable mail item")
                    }
                }
            }
            if budget.expired() {
                cut = true;
                break;
            }
        }
        self.advance(!cut && read < u64::try_from(cap).unwrap_or(u64::MAX));
        Ok(read)
    }

    fn grants(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        visit: &mut dyn FnMut(Visit<'_>),
    ) -> Result<u64, DbError> {
        let cap = budget.max_rows();
        let mut stmt = conn.prepare_cached(
            "SELECT g.id, g.character_id, ch.name, ch.account_id, a.name, ch.state, g.created_at, g.item
             FROM pending_grants g
             JOIN characters ch ON ch.id = g.character_id
             JOIN accounts a ON a.id = ch.account_id
             WHERE g.id > ?1 AND g.kind = 'item' AND g.item IS NOT NULL
               AND g.delivered_at IS NULL AND g.cancelled_at IS NULL
             ORDER BY g.id LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![self.cursor, i64::try_from(cap).unwrap_or(i64::MAX)])?;
        let (mut read, mut cut) = (0_u64, false);
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            self.cursor = id;
            read += 1;
            let text: String = row.get(7)?;
            if self.wanted(&text) {
                let owner = ItemOwner {
                    grant_id: Some(id),
                    character_id: Some(CharacterId(row.get(1)?)),
                    character_name: Some(row.get(2)?),
                    account_id: Some(AccountId(row.get(3)?)),
                    account_name: Some(row.get(4)?),
                    character_state: character_state_from_db(row.get::<_, i64>(5)?).ok(),
                    saved_at: Some(UnixTime(row.get(6)?)),
                    ..ItemOwner::default()
                };
                match StoredItem::from_json(&text) {
                    Ok(stored) => visit(Visit {
                        owner: &owner,
                        site: ItemSite::PendingGrant,
                        slot: None,
                        item: &stored.item,
                    }),
                    Err(e) => {
                        tracing::warn!(grant = id, error = %e, "item scan: unreadable grant item")
                    }
                }
            }
            if budget.expired() {
                cut = true;
                break;
            }
        }
        self.advance(!cut && read < u64::try_from(cap).unwrap_or(u64::MAX));
        Ok(read)
    }
}

/// Calls the visitor for every item of `container`, with the visible slot (`index[slot] == obj_id`) and the site
/// `site_of(slot)` decides.
fn walk_container(
    owner: &ItemOwner,
    container: &crate::model::ContainerRecord,
    visit: &mut dyn FnMut(Visit<'_>),
    site_of: impl Fn(u16) -> ItemSite,
) {
    // storage index -> visible slot
    let mut slot_of: BTreeMap<i32, u16> = BTreeMap::new();
    for (slot, &obj) in container.index.iter().enumerate() {
        if obj >= 0 {
            slot_of
                .entry(obj)
                .or_insert(u16::try_from(slot).unwrap_or(u16::MAX));
        }
    }
    for item in &container.items {
        let slot = slot_of.get(&i32::from(item.obj_id)).copied();
        let site = site_of(slot.unwrap_or(0));
        visit(Visit {
            owner,
            site,
            slot,
            item,
        });
    }
}

// ---- ItemSearch ------------------------------------------------------------------------------------------------

/// What an ownership search looks for (Plan/25 §5.5): an item id, optionally with a minimum upgrade level and an
/// element, or a serial.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSearchQuery {
    /// The item id.
    #[serde(default)]
    pub item_id: Option<u32>,
    /// The serial (an exact item).
    #[serde(default)]
    pub serial: Option<u64>,
    /// Only items with this `+N` or more.
    #[serde(default)]
    pub min_upgrade: Option<i32>,
    /// Only items with this element (`1..=5`).
    #[serde(default)]
    pub element: Option<u8>,
    /// Stop after this many hits (`0` = [`HITS_DEFAULT`], at most [`HITS_MAX`]).
    #[serde(default)]
    pub max_hits: usize,
}

impl ItemSearchQuery {
    /// Every item with `item_id`.
    pub fn by_item(item_id: u32) -> ItemSearchQuery {
        ItemSearchQuery {
            item_id: Some(item_id),
            ..ItemSearchQuery::default()
        }
    }

    /// The item with `serial`.
    pub fn by_serial(serial: u64) -> ItemSearchQuery {
        ItemSearchQuery {
            serial: Some(serial),
            ..ItemSearchQuery::default()
        }
    }

    /// The effective cap.
    pub fn cap(&self) -> usize {
        match self.max_hits {
            0 => HITS_DEFAULT,
            n => n.min(HITS_MAX),
        }
    }

    /// `Invalid` unless the query names an item id or a serial.
    pub fn validate(&self) -> Result<(), DbError> {
        if self.item_id.is_none() && self.serial.is_none() {
            return Err(invalid("an item search needs an item id or a serial"));
        }
        if self.serial == Some(0) {
            return Err(invalid(
                "serial 0 means \"not numbered yet\", it cannot be searched",
            ));
        }
        if self.element.is_some_and(|e| e == 0 || e > 5) {
            return Err(invalid("element must be 1..=5"));
        }
        Ok(())
    }

    /// Does `item` match?
    pub fn matches(&self, item: &ItemRecord) -> bool {
        self.item_id.is_none_or(|id| item.item_id == id)
            && self.serial.is_none_or(|s| item.serial == s)
            && self.min_upgrade.is_none_or(|n| item.upgrade_level >= n)
            && self.element.is_none_or(|e| item.element == e)
    }
}

/// One found item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemHit {
    /// Where.
    pub site: ItemSite,
    /// Whose.
    pub owner: ItemOwner,
    /// The visible slot (containers only).
    pub slot: Option<u16>,
    /// The item.
    pub item: ItemRecord,
}

/// The streaming item search (see [`PersistHandle::admin_item_search`]).
pub struct ItemSearchScan {
    walk: ItemWalk,
    query: ItemSearchQuery,
    found: usize,
    cut: bool,
}

impl ItemSearchScan {
    /// A search for `query` (validated by the caller).
    pub fn new(query: ItemSearchQuery) -> ItemSearchScan {
        let prefilter = match (query.serial, query.item_id) {
            (Some(s), _) => Some(s.to_string()),
            (None, Some(id)) => Some(id.to_string()),
            (None, None) => None,
        };
        ItemSearchScan {
            walk: ItemWalk::new(prefilter),
            query,
            found: 0,
            cut: false,
        }
    }

    /// `true` when the search stopped at its hit cap (more exist).
    pub fn was_cut(&self) -> bool {
        self.cut
    }
}

impl AdminScan for ItemSearchScan {
    type Item = ItemHit;

    fn heavy(&self) -> bool {
        true
    }

    fn step(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        out: &mut Vec<ItemHit>,
    ) -> Result<Progress, DbError> {
        let cap = self.query.cap();
        let query = &self.query;
        let found = &mut self.found;
        let cut = &mut self.cut;
        let progress = self.walk.step(conn, budget, &mut |v: Visit<'_>| {
            if *found >= cap {
                *cut = true;
                return;
            }
            if query.matches(v.item) {
                *found += 1;
                out.push(ItemHit {
                    site: v.site,
                    owner: v.owner.clone(),
                    slot: v.slot,
                    item: v.item.clone(),
                });
            }
        })?;
        if self.cut {
            // the cap was reached: stop early, the stream tells the consumer how many rows were examined
            return Ok(Progress {
                done: true,
                ..progress
            });
        }
        Ok(progress)
    }
}

// ---- DuplicateSerials ------------------------------------------------------------------------------------------

/// One place a duplicated serial was found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicatePlace {
    /// Where.
    pub site: ItemSite,
    /// Whose.
    pub owner: ItemOwner,
    /// The visible slot (containers only).
    pub slot: Option<u16>,
    /// The stack size there.
    pub count: u16,
    /// The item id there.
    pub item_id: u32,
}

/// A serial that exists more than once (the report of `flyff db check --serials`, with places).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateSerial {
    /// The serial.
    pub serial: u64,
    /// Every place it was found in, at least two.
    pub places: Vec<DuplicatePlace>,
}

/// Compact record of one numbered item, kept while the walk runs.
struct Numbered {
    serial: u64,
    site: ItemSite,
    slot: Option<u16>,
    count: u16,
    item_id: u32,
    owner: std::sync::Arc<ItemOwner>,
}

/// The duplicate-serial pass: walks everything, remembers the numbered items and emits the duplicated ones in
/// the last chunk. Same coverage as `flyff db check --serials`.
pub struct DuplicateSerialsScan {
    walk: ItemWalk,
    seen: Vec<Numbered>,
    last_owner: Option<std::sync::Arc<ItemOwner>>,
    reported: bool,
    /// Items examined (the check's `items_scanned`).
    pub items: u64,
}

impl Default for DuplicateSerialsScan {
    fn default() -> Self {
        Self::new()
    }
}

impl DuplicateSerialsScan {
    /// A fresh pass.
    pub fn new() -> DuplicateSerialsScan {
        DuplicateSerialsScan {
            walk: ItemWalk::new(None),
            seen: Vec::new(),
            last_owner: None,
            reported: false,
            items: 0,
        }
    }
}

impl AdminScan for DuplicateSerialsScan {
    type Item = DuplicateSerial;

    fn heavy(&self) -> bool {
        true
    }

    fn step(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        out: &mut Vec<DuplicateSerial>,
    ) -> Result<Progress, DbError> {
        let seen = &mut self.seen;
        let last_owner = &mut self.last_owner;
        let items = &mut self.items;
        let progress = self.walk.step(conn, budget, &mut |v: Visit<'_>| {
            *items += 1;
            if v.item.serial == 0 {
                return;
            }
            // consecutive items share their row's owner: clone it once per row
            let owner = match last_owner {
                Some(o) if **o == *v.owner => std::sync::Arc::clone(o),
                _ => {
                    let o = std::sync::Arc::new(v.owner.clone());
                    *last_owner = Some(std::sync::Arc::clone(&o));
                    o
                }
            };
            seen.push(Numbered {
                serial: v.item.serial,
                site: v.site,
                slot: v.slot,
                count: v.item.count,
                item_id: v.item.item_id,
                owner,
            });
        })?;
        if progress.done && !self.reported {
            self.reported = true;
            self.seen.sort_by_key(|n| n.serial);
            for group in self.seen.chunk_by(|a, b| a.serial == b.serial) {
                if out.len() >= DUPLICATES_MAX {
                    break;
                }
                let (Some(first), true) = (group.first(), group.len() > 1) else {
                    continue;
                };
                out.push(DuplicateSerial {
                    serial: first.serial,
                    places: group
                        .iter()
                        .map(|n| DuplicatePlace {
                            site: n.site,
                            owner: (*n.owner).clone(),
                            slot: n.slot,
                            count: n.count,
                            item_id: n.item_id,
                        })
                        .collect(),
                });
            }
            self.seen = Vec::new();
        }
        Ok(progress)
    }
}

// ---- EconomySummary --------------------------------------------------------------------------------------------

/// What the economy summary computes (Plan/25 §5.7).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EconomyQuery {
    /// The item whose pieces are counted (`None` = the Perin, `II_SYS_SYS_SCR_PERIN`).
    #[serde(default)]
    pub item_id: Option<u32>,
}

/// Penya by holder.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PenyaTotals {
    /// Wallets of playable characters.
    pub inventories: i64,
    /// Bank penya of playable characters.
    pub banks: i64,
    /// Guild bank penya.
    pub guild_banks: i64,
    /// Penya attached to open mails.
    pub mails: i64,
    /// Personal vendors hold no penya of their own (an open stall keeps its proceeds in the wallet): always 0.
    pub vendors: i64,
    /// The sum of the five lines.
    pub total: i64,
    /// Penya in undelivered pending grants (not in circulation yet).
    pub pending_grants: i64,
}

/// A rich character.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichCharacter {
    /// The character.
    pub character_id: CharacterId,
    /// Its name.
    pub name: String,
    /// Its account's name.
    pub account_name: String,
    /// Wallet.
    pub gold: i64,
    /// Its own bank.
    pub bank_gold: i64,
}

/// A rich account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichAccount {
    /// The account.
    pub account_id: AccountId,
    /// Its name.
    pub name: String,
    /// Playable characters' wallets.
    pub wallets: i64,
    /// Their banks.
    pub banks: i64,
    /// Wallets and banks.
    pub total: i64,
}

/// Pieces of one item by site.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemCensus {
    /// The item.
    pub item_id: u32,
    /// Pieces in total.
    pub pieces: u64,
    /// Stacks in total.
    pub stacks: u64,
    /// Pieces by site label (`inventory`, `bank`, `mail`, ...).
    pub by_site: BTreeMap<String, u64>,
}

/// Grants created on one day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantDay {
    /// The day, `YYYY-MM-DD` (UTC).
    pub day: String,
    /// Pending grant rows created that day.
    pub grants: u64,
    /// Of which gold grants.
    pub gold_grants: u64,
    /// Penya in them.
    pub gold: i64,
}

/// The economy overview (Plan/25 §5.7): computed on demand, shown with its "as of" time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EconomySummary {
    /// When it was computed (unix seconds).
    pub as_of: UnixTime,
    /// Playable characters / accounts counted.
    pub characters: u64,
    /// Accounts with at least one playable character.
    pub accounts: u64,
    /// Penya by holder.
    pub penya: PenyaTotals,
    /// The 50 richest characters (wallet + own bank).
    pub richest_characters: Vec<RichCharacter>,
    /// The 50 richest accounts (wallets + banks).
    pub richest_accounts: Vec<RichAccount>,
    /// The counted item.
    pub census: ItemCensus,
    /// Grants per day, newest day first, the last 30 days.
    pub grants_per_day: Vec<GrantDay>,
}

/// The pieces the economy scan streams (assembled by [`PersistHandle::admin_economy_summary`]).
#[derive(Clone, Debug)]
pub enum EconomyPart {
    /// The SQL aggregates (first chunk).
    Aggregates(Box<EconomySummary>),
    /// A chunk of the item census.
    Census(ItemCensus),
}

/// The economy pass: SQL aggregates in the first chunk, then the chunked census of one item over every container.
pub struct EconomyScan {
    walk: Option<ItemWalk>,
    census_item: u32,
    now: UnixTime,
    first: bool,
    census: ItemCensus,
}

impl EconomyScan {
    /// A pass at time `now`.
    pub fn new(query: &EconomyQuery, now: UnixTime) -> EconomyScan {
        let census_item = query
            .item_id
            .unwrap_or(flyff_common::defs::item::II_SYS_SYS_SCR_PERIN);
        EconomyScan {
            walk: Some(ItemWalk::new(Some(census_item.to_string()))),
            census_item,
            now,
            first: true,
            census: ItemCensus {
                item_id: census_item,
                ..ItemCensus::default()
            },
        }
    }
}

fn day_of(unix: i64) -> String {
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

impl EconomyScan {
    fn aggregates(&self, conn: &Connection) -> Result<EconomySummary, DbError> {
        let one = |sql: &str| -> Result<i64, DbError> {
            Ok(conn
                .query_row(sql, [], |r| r.get::<_, Option<i64>>(0))?
                .unwrap_or(0))
        };
        let inventories = one("SELECT SUM(gold) FROM characters WHERE state = 0")?;
        let banks = one(
            "SELECT SUM(b.gold) FROM banks b JOIN characters ch ON ch.id = b.character_id WHERE ch.state = 0",
        )?;
        let guild_banks = one("SELECT SUM(gold) FROM guilds")?;
        let mails = one(
            "SELECT SUM(gold) FROM mails WHERE deleted_at IS NULL AND gold_taken_at IS NULL AND gold > 0",
        )?;
        let pending = one(
            "SELECT SUM(gold) FROM pending_grants WHERE kind = 'gold' AND delivered_at IS NULL AND cancelled_at IS NULL",
        )?;
        let characters =
            u64::try_from(one("SELECT COUNT(*) FROM characters WHERE state = 0")?).unwrap_or(0);
        let accounts = u64::try_from(one(
            "SELECT COUNT(DISTINCT account_id) FROM characters WHERE state = 0",
        )?)
        .unwrap_or(0);
        let richest_characters = {
            let mut stmt = conn.prepare_cached(
                "SELECT ch.id, ch.name, a.name, ch.gold, COALESCE(b.gold, 0)
                 FROM characters ch JOIN accounts a ON a.id = ch.account_id
                 LEFT JOIN banks b ON b.character_id = ch.id
                 WHERE ch.state = 0
                 ORDER BY ch.gold + COALESCE(b.gold, 0) DESC, ch.id LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![i64::try_from(RICHEST_TOP).unwrap_or(50)], |r| {
                Ok(RichCharacter {
                    character_id: CharacterId(r.get(0)?),
                    name: r.get(1)?,
                    account_name: r.get(2)?,
                    gold: r.get(3)?,
                    bank_gold: r.get(4)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let richest_accounts = {
            let mut stmt = conn.prepare_cached(
                "SELECT a.id, a.name, SUM(ch.gold), SUM(COALESCE(b.gold, 0))
                 FROM characters ch JOIN accounts a ON a.id = ch.account_id
                 LEFT JOIN banks b ON b.character_id = ch.id
                 WHERE ch.state = 0
                 GROUP BY a.id
                 ORDER BY SUM(ch.gold) + SUM(COALESCE(b.gold, 0)) DESC, a.id LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![i64::try_from(RICHEST_TOP).unwrap_or(50)], |r| {
                let wallets: i64 = r.get(2)?;
                let banks: i64 = r.get(3)?;
                Ok(RichAccount {
                    account_id: AccountId(r.get(0)?),
                    name: r.get(1)?,
                    wallets,
                    banks,
                    total: wallets.saturating_add(banks),
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let since = self.now.0 - GRANT_DAYS * 86_400;
        let grants_per_day = {
            let mut stmt = conn.prepare_cached(
                "SELECT created_at / 86400 AS d, COUNT(*), SUM(kind = 'gold'), SUM(CASE WHEN kind = 'gold' THEN gold ELSE 0 END)
                 FROM pending_grants WHERE created_at >= ?1 GROUP BY d ORDER BY d DESC",
            )?;
            let rows = stmt.query_map(params![since], |r| {
                let d: i64 = r.get(0)?;
                Ok(GrantDay {
                    day: day_of(d * 86_400),
                    grants: u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
                    gold_grants: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                    gold: r.get(3)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let total = inventories
            .saturating_add(banks)
            .saturating_add(guild_banks)
            .saturating_add(mails);
        Ok(EconomySummary {
            as_of: self.now,
            characters,
            accounts,
            penya: PenyaTotals {
                inventories,
                banks,
                guild_banks,
                mails,
                vendors: 0,
                total,
                pending_grants: pending,
            },
            richest_characters,
            richest_accounts,
            census: ItemCensus {
                item_id: self.census_item,
                ..ItemCensus::default()
            },
            grants_per_day,
        })
    }
}

impl AdminScan for EconomyScan {
    type Item = EconomyPart;

    fn heavy(&self) -> bool {
        true
    }

    fn step(
        &mut self,
        conn: &Connection,
        budget: &Budget,
        out: &mut Vec<EconomyPart>,
    ) -> Result<Progress, DbError> {
        if self.first {
            self.first = false;
            out.push(EconomyPart::Aggregates(Box::new(self.aggregates(conn)?)));
            return Ok(Progress::more(0));
        }
        let Some(walk) = self.walk.as_mut() else {
            return Ok(Progress::finished(0));
        };
        let wanted = self.census_item;
        let census = &mut self.census;
        let progress = walk.step(conn, budget, &mut |v: Visit<'_>| {
            if v.item.item_id == wanted {
                census.pieces += u64::from(v.item.count);
                census.stacks += 1;
                *census.by_site.entry(site_group(v.site)).or_insert(0) += u64::from(v.item.count);
            }
        })?;
        if progress.done {
            out.push(EconomyPart::Census(std::mem::take(&mut self.census)));
        }
        Ok(progress)
    }
}

/// The site label of the census (pockets and the bag are listed apart, as the tables show them).
fn site_group(site: ItemSite) -> String {
    match site {
        ItemSite::Pocket(_) => "pocket".to_owned(),
        other => other.label().replace(' ', "_"),
    }
}

// ---- RecipientFacts --------------------------------------------------------------------------------------------

/// What the grant route needs to know about a recipient before it asks the game thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipientFact {
    /// The character.
    pub character_id: CharacterId,
    /// Its name.
    pub name: String,
    /// Its state.
    pub state: CharacterState,
    /// Its account.
    pub account_id: AccountId,
    /// The account's name.
    pub account_name: String,
    /// `characters.authority` (an ASCII letter).
    pub character_authority: u8,
    /// `accounts.authority` (an ASCII letter).
    pub account_authority: u8,
}

fn recipient_facts(conn: &Connection, ids: &[CharacterId]) -> Result<Vec<RecipientFact>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT ch.id, ch.name, ch.state, ch.account_id, a.name, ch.authority, a.authority
         FROM characters ch JOIN accounts a ON a.id = ch.account_id WHERE ch.id = ?1",
    )?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let row = stmt
            .query_row(params![id.0], |r| {
                Ok((
                    r.get::<_, u32>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, u32>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })
            .optional()?;
        if let Some((cid, name, state, account, account_name, cauth, aauth)) = row {
            out.push(RecipientFact {
                character_id: CharacterId(cid),
                name,
                state: character_state_from_db(state)?,
                account_id: AccountId(account),
                account_name,
                character_authority: cauth.bytes().next().unwrap_or(b'F'),
                account_authority: aauth.bytes().next().unwrap_or(b'F'),
            });
        }
    }
    Ok(out)
}

fn recipient_by_name(conn: &Connection, name: &str) -> Result<Vec<RecipientFact>, DbError> {
    // `characters.name` is NOCASE; a live character wins over a deleted one with the same name
    let id: Option<u32> = conn
        .prepare_cached(
            "SELECT id FROM characters WHERE name = ?1 ORDER BY (state = 1), id DESC LIMIT 1",
        )?
        .query_row(params![name], |r| r.get(0))
        .optional()?;
    match id {
        Some(id) => recipient_facts(conn, &[CharacterId(id)]),
        None => Ok(Vec::new()),
    }
}

impl PersistHandle {
    /// Starts the streaming item search of Plan/25 §5.5 (chunked, cancellable, one heavy scan at a time).
    pub fn admin_item_search(
        &self,
        query: ItemSearchQuery,
    ) -> Result<super::scan::ScanStream<ItemHit>, DbError> {
        query.validate()?;
        self.admin_scan(ItemSearchScan::new(query))
    }

    /// The duplicate-serial report (the same coverage and the same verdict as `flyff db check --serials`).
    pub async fn admin_duplicate_serials(&self) -> Result<Vec<DuplicateSerial>, DbError> {
        self.admin_scan(DuplicateSerialsScan::new())?
            .collect()
            .await
    }

    /// Blocking variant of [`PersistHandle::admin_duplicate_serials`].
    pub fn admin_duplicate_serials_blocking(&self) -> Result<Vec<DuplicateSerial>, DbError> {
        self.admin_scan(DuplicateSerialsScan::new())?
            .collect_blocking()
    }

    /// The economy overview (Plan/25 §5.7) at `now`.
    pub async fn admin_economy_summary(
        &self,
        query: EconomyQuery,
        now: UnixTime,
    ) -> Result<EconomySummary, DbError> {
        let parts = self
            .admin_scan(EconomyScan::new(&query, now))?
            .collect()
            .await?;
        assemble_economy(parts)
    }

    /// Blocking variant of [`PersistHandle::admin_economy_summary`].
    pub fn admin_economy_summary_blocking(
        &self,
        query: EconomyQuery,
        now: UnixTime,
    ) -> Result<EconomySummary, DbError> {
        let parts = self
            .admin_scan(EconomyScan::new(&query, now))?
            .collect_blocking()?;
        assemble_economy(parts)
    }

    /// The facts about up to [`RECIPIENTS_MAX`] characters (unknown ids are absent from the answer).
    pub async fn admin_recipient_facts(
        &self,
        ids: Vec<CharacterId>,
    ) -> Result<Vec<RecipientFact>, DbError> {
        recipient_scan(self, ids)?.collect().await
    }

    /// The facts about the character called `name` (case-insensitive), `None` when nobody is.
    pub async fn admin_recipient_by_name(
        &self,
        name: String,
    ) -> Result<Option<RecipientFact>, DbError> {
        let rows = self
            .admin_scan(Once::new(move |conn: &Connection| {
                recipient_by_name(conn, &name)
            }))?
            .collect()
            .await?;
        Ok(rows.into_iter().next())
    }

    /// Blocking variant of [`PersistHandle::admin_recipient_facts`].
    pub fn admin_recipient_facts_blocking(
        &self,
        ids: Vec<CharacterId>,
    ) -> Result<Vec<RecipientFact>, DbError> {
        recipient_scan(self, ids)?.collect_blocking()
    }
}

fn recipient_scan(
    h: &PersistHandle,
    ids: Vec<CharacterId>,
) -> Result<super::scan::ScanStream<RecipientFact>, DbError> {
    if ids.len() > RECIPIENTS_MAX {
        return Err(invalid(format!("at most {RECIPIENTS_MAX} recipients")));
    }
    h.admin_scan(Once::new(move |conn: &Connection| {
        recipient_facts(conn, &ids)
    }))
}

fn assemble_economy(parts: Vec<EconomyPart>) -> Result<EconomySummary, DbError> {
    let mut summary: Option<EconomySummary> = None;
    let mut census: Option<ItemCensus> = None;
    for part in parts {
        match part {
            EconomyPart::Aggregates(s) => summary = Some(*s),
            EconomyPart::Census(c) => census = Some(c),
        }
    }
    let mut summary = summary
        .ok_or_else(|| DbError::new(DbErrorKind::Bug, "the economy scan produced no aggregates"))?;
    if let Some(c) = census {
        summary.census = c;
    }
    Ok(summary)
}
