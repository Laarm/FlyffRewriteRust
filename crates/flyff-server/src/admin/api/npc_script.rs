//! `GET /admin/api/npc-scripts/{key}` (`view.map`; Plan/25 §3.10 "Open NPC script"; WP-ADMIN-13): the read-only text of a transpiled NPC dialog
//! (`<home>/scripts/npc/<key>.lua`), for the NPC card of the live map. The key is the `dialog_key` of the `npcs` bundle (lower case); anything
//! but letters, digits and `_` is refused, so the route cannot leave the scripts folder. The text is cut at 256 KiB.

use super::{ApiState, Routes};
use crate::admin::auth::Who;
use crate::admin::error::{ApiError, ApiResult};
use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;

/// The most bytes of a script the route answers.
pub const MAX_BYTES: usize = 256 * 1024;

/// The longest key.
const KEY_MAX: usize = 64;

/// The answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NpcScript {
    /// The key as the route read it (lower case).
    pub key: String,
    /// The file below the home, `scripts/npc/<key>.lua` with the configured folder.
    pub file: String,
    /// The text (UTF-8, invalid bytes replaced).
    pub text: String,
    /// The script is longer than [`MAX_BYTES`] and `text` is its beginning.
    pub truncated: bool,
}

/// Mounts the route.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/npc-scripts/{key}", script)
}

/// A key as the file stem: ASCII letters, digits and `_`, lower-cased; `None` for anything else.
pub fn clean_key(key: &str) -> Option<String> {
    let k = key.trim();
    (!k.is_empty()
        && k.len() <= KEY_MAX
        && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
    .then(|| k.to_ascii_lowercase())
}

/// The text to answer for the bytes of a file.
pub fn script_of(key: &str, file: String, bytes: &[u8]) -> NpcScript {
    let cut = bytes.len() > MAX_BYTES;
    let shown = bytes.get(..MAX_BYTES).unwrap_or(bytes);
    NpcScript {
        key: key.to_owned(),
        file,
        text: String::from_utf8_lossy(shown).into_owned(),
        truncated: cut,
    }
}

async fn script(
    State(st): State<ApiState>,
    Who(_who): Who,
    Path(key): Path<String>,
) -> ApiResult<Json<NpcScript>> {
    let stem = clean_key(&key).ok_or_else(|| {
        ApiError::invalid("key", "a script key is letters, digits and _ (at most 64)")
    })?;
    let folder = st.ops.current.lock().paths.npc_scripts_dir.clone();
    let path = st
        .ops
        .reload_source
        .home
        .join(&folder)
        .join(format!("{stem}.lua"));
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ApiError::not_found(format!("no script {stem}.lua")));
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "an NPC script could not be read");
            return Err(ApiError::unavailable("the script could not be read"));
        }
    };
    let shown = format!("{}/{stem}.lua", folder.trim_end_matches('/'));
    Ok(Json(script_of(&stem, shown, &bytes)))
}

/// Declares the types of this module for the generated TypeScript of the DTO catalogue (feature `ts`).
#[cfg(feature = "ts")]
pub fn declare_ts(out: &mut String, cfg: &ts_rs::Config) {
    use ts_rs::TS;
    out.push_str("export ");
    out.push_str(&NpcScript::decl(cfg));
    out.push('\n');
}
