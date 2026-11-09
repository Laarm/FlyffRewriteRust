//! The admin IP blocks and the live `net.ip_block` set (Plan/25 §4.8, §11.6; WP-ADMIN-17).
//!
//! The rows of `admin_ip_blocks` (CIDR, reason, end time, author) are merged into the HTTP layer's block check
//! ([`LiveLists::set_admin_blocks`](crate::http::live::LiveLists::set_admin_blocks)): a request from a blocked
//! address is refused with `403` by layer 0, before anything is counted (24 §3.3), and so is a `/ws` upgrade.
//! The set is loaded at start ([`load`]) and reloaded after every add or remove of the console ([`reload`]); a block
//! with an end time stops matching by itself.

use crate::http::live::LiveLists;
use flyff_db::{AdminReadQuery, DbError, PersistHandle};

/// The `(cidr, until)` pairs of the table, every row (the check compares `until` with the clock).
fn rows_of(rows: &[flyff_db::IpBlockRecord]) -> Vec<(String, Option<i64>)> {
    rows.iter()
        .map(|b| (b.cidr.clone(), b.until.map(|t| t.0)))
        .collect()
}

/// Loads the table into `live` (start-up; blocking). A read error leaves the set as it is and is returned.
pub fn load(persist: &PersistHandle, live: &LiveLists) -> Result<usize, DbError> {
    let rows = persist
        .admin_read_blocking(AdminReadQuery::IpBlocks { active_at: None })?
        .ip_blocks()?;
    Ok(apply(live, &rows))
}

/// Reloads the table into `live` (after an add or a remove).
pub async fn reload(persist: &PersistHandle, live: &LiveLists) -> Result<usize, DbError> {
    let rows = persist
        .admin_read(AdminReadQuery::IpBlocks { active_at: None })
        .await?
        .ip_blocks()?;
    Ok(apply(live, &rows))
}

fn apply(live: &LiveLists, rows: &[flyff_db::IpBlockRecord]) -> usize {
    let bad = live.set_admin_blocks(&rows_of(rows));
    for b in &bad {
        tracing::warn!(entry = %b, "admin_ip_blocks: not a CIDR network, ignored");
    }
    live.admin_block_count()
}
