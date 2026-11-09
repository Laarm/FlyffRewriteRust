//! `propPackItem.inc` (`CProject::LoadPackItem`, Plan/14 §6.12, L41; ref/13 §10.5): the pack contents.
//!
//! The types and the parser live next to the gift boxes in [`super::giftbox`] (the two files share the
//! scanner idioms and one test module); this module is the plan's name for them.
//!
//! ```text
//! PackItem <pack item> <span minutes> '{' (ITEM ABILITY_OPTION NUM)* '}'      at most 16 entries
//! ```

pub use super::giftbox::{MAX_ITEM_PER_PACK, PackItem, PackTable};
