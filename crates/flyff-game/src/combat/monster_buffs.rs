//! Monster buffs (Plan/17 §7.12; WP-COMBAT-14; `__MONSTER_BUFFS`, `src:WORLDSERVER/AttackArbiter.cpp:908-931`):
//! killing a monster listed in `MonsterBuffs.inc` buffs the killer's whole party.

use super::hooks::KillCtx;
use crate::ctx::{GameCtx, Services};
use flyff_common::ObjId;

/// Step 8 of `OnDied` (`DeathHooks::monster_buffs`), with `features.monster_buffs`: when the dead mover's id has a
/// pair in `MonsterBuffs.inc` (the first pair wins) and the item prop exists, every member of the killer's party
/// **online anywhere** (any world, any distance, dead or alive: the original filters nothing) gets the item buff
/// (`DoApplySkill( member, prop, NULL )`); a killer without a party gets it alone. A killer that is not a player
/// (a monster or pet) has no party and receives the buff itself, as the original did.
pub fn apply(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    if !ctx.cfg.rules.features.monster_buffs {
        return;
    }
    let Some(victim) = ctx.worlds.mover(k.victim) else {
        return;
    };
    let Some(item) = ctx.data.monster_buffs.item_for(victim.prop_id()) else {
        return;
    };
    if ctx.data.item(item).is_none() {
        return;
    }
    let recipients: Vec<ObjId> = match ctx
        .worlds
        .player(k.attacker)
        .and_then(|a| a.player())
        .and_then(|p| svc.social.party.party_record_of(p.ident.character_id))
    {
        Some(party) => party.online_objs().collect(),
        None => vec![k.attacker],
    };
    for member in recipients {
        // an invalid object (a member that just left) is skipped by the buff engine
        let _ = crate::buff::apply_item(svc, ctx, member, item, None);
    }
}
