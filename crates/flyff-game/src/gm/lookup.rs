//! The lookup conveniences of the GM commands (Plan/23 §13 Q2, PL-099): a name matches **exactly** first (the
//! legacy `strcmp`), then ASCII case-insensitively, then as the define name (`II_*`, `MI_*`, `JOB_*`), in this
//! order. A NUMBER token is an id. Items, movers and jobs share the rule; characters are the directory's (the
//! names are unique case-insensitively, 23 §3.7).

use flyff_common::defs;
use flyff_data::GameData;
use flyff_rules::consts::job::MAX_JOB;

/// A stored name against a typed token: exact, or ASCII case-insensitive when `fold`.
pub fn name_matches(stored: &str, token: &str, fold: bool) -> bool {
    if fold {
        stored.eq_ignore_ascii_case(token)
    } else {
        stored == token
    }
}

/// The define named `token` (case-insensitive) when it starts with `prefix`, as an id.
fn define_id(token: &str, prefix: &str) -> Option<u32> {
    let upper = token.to_ascii_uppercase();
    if !upper.starts_with(prefix) {
        return None;
    }
    u32::try_from(defs::lookup(&upper)?).ok()
}

/// The item a GM command word names: a NUMBER is an item id (it must exist), otherwise the display name
/// (exact, then case-insensitive, first id wins) or the `II_*` define name. `None` for an unknown item.
pub fn item(gd: &GameData, token: &str, is_number: bool) -> Option<u32> {
    if is_number {
        let id = u32::try_from(flyff_script::cnum::c_atoi(token.as_bytes())).ok()?;
        return gd.item(id).map(|p| p.id);
    }
    if token.is_empty() {
        return None;
    }
    // the legacy map (`m_mapII`, first wins) is the exact step
    if let Some(&id) = gd.item_index.by_name.get(token) {
        return Some(id);
    }
    if let Some((id, _)) = gd
        .items
        .iter()
        .find(|(_, p)| name_matches(&p.name, token, true))
    {
        return Some(id);
    }
    let id = define_id(token, "II_")?;
    gd.item(id).map(|p| p.id)
}

/// The mover a GM command word names (`/createnpc`, `/disguise`): a NUMBER is a mover id, otherwise the name
/// (exact, then case-insensitive) or the `MI_*` define name.
pub fn mover(gd: &GameData, token: &str, is_number: bool) -> Option<u32> {
    if is_number {
        let id = u32::try_from(flyff_script::cnum::c_atoi(token.as_bytes())).ok()?;
        return gd.mover(id).map(|_| id);
    }
    if token.is_empty() {
        return None;
    }
    for fold in [false, true] {
        if let Some((id, _)) = gd
            .movers
            .iter()
            .find(|(_, p)| name_matches(&p.name, token, fold))
        {
            return Some(id);
        }
    }
    let id = define_id(token, "MI_")?;
    gd.mover(id).map(|_| id)
}

/// The job a command word names (the legacy loop `strcmp(strJob, m_aJob[i].szName)`, first match): the exact
/// name, else the name ignoring case, else the `JOB_*` define name (`JOB_KNIGHT_MASTER`); `None` when nothing
/// matches (the legacy commands then fall back to the vagrant).
pub fn job(gd: &GameData, token: &str) -> Option<usize> {
    if token.is_empty() {
        return None;
    }
    for fold in [false, true] {
        let hit = (0..MAX_JOB as usize).find(|&i| {
            gd.jobs
                .info
                .get(i)
                .and_then(Option::as_ref)
                .is_some_and(|j| name_matches(&j.name, token, fold))
        });
        if hit.is_some() {
            return hit;
        }
    }
    define_id(token, "JOB_")
        .and_then(|v| usize::try_from(v).ok())
        .filter(|&j| j < MAX_JOB as usize)
}

/// Every item whose display name or `II_*` define name contains `needle` (case-insensitive substring), in
/// ascending id order: `(id, display name)`.
pub fn search_items(gd: &GameData, needle: &str) -> Vec<(u32, String)> {
    let needle = needle.to_ascii_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    // ids that have a define name containing the needle
    let mut by_define = std::collections::BTreeSet::new();
    for (name, value) in defs::ALL {
        if name.starts_with("II_")
            && name.to_ascii_lowercase().contains(&needle)
            && let Ok(id) = u32::try_from(*value)
        {
            by_define.insert(id);
        }
    }
    gd.items
        .iter()
        .filter(|(id, p)| by_define.contains(id) || p.name.to_ascii_lowercase().contains(&needle))
        .map(|(id, p)| (id, p.name.clone()))
        .collect()
}
