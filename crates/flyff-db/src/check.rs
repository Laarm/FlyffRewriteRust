//! `flyff db check` (Plan/13 §21.5): read-only consistency checks that are safe while the server runs.
//!
//! | mode | checks |
//! |---|---|
//! | `Quick` (default) | `quick_check`, `foreign_key_check`, `application_id`, `user_version`, every `meta` counter at least `MAX(id) + 1` of its table |
//! | `Full` | everything of `Quick` with `integrity_check`, plus: every JSON column parses and upgrades, every container passes `validate_and_repair` without a repair |
//! | `Serials` | the header checks and the **duplicate item serial detector**: no item serial appears twice across `character_docs` (inventory, pockets), `banks`, `guild_banks`, open mails and undelivered grants, and `meta.next_item_serial` is above the highest serial in use (PL-050) |
//! | `Items` | the header checks and, through a callback that knows the game data, unknown item ids and counts outside the allowed range |
//!
//! Findings are printed as a table by the CLI (exit code 1); a database that cannot be opened is an error (2).

use crate::config::PersistenceConfig;
use crate::conn::{APPLICATION_ID, open_reader, pragma_i64};
use crate::error::{DbError, DbErrorKind};
use crate::migrate::{NEXT_ID_COUNTERS, ROUND_COUNTERS, latest_version, meta_get, table_exists};
use crate::model::{
    BankDoc, CharacterDocs, ContainerRecord, GuildBankDoc, InventoryDoc, ItemRecord, JsonDoc,
    PocketsDoc, StoredItem,
};
use rusqlite::{Connection, OptionalExtension};
use std::collections::BTreeMap;
use std::path::Path;

/// What to check (the `--quick | --full | --serials | --items` flags).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CheckMode {
    /// `quick_check`, foreign keys, header, counters.
    #[default]
    Quick,
    /// `integrity_check`, documents, containers.
    Full,
    /// Item serial uniqueness.
    Serials,
    /// Item ids and counts against the game data.
    Items,
}

/// One problem found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The check that found it (`sqlite`, `foreign_key`, `header`, `counter`, `document`, `container`,
    /// `serial`, `item`).
    pub check: &'static str,
    /// Where and what.
    pub message: String,
}

/// The result of a run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// Problems; empty = clean.
    pub findings: Vec<Finding>,
    /// Findings beyond the per-check cap that are not listed.
    pub suppressed: u64,
    /// Informational lines (an older schema, scan sizes).
    pub notes: Vec<String>,
    /// `PRAGMA user_version`.
    pub user_version: i64,
    /// Items looked at by the serial and item checks.
    pub items_scanned: u64,
}

impl CheckReport {
    /// `true` when nothing was found.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty() && self.suppressed == 0
    }

    fn add(&mut self, check: &'static str, message: impl Into<String>) {
        const CAP: usize = 100;
        if self.findings.iter().filter(|f| f.check == check).count() >= CAP {
            self.suppressed += 1;
        } else {
            self.findings.push(Finding {
                check,
                message: message.into(),
            });
        }
    }
}

/// A callback that judges one item: `(item_id, count)` -> a problem text, `None` when fine.
pub type ItemJudge<'a> = &'a dyn Fn(u32, u16) -> Option<String>;

/// Opens `base_dir/<database.path>` read-only and runs the checks. `NotFound` when there is no database.
pub fn cli_check(
    cfg: &PersistenceConfig,
    base_dir: &Path,
    mode: CheckMode,
    judge: Option<ItemJudge<'_>>,
) -> Result<CheckReport, DbError> {
    cfg.validate()?;
    let db_path = base_dir.join(&cfg.database.path);
    if !db_path.is_file() {
        return Err(DbError::new(
            DbErrorKind::NotFound,
            format!("there is no database at {}", db_path.display()),
        ));
    }
    let conn = open_reader(&db_path, &cfg.database)?;
    check_database(&conn, mode, judge)
}

