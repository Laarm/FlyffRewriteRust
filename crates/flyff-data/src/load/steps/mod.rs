//! Loader glue (Plan/14 §5.1): each function reads its file through `LoadCtx`, runs the pure parser of its
//! module, stores the rows (applying the fix-ups of 14 §5.4 that belong to the step) and counts them.
//!
//! One file per group so that parallel work packages do not touch the same file: [`props`] (items,
//! skills, party skills, add-skills, movers, ctrls, motions, jobs: WP-DATA-05/06) and [`tables`]
//! (`expTable`, `jobItem`, `etc`, the rules tables: WP-DATA-10). The other groups (NPCs, worlds, item
//! extensions, events, social, Lua, client data) add their own files here.

pub mod client;
pub mod ext;
pub mod lua;
pub mod models;
pub mod npcs;
pub mod props;
pub mod quests;
pub mod tables;
pub mod worlds;

use crate::load::ctx::{LoadCtx, Need};
use crate::texts::parse_text_table;

/// L03: `textClient.inc` (required).
pub fn load_texts(cx: &mut LoadCtx<'_>, path: &str) {
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let parsed = parse_text_table(&text, &file, &sctx, &mut cx.diags);
    let c = &mut cx.gd.meta.counts;
    c.text_entries = parsed.entries;
    c.text_tids = u32::try_from(parsed.table.len()).unwrap_or(u32::MAX);
    cx.gd.texts = parsed.table;
}
