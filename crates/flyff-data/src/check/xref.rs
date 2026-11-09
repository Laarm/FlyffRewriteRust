//! The cross-reference checks of Plan/14 §9.3 (P8) and the whole `flyff check` data run (WP-DATA-17).
//!
//! [`cross_references`] evaluates the checks on a loaded [`GameData`] (and, for asset existence, the VFS).
//! Their severities are the plan's: a finding is a **Warning** (references between tables), an **Error** (a
//! missing shop item, reported by the loader) or **Info** (assets). A finding never stops the start; the
//! numbers are pinned as a baseline by the corpus suite (`tests/golden/data-xref.json`).
//!
//! | id | check | severity |
//! |---|---|---|
//! | X1 | every `propMoverEx` block belongs to an existing mover | Warning |
//! | X2a | NPC `figure.mover`, `equip` items, `buff_skills.skill`, vendor-rule `ik3 < 200`, chip and shop items exist | Warning |
//! | X3 | skill level chains: the add-skill rows `sub_define ..` exist and point back at the skill | Warning |
//! | X4a | drop items (incl. the merged drop events), gift-box and pack items, `jobItem` items | Warning |
//! | X4b | `MonsterBuffs.inc` movers and items, ticket items and worlds, teleport16 worlds and bosses | Warning |
//! | X4c | the movers and drop items of the world-map monster records | Warning |
//! | X5 | item and skill `sfx_elemental` / `sfx_obj` ids are `sfx` models | Info |
//! | X6a | model files: `.o3d` of every entry, `.chr` of movers, `.ani` of every motion | Info |
//! | X6b | NPC portraits in `Char/` | Info |
//! | X7 | unlisted non-empty `world/*/*.txt.txt` string tables (expected 0) | Warning |
//!
//! Check 8 of §9.3 (`.dyo` NPC keys and dialog keys) belongs to docs 15 and 20: they register their function with
//! [`ExtraCheck`] and [`run`] / [`build_report`] call it.

use super::report::{DataReport, XrefResult};
use crate::client_data::ClientData;
use crate::game_data::GameData;
use crate::load::{LoadError, LoadOutput, load_all};
use crate::options::DataOptions;
use crate::types::NULL_ID;
use flyff_common::defs::define::{OT_MOVER, OT_SFX};
use flyff_script::{Diagnostic, Severity};
use flyff_vfs::Vfs;
use std::collections::BTreeSet;

/// A check contributed by another document through the `extra_checks` hook (§9.3 item 8): it gets the loaded data
/// and the VFS and returns diagnostics, which the report lists.
pub type ExtraCheck = dyn Fn(&GameData, &Vfs) -> Vec<Diagnostic>;

fn exists_item(g: &GameData, id: u32) -> bool {
    g.items.contains(id)
}

/// `true` for a reference that points at something (`0` and `NULL_ID` mean "none").
fn some(id: u32) -> bool {
    id != 0 && id != NULL_ID
}

/// The §9.3 checks over a loaded data set. The VFS-dependent ones (X6) need `vfs`; X1 and X7 read the counts
/// the loaders recorded in `g.meta`.
pub fn cross_references(
    g: &GameData,
    client: Option<&ClientData>,
    vfs: Option<&Vfs>,
) -> Vec<XrefResult> {
    let mut out = vec![x1(g), x2(g), x3(g), x4a(g), x4b(g), x4c(g, client), x5(g)];
    if let Some(vfs) = vfs {
        out.extend(x6(g, vfs));
    }
    out.push(x7(g));
    out
}

fn x1(g: &GameData) -> XrefResult {
    let mut r = XrefResult::new(
        "X1",
        "propMoverEx blocks belong to an existing mover",
        Severity::Warning,
    );
    let n = g.meta.counts.mover_ex_blocks;
    r.checked = n;
    r.missing = g.meta.counts.mover_ex_unknown;
    r
}

fn x2(g: &GameData) -> XrefResult {
    let mut r = XrefResult::new(
        "X2a",
        "NPC references: figure mover, equip, buff skills, vendor rules, shop and chip items",
        Severity::Warning,
    );
    for n in g.npcs.iter() {
        if let Some(f) = &n.figure {
            r.check(g.movers.contains(f.mover), || {
                format!("{}: figure mover {}", n.key, f.mover)
            });
        }
        for it in n.equip.iter().copied().filter(|i| some(*i)) {
            r.check(exists_item(g, it), || format!("{}: equip item {it}", n.key));
        }
        for b in &n.buff_skills {
            r.check(g.skills.contains(b.skill), || {
                format!("{}: buff skill {}", n.key, b.skill)
            });
        }
        for t in &n.vendor.tabs {
            for rule in &t.rules {
                r.check((0..200).contains(&rule.ik3), || {
                    format!("{}: vendor rule item kind 3 {}", n.key, rule.ik3)
                });
            }
            for it in &t.chip_items {
                r.check(exists_item(g, *it), || format!("{}: chip item {it}", n.key));
            }
            for s in &t.shop_items {
                r.check(exists_item(g, s.item), || {
                    format!("{}: shop item {}", n.key, s.item)
                });
            }
        }
    }
    r
}