/// Runs the checks of `mode` on `conn` (a read-only connection is enough).
pub fn check_database(
    conn: &Connection,
    mode: CheckMode,
    judge: Option<ItemJudge<'_>>,
) -> Result<CheckReport, DbError> {
    // one read transaction: the whole report describes one consistent state while the server writes
    let tx = conn.unchecked_transaction()?;
    let mut report = CheckReport::default();
    header(&tx, &mut report)?;
    match mode {
        CheckMode::Quick => {
            sqlite_check(&tx, "quick_check", &mut report)?;
            foreign_keys(&tx, &mut report)?;
            counters(&tx, &mut report)?;
        }
        CheckMode::Full => {
            sqlite_check(&tx, "integrity_check", &mut report)?;
            foreign_keys(&tx, &mut report)?;
            counters(&tx, &mut report)?;
            documents(&tx, &mut report)?;
        }
        CheckMode::Serials => serials(&tx, &mut report)?,
        CheckMode::Items => match judge {
            Some(j) => items(&tx, j, &mut report)?,
            None => {
                return Err(DbError::new(
                    DbErrorKind::InvalidConfig,
                    "db check --items needs the game data (Resource/)",
                ));
            }
        },
    }
    tx.commit()?;
    Ok(report)
}

fn header(conn: &Connection, report: &mut CheckReport) -> Result<(), DbError> {
    let app_id = pragma_i64(conn, "application_id")?;
    if app_id != APPLICATION_ID {
        report.add(
            "header",
            format!(
                "application_id is {app_id:#x}, expected {APPLICATION_ID:#x} (not a flyff database)"
            ),
        );
    }
    let version = pragma_i64(conn, "user_version")?;
    report.user_version = version;
    let latest = i64::from(latest_version());
    if version > latest {
        report.add(
            "header",
            format!("schema v{version} is newer than this binary (v{latest})"),
        );
    } else if version < latest {
        report
            .notes
            .push(format!("schema v{version} is older than this binary (v{latest}): `flyff db migrate` or the next start upgrades it"));
    }
    Ok(())
}

fn sqlite_check(conn: &Connection, pragma: &str, report: &mut CheckReport) -> Result<(), DbError> {
    let mut stmt = conn.prepare(&format!("PRAGMA {pragma}"))?;
    let lines: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    if !(lines.len() == 1 && lines.first().is_some_and(|l| l == "ok")) {
        for line in lines {
            report.add("sqlite", format!("{pragma}: {line}"));
        }
    }
    Ok(())
}

