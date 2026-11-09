//! `CoupleNtf` in the join batch (Plan/21 §11.5, 11 §2.5 step 16): the experience and the two partners of the couple
//! of the joining character, nothing for a character without one. The re-attach replay writes it again.

use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_proto::pb;

/// The join step of `CCouple::Serialize` (`CUser::AddCouple`).
pub struct CoupleStep;

impl JoinContributor for CoupleStep {
    fn step(&self) -> JoinStep {
        JoinStep::Couple
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
        let Some(c) = cx.svc.social.couple.get(character) else {
            return;
        };
        out.to_session(
            cx.session,
            0,
            pb::CoupleNtf {
                couple: Some(pb::Couple {
                    experience: c.experience,
                    id_first: c.first.0,
                    id_second: c.second.0,
                }),
            },
        );
    }
}
