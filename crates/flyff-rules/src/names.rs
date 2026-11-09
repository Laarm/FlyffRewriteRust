//! Name rules (Plan/12 §9.3, WP-AUTH-01; VR-070, KQ-23): the character-name rule as a pure function shared
//! by the lobby (authoritative) and the browser (the create-character pre-check, doc 45).
//!
//! The legacy client ran `IsValidPlayerName` + `IsInvalidName` + `IsAllowedLetter`
//! (`src:_Interface/WndTitle.cpp:2796-2947`, ref/24 §10) and the DatabaseServer only re-checked the last two;
//! here the server applies the same rules in the same order with the same texts (**FIX** PL-033: the
//! legacy server did not check length or leading digits).
//!
//! Callers (23 §7, WP-GM-02; [`NameKind`] picks the rule set):
//!
//! | kind | callers |
//! |---|---|
//! | [`NameKind::Character`] | doc 12 character creation (the lobby calls [`validate_character_name`] itself), doc 21 rename scroll, GM `/SetPlayerName` (row 120) |
//! | [`NameKind::Guild`] | doc 21 guild creation and rename, GM `/createguild` and `/SetGuildName` (rows 67, 121) |
//! | [`NameKind::GuildNickname`] | doc 21 guild nicknames |
//! | [`NameKind::PartyName`] | doc 21 advanced-party names |
//! | [`NameKind::VendorTitle`] | doc 19 private vendor titles (with a [`TitleFilter`]) |
//! | [`NameKind::PetName`] | doc 19 pet naming scroll, GM `/SetPetName` (row 104) |
//!
//! Callers: character creation (doc 12). Doc 23 extends this module with the guild, nickname, party, pet and
//! vendor-title kinds (WP-GM-02) and reuses [`NameError`].
//!
//! The rules come from `flyff_data::names::NameRules` (`InvalidName_ENG.inc` as lower-cased substrings,
//! `Letter_ENG.inc` as the allowed first bytes), loaded by the server and exported in the `names` bundle.

use flyff_common::defs::text::{
    TID_DIAG_0011, TID_DIAG_0011_01, TID_DIAG_0012, TID_DIAG_0013, TID_DIAG_0020, TID_DIAG_0031,
    TID_DIAG_RULE_0, TID_GAME_NAME_PET_E01,
};
pub use flyff_data::names::NameRules;

/// Why a character name is refused (12 §9.3 rules 1-6; rule 7, uniqueness, is the game thread's).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameError {
    /// Rule 1: empty after trimming (`TID_DIAG_0031`, "Enter name.").
    Empty,
    /// Rule 2: byte length outside `min..=max` (`TID_DIAG_RULE_0`, "Character Name must be 4 to 16 characters
    /// long.").
    Length,
    /// Rule 3: starts with `'0'..='9'` (`TID_DIAG_0012`).
    LeadingDigit,
    /// Rule 4: a byte that is not ASCII alphanumeric: spaces, symbols, control and non-ASCII bytes
    /// (`TID_DIAG_0013`).
    InvalidChar,
    /// Rules 5 and 6: contains an `InvalidName` entry as a (case-insensitive) substring, or a byte outside
    /// `Letter_ENG.inc` (`TID_DIAG_0020`, "Character name is not allowed.").
    Forbidden,
}

impl NameError {
    /// The client text id (`TID_*` value of `defineText.h`) that explains the refusal: 2030, 3341, 2011, 2012,
    /// 2019.
    pub const fn tid(self) -> i32 {
        let tid = match self {
            NameError::Empty => TID_DIAG_0031,
            NameError::Length => TID_DIAG_RULE_0,
            NameError::LeadingDigit => TID_DIAG_0012,
            NameError::InvalidChar => TID_DIAG_0013,
            NameError::Forbidden => TID_DIAG_0020,
        };
        tid as i32
    }
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NameError::Empty => "Enter name.",
            NameError::Length => "Character Name must be 4 to 16 characters long.",
            NameError::LeadingDigit => "The name cannot start with a number.",
            NameError::InvalidChar => "You cannot use special characters or spaces in a name.",
            NameError::Forbidden => "Character name is not allowed.",
        })
    }
}

impl std::error::Error for NameError {}

/// Trims ASCII whitespace (legacy `Trim`).
fn trim_ascii(raw: &str) -> &str {
    raw.trim_matches(|c: char| c.is_ascii_whitespace())
}