fn foreign_keys(conn: &Connection, report: &mut CheckReport) -> Result<(), DbError> {
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let rows: Vec<(String, Option<i64>, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (table, rowid, parent) in rows {
        report.add(
            "foreign_key",
            format!(
                "{table} row {} references a missing row of {parent}",
                rowid.map_or_else(|| "?".to_owned(), |r| r.to_string())
            ),
        );
    }
    Ok(())
}

fn counters(conn: &Connection, report: &mut CheckReport) -> Result<(), DbError> {
    let all = NEXT_ID_COUNTERS
        .iter()
        .map(|c| (c, 1_i64))
        .chain(ROUND_COUNTERS.iter().map(|c| (c, 0_i64)));
    for (&(key, table, column), plus) in all {
        if !table_exists(conn, table)? {
            continue;
        }
        let max: i64 = conn.query_row(
            &format!("SELECT COALESCE(MAX({column}), 0) FROM {table}"),
            [],
            |r| r.get(0),
        )?;
        let wanted = max.saturating_add(plus);
        match meta_get(conn, key)? {
            Some(have) if have >= wanted => {}
            Some(have) => report.add(
                "counter",
                format!("meta.{key} is {have} but {table}.{column} reaches {max}: the counter must be at least {wanted}"),
            ),
            None => report.add("counter", format!("meta.{key} is missing")),
        }
    }
    Ok(())
}

// ---- documents and containers ---------------------------------------------------------------------------------

/// Expected container capacities, from the init-only values recorded in `meta` (the loader's rule).
struct Capacities {
    inventory: u16,
    pockets: [u16; 3],
}

fn capacities(conn: &Connection) -> Result<Capacities, DbError> {
    let init = crate::migrate::stored_init_only(conn)?.unwrap_or_default();
    Ok(Capacities {
        inventory: if init.inventory_168 {
            InventoryDoc::CAPACITY_168
        } else {
            42 + crate::model::EQUIP_SLOTS
        },
        pockets: if init.bigger_pocket {
            PocketsDoc::CAPACITIES_BIG
        } else {
            [6, 24, 24]
        },
    })
}

fn documents(conn: &Connection, report: &mut CheckReport) -> Result<(), DbError> {
    let cap = capacities(conn)?;
    let mut stmt = conn.prepare(
        "SELECT d.character_id, d.inventory, d.pockets, d.skills, d.quests, d.taskbar, d.buffs,
                d.sm_timers, d.honor, d.settings
         FROM character_docs d ORDER BY d.character_id",
    )?;
    let mut rows = stmt.query([])?;
    let mut scanned = 0_u64;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let mut columns: Vec<String> = Vec::with_capacity(9);
        for i in 1..=9 {
            columns.push(row.get(i)?);
        }
        scanned += 1;
        let Ok(columns) = <[String; 9]>::try_from(columns) else {
            continue;
        };
        match CharacterDocs::from_json_columns(&columns) {
            Err(e) => report.add("document", format!("character {id}: {}", e.context)),
            Ok(mut docs) => {
                for note in docs.inventory.validate_and_repair(cap.inventory) {
                    report.add(
                        "container",
                        format!("character {id} inventory needs a repair: {note:?}"),
                    );
                }
                for note in docs.pockets.validate_and_repair(cap.pockets) {
                    report.add(
                        "container",
                        format!("character {id} pockets need a repair: {note:?}"),
                    );
                }
            }
        }
    }
    drop(rows);
    drop(stmt);
    scan_banks(
        conn,
        report,
        |report, owner, text| match BankDoc::from_json(text) {
            Err(e) => report.add("document", format!("{owner}: {}", e.context)),
            Ok(mut bank) => {
                for note in bank.validate_and_repair() {
                    report.add("container", format!("{owner} needs a repair: {note:?}"));
                }
            }
        },
    )?;
    if table_exists(conn, "guild_banks")? {
        let mut stmt = conn.prepare("SELECT guild_id, container FROM guild_banks")?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (guild, text) in rows {
            match GuildBankDoc::from_json(&text) {
                Err(e) => report.add("document", format!("guild bank {guild}: {}", e.context)),
                Ok(mut bank) => {
                    for note in bank.validate_and_repair() {
                        report.add(
                            "container",
                            format!("guild bank {guild} needs a repair: {note:?}"),
                        );
                    }
                }
            }
        }
    }
    for (table, column, label) in [
        ("mails", "item", "mail"),
        ("pending_grants", "item", "grant"),
    ] {
        if !table_exists(conn, table)? {
            continue;
        }
        let mut stmt = conn.prepare(&format!(
            "SELECT id, {column} FROM {table} WHERE {column} IS NOT NULL"
        ))?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (id, text) in rows {
            if let Err(e) = StoredItem::from_json(&text) {
                report.add("document", format!("{label} {id} item: {}", e.context));
            }
        }
    }
    report
        .notes
        .push(format!("{scanned} character document sets parsed"));
    Ok(())
}

fn scan_banks(
    conn: &Connection,
    report: &mut CheckReport,
    mut each: impl FnMut(&mut CheckReport, &str, &str),
) -> Result<(), DbError> {
    let mut stmt = conn.prepare("SELECT character_id, container FROM banks")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (character, text) in rows {
        each(report, &format!("bank of character {character}"), &text);
    }
    Ok(())
}

// ---- walking every item -----------------------------------------------------------------------------------------

/// Where an item was found, for the messages.
type Place = String;

