//! L90 (P6): the Lua configuration bridge (Plan/14 §5.3, §6.17; WP-DATA-14).
//!
//! * The **static** scripts (`PartyDungeon`, `EventMonster`, `MonsterSkill`, `CreateMonster`, `QuizEvent`,
//!   `Campus`, `ItemUpgrade`) run once each in a fresh sandbox VM ([`crate::lua_config::vm`]); their
//!   documented globals become typed structs ([`crate::lua_config::extract`]).
//! * The **live** scripts (`Event`, `Tax`, `SecretRoom`, `RainbowRace`, `GuildCombat1TO1`) are only
//!   compiled (syntax check) together with the helper files they `dofile`, and registered with their source
//!   text in `GameData.lua.live_scripts` for the game thread's Lua host.
//!
//! A syntax error (`LuaSyntax`), a runtime error (`LuaError`) or an exhausted sandbox (`LuaLimit`) is an Error
//! diagnostic, never an exit (the original did `exit(0)`): the static config stays `None`/empty and the live
//! script is not registered, so the subsystem runs with its documented defaults. A missing file is Info
//! (`Event.lua`: Warning, "no events"). Every file that was read is registered as a source of the data
//! fingerprint, `dofile`d helpers included.

use crate::load::ctx::{LoadCtx, Need};
use crate::lua_config::extract::{self, Resolver};
use crate::lua_config::vm::{self, Executed, VmError};
use crate::lua_config::{LIVE_SCRIPTS, LuaConfigs, LuaSource};
use flyff_script::{DiagCode, Severity};
use flyff_vfs::lua_path_key;
use std::sync::Arc;

