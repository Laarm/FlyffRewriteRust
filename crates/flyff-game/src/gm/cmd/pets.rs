//! The pet GM commands (Plan/23 §4.6, §7; WP-GM-15): `/petlevel` (row 91), `/petexp` (92), `/Pet` (94) and
//! `/SetPetName` (104).
//!
//! They work on the **summoned system pet** (`PlayerItems.pet_obj_id`, the egg item that carries the `PetState`)
//! through the pet module of doc 19 (`item::pets::system`), so the item id, the stat bonus, the messages and the
//! `pet.level_up` audit line stay in step with the normal level-up. Like the legacy handlers they are silent
//! when there is no pet or an argument is bad; only `/SetPetName` answers (`TID_GAME_NAME_PET_E01`, the
//! pet-name rules of 23 §7, PL-116).

use crate::buff::core::BuffCx;
use crate::ctx::PlayerRef;
use crate::gm::target::{self, Target};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::item::fx::Fx;
use crate::item::model::PetLevel;
use crate::item::pets::system::hooks::with_player;
use crate::item::pets::system::{
    EV_LEVEL_UP, PetCause, active_pet, apply_avail, audit_pet, item_mut, level_up, pet_mut,
    remove_bonus,
};
use crate::mover::Mover;
use flyff_common::defs::text::TID_GAME_NAME_PET_E01;
use flyff_proto::pb;
use flyff_rules::item::pet::{MAX_PET_EXP, PF_PET_GET_AVAIL, PL_S, max_exp};
use flyff_rules::names::{NameKind, NameLimits, validate_name};

/// `PK_MAX`: the number of pet kinds (`Resource/define.h`); `/Pet` takes `0..PK_MAX`.
pub const PK_MAX: u8 = 7;
/// The most life `/Pet` sets.
pub const MAX_LIFE: u8 = 99;

fn me(cx: &CmdCtx<'_, '_>) -> Result<PlayerRef, CmdError> {
    cx.self_player.ok_or(CmdError::NeedsSelf)
}

/// C `atoi`: the leading integer of `s` (blanks skipped, an optional sign), 0 without one, saturating.
fn atoi(s: &str) -> i32 {
    let t = s.trim_start();
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let mut v: i64 = 0;
    for c in rest.chars().take_while(char::is_ascii_digit) {
        v = (v * 10 + i64::from(c as u8 - b'0')).min(i64::from(i32::MAX) + 1);
    }
    let v = if neg { -v } else { v };
    i32::try_from(v).unwrap_or(if neg { i32::MIN } else { i32::MAX })
}

/// `BYTE b = atoi( token )`: the number truncated to a byte.
fn byte_of(token: &str) -> u8 {
    (atoi(token) & 0xFF) as u8
}

/// Row 91 `/petlevel (pl)` (`TextCmd_PetLevel`): when the summoned pet's EXP bar is full (100 %) it levels up
/// (an egg hatches). A pet of grade S cannot level (the legacy `IncLevel` had no cap; FIX of pets-core).
pub fn pet_level(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let _ = with_player(cx.svc, cx.ctx, who, |m, pcx, rng, fx| {
        let full = active_pet(m, pcx.data())
            .is_some_and(|(_, _, p)| u64::from(p.exp) * 100 / u64::from(p.max_exp().max(1)) == 100);
        if full {
            level_up(m, pcx, rng, PetCause::Gm, fx);
        }
    });
    Ok(CmdDone::default())
}

/// Row 92 `/petexp (pe)` (`TextCmd_PetExp`): the summoned pet's EXP becomes the maximum of its grade (an egg
/// 50 000, the others 100 000) unless it is grade S; `PetSetExp` tells the owner.
pub fn pet_exp(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let _ = with_player(cx.svc, cx.ctx, who, |m, pcx, _, fx| {
        let Some((id, _, pet)) = active_pet(m, pcx.data()) else {
            return;
        };
        if pet.level == PetLevel::S {
            return;
        }
        if let Some(p) = pet_mut(m, id) {
            p.set_exp(MAX_PET_EXP);
            fx.out.to_self(pb::PetSetExp { exp: p.exp });
        }
    });
    Ok(CmdDone::default())
}

/// What `/Pet` writes into the pet (all values already checked).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PetSpec {
    /// The kind, `0..PK_MAX`.
    pub kind: u8,
    /// The grade, `0..=5` (the number of digits of the ability argument).
    pub level: u8,
    /// The EXP as a percentage of the grade's maximum (a byte; the item layer clamps the result).
    pub exp_rate: u8,
    /// The stat roll of the grades D..S (`1..=9` each, only the first `level` count).
    pub avail: [u8; 5],
    /// The life, at most 99.
    pub life: u8,
}

