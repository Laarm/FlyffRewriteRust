//! Lobby rule functions for the browser (Plan/45 §2.7 `LobbyRules`, WP-CFEAT-04; shared rules of Plan/12 §9).
//!
//! Three FREE `#[wasm_bindgen]` functions, usable before the bundles and `RulesContext` exist (the character-create
//! window of the title screens): thin wrappers over `flyff_rules::{names, pin, appearance}` (WP-AUTH-01), so the
//! browser pre-check and the server (`charsel::validate_create`) apply the very same code and texts.
//!
//! * [`validate_character_name`]`(raw, names, min, max) -> { name?, tid? }`: `names` is the JSON of the name rules,
//!   the shape of the `names` bundle (`{"invalid_substrings": [...], "allowed_letters": "ABC..." | null}`); the result
//!   has `name` (trimmed) on success and `tid` (the client text id of 12 §9.3) on failure;
//! * [`validate_bank_pin`]`(pin) -> u32`: 0 when the PIN is acceptable, else the text id of 12 §9.4;
//! * [`default_hair_color`]`(sex, hair_mesh) -> u32 | undefined`: the `0xAARRGGBB` colour of 12 §9.2.
//!
//! Nothing here panics: malformed rule JSON is a `JsError`, everything else is a value.

use flyff_rules::appearance::default_hair_color as rules_default_hair_color;
use flyff_rules::names::{NameRules, validate_character_name as rules_validate_character_name};
use flyff_rules::pin::validate_bank_pin as rules_validate_bank_pin;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

/// The input of the name rules: the `names` bundle's `invalid_substrings` and `allowed_letters`.
#[derive(Debug, Deserialize)]
struct NamesInput {
    /// Lower-cased banned substrings.
    #[serde(default)]
    invalid_substrings: Vec<String>,
    /// The allowed characters as a string (`None`/`null`/empty = no restriction), the bundle's form.
    #[serde(default)]
    allowed_letters: Option<String>,
}

impl NamesInput {
    fn into_rules(self) -> NameRules {
        let allowed: Option<BTreeSet<u8>> = self
            .allowed_letters
            .filter(|s| !s.is_empty())
            .map(|s| s.bytes().collect());
        NameRules {
            invalid_substrings: self.invalid_substrings,
            allowed_letters: allowed,
            vendor_allowed_letters: None,
        }
    }
}

/// The result of [`validate_character_name`]: `name` on success, `tid` on failure (absent keys are left out).
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct NameCheck {
    /// The trimmed name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The `TID_*` value that explains the refusal (2030, 3341, 2011, 2012, 2019).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tid: Option<i32>,
}

/// Parses the name rules JSON (`names` bundle shape).
fn parse_rules(names: &[u8]) -> Result<NameRules, String> {
    serde_json::from_slice::<NamesInput>(names)
        .map(NamesInput::into_rules)
        .map_err(|e| format!("lobby rules: invalid name rules: {e}"))
}

/// The core of [`validate_character_name`] (natively testable).
pub fn check_name(raw: &str, rules: &NameRules, min: u32, max: u32) -> NameCheck {
    match rules_validate_character_name(raw, rules, min, max) {
        Ok(name) => NameCheck {
            name: Some(name.to_owned()),
            tid: None,
        },
        Err(e) => NameCheck {
            name: None,
            tid: Some(e.tid()),
        },
    }
}

/// The core of [`validate_bank_pin`]: 0 = acceptable, else the text id.
pub fn pin_tid(pin: &str) -> u32 {
    match rules_validate_bank_pin(pin) {
        Ok(_) => 0,
        Err(e) => u32::try_from(e.tid()).unwrap_or(0),
    }
}

/// `validate_character_name(raw, names, min, max) -> { name?: string, tid?: number }` (12 §9.3 rules 1-6).
///
/// `names` is the JSON of the `names` bundle's name rules (`invalid_substrings`, `allowed_letters`).
#[wasm_bindgen]
pub fn validate_character_name(
    raw: &str,
    names: &[u8],
    min: u32,
    max: u32,
) -> Result<JsValue, JsError> {
    let rules = parse_rules(names).map_err(|e| JsError::new(&e))?;
    let check = check_name(raw, &rules, min, max);
    serde_wasm_bindgen::to_value(&check).map_err(|e| JsError::new(&e.to_string()))
}

/// `validate_bank_pin(pin) -> number`: 0 when the PIN is four digits and not `"0000"`, else the `TID_2ND_PASSWORD_*`
/// text id of 12 §9.4 (4296 format, 4297 all zero).
#[wasm_bindgen]
pub fn validate_bank_pin(pin: &str) -> u32 {
    pin_tid(pin)
}

/// `default_hair_color(sex, hair_mesh) -> number | undefined`: `0xFF000000 | r << 16 | g << 8 | b` of the table of 12
/// §9.2; `undefined` for a sex above 1 or a hair mesh above 9.
#[wasm_bindgen]
pub fn default_hair_color(sex: u32, hair_mesh: u32) -> Option<u32> {
    rules_default_hair_color(sex, hair_mesh)
}
