//! What the command bodies of the console share (Plan/25 §3.12 "Execution rules", §11.5 "Group caps", §12 rule 2;
//! WP-ADMIN-12): the group cap, rank protection, the lookup of an online character and the shapes of the answers.
//!
//! **Every command is validated on the game thread first**: the target exists, is online, is in the expected place,
//! the rules allow the change and rank protection passes. Only then is it applied. A target that fails is reported
//! in the answer ([`TargetResult`]); a request that is wrong as a whole (an empty list, a destination in a world that
//! is not loaded) answers [`AdminOutput::Failed`] and changes nothing.

use super::{TargetId, TargetResult};
use crate::admin::types::{AdminErrorCode, AdminFailure};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::input::{AdminActor, AdminOutput};
use crate::mover::Mover;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, LayerId, ObjId, WorldId};
use serde_json::{Value, json};

/// The most targets any group command accepts on the game thread. The configured cap (`admin.max_objects_per_action`,
/// default 500, at most 10,000) is enforced by the HTTP layer with `413 too_many_targets`; this is the ceiling of the
/// configuration, kept here so no caller can hand the tick an unbounded list.
pub const GROUP_HARD_CAP: usize = 10_000;

/// A short failure.
pub fn fail(code: AdminErrorCode, message: impl Into<String>) -> AdminFailure {
    AdminFailure::new(code, message)
}

/// The answer of a request that failed as a whole.
pub fn failed(code: AdminErrorCode, message: impl Into<String>) -> AdminOutput {
    AdminOutput::Failed(fail(code, message))
}

/// A group command needs at least one and at most [`GROUP_HARD_CAP`] targets.
pub fn check_group(len: usize, what: &str) -> Result<(), AdminFailure> {
    if len == 0 {
        return Err(fail(AdminErrorCode::Invalid, format!("no {what} given")));
    }
    if len > GROUP_HARD_CAP {
        return Err(fail(
            AdminErrorCode::TooManyTargets,
            format!("at most {GROUP_HARD_CAP} {what} per command"),
        ));
    }
    Ok(())
}

/// The answer of a group command: one entry per target in request order.
pub fn report(per_target: Vec<TargetResult>) -> AdminOutput {
    AdminOutput::Report { per_target }
}

/// A place in the world as the audit and the answers show it: `{world, layer, pos: [x, y, z]}`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    /// World.
    pub world: WorldId,
    /// Layer.
    pub layer: LayerId,
    /// Position (metres).
    pub pos: Vec3,
}

impl Place {
    /// The place of a mover.
    pub fn of(m: &Mover) -> Place {
        Place {
            world: m.core.world,
            layer: m.core.layer,
            pos: m.core.pos,
        }
    }

    /// The JSON object.
    pub fn json(&self) -> Value {
        json!({
            "world": self.world.0,
            "layer": self.layer.0,
            "pos": [self.pos.x, self.pos.y, self.pos.z],
        })
    }
}

/// Rank protection (25 §1.6, §12 rule 2): a staff member may not act on a player whose authority is not below its
/// own letter. The owners, the console and the CLI are exempt, and so are monsters and the other objects (they have no
/// authority). A staff member's **own** characters are exempt too, so that the toggles of the acting character
/// ("invisible", "undying") work.
pub fn check_rank(actor: &AdminActor, ctx: &GameCtx<'_>, obj: ObjId) -> Result<(), AdminFailure> {
    let AdminActor::Staff { account_id, .. } = actor else {
        return Ok(());
    };
    let Some(entry) = ctx.sessions.get(obj) else {
        return Ok(());
    };
    if entry.account == *account_id || entry.authority < actor.authority() {
        Ok(())
    } else {
        Err(fail(
            AdminErrorCode::TargetOutranks,
            format!(
                "character {} has an authority that is not below yours",
                entry.character.0
            ),
        ))
    }
}

/// The avatar of an online character: `NotFound` for an id nobody has, `CharacterOnline` ("is offline") for a
/// character that is not in the world.
pub fn online_player(
    svc: &Services,
    ctx: &GameCtx<'_>,
    id: CharacterId,
) -> Result<PlayerRef, AdminFailure> {
    let Some(entry) = svc.presence.get(id) else {
        return Err(fail(
            AdminErrorCode::NotFound,
            format!("character {} does not exist", id.0),
        ));
    };
    match entry.online {
        Some(o) if ctx.worlds.player(o.obj).is_some_and(|m| !m.core.deleted) => {
            Ok(PlayerRef(o.obj))
        }
        _ => Err(fail(
            AdminErrorCode::CharacterOnline,
            format!("character {} ({}) is not in the world", id.0, entry.name),
        )),
    }
}

/// A live (not deleted) object, else `StaleTarget`: the console looked at a map that is out of date.
pub fn live_core<'a>(
    ctx: &'a GameCtx<'_>,
    obj: ObjId,
) -> Result<&'a crate::world::ObjCore, AdminFailure> {
    match ctx.worlds.core(obj) {
        Some(c) if !c.deleted => Ok(c),
        _ => Err(fail(
            AdminErrorCode::StaleTarget,
            format!("object {} is gone", obj.0),
        )),
    }
}

/// The character id of a player avatar (`None` for anything else).
pub fn character_of(ctx: &GameCtx<'_>, obj: ObjId) -> Option<CharacterId> {
    ctx.sessions.get(obj).map(|e| e.character)
}

/// The target id an answer uses for a character.
pub fn character_target(id: CharacterId) -> TargetId {
    TargetId::Character(id)
}