/// `TextCmd_Pet` on the summoned pet of one player: the kind, the grade, full energy, the EXP, the stat rolls
/// and the life are written; the item takes the id of the kind (a kind without an item row keeps the old id);
/// the bonus is taken off (`RemovePet`), `pet.level_up{cause: "gm"}` is audited, the owner gets
/// `Pet{PF_PET_GET_AVAIL}` and everybody else around `PetLevelup`, the bonus is applied again and an urgent
/// snapshot is asked for. `false` (nothing changed) without a summoned pet.
pub fn rewrite_pet(m: &mut Mover, pcx: &BuffCx<'_>, s: &PetSpec, fx: &mut Fx) -> bool {
    let data = pcx.data();
    let Some((id, _, _)) = active_pet(m, data) else {
        return false;
    };
    let tables = &data.pet;
    let new_item = {
        let Some(p) = pet_mut(m, id) else {
            return false;
        };
        p.kind = if s.level == 0 { 0 } else { s.kind };
        p.level = PetLevel::from_u8(s.level);
        let max_energy = u32::from(p.max_energy(tables));
        p.set_energy(tables, max_energy);
        let exp = u64::from(max_exp(s.level)) * u64::from(s.exp_rate) / 100;
        p.exp = 0;
        p.set_exp(u32::try_from(exp).unwrap_or(u32::MAX));
        for (i, slot) in p.avail_levels.iter_mut().enumerate() {
            // the stat roll of grade i + 1: the digit while the grade is reached, else 0
            *slot = if i < usize::from(s.level) {
                s.avail.get(i).copied().unwrap_or(0)
            } else {
                0
            };
        }
        p.life = u16::from(s.life);
        p.item_id(tables)
    };
    if new_item != 0
        && let Some(item) = item_mut(m, id)
    {
        item.item_id = new_item;
    }
    remove_bonus(m, pcx, fx);
    audit_pet(fx, EV_LEVEL_UP, m, id, Some(PetCause::Gm), &[]);
    if let Some((_, _, p)) = active_pet(m, data) {
        fx.out.to_self(pb::Pet {
            pet: Some(p.to_pb()),
            flag: PF_PET_GET_AVAIL,
        });
        fx.out.near_excl(pb::PetLevelup {
            pet_id: p.levelup_word(tables),
        });
    }
    fx.mark_urgent();
    apply_avail(m, pcx, fx);
    true
}

/// Row 94 `/Pet <player> <kind> <expRate%> <availDigits> <life>` (`TextCmd_Pet`) rewrites the summoned pet of the
/// **online** player: `kind < PK_MAX` (a byte), the EXP is `expRate` percent of the grade's maximum, the level
/// is the number of digits of `availDigits` (at most five) and each digit `1..=9` is the stat roll of the
/// grades D, C, ... in turn, the energy is full, the life is `life` (at most 99). The bonus is taken off and
/// applied again (the legacy `RemovePet` / `AddPet`), the owner gets `Pet{PF_PET_GET_AVAIL}`, everybody else
/// around `PetLevelup`, and `pet.level_up{cause: "gm"}` is audited. An unknown or offline player, a target
/// without a summoned pet and any argument that breaks a rule end the command silently.
pub fn pet(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    if args.finished() {
        return Ok(CmdDone::default());
    }
    let (target_ref, stored) = match target::find(cx.svc, &name) {
        Target::Online { player, name, .. } => (player, name),
        _ => return Ok(CmdDone::default()),
    };
    let has_pet = cx
        .ctx
        .player(target_ref)
        .is_some_and(|m| active_pet(m, cx.ctx.data).is_some());
    if !has_pet {
        return Ok(CmdDone::default());
    }
    let kind_token = args.token().to_owned();
    if args.finished() {
        return Ok(CmdDone::default());
    }
    let kind = byte_of(&kind_token);
    if kind >= PK_MAX {
        return Ok(CmdDone::default());
    }
    let rate_token = args.token().to_owned();
    if args.finished() {
        return Ok(CmdDone::default());
    }
    let avail_token = args.token().to_owned();
    if args.finished() {
        return Ok(CmdDone::default());
    }
    let level = avail_token.len();
    if level > usize::from(PL_S) {
        return Ok(CmdDone::default());
    }
    let mut avail = [0_u8; 5];
    for (slot, c) in avail.iter_mut().zip(avail_token.chars()) {
        let digit = c
            .to_digit(10)
            .and_then(|d| u8::try_from(d).ok())
            .unwrap_or(0);
        if !(1..=9).contains(&digit) {
            return Ok(CmdDone::default());
        }
        *slot = digit;
    }
    let life_token = args.token().to_owned();
    if args.finished() {
        return Ok(CmdDone::default());
    }
    let life = byte_of(&life_token).min(MAX_LIFE);
    let spec = PetSpec {
        kind,
        level: level as u8,
        exp_rate: byte_of(&rate_token),
        avail,
        life,
    };
    let _ = with_player(cx.svc, cx.ctx, target_ref, |m, pcx, _, fx| {
        rewrite_pet(m, pcx, &spec, fx);
    });
    Ok(CmdDone::on(stored))
}

/// Row 104 `/SetPetName (setpetname) <name>` (`TextCmd_SetPetName`): the summoned pet of the issuer is renamed
/// and `SetPetName` goes to everybody around. FIX (PL-116): the name must pass the pet-name rules of 23 §7
/// (1 to 16 characters, `InvalidName` and the allowed letters), else `TID_GAME_NAME_PET_E01` and nothing
/// changes (the legacy stored whatever was typed). Without a summoned pet nothing happens.
pub fn set_pet_name(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let raw = args.token().to_owned();
    let _ = with_player(cx.svc, cx.ctx, who, |m, pcx, _, fx| {
        let data = pcx.data();
        let Some((id, _, _)) = active_pet(m, data) else {
            return;
        };
        let Ok(name) = validate_name(
            NameKind::PetName,
            &raw,
            &data.name_rules,
            NameLimits::default(),
            None,
        ) else {
            fx.text(TID_GAME_NAME_PET_E01);
            return;
        };
        if let Some(p) = pet_mut(m, id) {
            p.set_name(&name);
            fx.out.near(pb::SetPetName {
                name: p.name.clone(),
            });
        }
    });
    Ok(CmdDone::default())
}
