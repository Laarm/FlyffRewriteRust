//! `Terrain.inc`: the terrain texture table (`CTerrainMng::LoadScript`, Plan/14 §6.19, ref/19 §14;
//! src:_Common/TerrainMng.cpp:75-202).
//!
//! ```text
//! file  := (group | entry)*
//! group := STRING NUMBER '{' (group | entry)* '}'             // "name" frameCount { ... }
//! entry := NUMBER NUMBER STRING NUMBER STRING                  // id frames "texture.dds" block "sound"
//! ```
//!
//! The original is a brace-counting state machine; this is the proper recursive grammar the plan asks for.
//! The rows keep **file order** and carry the chain of enclosing group headers (`group_path`, outermost
//! first), from which the renderer rebuilds the water animation sets exactly as the original did: the
//! top-level group with a non-zero count (`["물", 2]`) gives the number of sets, its sub-groups with a
//! non-zero count (`["물1", 54]`, `["물2", 54]`) give the sets, and the rows below each of them its frame
//! ids in order. `folder` is the innermost group name (`""` at the top level).

use serde::{Deserialize, Serialize};

/// One texture row (`TERRAIN`) with its place in the group tree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerrainTex {
    /// The innermost enclosing group name, `""` for a row outside any group.
    pub folder: String,
    /// The terrain texture id used by the `.lnd` layers.
    pub id: u32,
    /// The second number of the row (always 0 in the data; the original read it as the frame count).
    pub frames: u32,
    /// The texture file (a bare file name under `World/Texture/`).
    pub file: String,
    /// `m_bBlock` (parsed, never used by the original).
    pub block: u32,
    /// The last string of the row (unused by the original, `""` in the data).
    pub sound: String,
    /// The enclosing `"name" N` group headers, outermost first, e.g. `[("물", 2), ("물1", 54)]`.
    pub group_path: Vec<(String, u32)>,
}

/// The deepest group nesting accepted (the data nests twice); deeper groups are flattened with a Warning.
pub const MAX_GROUP_DEPTH: usize = 16;

#[cfg(feature = "load")]
pub(crate) use parse::parse_terrain;

#[cfg(feature = "load")]
mod parse {
    use super::{MAX_GROUP_DEPTH, TerrainTex};
    use flyff_script::{
        DiagCode, Diagnostics, Scanner, ScannerOptions, Severity, TokenType, c_atoi,
    };
    use std::sync::Arc;

    /// Parses `Terrain.inc` (raw scanner: no substitution, like `CScanner`).
    pub(crate) fn parse_terrain(
        text: &str,
        file: &Arc<str>,
        diags: &mut Diagnostics,
    ) -> Vec<TerrainTex> {
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        let mut out: Vec<TerrainTex> = Vec::new();
        let mut path: Vec<(String, u32)> = Vec::new();
        let mut warned_depth = false;
        let mut flattened = 0usize; // blocks beyond MAX_GROUP_DEPTH: they close without popping
        loop {
            let ty = sc.get_token(false);
            if sc.finished() {
                break;
            }
            let first = sc.token().as_bytes().first().copied();
            match (ty, first) {
                (TokenType::Delimiter, Some(b'}')) => {
                    if flattened > 0 {
                        flattened -= 1;
                    } else {
                        path.pop();
                    }
                }
                (TokenType::Delimiter, Some(b'{')) => {
                    // a block without a header: keep the nesting balanced
                    if path.len() >= MAX_GROUP_DEPTH {
                        flattened += 1;
                    } else {
                        path.push((String::new(), 0));
                    }
                }
                (TokenType::Number | TokenType::Hex, _) => {
                    let id = c_atoi(sc.token().as_bytes()) as u32;
                    let frames = sc.get_number(false);
                    sc.get_token(false);
                    let file_name = sc.token().to_owned();
                    let block = sc.get_number(false);
                    sc.get_token(false);
                    let sound = sc.token().to_owned();
                    let folder = path.last().map(|(n, _)| n.clone()).unwrap_or_default();
                    out.push(TerrainTex {
                        folder,
                        id,
                        frames,
                        file: file_name,
                        block,
                        sound,
                        group_path: path.clone(),
                    });
                }
                _ => {
                    // a group header: "name" N '{'
                    let name = sc.token().to_owned();
                    let count = sc.get_number(false);
                    let m = sc.mark();
                    sc.get_token(false);
                    if sc.token().starts_with('{') {
                        if path.len() >= MAX_GROUP_DEPTH {
                            flattened += 1;
                            if !warned_depth {
                                warned_depth = true;
                                diags.report(
                                    Severity::Warning,
                                    DiagCode::CapacityExceeded,
                                    Some(file),
                                    sc.line(),
                                    format!(
                                        "Terrain.inc: groups nested deeper than {MAX_GROUP_DEPTH}"
                                    ),
                                );
                            }
                        } else {
                            path.push((name, count));
                        }
                    } else {
                        // not a header after all: re-read the token as the start of the next item
                        sc.reset(m);
                        diags.report(
                            Severity::Warning,
                            DiagCode::ValueOutOfRange,
                            Some(file),
                            sc.line(),
                            format!("Terrain.inc: `{name}` is not followed by a block"),
                        );
                    }
                }
            }
        }
        diags.extend(sc.take_diagnostics());
        out
    }
}
