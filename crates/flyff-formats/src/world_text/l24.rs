//! `.24l` hourly light table (Plan/30 §9.2; `CWorld::ReadFile24Light`, src:_Common/WorldEnvironment.cpp:183-204).
//!
//! 24 rows of 6 floats: `r1 g1 b1` (diffuse) and `r2 g2 b2` (ambient) for each hour. No such file exists in
//! the reference data; `use24light 1` in a `.wld` makes the loader read `<path>.24l`.

use flyff_script::{DiagCode, Diagnostics, ScriptCtx, Severity};

use super::open_script;

/// Number of rows (hours) of the table.
pub const L24_ROWS: usize = 24;

/// Parses a `.24l`: always [`L24_ROWS`] rows. A short file is zero-filled like the original (`GetFloat` on an
/// empty token is 0) and reported once as `L24Short` (Warning); extra values are ignored.
pub fn parse_24l(
    text: &str,
    file: &str,
    ctx: &ScriptCtx,
    diags: &mut Diagnostics,
) -> Vec<[f32; 6]> {
    let mut s = open_script(text, file, ctx, diags);
    let mut rows = Vec::with_capacity(L24_ROWS);
    let mut complete = 0usize;
    for _ in 0..L24_ROWS {
        let mut row = [0.0f32; 6];
        for v in &mut row {
            *v = s.get_float();
        }
        if !s.finished() {
            complete += 1;
        }
        rows.push(row);
    }
    if complete < L24_ROWS {
        s.report(
            Severity::Warning,
            DiagCode::L24Short,
            format!(
                "the light table has {complete} complete row(s) instead of {L24_ROWS}; the rest is 0"
            ),
        );
    }
    rows
}
