//! Glue of the prop loaders: items (L14, F2/F3/F6 prerequisites, L14b), skills (L15, F4), party skills
//! (L16), jobs (L17), ctrls (L18), add-skills (L19, F5), movers (L13, F1), motions (L31), the drop part of
//! `PropMoverEx.inc` (L32, F8) and `propDropEvent.inc` (L39, F9).

use crate::items_ext::except::apply_except;
use crate::load::ctx::{LoadCtx, Need};
use crate::mover_ex::{apply_drop_events, parse_mover_ex};
use crate::props::ctrl::parse_ctrl_file;
use crate::props::item::{ItemProp, parse_item_file};
use crate::props::item_index::after_load_prop_item;
use crate::props::job::{MAX_JOB, parse_job_file};
use crate::props::motion::parse_motion_file;
use crate::props::mover::parse_mover_file;
use crate::props::skill::build_job_skills;
use crate::props::skill_add::parse_add_skill_file;
use crate::teleport16::fill_boss_drops;
use crate::types::IdTable;
use flyff_script::{DiagCode, Severity};

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Stores `row` in `table` (last wins). A repeated id is a Warning (`PropIdDuplicate`), an id above
/// `IdTable::MAX_ID` an Error (`PropIdOutOfRange`).
fn store_row(
    cx_diags: &mut flyff_script::Diagnostics,
    table: &mut IdTable<ItemProp>,
    file: &str,
    row: ItemProp,
) {
    let id = row.id;
    match table.insert(id, row) {
        Ok(None) => {}
        Ok(Some(_)) => cx_diags.report(
            Severity::Warning,
            DiagCode::PropIdDuplicate,
            None,
            0,
            format!("{file}: id {id} appears more than once; the last row wins"),
        ),
        Err(_) => cx_diags.report(
            Severity::Error,
            DiagCode::PropIdOutOfRange,
            None,
            0,
            format!(
                "{file}: id {id} is above {} and ignored",
                IdTable::<ItemProp>::MAX_ID
            ),
        ),
    }
}

/// L14 + L14b: `propItem.txt` with F2 (in the row parser), the name map, F3 (`OnAfterLoadPropItem`) and
/// `except.txt`. The original ignores the `path` of the project file and loads `"propItem.txt"`.
pub fn load_prop_item(cx: &mut LoadCtx<'_>, path: &str) {
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let opts = cx.opts;
    let f = parse_item_file(&text, &file, &sctx, opts, &mut cx.diags);
    if f.skipped_ver > 0 {
        cx.report(
            Severity::Info,
            DiagCode::PropRowSkipped,
            Some(path),
            0,
            format!("{} row(s) with ver > 15 skipped", f.skipped_ver),
        );
    }
    let mut rows = 0usize;
    for row in f.rows {
        rows += 1;
        // m_mapII.insert: the first display name wins
        cx.gd
            .item_index
            .by_name
            .entry(row.name.clone())
            .or_insert(row.id);
        store_row(&mut cx.diags, &mut cx.gd.items, &file, row);
    }
    let rep = after_load_prop_item(
        &mut cx.gd.items,
        &mut cx.gd.item_index,
        opts.cs_item_price,
        opts.pet_item_price,
    );
    for (id, why) in rep.out_of_range {
        cx.report(
            Severity::Error,
            DiagCode::IndexOutOfRange,
            Some(path),
            0,
            format!("item {id}: {why}; left out of the kind index"),
        );
    }
    let c = &mut cx.gd.meta.counts;
    c.item_rows = count(rows);
    c.item_unique_ids = count(cx.gd.items.len());
    c.item_max_id = cx.gd.items.max_id().unwrap_or(0);
    load_except(cx);
}

/// L14b: `except.txt` (applies only for `LANG_ENG` / sub-language 0: no such block here).
fn load_except(cx: &mut LoadCtx<'_>) {
    let Some(text) = cx.read_text("except.txt", Need::Info) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc("except.txt");
    let ex = apply_except(&text, &file, &sctx, &mut cx.gd.items, &mut cx.diags);
    let c = &mut cx.gd.meta.counts;
    c.except_world_drop_excluded = count(ex.world_drop_excluded.len());
    c.except_patches = ex.patched;
    cx.gd.item_except = ex;
}