fn x3(g: &GameData) -> XrefResult {
    let mut r = XrefResult::new(
        "X3",
        "skill level chains: the add-skill rows exist and point back at the skill",
        Severity::Warning,
    );
    for (id, s) in g.skills.iter() {
        if !some(s.sub_define) {
            continue;
        }
        for lvl in 1..=s.expert_max.max(1) {
            match g.add_skill(s.sub_define, lvl) {
                Some(a) => r.check(a.skill_id == id, || {
                    format!(
                        "{} ({id}) level {lvl}: row {} belongs to skill {}",
                        s.name, a.id, a.skill_id
                    )
                }),
                None => r.check(false, || {
                    format!("{} ({id}) level {lvl}: no add-skill row", s.name)
                }),
            }
        }
    }
    r
}

fn x4a(g: &GameData) -> XrefResult {
    let mut r = XrefResult::new(
        "X4a",
        "drop, drop-event, gift-box, pack and starting items exist",
        Severity::Warning,
    );
    // the merged drop tables include the rows of propDropEvent.inc
    let mut seen = BTreeSet::new();
    for (id, m) in g.movers.iter() {
        for d in &m.ex.drops.items {
            if d.dtype == crate::mover_ex::DropType::Normal && seen.insert(d.item) {
                r.check(exists_item(g, d.item), || {
                    format!("drop item {} (first seen at mover {id})", d.item)
                });
            }
        }
    }
    for (b, bx) in &g.giftboxes.boxes {
        r.check(exists_item(g, *b), || format!("gift box item {b}"));
        for e in &bx.entries {
            r.check(exists_item(g, e.item), || {
                format!("gift box {b}: item {}", e.item)
            });
        }
    }
    for (pid, p) in &g.pack_items.packs {
        r.check(exists_item(g, *pid), || format!("pack item {pid}"));
        for (it, _, _) in &p.items {
            r.check(exists_item(g, *it), || format!("pack {pid}: item {it}"));
        }
    }
    for (job, ji) in &g.job_items.jobs {
        for it in ji.equip.iter().flatten().copied().filter(|i| some(*i)) {
            r.check(exists_item(g, it), || format!("job {job}: equip item {it}"));
        }
        for b in &ji.begin_items {
            r.check(exists_item(g, b.item), || {
                format!("job {job}: begin item {}", b.item)
            });
        }
    }
    r
}

fn x4b(g: &GameData) -> XrefResult {
    let mut r = XrefResult::new(
        "X4b",
        "monster buffs, tickets and teleport16 rows point at existing movers, items and worlds",
        Severity::Warning,
    );
    for (m, i) in &g.monster_buffs.pairs {
        r.check(g.movers.contains(*m), || format!("monster buff: mover {m}"));
        r.check(exists_item(g, *i), || format!("monster buff: item {i}"));
    }
    for (item, t) in &g.tickets.tickets {
        r.check(exists_item(g, *item), || format!("ticket item {item}"));
        r.check(g.worlds.worlds.contains_key(&t.world), || {
            format!("ticket {item}: world {}", t.world)
        });
    }
    for (world, _) in &g.tickets.layers {
        r.check(g.worlds.worlds.contains_key(world), || {
            format!("layer.inc: world {world}")
        });
    }
    for (i, t) in g.teleports16.iter().enumerate() {
        r.check(g.worlds.worlds.contains_key(&t.world), || {
            format!("teleport16 row {i}: world {}", t.world)
        });
        if t.boss != 0 {
            r.check(g.movers.contains(t.boss), || {
                format!("teleport16 row {i}: boss {}", t.boss)
            });
        }
    }
    r
}

fn x4c(g: &GameData, client: Option<&ClientData>) -> XrefResult {
    let mut r = XrefResult::new(
        "X4c",
        "world-map monster records name existing movers and drop items",
        Severity::Warning,
    );
    if let Some(c) = client {
        for info in &c.map_monsters {
            for e in &info.entries {
                for m in &e.movers {
                    r.check(g.movers.contains(*m), || {
                        format!("map {} record {}: mover {m}", info.map, e.frame)
                    });
                }
                if some(e.drop_item) {
                    r.check(exists_item(g, e.drop_item), || {
                        format!(
                            "map {} record {}: drop item {}",
                            info.map, e.frame, e.drop_item
                        )
                    });
                }
            }
        }
    }
    r
}

