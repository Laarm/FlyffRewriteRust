//! `HousingAllInfo` in the join batch (Plan/22 §13.2, §19.2; 11 §2.5; TQ-11-14).
//!
//! The step sits between `GuildHouseAllInfo` and `CampusUpdate` and is written ONLY for a room with an entry or a
//! visitor (deviation from the original, which sent an empty message to everybody; open question 2): the client's store
//! starts empty per session, and the idle join batch of every exact assertion stays unchanged. A re-attach replays it
//! like every step.

use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_proto::pb;

/// The join step `HousingAllInfo` (`CUser::AddHousingAllInfo`).
pub struct HousingAllInfoStep;

impl JoinContributor for HousingAllInfoStep {
    fn step(&self) -> JoinStep {
        JoinStep::HousingAllInfo
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(owner) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return;
        };
        let Some(room) = cx.svc.event.housing.room(owner) else {
            return;
        };
        // an entry whose list-up has not answered is not announced yet (`HousingFurnitureList` follows)
        let items: Vec<pb::HousingInfo> = room
            .items
            .iter()
            .filter(|i| !i.pending)
            .map(|i| i.info())
            .collect();
        if items.is_empty() && room.visitors.is_empty() {
            return;
        }
        out.to_session(
            cx.session,
            0,
            pb::HousingAllInfo {
                items,
                visitors: room.visitors.iter().map(|c| c.0).collect(),
            },
        );
    }
}
