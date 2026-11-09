//! The command handlers (Plan/23 §4). One module per group of §4; every handler is
//! `fn(&mut CmdCtx, &mut CmdArgs) -> CmdResult` and is referenced from the generated `table.rs` (add it to the
//! `HANDLERS` dict of `tools/gen_gm_table.py` and regenerate).

pub mod admin;
pub mod char_;
pub mod chat;
pub mod content;
pub mod create;
pub mod event;
pub mod info;
pub mod items;
pub mod mode;
pub mod move_;
pub mod party;
pub mod pets;
pub mod progress;
pub mod quest;
pub mod reload;
pub mod siege;
pub mod social;
pub mod spawn;
pub mod triggers;
