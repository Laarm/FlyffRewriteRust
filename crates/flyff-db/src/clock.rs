//! Wall-clock helper of the persistence layer (stamps `saved_at`, `server_kv`, backup names).

use flyff_common::UnixTime;

/// The current UTC time in whole seconds (`0` when the clock is before the epoch).
pub(crate) fn now_unix() -> UnixTime {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    UnixTime(secs)
}
