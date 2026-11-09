//! Glue of the table loaders: `expTable.inc` (L22, F11), `jobItem.inc` (L33), `etc.inc` (L34),
//! `propEnchant.inc` (L35), `Constant.inc` (L36), `DiePenalty.inc` (L42) and `PKSetting.inc` (L43).

use crate::etc::parse_etc;
use crate::exp_table::parse_exp_table;
use crate::items_ext::except::SERVER_LANG;
use crate::job_item::parse_job_item;
use crate::load::ctx::{LoadCtx, Need};
use crate::rules::constant::parse_constant;
use crate::rules::die_penalty::parse_die_penalty;
use crate::rules::enchant::parse_enchant;
use crate::rules::pk_setting::parse_pk_setting;

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// L22: `expTable.inc`; the `expCompanyTest` block becomes `guild.levels` with the PXP divisor (F11).
pub fn load_exp_table(cx: &mut LoadCtx<'_>, path: &str) {
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let f = parse_exp_table(
        &text,
        &file,
        &sctx,
        cx.opts.guild_exp_divisor,
        &mut cx.diags,
    );
    let c = &mut cx.gd.meta.counts;
    c.exp_character_rows = count(f.exp.character.len());
    c.guild_level_rows = count(f.guild.levels.len());
    cx.gd.exp = f.exp;
    cx.gd.guild.levels = f.guild.levels;
}

/// L33: `jobItem.inc` (required).
pub fn load_job_item(cx: &mut LoadCtx<'_>) {
    let path = "jobItem.inc";
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let t = parse_job_item(&text, &file, &sctx, &mut cx.diags);
    cx.gd.meta.counts.job_item_jobs = count(t.jobs.len());
    cx.gd.job_items = t;
}

/// L34: `etc.inc` (required): the job names and kinds, the structure and guild appellation names.
pub fn load_etc(cx: &mut LoadCtx<'_>) {
    let path = "etc.inc";
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let f = parse_etc(&text, &file, &sctx, &mut cx.diags);
    for (job, info) in f.jobs {
        if let Some(slot) = cx.gd.jobs.info.get_mut(job as usize) {
            *slot = Some(info);
        }
    }
    cx.gd.etc = f.names;
}

/// L35: `propEnchant.inc`.
pub fn load_enchant(cx: &mut LoadCtx<'_>) {
    let path = "propEnchant.inc";
    let Some(text) = cx.read_text(path, Need::Warn) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    cx.gd.enchant = parse_enchant(&text, &file, &sctx, &mut cx.diags);
}

/// L36: `Constant.inc` (only the `LANG_ENG` block applies; there is none in this data set).
pub fn load_constant(cx: &mut LoadCtx<'_>) {
    let path = "Constant.inc";
    let Some(text) = cx.read_text(path, Need::Warn) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    cx.gd.constant = parse_constant(&text, &file, &sctx, SERVER_LANG, &mut cx.diags);
}

/// L42: `DiePenalty.inc`.
pub fn load_die_penalty(cx: &mut LoadCtx<'_>) {
    let path = "DiePenalty.inc";
    let Some(text) = cx.read_text(path, Need::Warn) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    cx.gd.die_penalty = parse_die_penalty(&text, &file, &sctx, &mut cx.diags);
}

/// L43: `PKSetting.inc`.
pub fn load_pk_setting(cx: &mut LoadCtx<'_>) {
    let path = "PKSetting.inc";
    let Some(text) = cx.read_text(path, Need::Warn) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    cx.gd.pk = parse_pk_setting(&text, &file, &sctx, &mut cx.diags);
}
