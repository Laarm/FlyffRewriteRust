//! The items area (Plan/25 §5, §11.3; **WP-ADMIN-20**, **WP-ADMIN-22**): catalogue metadata, item validation,
//! grants v2 with its idempotency, the grant and removal lists with their cancels, templates and kits, the streaming
//! ownership search, the duplicate report and the economy overview.
//!
//! | route | cap | module |
//! |---|---|---|
//! | `GET /admin/api/items/catalog-meta` | `act.items` | `meta` |
//! | `POST /admin/api/items/validate` | `act.items` | `meta` |
//! | `POST /admin/api/grants/v2` ✱ | `act.items` | `v2` |
//! | `POST /admin/api/grants` (the alias of 23 §8.3) | `act.items` | `alias` (mounted by `routes.rs`) |
//! | `GET /admin/api/grants`, `POST /admin/api/grants/{id}/cancel`, `GET /admin/api/removals`, `POST /admin/api/removals/{id}/cancel` | `act.items` | `lists` |
//! | `GET/POST/PUT/DELETE /admin/api/templates` | `act.items` | `templates` |
//! | `POST /admin/api/items/search` (NDJSON), `POST /admin/api/items/duplicates` | `view.inventory` | `search` |
//! | `GET /admin/api/economy` | `view.inventory` | `economy` |

mod alias;
mod economy;
mod lists;
mod meta;
mod search;
mod templates;
mod v2;

pub use alias::grants_alias;
pub use meta::{build as build_catalog_meta, limits as item_limits};

use super::{ApiState, Routes};
use crate::admin::dto_items::{CatalogMeta, EconomyDto};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::ops::AdminOps;
use flyff_common::UnixTime;
use flyff_game::GameInput;
use flyff_game::admin::{AdminConnId, AdminQuery, AdminQueryResult, AdminViewOp};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// How long a computed economy overview is served again (25 §5.7).
pub const ECONOMY_CACHE: Duration = Duration::from_secs(10 * 60);

/// The economy overviews of the last 10 minutes by counted item: `(computed at, when, answer)`.
pub type EconomyCache = BTreeMap<u32, (Instant, UnixTime, Arc<EconomyDto>)>;

/// What the items area keeps between requests (a field of `AdminShared`).
#[derive(Default)]
pub struct ItemsState {
    /// The catalogue metadata: the game data does not change while the server runs.
    pub meta: OnceLock<Arc<CatalogMeta>>,
    /// The economy overviews of the last 10 minutes by counted item.
    pub economy: Mutex<EconomyCache>,
}

impl std::fmt::Debug for ItemsState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ItemsState")
    }
}

/// Parses a JSON request body: `400 bad_request` with the parser's message.
pub(crate) fn parse_json<T: DeserializeOwned>(body: &[u8]) -> ApiResult<T> {
    serde_json::from_slice(body).map_err(|e| ApiError::bad_request(format!("malformed body: {e}")))
}

/// Asks the game thread a read-only question (`GameInput::AdminView{Query}`; the connection id is not used by
/// queries).
pub(crate) async fn game_query(ops: &AdminOps, q: AdminQuery) -> ApiResult<AdminQueryResult> {
    let (reply, rx) = flyff_common::rt::reply_pair();
    let sent = tokio::time::timeout(
        Duration::from_secs(2),
        ops.game_tx.send_async(GameInput::AdminView {
            conn: AdminConnId(0),
            op: AdminViewOp::Query { q, reply },
        }),
    )
    .await;
    match sent {
        Ok(Ok(())) => {}
        Ok(Err(_)) => return Err(ApiError::unavailable("the game thread is not running")),
        Err(_) => {
            return Err(ApiError::unavailable(
                "the game thread did not accept the query",
            ));
        }
    }
    match tokio::time::timeout(Duration::from_secs(5), rx.recv_async()).await {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(_)) => Err(ApiError::unavailable("the game thread did not answer")),
        Err(_) => Err(ApiError::unavailable(
            "the game thread did not answer in time",
        )),
    }
}

/// Mounts the routes of this area.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/api/items/catalog-meta", meta::catalog_meta)
        .post("/admin/api/items/validate", meta::validate)
        .post("/admin/api/grants/v2", v2::grant)
        .get("/admin/api/grants", lists::grants)
        .post("/admin/api/grants/{id}/cancel", lists::cancel_grant)
        .get("/admin/api/removals", lists::removals)
        .post("/admin/api/removals/{id}/cancel", lists::cancel_removal)
        .get("/admin/api/templates", templates::list)
        .post("/admin/api/templates", templates::create)
        .put("/admin/api/templates/{id}", templates::update)
        .delete("/admin/api/templates/{id}", templates::delete)
        .post("/admin/api/items/search", search::search)
        .post("/admin/api/items/duplicates", search::duplicates)
        .get("/admin/api/economy", economy::economy)
}