/// Calls `each(place, item)` for every item in the places the serial and item checks cover. Documents that do
/// not parse are reported as findings and skipped.
fn for_each_item(
    conn: &Connection,
    report: &mut CheckReport,
    mut each: impl FnMut(&mut CheckReport, &Place, &ItemRecord),
) -> Result<(), DbError> {
    fn visit(
        report: &mut CheckReport,
        place: &str,
        container: &ContainerRecord,
        each: &mut impl FnMut(&mut CheckReport, &Place, &ItemRecord),
    ) {
        for item in &container.items {
            each(report, &place.to_owned(), item);
        }
    }

    let mut stmt = conn.prepare(
        "SELECT d.character_id, ch.state, d.inventory, d.pockets
         FROM character_docs d JOIN characters ch ON ch.id = d.character_id
         ORDER BY d.character_id",
    )?;
    let rows: Vec<(i64, i64, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    for (id, state, inventory, pockets) in rows {
        let who = if state == 1 {
            format!("deleted character {id}")
        } else {
            format!("character {id}")
        };
        match InventoryDoc::from_json(&inventory) {
            Ok(doc) => visit(
                report,
                &format!("{who} inventory"),
                &doc.container,
                &mut each,
            ),
            Err(e) => report.add("document", format!("{who} inventory: {}", e.context)),
        }
        match PocketsDoc::from_json(&pockets) {
            Ok(doc) => {
                for (i, pocket) in doc.pockets.iter().enumerate() {
                    visit(
                        report,
                        &format!("{who} pocket {i}"),
                        &pocket.container,
                        &mut each,
                    );
                }
            }
            Err(e) => report.add("document", format!("{who} pockets: {}", e.context)),
        }
    }
    drop(stmt);
    scan_banks(
        conn,
        report,
        |report, owner, text| match BankDoc::from_json(text) {
            Ok(doc) => visit(report, owner, &doc.container, &mut each),
            Err(e) => report.add("document", format!("{owner}: {}", e.context)),
        },
    )?;
    if table_exists(conn, "guild_banks")? {
        let mut stmt = conn.prepare("SELECT guild_id, container FROM guild_banks")?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (guild, text) in rows {
            match GuildBankDoc::from_json(&text) {
                Ok(doc) => visit(
                    report,
                    &format!("guild bank {guild}"),
                    &doc.container,
                    &mut each,
                ),
                Err(e) => report.add("document", format!("guild bank {guild}: {}", e.context)),
            }
        }
    }
    // open mails: an attachment that was not taken yet and a mail that is not deleted
    if table_exists(conn, "mails")? {
        let mut stmt = conn.prepare(
            "SELECT id, item FROM mails WHERE item IS NOT NULL AND deleted_at IS NULL AND item_taken_at IS NULL",
        )?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (id, text) in rows {
            match StoredItem::from_json(&text) {
                Ok(stored) => each(report, &format!("mail {id}"), &stored.item),
                Err(e) => report.add("document", format!("mail {id} item: {}", e.context)),
            }
        }
    }
    // undelivered, uncancelled grants: serial 0 = "assign one at delivery" (a template), not a serial in use
    if table_exists(conn, "pending_grants")? {
        let mut stmt = conn.prepare(
            "SELECT id, item FROM pending_grants
             WHERE item IS NOT NULL AND delivered_at IS NULL AND cancelled_at IS NULL",
        )?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (id, text) in rows {
            match StoredItem::from_json(&text) {
                Ok(stored) => each(report, &format!("grant {id}"), &stored.item),
                Err(e) => report.add("document", format!("grant {id} item: {}", e.context)),
            }
        }
    }
    Ok(())
}

/// PL-050: every item serial appears once.
fn serials(conn: &Connection, report: &mut CheckReport) -> Result<(), DbError> {
    let mut seen: BTreeMap<u64, Vec<Place>> = BTreeMap::new();
    let mut scanned = 0_u64;
    for_each_item(conn, report, |_, place, item| {
        scanned += 1;
        if item.serial != 0 {
            seen.entry(item.serial).or_default().push(place.clone());
        }
    })?;
    report.items_scanned = scanned;
    let mut highest = 0_u64;
    for (serial, places) in &seen {
        highest = highest.max(*serial);
        if places.len() > 1 {
            report.add(
                "serial",
                format!(
                    "serial {serial} appears {} times: {}",
                    places.len(),
                    places.join(", ")
                ),
            );
        }
    }
    let next: Option<i64> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'next_item_serial'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match next {
        Some(n) if u64::try_from(n).is_ok_and(|n| n > highest) => {}
        Some(n) => report.add(
            "counter",
            format!("meta.next_item_serial is {n} but serial {highest} is in use"),
        ),
        None => report.add("counter", "meta.next_item_serial is missing"),
    }
    report
        .notes
        .push(format!("{scanned} items, {} distinct serials", seen.len()));
    Ok(())
}

/// `--items`: the callback judges every item.
fn items(conn: &Connection, judge: ItemJudge<'_>, report: &mut CheckReport) -> Result<(), DbError> {
    let mut scanned = 0_u64;
    for_each_item(conn, report, |report, place, item| {
        scanned += 1;
        if let Some(problem) = judge(item.item_id, item.count) {
            report.add(
                "item",
                format!("{place}: item {} x{}: {problem}", item.item_id, item.count),
            );
        }
    })?;
    report.items_scanned = scanned;
    report.notes.push(format!("{scanned} items checked"));
    Ok(())
}