/// The character-name rule (12 §9.3, rules 1-6), in the legacy order. Returns the trimmed name.
///
/// * `min` / `max`: `gameplay.min_name_letters` (4) and `gameplay.max_name_letters` (16), compared with the
///   **byte** length of the trimmed name (the legacy `strlen`);
/// * `rules`: the loaded [`NameRules`] (an empty value imposes nothing).
///
/// No allocation: the result borrows from `raw`.
pub fn validate_character_name<'a>(
    raw: &'a str,
    rules: &NameRules,
    min: u32,
    max: u32,
) -> Result<&'a str, NameError> {
    let name = trim_ascii(raw);
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    let len = u32::try_from(name.len()).unwrap_or(u32::MAX);
    if len < min || len > max {
        return Err(NameError::Length);
    }
    if name.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return Err(NameError::LeadingDigit);
    }
    if !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(NameError::InvalidChar);
    }
    if rules.is_invalid_name(name) || !rules.is_allowed_letter(name, false) {
        return Err(NameError::Forbidden);
    }
    Ok(name)
}

// ---- the other kinds (23 §7, WP-GM-02) ------------------------------------------------------------------------

/// What a name is for (23 §7). The rule set, the error texts and the length limits depend on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameKind {
    /// A character name (12 §9.3): trimmed, `limits.min..=limits.max` bytes, no leading digit, ASCII
    /// alphanumeric, `InvalidName_ENG.inc` and `Letter_ENG.inc`.
    Character,
    /// A guild name: trimmed, 3..=16 characters (UD-21: the `guilds.name` column and 21 §5.4 say 16; Plan/23 §7
    /// still says 32), the character rules.
    Guild,
    /// A guild member's nickname: trimmed, 2..=12 characters (`__RULE_0615`), the character rules.
    GuildNickname,
    /// An advanced party's name: trimmed, 4..=16 characters, the character rules.
    PartyName,
    /// A private vendor's title: trimmed, any printable text, `InvalidName_ENG.inc` and `Letter1_ENG.inc`
    /// checked on the typed text, then the chat filter, then 6..=32 characters of the **filtered** text.
    VendorTitle,
    /// A pet's name: **not** trimmed, 1..=16 characters (`MAX_PET_NAME` is 17 with the terminator), any printable
    /// text, `InvalidName_ENG.inc` and `Letter_ENG.inc`.
    PetName,
}

/// The vendor-title profanity filter (`word_change`, 23 §6.9) as seen by this wasm-safe crate: `flyff-game`'s
/// `ChatFilter` implements it, the browser passes `None` (its pre-check skips the filter; the server is
/// authoritative).
pub trait TitleFilter {
    /// The filtered text.
    fn filter(&self, s: &str) -> String;
}

/// The two config limits of the character rule (`gameplay.min_name_letters`, `gameplay.max_name_letters`;
/// 4 and 16 in the repack). The other kinds have fixed limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameLimits {
    /// `gameplay.min_name_letters`.
    pub min_name_letters: u32,
    /// `gameplay.max_name_letters`.
    pub max_name_letters: u32,
}

impl Default for NameLimits {
    fn default() -> Self {
        Self {
            min_name_letters: 4,
            max_name_letters: 16,
        }
    }
}

/// `(min, max)` length in characters of a kind other than the character (whose limits are the config's).
const fn fixed_limits(kind: NameKind) -> (usize, usize) {
    match kind {
        NameKind::Guild => (3, 16),
        NameKind::GuildNickname => (2, 12),
        NameKind::PartyName => (4, 16),
        NameKind::VendorTitle => (6, 32),
        NameKind::PetName => (1, 16),
        // unused: the character rule has its own entry point
        NameKind::Character => (4, 16),
    }
}

