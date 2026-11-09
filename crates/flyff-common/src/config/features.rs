//! `[features]` (Plan/03 §4.2–§4.4): the `ServerCommon.h` toggles, 110 keys. The struct is generated in
//! [`tables`](super::tables).

pub use super::tables::{ClientFeatures, Features};

/// `ServerCommon.h` macros (lines 4–124) that have no key, with the reason (Plan/03 §4.2–§4.4). The
/// completeness test checks every macro of the header against the registry and this list.
pub const NO_KEY_MACROS: &[(&str, &str)] = &[
    ("__FAST_NEUZ", "N/A: the browser streams lazily anyway"),
    (
        "__SAVE_PASSWORD",
        "DROP: browser password managers instead (PL-129)",
    ),
    ("__FIX_SETFST", "N/A: superseded by the 60 Hz loop (PL-001)"),
    ("__LOGIN_TEXT_FIX", "UI: rewritten login UI"),
    (
        "__LONG_BUTTON_PRESS_FREEZE_FIX",
        "UI: rewritten input layer",
    ),
    ("__LORDNAME_FIX", "UI: lord name shown correctly"),
    ("__PATCHER_FOLDER_FIX", "N/A: patcher dropped"),
    ("__TAX_FIX", "UI: tax UI shown correctly"),
    (
        "__WND_BUTTONS",
        "N/A: network kept alive when inactive (PL-014)",
    ),
];
