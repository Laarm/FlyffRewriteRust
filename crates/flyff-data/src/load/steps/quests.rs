//! Glue of the quest loaders: `propQuest*.inc` (L21, one call per `propQuest` line of `Masquerade.prj`),
//! `propGuildQuest.inc` (L37) and `propPartyQuest.inc` (L38); Plan/14 §6.18, WP-DATA-15.
//!
//! The grammar parsers are `crate::quest`'s; this file reads the files, supplies the NPC and mover lookups
//! (`GetCharacter`, `GetMoverProp`) and keeps the C18 counts. [`quest_tables_from`] runs the same parsers without
//! the pipeline for tools and for the public-CI proof of a quest set.

use crate::load::ctx::{LoadCtx, Need};
use crate::quest::{
    QuestEnv, QuestTables, parse_guild_quest_file, parse_party_quest_file, parse_quest_file,
};
use flyff_script::{DiagCode, Diagnostics, ScriptCtx, ScriptVariant, Severity, load_pre_files};
use std::sync::Arc;

/// The file L37 reads.
pub const GUILD_QUEST_FILE: &str = "propGuildQuest.inc";
/// The file L38 reads.
pub const PARTY_QUEST_FILE: &str = "propPartyQuest.inc";

fn update_counts(cx: &mut LoadCtx<'_>) {
    cx.gd.meta.counts.quests = cx.gd.quests.counts();
}

/// L21: `CProject::LoadPropQuest(path)` for one `propQuest` line of the project file. A missing file is a
/// Warning (the original returned FALSE and went on). The goal counter and the per-mover quest-item counts
/// run on across the calls.
pub fn load_prop_quest(cx: &mut LoadCtx<'_>, path: &str) {
    let Some(text) = cx.read_text(path, Need::Warn) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    {
        let gd = &mut cx.gd;
        let npcs = &gd.npcs;
        let movers = &gd.movers;
        let npc_exists = |key: &str| npcs.get(key).is_some();
        let mover_exists = |id: u32| movers.contains(id);
        let env = QuestEnv {
            npc_exists: &npc_exists,
            mover_exists: Some(&mover_exists),
        };
        parse_quest_file(&text, &file, &sctx, &mut gd.quests, &env, &mut cx.diags);
    }
    update_counts(cx);
}

/// L37: `propGuildQuest.inc` (absent by design in some trees: Info).
pub fn load_guild_quests(cx: &mut LoadCtx<'_>) {
    let Some(text) = cx.read_text(GUILD_QUEST_FILE, Need::Info) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(GUILD_QUEST_FILE);
    parse_guild_quest_file(
        &text,
        &file,
        &sctx,
        &mut cx.gd.quests.guild_quests,
        &mut cx.diags,
    );
    update_counts(cx);
}

/// L38: `propPartyQuest.inc` (absent by design in some trees: Info).
pub fn load_party_quests(cx: &mut LoadCtx<'_>) {
    let Some(text) = cx.read_text(PARTY_QUEST_FILE, Need::Info) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(PARTY_QUEST_FILE);
    parse_party_quest_file(
        &text,
        &file,
        &sctx,
        &mut cx.gd.quests.party_quests,
        &mut cx.diags,
    );
    update_counts(cx);
}

/// The quest tables of a set of files without the pipeline: the P1 pre-pass (`load_pre_files`: the define
/// headers and the string tables under their fixed names, so a fixture needs `defineQuest.h` and the five
/// `propQuest*.txt.txt` files), then `prop_files` in the given order with one goal counter (the `propQuest`
/// lines of the project file), then `propGuildQuest.inc` and `propPartyQuest.inc` (absent = Info). `read` maps a
/// file name to its decoded text (case-insensitive, `None` when absent); `npc_exists` stands for
/// `GetCharacter(key) != NULL` and must be case-insensitive. The parsers are the pipeline's, so the tables equal
/// those of `load_all` over the same files; the one difference is that the `QuestItem` mover check (a Warning)
/// needs the mover table and is left out.
pub fn quest_tables_from(
    mut read: impl FnMut(&str) -> Option<String>,
    prop_files: &[&str],
    npc_exists: impl Fn(&str) -> bool,
) -> (QuestTables, Diagnostics) {
    let pre = load_pre_files(&mut read);
    let mut diags = pre.diags;
    let sctx = ScriptCtx::new(
        Arc::new(pre.defines),
        Arc::new(pre.strings),
        ScriptVariant::Server,
    );
    let env = QuestEnv {
        npc_exists: &npc_exists,
        mover_exists: None,
    };
    let mut tables = QuestTables::default();
    let missing = |diags: &mut Diagnostics, name: &str, severity: Severity| {
        diags.report(
            severity,
            DiagCode::FileMissingOptional,
            None,
            0,
            format!("{name}: file is missing"),
        );
    };
    for name in prop_files {
        match read(name) {
            Some(text) => {
                let file: Arc<str> = Arc::from(*name);
                parse_quest_file(&text, &file, &sctx, &mut tables, &env, &mut diags);
            }
            None => missing(&mut diags, name, Severity::Warning),
        }
    }
    match read(GUILD_QUEST_FILE) {
        Some(text) => {
            let file: Arc<str> = Arc::from(GUILD_QUEST_FILE);
            parse_guild_quest_file(&text, &file, &sctx, &mut tables.guild_quests, &mut diags);
        }
        None => missing(&mut diags, GUILD_QUEST_FILE, Severity::Info),
    }
    match read(PARTY_QUEST_FILE) {
        Some(text) => {
            let file: Arc<str> = Arc::from(PARTY_QUEST_FILE);
            parse_party_quest_file(&text, &file, &sctx, &mut tables.party_quests, &mut diags);
        }
        None => missing(&mut diags, PARTY_QUEST_FILE, Severity::Info),
    }
    (tables, diags)
}