/// L15: `propSkill.txt` (hard-coded name) and the per-job skill lists (F4).
pub fn load_skills(cx: &mut LoadCtx<'_>) {
    let path = "propSkill.txt";
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let f = parse_item_file(&text, &file, &sctx, cx.opts, &mut cx.diags);
    let mut rows = 0usize;
    for row in f.rows {
        rows += 1;
        cx.gd.skill_names.entry(row.name.clone()).or_insert(row.id);
        store_row(&mut cx.diags, &mut cx.gd.skills, &file, row);
    }
    let (js, rep) = build_job_skills(&cx.gd.skills);
    for (id, job) in rep.bad_job {
        cx.report(
            Severity::Error,
            DiagCode::IndexOutOfRange,
            Some(path),
            0,
            format!("skill {id}: owning job {job} >= MAX_JOB ({MAX_JOB}); not in a job list"),
        );
    }
    for (id, job) in rep.overflow {
        cx.report(
            Severity::Error,
            DiagCode::CapacityExceeded,
            Some(path),
            0,
            format!("skill {id}: job {job} already has 40 skills; not in the job list"),
        );
    }
    cx.gd.job_skills = js;
    cx.gd.meta.counts.skill_rows = count(rows);
}

/// L16: `propTroupeSkill.txt` (hard-coded name) into the party skill table.
pub fn load_party_skills(cx: &mut LoadCtx<'_>) {
    let path = "propTroupeSkill.txt";
    let Some(text) = cx.read_text(path, Need::Warn) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let f = parse_item_file(&text, &file, &sctx, cx.opts, &mut cx.diags);
    let mut rows = 0usize;
    for row in f.rows {
        rows += 1;
        cx.gd
            .party_skill_names
            .entry(row.name.clone())
            .or_insert(row.id);
        store_row(&mut cx.diags, &mut cx.gd.party_skills, &file, row);
    }
    cx.gd.meta.counts.party_skill_rows = count(rows);
}

/// L17: `propJob.inc` (hard-coded name; the project file says `propJob.txt`).
pub fn load_jobs(cx: &mut LoadCtx<'_>) {
    let path = "propJob.inc";
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let f = parse_job_file(&text, &file, &sctx, &mut cx.diags);
    let mut rows = 0usize;
    for (job, p) in f.rows {
        rows += 1;
        if let Some(slot) = cx.gd.jobs.props.get_mut(job as usize) {
            *slot = Some(p);
        }
    }
    cx.gd.meta.counts.job_rows = count(rows);
}

/// L18: `propCtrl.txt` (hard-coded name; the project file says `propCtrl.csv`).
pub fn load_ctrls(cx: &mut LoadCtx<'_>) {
    let path = "propCtrl.txt";
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let f = parse_ctrl_file(&text, &file, &sctx, &mut cx.diags);
    let mut rows = 0usize;
    for row in f.rows {
        rows += 1;
        cx.gd.ctrl_names.entry(row.name.clone()).or_insert(row.id);
        let id = row.id;
        if cx.gd.ctrls.insert(id, row).is_err() {
            cx.report(
                Severity::Error,
                DiagCode::PropIdOutOfRange,
                Some(path),
                0,
                format!("ctrl id {id} is out of range and ignored"),
            );
        }
    }
    cx.gd.meta.counts.ctrl_rows = count(rows);
}

/// L19: `propSkillAdd.csv` (comma mode) with F5; needs the skills (L15).
pub fn load_add_skills(cx: &mut LoadCtx<'_>, path: &str) {
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let f = parse_add_skill_file(&text, &file, &sctx, &cx.gd.skills, &mut cx.diags);
    let mut rows = 0usize;
    for row in f.rows {
        rows += 1;
        let id = row.id;
        match cx.gd.add_skills.insert(id, row) {
            Ok(Some(_)) => cx.report(
                Severity::Warning,
                DiagCode::PropIdDuplicate,
                Some(path),
                0,
                format!("add-skill id {id} appears more than once; the last row wins"),
            ),
            Ok(None) => {}
            Err(_) => cx.report(
                Severity::Error,
                DiagCode::PropIdOutOfRange,
                Some(path),
                0,
                format!("add-skill id {id} is out of range and ignored"),
            ),
        }
    }
    let c = &mut cx.gd.meta.counts;
    c.add_skill_rows = count(rows);
    c.add_skill_max_id = cx.gd.add_skills.max_id().unwrap_or(0);
    c.add_skill_empty_fields = f.empty_fields;
}

