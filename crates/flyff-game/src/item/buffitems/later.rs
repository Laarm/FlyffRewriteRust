//! What the item rows of this module cannot do on one `&mut Mover`: they queue a [`Later`] in the
//! [`Fx`] (as [`Followup::BuffItem`](crate::follow::Followup)) and the flush runs it with the
//! services, in order, right after the messages.

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::follow::Followup;
use crate::item::fx::Fx;
use crate::item::usage::blink::RETURN_VILLAGES;
use crate::world::ReplaceKind;
use flyff_common::{LayerId, ObjId};

/// One deferred action of an item row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Later {
    /// `DoApplySkill(target, item_prop, NULL)`: the item buff of `item` on `target` (17 §6.7).
    ApplyItem {
        /// Who gets the buff (the user).
        target: ObjId,
        /// The item prop that is applied (an EXP scroll applies its tier, not the scroll that was used).
        item: u32,
    },
    /// `DoActiveSkill(skill, level, target)` of an `IK2_SKILL` item (17 §6.9).
    ItemSkill {
        /// The user.
        player: ObjId,
        /// The skill.
        skill: u32,
        /// Its level.
        level: u32,
        /// The target (the user itself for a `WUI_NOW` skill).
        target: ObjId,
    },
    /// `II_SYS_SYS_SCR_SELPAR`: the party becomes level 10 with 180 points and 0 EXP (21 §4.5).
    PartyLevel {
        /// The user.
        player: ObjId,
    },
    /// The second half of the return scroll: the teleport to the revival point of the village chosen with
    /// `ReturnScrollReq` (`REPLACE(.., GetRevivalPos(world, village), REPLACE_NORMAL)`).
    ReturnVillage {
        /// The user.
        player: ObjId,
    },
}

impl Later {
    /// Replaces the mover id `from` by `to` (a join builds its follow-ups before the object store has given the
    /// player its id).
    pub fn rebind(&mut self, from: ObjId, to: ObjId) {
        let fix = |id: &mut ObjId| {
            if *id == from {
                *id = to;
            }
        };
        match self {
            Later::ApplyItem { target, .. } => fix(target),
            Later::ItemSkill { player, target, .. } => {
                fix(player);
                fix(target);
            }
            Later::PartyLevel { player } | Later::ReturnVillage { player } => fix(player),
        }
    }

    /// Executes the action with the services.
    pub fn run(self, svc: &mut Services, ctx: &mut GameCtx<'_>) {
        match self {
            Later::ApplyItem { target, item } => {
                if let Err(e) = crate::buff::apply_item(svc, ctx, target, item, None) {
                    tracing::debug!(item, mover = target.0, error = ?e, "an item buff was not applied");
                }
            }
            Later::ItemSkill {
                player,
                skill,
                level,
                target,
            } => {
                crate::skill::apply::do_active_skill(svc, ctx, player, skill, level, target, false)
            }
            Later::PartyLevel { player } => party_level(svc, ctx, PlayerRef(player)),
            Later::ReturnVillage { player } => return_village(ctx, player),
        }
    }
}

impl Fx {
    /// Queues a [`Later`] item action.
    pub fn later(&mut self, l: Later) {
        self.then(Followup::BuffItem(l));
    }
}

/// `pParty->SetPartyLevel(user, 10, 180, 0)`.
fn party_level(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    use crate::social::party::{points::set_party_level, skills::refresh_exp};
    let Some(ch) = ctx
        .worlds
        .player(who.0)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
    else {
        return;
    };
    let Some(pid) = svc.social.party.party_of(ch) else {
        return;
    };
    set_party_level(svc, ctx, pid, flyff_rules::social::MAX_PARTY_LEVEL, 180);
    if let Some(p) = svc.social.party.get_mut(pid) {
        p.exp = 0;
    }
    refresh_exp(svc, ctx, pid);
}

/// The village teleport of the return scroll. Nothing happens when no village was chosen or the world has no
/// revival point with that key (the buff and the scroll are gone anyway, as in the original).
fn return_village(ctx: &mut GameCtx<'_>, mover: ObjId) {
    let Some(m) = ctx.worlds.mover(mover) else {
        return;
    };
    let world = m.core.world;
    let village = m
        .player()
        .and_then(|p| p.misc.return_village)
        .and_then(|v| RETURN_VILLAGES.get(usize::from(v)).copied());
    let Some(key) = village else {
        tracing::warn!(
            mover = mover.0,
            "the return scroll was used without a chosen village"
        );
        return;
    };
    let Some(dest) = ctx.worlds.revival.revival_pos(world, key) else {
        return;
    };
    if let Err(e) = ctx
        .worlds
        .replace(mover, dest.world, dest.pos, LayerId(0), ReplaceKind::Normal)
    {
        tracing::debug!(
            mover = mover.0,
            "the return scroll teleport was refused: {e}"
        );
    }
}
