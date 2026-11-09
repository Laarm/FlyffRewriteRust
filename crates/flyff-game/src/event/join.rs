//! `event::join`: the join-batch contributions of doc 22 (Plan/22 §19.2, 15 §9.2).
//!
//! | step | message | WP |
//! |---|---|---|
//! | [`JoinStep::FlyffEvent`] (position 14) | `FlyffEvent{flags}`: the 1024 raw bytes of the store | WP-EVENT-02 |
//! | [`JoinStep::EventLuaDesc`] | `EventLuaDesc{2, desc}` per running event ([`super::desc::EventLuaDescStep`]) | WP-EVENT-04 |
//!
//! The other steps of this doc (`EventLuaDesc`, `GameRate`, the lord, secret-room, tax, rainbow and guild-house
//! parts) are added with their WPs; a step whose feature is not implemented has no contributor and is skipped.

use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_proto::pb;

/// `FlyffEvent` (11 §2.5 position 14, legacy `CUser::AddFlyffEvent`): the raw array of the event flags, 1024 bytes
/// in the legacy `CFlyffEvent` layout.
pub struct FlyffEventStep;

impl JoinContributor for FlyffEventStep {
    fn step(&self) -> JoinStep {
        JoinStep::FlyffEvent
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        out.to_session(
            cx.session,
            0,
            pb::FlyffEvent {
                flags: cx.svc.event.flags.bytes().to_vec().into(),
            },
        );
    }
}