/// `validate_name` (23 §7): the normalised name or the first broken rule, in the legacy order of the kind. The
/// result is the trimmed name (the pet name is left as typed), and for a vendor title the filtered text.
///
/// * [`NameKind::Character`] is exactly [`validate_character_name`] (one rule set, one TID table);
/// * guild, nickname and party names: empty, length (characters), leading digit, ASCII alphanumeric, then
///   `InvalidName` substrings and the allowed letters ([`NameError::Forbidden`]);
/// * vendor titles: empty, control characters, `InvalidName` and `Letter1` on the typed text, the `filter`, the
///   length of the filtered text;
/// * pet names: the same checks without trimming and without the digit and class rules.
///
/// An empty `rules` imposes nothing; `filter = None` leaves a vendor title unfiltered.
pub fn validate_name(
    kind: NameKind,
    raw: &str,
    rules: &NameRules,
    limits: NameLimits,
    filter: Option<&dyn TitleFilter>,
) -> Result<String, NameError> {
    if kind == NameKind::Character {
        return validate_character_name(
            raw,
            rules,
            limits.min_name_letters,
            limits.max_name_letters,
        )
        .map(str::to_owned);
    }
    let (min, max) = fixed_limits(kind);
    let name = if kind == NameKind::PetName {
        raw
    } else {
        trim_ascii(raw)
    };
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    match kind {
        NameKind::Guild | NameKind::GuildNickname | NameKind::PartyName => {
            let len = name.chars().count();
            if len < min || len > max {
                return Err(NameError::Length);
            }
            if name.as_bytes().first().is_some_and(u8::is_ascii_digit) {
                return Err(NameError::LeadingDigit);
            }
            if !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Err(NameError::InvalidChar);
            }
            if rules.is_invalid_name(name) || !rules.is_allowed_letter(name, false) {
                return Err(NameError::Forbidden);
            }
            Ok(name.to_owned())
        }
        NameKind::VendorTitle => {
            if name.chars().any(char::is_control) {
                return Err(NameError::InvalidChar);
            }
            // the legacy checks the typed text, then filters, then measures
            if rules.is_invalid_name(name) || !rules.is_allowed_letter(name, true) {
                return Err(NameError::Forbidden);
            }
            let filtered = filter.map_or_else(|| name.to_owned(), |f| f.filter(name));
            let len = filtered.chars().count();
            if len < min || len > max {
                return Err(NameError::Length);
            }
            Ok(filtered)
        }
        NameKind::PetName => {
            let len = name.chars().count();
            if len < min || len > max {
                return Err(NameError::Length);
            }
            if name.chars().any(char::is_control) {
                return Err(NameError::InvalidChar);
            }
            if rules.is_invalid_name(name) || !rules.is_allowed_letter(name, false) {
                return Err(NameError::Forbidden);
            }
            Ok(name.to_owned())
        }
        NameKind::Character => Ok(name.to_owned()),
    }
}

/// The client text id that explains a refusal of `kind` (the single TID table of 23 §7). For
/// [`NameKind::Character`] it is [`NameError::tid`]. Where the plan has no entry for a pair (an empty guild name,
/// a leading digit in a vendor title) the pair maps to the closest text: an empty name is a length error, a
/// control character is "not allowed".
pub const fn name_error_tid(kind: NameKind, e: NameError) -> i32 {
    let tid = match kind {
        NameKind::Character => return e.tid(),
        NameKind::Guild => match e {
            NameError::Empty | NameError::Length => TID_DIAG_0011,
            NameError::LeadingDigit => TID_DIAG_0012,
            NameError::InvalidChar => TID_DIAG_0013,
            NameError::Forbidden => TID_DIAG_0020,
        },
        NameKind::GuildNickname => match e {
            NameError::Empty | NameError::Length => TID_DIAG_0011_01,
            NameError::LeadingDigit => TID_DIAG_0012,
            NameError::InvalidChar => TID_DIAG_0013,
            NameError::Forbidden => TID_DIAG_0020,
        },
        NameKind::PartyName => match e {
            NameError::Empty | NameError::Length => TID_DIAG_RULE_0,
            NameError::LeadingDigit => TID_DIAG_0012,
            NameError::InvalidChar => TID_DIAG_0013,
            NameError::Forbidden => TID_DIAG_0020,
        },
        NameKind::VendorTitle => match e {
            NameError::Empty | NameError::Length => TID_DIAG_0011,
            NameError::LeadingDigit | NameError::InvalidChar | NameError::Forbidden => {
                TID_DIAG_0020
            }
        },
        NameKind::PetName => TID_GAME_NAME_PET_E01,
    };
    tid as i32
}

/// The uniqueness key of a name (13 §5.3 `characters_name_live`, 21 `NameKey`): ASCII case-folded. Two names
/// that differ only in ASCII case collide ("Adam" and "ADAM", 12 §9.3 rule 7).
pub fn ascii_name_key(name: &str) -> String {
    name.to_ascii_lowercase()
}
