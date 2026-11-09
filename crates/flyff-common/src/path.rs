//! Resource path normalisation (Plan/14 §2.2, exact).
//!
//! Every path found in game data or code (`"World\\WdMadrigal\\wdMadrigal.txt.txt"`,
//! `".\\LuaFunc\\EventFunc.lua"`, `Theme\texDialogBox.bmp`) is reduced to one canonical form: ASCII
//! lower-case, `/`-separated, no leading or trailing slash, no `.` or empty segments.

use thiserror::Error;

/// Why a path could not be normalised.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum PathError {
    /// The path is empty after normalisation.
    #[error("empty resource path")]
    Empty,
    /// A `..` segment would pop above the Resource root.
    #[error("resource path escapes the root: {0:?}")]
    EscapesRoot(String),
}

/// Normalises a resource path (rules of Plan/14 §2.2):
///
/// 1. trim ASCII whitespace at both ends,
/// 2. replace every `\` with `/`,
/// 3. split on `/`, drop empty and `.` segments; a `..` segment pops the previous segment (popping
///    above the root is [`PathError::EscapesRoot`]),
/// 4. `to_ascii_lowercase` every segment (non-ASCII bytes are left unchanged),
/// 5. join with `/`; the empty path is [`PathError::Empty`].
pub fn normalize_resource_path(p: &str) -> Result<String, PathError> {
    let trimmed = p.trim_matches(|c: char| c.is_ascii_whitespace());
    let unified = trimmed.replace('\\', "/");
    let mut segments: Vec<String> = Vec::new();
    for seg in unified.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(PathError::EscapesRoot(p.to_owned()));
                }
            }
            other => segments.push(other.to_ascii_lowercase()),
        }
    }
    if segments.is_empty() {
        return Err(PathError::Empty);
    }
    Ok(segments.join("/"))
}
