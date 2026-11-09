//! The world area: `GET /admin/api/worlds` and `GET /admin/api/world/{id}/meta` (Plan/25 §3.3, §3.4, §3.13, §11.3;
//! WP-ADMIN-08). The documents are built by [`meta`]; this file mounts the routes. `/world/find` (WP-ADMIN-10), the
//! quick locations (WP-ADMIN-14) and the spawn, census and weather routes (WP-ADMIN-30) are mounted here by their
//! work packages; until then the catalogue entries answer `501 needs_feature`.
//!
//! * `GET /admin/api/worlds` (`view.map`): every world the server loads with its group, size, minimap summary, layer
//!   policy and the number of players in it (the game thread's online list, grouped by world).
//! * `GET /admin/api/world/{id}/meta` (`view.map`): the geometry, the `.wld` camera, where the minimap tiles are, the
//!   regions with titles, the continents, the revival points, the portals, the spawn regions, the NPC placements and
//!   the `teleport16` rows of one world; `404` for a world that is not loaded. It never builds a tile: the overview
//!   pyramid is built by the first request for one of its tiles (`/gen/admin-map/...`).

pub mod find;
pub mod meta;

use super::{ApiState, Routes};
use crate::admin::auth::Who;
use crate::admin::error::{ApiError, ApiResult};
use axum::Json;
use axum::extract::{Path, Query, State};
use find::{FindDoc, LIVE_LIMIT, STATIC_LIMIT};
use meta::{MetaDoc, WorldEnv, WorldsDoc};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Mounts the routes of this area (the owning work packages add theirs here).
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/worlds", worlds)
        .get("/admin/api/world/find", world_find)
        .get("/admin/api/world/{id}/meta", world_meta)
}

fn env(st: &ApiState) -> WorldEnv<'_> {
    let a = &st.ops.http.assets;
    WorldEnv {
        vfs: &a.vfs,
        data: &a.data,
        manifest: &a.manifest,
        prep: &a.prep,
    }
}

/// `GET /admin/api/worlds`.
async fn worlds(State(st): State<ApiState>, Who(who): Who) -> ApiResult<Json<WorldsDoc>> {
    // the players per world; a game thread that does not answer leaves the counts at zero rather than the list empty
    let mut online: BTreeMap<u32, u32> = BTreeMap::new();
    if let Ok(rows) = st.ops.online(None, who.actor()).await {
        for r in rows {
            *online.entry(r.world).or_default() += 1;
        }
    }
    Ok(Json(meta::worlds_doc(&env(&st), &online)))
}

/// `GET /admin/api/world/{id}/meta`.
async fn world_meta(State(st): State<ApiState>, Path(id): Path<u32>) -> ApiResult<Json<MetaDoc>> {
    // the document reads a few files (the `.wld`, the `.dyo`) and walks the tables: off the async threads
    let st2 = st.clone();
    let doc = tokio::task::spawn_blocking(move || meta::meta_doc(&env(&st2), id))
        .await
        .map_err(|_| ApiError::unavailable("the world document could not be built"))?;
    doc.map(Json)
        .ok_or_else(|| ApiError::not_found(format!("world {id} is not loaded")))
}

/// The query string of `GET /admin/api/world/find`.
#[derive(Debug, Deserialize)]
struct FindQuery {
    /// What to look for: a player name or character id, a monster, NPC, region or place name, a prop id.
    #[serde(default)]
    q: String,
}

/// `GET /admin/api/world/find?q=` (Plan/25 §3.9): live players, monsters and NPCs from the game thread, then the titled areas and places of
/// every world. A query shorter than two characters answers an empty list.
async fn world_find(
    State(st): State<ApiState>,
    Query(query): Query<FindQuery>,
) -> ApiResult<Json<FindDoc>> {
    let Some(q) = find::clean_query(&query.q) else {
        return Ok(Json(FindDoc { hits: Vec::new() }));
    };
    let mut hits = find::live_hits(st.ops.as_ref(), &q).await?;
    hits.truncate(LIVE_LIMIT);
    let lower = q.to_lowercase();
    // the index is built from the world documents on the first search (off the async threads) and kept
    let st2 = st.clone();
    let statics = tokio::task::spawn_blocking(move || {
        let env = env(&st2);
        find::static_hits(find::static_index(&env), &lower, STATIC_LIMIT)
    })
    .await
    .map_err(|_| ApiError::unavailable("the place index could not be built"))?;
    hits.extend(statics);
    Ok(Json(FindDoc { hits }))
}
