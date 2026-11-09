//! Moderation queries (Plan/25 §4.8, §11.5): `ListMutes` (WP-ADMIN-17).

use super::AdminQueryResult;
use crate::tick::Game;

/// One active mute (Moderation -> Mutes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MuteRow {
    /// The muted character.
    pub character_id: u32,
    /// Its account.
    pub account_id: u32,
    /// Its name.
    pub name: String,
    /// Seconds left (`PlayerMisc.mute_remaining_s`, 23 §2.6).
    pub remaining_s: u32,
    /// Unix seconds the mute ends.
    pub until: i64,
}

/// `ListMutes`: the online characters with a running mute, longest first, then by character id. A mute of an
/// offline character is stored with the character (`characters.mute_remaining_s`) and counts down only while
/// the character is online (KQ-22), so it has no end time to list.
pub(super) fn list_mutes(g: &Game) -> AdminQueryResult {
    let now = g.clock.unix_ms.to_unix_time().0;
    let mut rows: Vec<MuteRow> = g
        .sessions
        .iter()
        .filter_map(|e| {
            let m = g.worlds.player(e.player)?;
            let p = m.player()?;
            let left = p.misc.mute_remaining_s;
            (left > 0).then(|| MuteRow {
                character_id: e.character.0,
                account_id: e.account.0,
                name: p.ident.name.clone(),
                remaining_s: left,
                until: now.saturating_add(i64::from(left)),
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        b.remaining_s
            .cmp(&a.remaining_s)
            .then(a.character_id.cmp(&b.character_id))
    });
    AdminQueryResult::Mutes(rows)
}
