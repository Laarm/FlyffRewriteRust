//! The text panels of `Client/`: the login notice (`notice_ENG.inc`), the guild-combat rule texts, the
//! smelting, rainbow-race, secret-room, pet, lord and couple explanations and so on (Plan/14 §6.19,
//! ref/17 §10.2). They are plain text with the `#b #cAARRGGBB #nb #nc` markup of the UI text control and are
//! read as a whole by the windows that show them; this module only decides **which** files are panels.

/// The files of `Client/` that are **not** panels: the id tables, the help family and the files other
/// loaders read (lower-case stems).
pub const NON_PANEL_STEMS: [&str; 10] = [
    "sound",
    "help",
    "treehelp",
    "faq",
    "tip",
    "instanthelp",
    "tutorial",
    "guide",
    "patchclient",
    "gameguard",
];

/// The key of the login notice in `ClientData::panels` (lower-case stem of `notice_ENG.inc`).
pub const NOTICE_KEY: &str = "notice_eng";

/// The panel key of a file name in `Client/` (`SmeltJewel.inc` is `smeltjewel`), or `None` when the file is
/// not a panel (wrong extension, or one of [`NON_PANEL_STEMS`]).
pub fn panel_key(file_name: &str) -> Option<String> {
    let lower = file_name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".inc")?;
    if stem.is_empty() || stem.contains(['/', '\\']) || NON_PANEL_STEMS.contains(&stem) {
        return None;
    }
    Some(stem.to_owned())
}
