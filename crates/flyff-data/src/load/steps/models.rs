//! Glue of the model registry loader: `mdlObj.inc` and `mdlDyna.inc` (L10, L11; Plan/14 §6.10).

use crate::load::ctx::{LoadCtx, Need};
use crate::models::parse_model_file;
use flyff_script::{DiagCode, Severity};

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `m_modelMng.LoadScript(path)`: the entries of one model file are added to the registry (last wins).
/// The file name is the project-file argument (`mdlObj.inc`, then `mdlDyna.inc`).
pub fn load_model_file(cx: &mut LoadCtx<'_>, path: &str) {
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let elems = parse_model_file(&text, &file, &sctx, &mut cx.diags);
    let mut refs = 0usize;
    let total = elems.len();
    for e in elems {
        refs += e.motions.len();
        let (ty, idx, name) = (e.obj_type, e.index, e.name.clone());
        match cx.gd.models.insert(e) {
            Ok(None) => {}
            Ok(Some(_)) => cx.report(
                Severity::Warning,
                DiagCode::ModelDuplicate,
                Some(path),
                0,
                format!(
                    "duplicate model id: type = {ty}, idx = {idx}, name = {name}; the last wins"
                ),
            ),
            Err(_) => cx.report(
                Severity::Error,
                DiagCode::PropIdOutOfRange,
                Some(path),
                0,
                format!("model id {idx} (type {ty}, {name}) is too large and ignored"),
            ),
        }
    }
    let c = &mut cx.gd.meta.counts;
    c.model_elems = c.model_elems.saturating_add(count(total));
    c.model_motion_refs = c.model_motion_refs.saturating_add(count(refs));
}
