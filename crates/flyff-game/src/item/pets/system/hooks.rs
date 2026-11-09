//! The `GameCtx` face of the system pets (Plan/19 §13, §20.5, WP-ITEM-16): the per-tick job, the death hooks, the
//! join, and the client handlers `PetReleaseReq`, `ClearPetName` and `DoUseItemInput`.

use super::active_pet;
use super::exp::{exp_on_kill, on_owner_killed};
use super::process::process;
use super::scrolls::clear_name;
use super::summon::{apply_avail, release};
use crate::buff::core::BuffCx;
use crate::buff::set::BuffKind;
use crate::combat::hooks::KillCtx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::usage;
use crate::mover::Mover;
use flyff_common::ObjId;
use flyff_common::defs::text::TID_GAME_PET_NOT_FOUND;
use flyff_common::rng::GameRng;
use flyff_data::GameData;
use flyff_proto::pb;

/// Runs `f` on a live player with the buff context, the world's random stream and a fresh [`Fx`], then flushes
/// the effects. `None` when `who` is not a live player.
pub(crate) fn with_player<R>(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    f: impl FnOnce(&mut Mover, &BuffCx<'_>, &mut dyn GameRng, &mut Fx) -> R,
) -> Option<R> {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let cx = BuffCx::new(env, ctx.clock.game_ms);
    let world = ctx.worlds.core(who.0)?.world;
    let mut fx = Fx::new();
    let r = {
        let m = ctx.worlds.player_mut(who.0)?;
        f(m, &cx, ctx.rng.world(world), &mut fx)
    };
    fx.flush(svc, ctx, who);
    Some(r)
}

/// The per-tick job (`item::tick::process` calls it): every player with a summoned pet whose phase it is
/// (`(tick + id) % 16 == 0`) runs `ProcessPet`.
pub fn tick(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let tick = ctx.clock.tick.0;
    let due: Vec<ObjId> = ctx
        .sessions
        .iter()
        .map(|e| e.player)
        .filter(|id| {
            (tick.wrapping_add(u64::from(id.0))) & u64::from(super::process::PASS_MASK) == 0
                && ctx
                    .worlds
                    .player(*id)
                    .and_then(|m| m.player())
                    .is_some_and(|p| p.items.pet_obj_id.is_some())
        })
        .collect();
    for id in due {
        let counter = (tick.wrapping_add(u64::from(id.0))) as u32;
        with_player(svc, ctx, PlayerRef(id), |m, cx, _, fx| {
            process(m, cx, counter, fx);
        });
    }
}

/// `DeathHooks::pet_exp` (`CAttackArbiter::OnDied` step 6, monster victims only): the killer's summoned pet earns
/// the EXP of the kill (`AddPetExpOnKill( monster class, monster level, player level )`).
pub fn pet_exp_hook(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let Some(victim) = ctx.worlds.mover(k.victim) else {
        return;
    };
    let class = victim.monster().map_or(0, |mo| mo.class);
    let level = victim.mv.level;
    with_player(svc, ctx, PlayerRef(k.attacker), |m, cx, rng, fx| {
        exp_on_kill(m, cx, rng, class, level, fx);
    });
}

/// `DeathHooks::owner_pets` (`DoDie` step 5, a player killed by an NPC): see [`on_owner_killed`].
pub fn owner_pets_hook(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    with_player(svc, ctx, PlayerRef(k.victim), |m, cx, _, fx| {
        on_owner_killed(m, cx, fx);
    });
    // the eat pet's part of the step (`InactivateEatPet`, `features.reactivate_eatpet`; ITEM-18)
    crate::item::pets::eat::on_owner_killed(svc, ctx, k.victim);
}

/// The load rule of 13 §17.4 step 2 and the join of the original (`if( !GetPet() ) SetPetId( NULL_ID )`): a
/// summoned slot that does not hold a pet item is cleared, and with it the stale `BUFF_PET` entry. Runs before
/// the buffs of the join are applied (`stats::join::apply_loaded`).
pub fn on_player_load(m: &mut Mover, data: &GameData) {
    if active_pet(m, data).is_some() {
        return;
    }
    if let Some(p) = m.player_mut() {
        p.items.pet_obj_id = None;
    }
    let stale: Vec<_> = m
        .buffs
        .iter()
        .map(|(k, _)| k)
        .filter(|k| k.kind == BuffKind::Pet)
        .collect();
    for key in stale {
        m.buffs.erase(key);
    }
}

/// After the join batch (13 §17.4): the summoned system pet is announced to its owner again (`PetCall`, header =
/// the owner; the others learn of the pet from `PlayerOther.pet_id` of the spawn snapshot, and the owner is not
/// among its own viewers before the first world phase) and its bonus is made sure of (the buff comes with the
/// character; the lines of an awakening are applied here).
pub fn after_join(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    with_player(svc, ctx, who, |m, cx, _, fx| {
        let data = cx.data();
        let Some((id, _, pet)) = active_pet(m, data) else {
            return;
        };
        fx.out.to_self_subject(pb::PetCall {
            item_obj_id: u32::from(id),
            mover_index: pet.mover_index(&data.pet),
            level: u32::from(pet.level_u8()),
            name: pet.name.clone(),
        });
        apply_avail(m, cx, fx);
    });
}

/// `PetReleaseReq`: needs a summoned pet (`TID_GAME_PET_NOT_FOUND`), then `PetRelease`.
pub fn on_pet_release_req(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    with_player(svc, ctx, who, |m, cx, _, fx| {
        if m.player().is_some_and(|p| p.items.pet_obj_id.is_some()) {
            release(m, cx, fx);
        } else {
            fx.text(TID_GAME_PET_NOT_FOUND);
        }
    });
}

/// `ClearPetName`: see [`clear_name`].
pub fn on_clear_pet_name(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    with_player(svc, ctx, who, |m, cx, _, fx| clear_name(m, cx, fx));
}

/// `MAX_INPUT_LEN - 1`: the most bytes of the text of a `DoUseItemInput`.
pub const MAX_INPUT_BYTES: usize = 99;

/// `DoUseItemInput{data, input}` (`__AZRIA_1023`, Plan/19 §5.9): the text becomes the player's input
/// (`SetInput`), the item is used (`OnDoUseItem( data, 0, -1 )`) and the input is cleared again
/// (`ResetInput`). The consumers of the text are the pet naming scroll (`scrolls::name_pet`) and, with ITEM-24,
/// the tickets.
pub fn on_do_use_item_input(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::DoUseItemInput,
) -> usage::UseOutcome {
    let Ok(id) = StorageId::try_from(msg.data >> 16) else {
        return usage::UseOutcome::Ignored;
    };
    let text = super::state::cut_name_to(&msg.input, MAX_INPUT_BYTES).to_owned();
    let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) else {
        return usage::UseOutcome::Ignored;
    };
    p.items.input = text;
    let out = usage::run_use(svc, ctx, who, id, -1);
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.items.input.clear();
    }
    out
}
