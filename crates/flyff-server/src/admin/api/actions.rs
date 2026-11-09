//! `POST /admin/api/actions/{action}` and `GET /admin/api/objects/{obj}` (Plan/25 §3.12, §4.6, §11.3; WP-ADMIN-12).
//!
//! | action | game command | capability |
//! |---|---|---|
//! | `teleport` | `TeleportCharacter` | `act.teleport.others` |
//! | `unstuck` | `TeleportCharacter` to the revival point of the continent | `act.unstuck` |
//! | `summon`, `goto` | `RunLine` `/su`, `/te` as the acting GM character, with the actor's authority | `act.teleport.self` |
//! | `heal`, `resurrect`, `kill`, `despawn`, `despawn-area`, `spawn` | `Heal`, `Resurrect`, `KillObject`, `DespawnObject`, `DespawnArea`, `SpawnMonsters` | `act.combat` |
//! | `drop-item` | `DropItem` | `act.items` |
//! | `respawn-region` | `RespawnRegion` | `act.world` |
//! | `mute`, `unmute` | `Mute`, `Unmute` (the mute needs a reason) | `act.mute` |
//! | `message` | `NotifyCharacter` | `act.message.player` |
//! | `mode-flags`, `edit-character` | `SetModeFlags`, `EditCharacter` | `act.character.edit` |
//!
//! The route is ✱ (it needs an `Idempotency-Key`); its catalogue capability is the lowest of the table, the handler
//! checks the one of the action. The flow (group cap, reason, typed confirmation, audit, answer mapping) is `run`;
//! the bodies and answers are [`dto`]. `GET /admin/api/objects/{obj}` answers the inspector card of an object
//! (`Query::InspectObject`, capability `view.map`).
//!
//! [`GameLink`] is what the flow needs from the server: the game thread (commands and read-only queries) and the
//! account registry. [`AdminOps`] implements it; the tests use a scripted game.

pub mod dto;
mod run;

use super::{ApiState, Routes};
use crate::admin::auth::Who;
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::guard::MutationJson;
use crate::admin::ops::{AdminOps, OpsError};
use axum::extract::{Path, State};
use dto::{ActionResult, ObjectDetail};
use flyff_common::rt::reply_pair;
use flyff_common::{AccountId, CharacterId, ObjId};
use flyff_game::GameInput;
use flyff_game::admin::{AdminConnId, AdminQuery, AdminQueryResult, AdminViewOp};
use flyff_game::input::{AdminActor, AdminCmd, AdminOutput};
use serde_json::Value;
use std::future::Future;
use std::time::Duration;

/// How long a query waits for room in the game inbox.
const SEND_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a query waits for its answer (the game answers within the tick).
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

/// What the action flow needs from the server (see the module documentation).
pub trait GameLink: Send + Sync {
    /// Sends a command to the game thread and waits for its answer.
    fn command(
        &self,
        cmd: AdminCmd,
        actor: AdminActor,
    ) -> impl Future<Output = Result<AdminOutput, OpsError>> + Send;

    /// A read-only question to the game thread (answered in the same tick, never recorded).
    fn query(
        &self,
        q: AdminQuery,
    ) -> impl Future<Output = Result<AdminQueryResult, OpsError>> + Send;

    /// The character of `account` that is in the world (the acting GM character of a staff member, 25 §1.5).
    fn in_world_character(&self, account: AccountId) -> Option<CharacterId>;
}

impl GameLink for AdminOps {
    async fn command(&self, cmd: AdminCmd, actor: AdminActor) -> Result<AdminOutput, OpsError> {
        self.game(cmd, actor).await
    }

    async fn query(&self, q: AdminQuery) -> Result<AdminQueryResult, OpsError> {
        let (reply, rx) = reply_pair();
        let input = GameInput::AdminView {
            // queries are not tied to a subscription: the connection id is not used
            conn: AdminConnId(0),
            op: AdminViewOp::Query { q, reply },
        };
        match tokio::time::timeout(SEND_TIMEOUT, self.game_tx.send_async(input)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return Err(OpsError::GameGone),
            Err(_) => return Err(OpsError::Timeout),
        }
        match tokio::time::timeout(ANSWER_TIMEOUT, rx.recv_async()).await {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(_)) => Err(OpsError::GameGone),
            Err(_) => Err(OpsError::Timeout),
        }
    }

    fn in_world_character(&self, account: AccountId) -> Option<CharacterId> {
        self.core.registry.view(account)?.character
    }
}

/// Mounts the routes of this area.
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.post("/admin/api/actions/{action}", action)
        .get("/admin/api/objects/{obj}", object)
}

/// `POST /admin/api/actions/{action}`.
async fn action(
    State(st): State<ApiState>,
    Who(who): Who,
    Path(name): Path<String>,
    MutationJson(m): MutationJson<Value>,
) -> ApiResult<axum::Json<ActionResult>> {
    run_action(&st, &who, &name, m).await.map(axum::Json)
}

/// Runs the action `name` for an identity with an envelope that is already parsed: what the route does, for the
/// routes that are a view of one action (`POST /admin/api/spawns/{region}/respawn` is `respawn-region`).
pub(crate) async fn run_action(
    st: &ApiState,
    who: &crate::admin::identity::Identity,
    name: &str,
    m: crate::admin::dto::Mutation<Value>,
) -> ApiResult<ActionResult> {
    let policy = st.policy();
    let env = run::Env {
        game: st.ops.as_ref(),
        policy: &policy,
        caps: &st.admin.caps,
        audit: &st.ops.audit,
        who,
    };
    run::handle(&env, name, m).await
}

/// `GET /admin/api/objects/{obj}`: the detail of one world object.
async fn object(
    State(st): State<ApiState>,
    Who(_who): Who,
    Path(obj): Path<String>,
) -> ApiResult<axum::Json<ObjectDetail>> {
    let id: u32 = obj
        .parse()
        .map_err(|_| ApiError::invalid("obj", "an object id is a number"))?;
    inspect(st.ops.as_ref(), ObjId(id)).await.map(axum::Json)
}

/// The inspector card of `obj`: `404 not_found` when it does not exist (any more).
async fn inspect<G: GameLink>(game: &G, obj: ObjId) -> ApiResult<ObjectDetail> {
    match game.query(AdminQuery::InspectObject(obj)).await? {
        AdminQueryResult::Object(d) => Ok(ObjectDetail::from(&*d)),
        AdminQueryResult::Failed(f) => Err(ApiError::from(&f)),
        other => Err(ApiError::internal(&format!(
            "InspectObject answered {other:?}"
        ))),
    }
}
