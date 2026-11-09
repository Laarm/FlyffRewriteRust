//! `.pat` patrol paths (Plan/30 §9.6; `CPatrolPath::LoadPatrol`, src:_Common/PathObj.cpp:147-227; ref/19 §10).
//!
//! Server only. All 15 `.pat` files of the reference data are 0 bytes, so the parser is exercised on
//! fixtures only.
//!
//! ```text
//! INDEX <n> { <cnt> x y z [dx dy dz state delay len]  <cnt> x y z [...]  ... }
//! ```
//!
//! Every point is preceded by its count word; when the count is not 0 the position is followed by the
//! direction (3 floats), the motion state, the delay (both integers) and the length (float). The closing
//! segment from the last to the first point uses the info of the point whose count was 1 and is built by the
//! consumer (doc 18), not stored here. Where the original crashes or loops forever (an empty list, a missing
//! `}` at the end of the file) the parser reports `PatMalformed` or `WorldTextTruncated` instead.

use flyff_script::{DiagCode, Diagnostics, Script, ScriptCtx, Severity};

use super::{open_script, report_truncated};

/// One patrol path (`INDEX`).
#[derive(Debug, Clone, PartialEq)]
pub struct PatrolPath {
    /// The number after `INDEX` (`m_dwPatrolIndex` of a respawn refers to it).
    pub index: u32,
    /// The points in file order.
    pub points: Vec<PatrolPoint>,
}

/// One point of a [`PatrolPath`].
#[derive(Debug, Clone, PartialEq)]
pub struct PatrolPoint {
    /// Position `x y z` in world units.
    pub pos: [f32; 3],
    /// Direction `dx dy dz`, present when `count != 0`.
    pub dir: Option<[f32; 3]>,
    /// Motion state (0 without info).
    pub state: u32,
    /// Delay (0 without info).
    pub delay: u32,
    /// Segment length (0 without info).
    pub len: f32,
    /// The count word that preceded the point (`1` marks the point whose info closes the loop).
    pub count: u32,
}

fn vec3(s: &mut Script<'_>) -> [f32; 3] {
    [s.get_float(), s.get_float(), s.get_float()]
}

/// Parses a `.pat` (Plan/30 §9.6). Tokens outside an `INDEX` block are skipped with `PatMalformed`; a path
/// cut off before its `}` is dropped with `WorldTextTruncated`.
pub fn parse_pat(
    text: &str,
    file: &str,
    ctx: &ScriptCtx,
    diags: &mut Diagnostics,
) -> Vec<PatrolPath> {
    let mut paths = Vec::new();
    let mut s = open_script(text, file, ctx, diags);
    s.get_token();
    while !s.finished() {
        if !s.is("INDEX") {
            let token = s.token().to_owned();
            s.report(
                Severity::Warning,
                DiagCode::PatMalformed,
                format!("expected `INDEX`, found `{token}`; the token is skipped"),
            );
            s.get_token();
            continue;
        }
        let index = s.get_number();
        s.get_token(); // the `{`
        if !s.is("{") && !s.finished() {
            let token = s.token().to_owned();
            s.report(
                Severity::Warning,
                DiagCode::PatMalformed,
                format!("expected `{{` after `INDEX {index}`, found `{token}`"),
            );
        }
        let mut points = Vec::new();
        let mut count = s.get_number();
        while !s.is("}") && !s.finished() {
            let pos = vec3(&mut s);
            let mut point = PatrolPoint {
                pos,
                dir: None,
                state: 0,
                delay: 0,
                len: 0.0,
                count,
            };
            if count != 0 {
                point.dir = Some(vec3(&mut s));
                point.state = s.get_number();
                point.delay = s.get_number();
                point.len = s.get_float();
            }
            points.push(point);
            count = s.get_number();
        }
        if s.finished() {
            report_truncated(&mut s, "a patrol path");
            break;
        }
        if points.is_empty() {
            s.report(
                Severity::Warning,
                DiagCode::PatMalformed,
                format!("path {index} has no points (the original crashes on it)"),
            );
        }
        paths.push(PatrolPath { index, points });
        s.get_token();
    }
    paths
}
