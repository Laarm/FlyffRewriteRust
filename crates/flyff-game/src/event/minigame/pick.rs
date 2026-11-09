//! The weighted prize sampler of the mini-games (`GetReassembleRandomItem`, `GetAlphabetRandomItem`; Plan/22 §16.5,
//! PL-105; src:_Common/MiniGame.cpp:262-285, 342-360).

use flyff_data::events::minigame::MiniGameItem;

/// The prize for the roll `r` (`xRandom( 1 000 000 )`): the entries' `per` (parts per million) are accumulated in
/// list order and the first entry with `r < cumulative` wins. A list whose probabilities sum to `r` or less returns
/// `None` and logs a data warning (FIX PL-105: the original ran past the end of the vector). Public for the rainbow
/// race (WP-EVENT-18), which follows the same rule.
pub fn weighted_pick(list: &[MiniGameItem], r: u32) -> Option<&MiniGameItem> {
    let mut sum: u64 = 0;
    for e in list {
        sum += u64::from(e.per);
        if u64::from(r) < sum {
            return Some(e);
        }
    }
    tracing::warn!(
        roll = r,
        total = sum,
        entries = list.len(),
        "mini-game prize list: the probabilities do not cover the roll; no prize"
    );
    None
}