fn x5(g: &GameData) -> XrefResult {
    let mut r = XrefResult::new(
        "X5",
        "item and skill SFX ids are sfx models",
        Severity::Info,
    );
    let sfx = g.models.by_type.get(&OT_SFX);
    let has = |id: u32| sfx.is_some_and(|t| t.contains(id));
    for (id, p) in g.items.iter().chain(g.skills.iter()) {
        if some(p.sfx_elemental) {
            r.check(has(p.sfx_elemental), || {
                format!("{} ({id}): sfx_elemental {}", p.name, p.sfx_elemental)
            });
        }
        for s in p.sfx_obj.iter().copied().filter(|s| some(*s)) {
            r.check(has(s), || format!("{} ({id}): sfx_obj {s}", p.name));
        }
    }
    r
}

/// X6: file existence through the VFS. Model files are counted per entry like the reference check (C25: 16
/// `.o3d` entries, 1 mover `.chr`, 743 `.ani` references = 449 distinct files).
fn x6(g: &GameData, vfs: &Vfs) -> Vec<XrefResult> {
    let exists = |f: &str| vfs.exists(&format!("Model/{f}"));
    let m = &g.models;
    let mut o3d = XrefResult::new("X6a-o3d", "model .o3d files exist", Severity::Info);
    let mut chr = XrefResult::new("X6a-chr", "mover .chr files exist", Severity::Info);
    let mut ani = XrefResult::new(
        "X6a-ani",
        "motion .ani files exist (per reference)",
        Severity::Info,
    );
    let mut distinct = BTreeSet::new();
    for (ty, table) in &m.by_type {
        for e in table.values() {
            if let Some(f) = m.model_file(*ty, e.index)
                && f.ends_with(".o3d")
            {
                o3d.check(exists(&f), || f.clone());
            }
            if *ty == OT_MOVER
                && let Some(b) = m.bone_file(*ty, e.index)
            {
                chr.check(exists(&b), || b.clone());
            }
            for (mti, _) in &e.motions {
                if let Some(a) = m.motion_file(*ty, e.index, *mti) {
                    let ok = exists(&a);
                    if !ok {
                        distinct.insert(a.clone());
                    }
                    ani.check(ok, || a.clone());
                }
            }
        }
    }
    let mut ani_files = XrefResult::new(
        "X6a-ani-files",
        "distinct missing .ani files",
        Severity::Info,
    );
    ani_files.checked = ani.checked;
    ani_files.missing = u32::try_from(distinct.len()).unwrap_or(u32::MAX);
    ani_files.samples = distinct.iter().take(5).cloned().collect();
    let mut portraits = XrefResult::new("X6b", "NPC portraits exist in Char/", Severity::Info);
    for n in g.npcs.iter() {
        let p = n.portrait.trim();
        if !p.is_empty() {
            portraits.check(vfs.exists(&format!("Char/{p}")), || {
                format!("{}: {p}", n.key)
            });
        }
    }
    vec![o3d, chr, ani, ani_files, portraits]
}

fn x7(g: &GameData) -> XrefResult {
    let mut r = XrefResult::new(
        "X7",
        "unlisted non-empty world string tables (expected 0)",
        Severity::Warning,
    );
    // the digest counts every diagnostic, also the ones the list no longer stores
    r.missing = g
        .meta
        .diag_summary
        .counts
        .iter()
        .filter(|(k, _)| k.ends_with(":UnlistedStringTable"))
        .map(|(_, n)| *n)
        .sum();
    r.checked = r.missing;
    r
}

/// Builds the whole report from a finished load: counts, spot checks, the cross-references and the output of
/// the registered extra checks.
pub fn build_report(
    game: &GameData,
    client: Option<&ClientData>,
    vfs: &Vfs,
    extra: &[&ExtraCheck],
) -> DataReport {
    let mut r = DataReport::from_game(game);
    r.xref = cross_references(game, client, Some(vfs));
    for f in extra {
        r.extra.extend(f(game, vfs));
    }
    r
}

/// `flyff_data::check::run(vfs, opts)` of Plan/14 §9: loads everything (P0-P8, the client data included unless
/// `opts` says otherwise) and evaluates the report. CLI wiring and the exit codes are doc 10's.
pub fn run(
    vfs: &Vfs,
    opts: &DataOptions,
    extra: &[&ExtraCheck],
) -> Result<(LoadOutput, DataReport), LoadError> {
    let out = load_all(vfs, opts)?;
    let report = build_report(&out.game, out.client.as_ref(), vfs, extra);
    Ok((out, report))
}
