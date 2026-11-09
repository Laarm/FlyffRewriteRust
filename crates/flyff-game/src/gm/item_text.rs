//! Item-triggered GM commands (Plan/23 §5, F-167, PL-114, PD-11; WP-GM-19; src:_Common/MoverSkill.cpp:1612-1667,
//! _Common/buff.cpp:223-247).
//!
//! The 36 `IK2_GMTEXT` items of `propItem.txt` carry a command text in `text_file`: `"/<on-use> /<on-release>"`
//! (`"/ud /noud"`, `"/dis 20 /nodis"`, `"/nodis"`, `"/createguild"`). This module owns everything the rest of the
//! server needs to know about it:
//!
//! * [`split`]: the one parser of the text (parentheses become quotes, the second `/` splits, the use half is
//!   cut to the legacy `char[64]`); the item rows ([`crate::item::buffitems::ik2`]) and the buff release
//!   ([`crate::buff::core`]) call it, so there is no second parser;
//! * [`build`] / [`install`]: at game start every `IK2_GMTEXT` prop is parsed and each half is **validated**: its
//!   command must be a row of the table on the item whitelist (`item_ok`: `ud`, `ud2`, `noud`, `createguild`,
//!   `dis`, `nodis`). A half that fails is reported (`tracing::warn!`, [`Built::warnings`]) and has no effect
//!   ([`GmItemCommands::permits`]);
//! * [`use_plan`]: what using an item adds to the buff of its row (run the text, or refuse: the guild-creation
//!   scroll carries no name, 23 §13 Q5);
//! * [`run`]: the executor. The text goes through the command registry with the issuer `Issuer::Item`: the
//!   whitelist bypasses the authority gate when `fidelity.item_gm_commands` is on (PL-114, the repack default);
//!   with the switch off the player's own authority decides - no effect for a player, the same as typing the
//!   command for a GM (the original, KEEP). The item is consumed in every case.
//!
//! The buff engine queues [`crate::follow::Followup::ItemCommand`] on use and on release (expiry, `/rb`, removal
//! by another item, death rules); [`crate::buff::api::run_item_command`] is [`run`].

use super::{CmdArgs, CommandSpec, table};
use flyff_common::defs::itemkind::{IK2_GMTEXT, IK2_PAPERING, IK3_ANGEL_BUFF};
use flyff_common::det::DetMap;
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;

/// `char szGMCommand[64]`: the most bytes of the command that runs on use.
pub const USE_COMMAND_MAX: usize = 63;

/// The two halves of an item's command text (23 §5.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GmItemText {
    /// The command run when the item is used (the text before the second `/`, the whole text without one).
    pub on_use: Option<String>,
    /// The command run when the item's buff is released (the text from the second `/` on).
    pub on_release: Option<String>,
}

/// Splits a `text_file` (23 §5.1): `(` and `)` become `"`; the split point is the first `/` at index 1 or later
/// (`CString::Find( "/", 1 )`); both halves are trimmed, an empty half is `None`; the use half is cut to
/// [`USE_COMMAND_MAX`] bytes (`szGMCommand`).
pub fn split(text_file: &str) -> GmItemText {
    if text_file.is_empty() {
        return GmItemText::default();
    }
    let s = text_file.replace(['(', ')'], "\"");
    let at = s.get(1..).and_then(|t| t.find('/')).map(|i| i + 1);
    let (head, tail) = match at {
        Some(p) => (s.get(..p).unwrap_or(""), s.get(p..)),
        None => (s.as_str(), None),
    };
    let mut head = head.trim().to_owned();
    if head.len() > USE_COMMAND_MAX {
        let mut n = USE_COMMAND_MAX;
        while !head.is_char_boundary(n) {
            n -= 1;
        }
        head.truncate(n);
    }
    let non_empty = |t: &str| (!t.is_empty()).then(|| t.to_owned());
    GmItemText {
        on_use: non_empty(&head),
        on_release: tail.map(str::trim).and_then(non_empty),
    }
}

/// The command that runs when the item's buff is released (`IBuffItemBase::Release`, 23 §5.3): the second half of
/// the text. None for a text without one and for the item kinds the release skips (`IK3_ANGEL_BUFF`,
/// `IK2_PAPERING`).
pub fn release_text(prop: &ItemProp) -> Option<String> {
    if prop.text_file.is_empty()
        || prop.item_kind3 == IK3_ANGEL_BUFF
        || prop.item_kind2 == IK2_PAPERING
    {
        return None;
    }
    split(&prop.text_file).on_release
}

