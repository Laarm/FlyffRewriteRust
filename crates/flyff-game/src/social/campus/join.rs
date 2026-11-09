//! `CampusUpdate` in the join batch (Plan/21 §12.4, 11 §2.5 last step; ruling 11 of B8).
//!
//! The original `OnJoin` sends the campus record of a member and NOTHING about the points: they ride in
//! `SpawnSelf.campus_point`. This deviates from 11 §2.5 / 21 §3.5 (always a `CampusUpdatePoint`) on purpose, so the
//! idle join batch stays unchanged. A character without a campus gets nothing; a campus id that no longer exists
//! was reset to 0 silently by `relation::on_join`.

use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_proto::pb;

/// The join step `CampusUpdate` (`CUser::AddUpdateCampus`).
pub struct CampusUpdateStep;

impl JoinContributor for CampusUpdateStep {
    fn step(&self) -> JoinStep {
        JoinStep::CampusUpdate
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(character) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return;
        };
        let Some(campus) = cx.svc.social.campus.campus_of(character) else {
            return;
        };
        out.to_session(
            cx.session,
            0,
            pb::CampusUpdate {
                campus: Some(campus.wire()),
            },
        );
    }
}
