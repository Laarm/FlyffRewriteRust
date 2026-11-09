//! The bank PIN rule (Plan/12 §9.4, WP-AUTH-01; VR-083): exactly four ASCII digits and not `0000`.
//!
//! The legacy server accepted any integer 0..9999 including 0 (**FIX** VR-083); the client checked the format
//! (`TID_2ND_PASSWORD_INPUT_ERROR02`) and the all-zero PIN (`…03`). The confirmation field and the empty check
//! (`…01`, `…04`, `…05`) are client-only.

use flyff_common::defs::text::{TID_2ND_PASSWORD_INPUT_ERROR02, TID_2ND_PASSWORD_INPUT_ERROR03};

/// Why a PIN is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PinError {
    /// Not exactly four bytes, or a byte outside `'0'..='9'` (`TID_2ND_PASSWORD_INPUT_ERROR02`, "The Bank PIN
    /// should be a %d digit number.", argument [`PIN_DIGITS`]).
    Format,
    /// `"0000"` (`TID_2ND_PASSWORD_INPUT_ERROR03`, "Invalid Bank PIN.").
    AllZero,
}

/// The PIN length (the `%d` argument of the format text).
pub const PIN_DIGITS: usize = 4;

impl PinError {
    /// The client text id: 4296 for [`PinError::Format`], 4297 for [`PinError::AllZero`].
    pub const fn tid(self) -> i32 {
        let tid = match self {
            PinError::Format => TID_2ND_PASSWORD_INPUT_ERROR02,
            PinError::AllZero => TID_2ND_PASSWORD_INPUT_ERROR03,
        };
        tid as i32
    }
}

impl std::fmt::Display for PinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PinError::Format => write!(f, "The Bank PIN should be a {PIN_DIGITS} digit number."),
            PinError::AllZero => f.write_str("Invalid Bank PIN."),
        }
    }
}

impl std::error::Error for PinError {}

/// Validates a bank PIN and returns its four ASCII digits (the form `banks.pin` stores).
pub fn validate_bank_pin(pin: &str) -> Result<[u8; 4], PinError> {
    let digits: [u8; 4] = pin.as_bytes().try_into().map_err(|_| PinError::Format)?;
    if !digits.iter().all(u8::is_ascii_digit) {
        return Err(PinError::Format);
    }
    if digits == *b"0000" {
        return Err(PinError::AllZero);
    }
    Ok(digits)
}