type Project = fn(&Executed, &mut Resolver<'_>, &mut LuaConfigs) -> Result<(), String>;

/// One static script: its file and how its globals become typed data.
struct StaticScript {
    file: &'static str,
    project: Project,
}

fn project_party_dungeon(
    ex: &Executed,
    r: &mut Resolver<'_>,
    out: &mut LuaConfigs,
) -> Result<(), String> {
    out.party_dungeon = Some(extract::party_dungeon(&ex.lua, &ex.globals(), r)?);
    Ok(())
}

fn project_event_monsters(
    ex: &Executed,
    r: &mut Resolver<'_>,
    out: &mut LuaConfigs,
) -> Result<(), String> {
    out.event_monsters = extract::event_monsters(&ex.lua, &ex.globals(), r)?;
    Ok(())
}

fn project_monster_skills(
    ex: &Executed,
    r: &mut Resolver<'_>,
    out: &mut LuaConfigs,
) -> Result<(), String> {
    out.monster_skills = extract::monster_skills(&ex.lua, &ex.globals(), r)?;
    Ok(())
}

fn project_create_monster(
    ex: &Executed,
    r: &mut Resolver<'_>,
    out: &mut LuaConfigs,
) -> Result<(), String> {
    out.create_monster = Some(extract::create_monster(&ex.lua, &ex.globals(), r)?);
    Ok(())
}

fn project_quiz(ex: &Executed, r: &mut Resolver<'_>, out: &mut LuaConfigs) -> Result<(), String> {
    out.quiz = Some(extract::quiz(&ex.lua, &ex.globals(), r)?);
    Ok(())
}

fn project_campus(ex: &Executed, r: &mut Resolver<'_>, out: &mut LuaConfigs) -> Result<(), String> {
    out.campus = Some(extract::campus(&ex.lua, &ex.globals(), r)?);
    Ok(())
}

fn project_item_upgrade(
    ex: &Executed,
    _r: &mut Resolver<'_>,
    out: &mut LuaConfigs,
) -> Result<(), String> {
    out.item_upgrade = Some(extract::item_upgrade(&ex.lua, &ex.globals())?);
    Ok(())
}

/// The static scripts in the load order of Plan/14 §5.3 (L47, L58, L71).
const STATIC: [StaticScript; 7] = [
    StaticScript {
        file: "PartyDungeon.lua",
        project: project_party_dungeon,
    },
    StaticScript {
        file: "EventMonster.lua",
        project: project_event_monsters,
    },
    StaticScript {
        file: "MonsterSkill.lua",
        project: project_monster_skills,
    },
    StaticScript {
        file: "CreateMonster.lua",
        project: project_create_monster,
    },
    StaticScript {
        file: "QuizEvent.lua",
        project: project_quiz,
    },
    StaticScript {
        file: "Campus.lua",
        project: project_campus,
    },
    StaticScript {
        file: "ItemUpgrade.lua",
        project: project_item_upgrade,
    },
];

fn report_vm_error(cx: &mut LoadCtx<'_>, file: &str, e: &VmError) {
    let (code, line) = match e {
        VmError::Syntax { line, .. } => (DiagCode::LuaSyntax, *line),
        VmError::Runtime { line, .. } => (DiagCode::LuaError, *line),
        VmError::Limit(_) => (DiagCode::LuaLimit, 0),
    };
    cx.report(
        Severity::Error,
        code,
        Some(file),
        line,
        format!("{file}: {e}"),
    );
}

/// L90: runs the static scripts and registers the live ones.
pub fn load_lua_configs(cx: &mut LoadCtx<'_>) {
    let defines = Arc::clone(&cx.gd.defines);
    let mut out = LuaConfigs::default();
    let mut statics = 0u32;
    for s in &STATIC {
        let Some(text) = cx.read_text(s.file, Need::Info) else {
            continue;
        };
        let key = cx.file_arc(s.file);
        match vm::run_config_script(cx.vfs, &key, &text) {
            Ok(ex) => {
                for dep in &ex.deps {
                    let _ = cx.read_text(dep.as_str(), Need::Silent); // source tracking, decoding notes
                }
                let mut r = Resolver::new(&defines, &mut cx.diags, &key);
                match (s.project)(&ex, &mut r, &mut out) {
                    Ok(()) => statics += 1,
                    Err(msg) => cx.report(
                        Severity::Error,
                        DiagCode::LuaError,
                        Some(&*key),
                        0,
                        format!("{key}: unexpected configuration table: {msg}"),
                    ),
                }
            }
            Err(e) => report_vm_error(cx, &key, &e),
        }
    }
    let mut live = 0u32;
    for (name, file) in LIVE_SCRIPTS {
        // a missing Event.lua matters (no events, no rates); the other live scripts are optional features
        let need = if name == "event" {
            Need::Warn
        } else {
            Need::Info
        };
        let Some(text) = cx.read_text(file, need) else {
            continue;
        };
        let key = cx.file_arc(file);
        if let Some(src) = register_live(cx, &key, text) {
            out.live_scripts.insert(name.to_owned(), src);
            live += 1;
        }
    }
    let c = &mut cx.gd.meta.counts;
    c.lua_static_configs = statics;
    c.lua_live_scripts = live;
    cx.gd.lua = out;
}

/// Syntax-checks a live script and its `dofile` targets; `None` (plus an Error) when one is broken or missing.
fn register_live(cx: &mut LoadCtx<'_>, key: &str, text: String) -> Option<LuaSource> {
    if let Err(e) = vm::check_syntax(key, &text) {
        report_vm_error(cx, key, &e);
        return None;
    }
    let mut deps = Vec::new();
    for arg in vm::scan_dofiles(&text) {
        let Ok(dep) = lua_path_key(&arg) else {
            cx.report(
                Severity::Error,
                DiagCode::RefUnresolved,
                Some(key),
                0,
                format!("{key}: dofile({arg:?}) is not a valid resource path"),
            );
            return None;
        };
        let Some(dep_text) = cx.read_text(dep.as_str(), Need::Silent) else {
            cx.report(
                Severity::Error,
                DiagCode::RefUnresolved,
                Some(key),
                0,
                format!("{key}: dofile target {} does not exist", dep.as_str()),
            );
            return None;
        };
        if let Err(e) = vm::check_syntax(dep.as_str(), &dep_text) {
            report_vm_error(cx, dep.as_str(), &e);
            return None;
        }
        deps.push(dep.as_str().to_owned());
    }
    Some(LuaSource {
        path: key.to_owned(),
        text,
        deps,
    })
}
