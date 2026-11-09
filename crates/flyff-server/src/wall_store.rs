//! The `WallStore` over `server_kv` (Plan/10 §5.5, 13 §15.2; WP-OPS-07): `schedule.last_fired.<job>` as unix
//! seconds. The values loaded at start come from `StartupState.server_kv`; every firing is written with
//! `DbOp::SetServerKv` through the persistence actor (queued, never blocking the game thread), so a calendar job
//! that fired before a restart does not fire a second time, and a `RunOnceAtStart` job catches up the firing it
//! missed while the server was down.

use flyff_common::UnixTime;
use flyff_db::{DbOp, PersistHandle};
use flyff_game::WallStore;
use std::collections::BTreeMap;

/// The `server_kv` key prefix of the last firings.
pub const LAST_FIRED_PREFIX: &str = "schedule.last_fired.";

/// The persistent store of the `last_fired` times.
pub struct KvWallStore {
    persist: PersistHandle,
    last: BTreeMap<String, UnixTime>,
}

impl KvWallStore {
    /// Reads every `schedule.last_fired.<job>` of the startup `server_kv` (non-numeric values are ignored).
    pub fn load(server_kv: &BTreeMap<String, serde_json::Value>, persist: PersistHandle) -> Self {
        let last = server_kv
            .iter()
            .filter_map(|(k, v)| {
                let job = k.strip_prefix(LAST_FIRED_PREFIX)?;
                Some((job.to_owned(), UnixTime(v.as_i64()?)))
            })
            .collect();
        Self { persist, last }
    }
}

impl WallStore for KvWallStore {
    fn last_fired(&self, job: &str) -> Option<UnixTime> {
        self.last.get(job).copied()
    }

    fn set_last_fired(&mut self, job: &str, at: UnixTime) {
        self.last.insert(job.to_owned(), at);
        self.persist.op(DbOp::SetServerKv {
            key: format!("{LAST_FIRED_PREFIX}{job}"),
            value: serde_json::json!(at.0),
        });
    }
}