/// L13: `propMover.txt` (hard-coded name) with F1. A duplicate or out-of-range id is **Fatal** (the
/// original returns `FALSE` and the server aborts).
pub fn load_movers(cx: &mut LoadCtx<'_>) {
    let path = "propMover.txt";
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    match parse_mover_file(&text, &file, &sctx, cx.opts, &mut cx.diags) {
        Ok(f) => {
            let mut rows = 0usize;
            for row in f.rows {
                rows += 1;
                cx.gd.mover_names.entry(row.name.clone()).or_insert(row.id);
                let id = row.id;
                // ids are unique and < MAX_PROPMOVER here
                if cx.gd.movers.insert(id, row).is_err() {
                    cx.report(
                        Severity::Fatal,
                        DiagCode::PropIdOutOfRange,
                        Some(path),
                        0,
                        format!("mover id {id} cannot be stored"),
                    );
                }
            }
            let c = &mut cx.gd.meta.counts;
            c.mover_rows = count(rows);
            c.mover_max_id = cx.gd.movers.max_id().unwrap_or(0);
            c.mover_size = cx.gd.movers.size();
        }
        Err(e) => {
            cx.report(Severity::Fatal, e.code, Some(path), e.line, e.message);
        }
    }
}

/// L31: `propMotion.txt` (emote/action icons) into `emotes`.
pub fn load_motion(cx: &mut LoadCtx<'_>, path: &str) {
    let Some(text) = cx.read_text(path, Need::Warn) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let f = parse_motion_file(&text, &file, &sctx, &mut cx.diags);
    let mut rows = 0usize;
    for row in f.rows {
        rows += 1;
        let id = row.id;
        if cx.gd.emotes.insert(id, row).is_err() {
            cx.report(
                Severity::Error,
                DiagCode::PropIdOutOfRange,
                Some(path),
                0,
                format!("motion id {id} is out of range and ignored"),
            );
        }
    }
    cx.gd.meta.counts.motion_rows = count(rows);
}

/// L32: `PropMoverEx.inc`, the drop statements (`Maxitem`, `DropItem`, `DropGold`, `DropKind`; F8). The scalar
/// statements, the `AI {}` blocks and `Transform` are not loaded yet (the rest of WP-DATA-07): every other field of
/// `MoverProp.ex` keeps its default.
pub fn load_mover_ex(cx: &mut LoadCtx<'_>) {
    let path = "PropMoverEx.inc";
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let stats = parse_mover_ex(&text, &file, &sctx, &mut cx.gd.movers, &mut cx.diags);
    cx.gd.meta.counts.mover_ex_blocks = count(stats.blocks as usize);
    cx.gd.meta.counts.mover_ex_unknown = count(stats.unknown_movers as usize);
    // the boss drops of the teleporter window (read at L30, filled now that the boss drops are known)
    fill_boss_drops(&mut cx.gd.teleports16, &cx.gd.movers, &cx.gd.items);
}

/// L39: `propDropEvent.inc` (F9): every row is appended to the drop table of each mover whose level is in the
/// row's range. Runs after L14b, so the `worldDrop = 0` exclusion set is known.
pub fn load_drop_event(cx: &mut LoadCtx<'_>) {
    let path = "propDropEvent.inc";
    let Some(text) = cx.read_text(path, Need::Warn) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let except = std::mem::take(&mut cx.gd.item_except);
    let stats = apply_drop_events(
        &text,
        &file,
        &sctx,
        &except,
        &mut cx.gd.movers,
        &mut cx.diags,
    );
    cx.gd.meta.counts.drop_event_rows = count(stats.event_rows as usize);
    cx.gd.item_except = except;
}
