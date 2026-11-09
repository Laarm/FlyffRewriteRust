//! Markup of chat lines (Plan/23 §6.8; WP-GM-06): `ParsingEffect` unless the speaker wears the `FONTEDIT` scroll,
//! then `RemoveCRLF`. The pure functions are `flyff_rules::chat`.

use crate::buff::{BUFF_ITEM, BUFF_ITEM2};
use crate::mover::Mover;
use flyff_common::defs::item::II_SYS_SYS_SCR_FONTEDIT;
pub use flyff_rules::chat::{parsing_effect, remove_crlf};

/// `HasBuff(BUFF_ITEM, id)`: the item buff list query of doc 17 (WP-SKILL-01) over the stand-in `BuffSet`;
/// timed premium items (`BUFF_ITEM2`) count too.
pub fn has_item_buff(m: &Mover, item_id: u32) -> bool {
    m.buffs.has(BUFF_ITEM, item_id) || m.buffs.has(BUFF_ITEM2, item_id)
}

/// `markup_text` (23 §6.0): `parsing_effect` unless the speaker has `II_SYS_SYS_SCR_FONTEDIT`, then
/// `remove_crlf`. Used by the say, shout, whisper, party, guild and vendor-room paths.
pub fn markup_text(speaker: &Mover, s: &str) -> String {
    let stripped;
    let text = if has_item_buff(speaker, II_SYS_SYS_SCR_FONTEDIT) {
        s
    } else {
        stripped = parsing_effect(s);
        &stripped
    };
    remove_crlf(text)
}
