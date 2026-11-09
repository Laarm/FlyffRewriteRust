//! `QuestDestination.txt.txt` and `PatrolDestination.txt.txt`: destination names of the quest UI
//! (`CProject::LoadQuestDestination`, `LoadPatrolDestination`; Plan/14 §6.19; src:_Common/Project.cpp:5718-5761).
//!
//! One entry per line, `KEY text`: the id is `atoi` of the last four characters of `KEY`
//! (`PATROL_DESTINATION_ID_0002` is 2) and the text is the rest of the line (`GetLastFull`); the first entry
//! of an id wins (`std::map::insert`). The quest file is empty in the reference data; the patrol file has
//! 9 entries.

#[cfg(feature = "load")]
pub(crate) use parse::parse_destinations;

#[cfg(feature = "load")]
mod parse {
    use flyff_script::{Diagnostics, Scanner, ScannerOptions, c_atoi};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// The last four bytes of `key` (the whole key when it is shorter), cut on a char boundary.
    fn right4(key: &str) -> &str {
        let mut start = key.len().saturating_sub(4);
        while start < key.len() && !key.is_char_boundary(start) {
            start += 1;
        }
        key.get(start..).unwrap_or("")
    }

    /// id to text, first definition wins.
    pub(crate) fn parse_destinations(
        text: &str,
        file: &Arc<str>,
        diags: &mut Diagnostics,
    ) -> BTreeMap<u32, String> {
        let mut out = BTreeMap::new();
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        sc.get_token(false);
        while !sc.finished() {
            let id = u32::try_from(c_atoi(right4(sc.token()).as_bytes())).unwrap_or(0);
            let value = sc.get_last_full().to_owned();
            out.entry(id).or_insert(value);
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
        out
    }
}