/// The row of a command token (a name or an alias, ASCII case-insensitive; the table's keys are unique, 23 §3.3).
pub fn spec_of(token: &str) -> Option<&'static CommandSpec> {
    if token.is_empty() {
        return None;
    }
    table::COMMANDS.iter().find(|s| {
        s.name.eq_ignore_ascii_case(token)
            || s.aliases
                .iter()
                .any(|a| !a.is_empty() && a.eq_ignore_ascii_case(token))
    })
}

/// Why a half cannot run, or the row it runs.
fn check_half(half: &str) -> Result<&'static CommandSpec, String> {
    let mut args = CmdArgs::new(half);
    if args.token() != "/" {
        return Err("does not start with '/'".to_owned());
    }
    let token = args.token().to_owned();
    let Some(spec) = spec_of(&token) else {
        return Err(format!("'{token}' is not a command"));
    };
    if !spec.item_ok {
        return Err(format!("'{}' is not on the item whitelist", spec.name));
    }
    Ok(spec)
}

/// The validated command texts of the `IK2_GMTEXT` items (23 §5.1).
#[derive(Clone, Debug, Default)]
pub struct GmItemCommands {
    items: DetMap<u32, GmItemText>,
    built: bool,
}

impl GmItemCommands {
    /// The validated halves of an item, `None` for an item without a valid command.
    pub fn get(&self, item_id: u32) -> Option<&GmItemText> {
        self.items.get(&item_id)
    }

    /// The validated use command of an item.
    pub fn on_use(&self, item_id: u32) -> Option<&str> {
        self.get(item_id)?.on_use.as_deref()
    }

    /// The validated release command of an item.
    pub fn on_release(&self, item_id: u32) -> Option<&str> {
        self.get(item_id)?.on_release.as_deref()
    }

    /// How many items have at least one valid half.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `true` when no item has a valid half.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// `true` once [`build`] filled the table (a game that has not built it - unit tests of other modules - lets
    /// the registry's authority gate decide alone).
    pub fn is_built(&self) -> bool {
        self.built
    }

    /// May `text` run for `item_id`? A built table lets exactly the validated halves through; an unbuilt one
    /// everything (the command gate of the registry still applies).
    pub fn permits(&self, item_id: u32, text: &str) -> bool {
        if !self.built {
            return true;
        }
        let text = text.trim();
        self.get(item_id).is_some_and(|t| {
            t.on_use.as_deref() == Some(text) || t.on_release.as_deref() == Some(text)
        })
    }
}

/// The result of [`build`]: the table and the validation warnings.
#[derive(Clone, Debug, Default)]
pub struct Built {
    /// The validated commands.
    pub commands: GmItemCommands,
    /// How many `IK2_GMTEXT` items were read.
    pub items: usize,
    /// One line per half that failed validation (the half has no effect).
    pub warnings: Vec<String>,
}

/// Parses and validates the command text of every `IK2_GMTEXT` item (23 §5.1). An item with a text but no
/// valid half is left out; a half that fails is a warning.
pub fn build(data: &GameData) -> Built {
    let mut out = Built::default();
    out.commands.built = true;
    for (id, prop) in data.items.iter() {
        if prop.item_kind2 != IK2_GMTEXT {
            continue;
        }
        out.items += 1;
        let text = split(&prop.text_file);
        let mut valid = GmItemText::default();
        for (half, slot) in [
            (text.on_use, &mut valid.on_use),
            (text.on_release, &mut valid.on_release),
        ] {
            let Some(half) = half else { continue };
            match check_half(&half) {
                Ok(_) => *slot = Some(half),
                Err(why) => out.warnings.push(format!(
                    "item {id} ({}): the command {half:?} {why}; it has no effect",
                    prop.name
                )),
            }
        }
        if valid != GmItemText::default() {
            out.commands.items.insert(id, valid);
        }
    }
    out
}

/// Builds the table for `data` into the GM service and logs the report (the init report of 23 §5.1).
pub fn install(gm: &mut super::GmService, data: &GameData) {
    let built = build(data);
    for w in &built.warnings {
        tracing::warn!("GM item commands: {w}");
    }
    tracing::info!(
        items = built.items,
        valid = built.commands.len(),
        warnings = built.warnings.len(),
        "GM item commands built"
    );
    gm.item_text = built.commands;
}

pub use super::cmd::triggers::{UsePlan, run, use_plan};
