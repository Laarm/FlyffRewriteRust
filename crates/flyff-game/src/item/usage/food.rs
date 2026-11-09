//! `use_food`: the HP / MP / FP recovery of food, potions and refreshers (Plan/19 §5.3;
//! src:_Common/MoverSkill.cpp:3072-3156, `__PROP_0827` = 3 parameters, `__REMOVE_MAX_RECOVER_RATE` active).

use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::mover::Mover;
use crate::stats::env::{max_of, point_param};
use crate::stats::pool::set_point_param;
use flyff_common::defs::item::II_GEN_FOO_INS_SUPERHOTDOG;
use flyff_data::props::item::ItemProp;
use flyff_rules::consts::{NULL_ID, dst::*};
use flyff_rules::regen::potion_restore;

/// What `use_food` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoodResult {
    /// The recovery happened; the item is used up.
    Used,
    /// Nothing happened and nothing is consumed (the SM food is refused, its text sent; a timed food that is
    /// none of the SM foods answers nothing: `DoUseItemFood` returned `FALSE`).
    Refused,
}

/// `DoUseItemFood`: `II_GEN_FOO_INS_SUPERHOTDOG` fills HP, MP and FP. Otherwise for each of the three prop
/// slots with a DST: a timed item (`circle_time != NULL_ID`) is the SM-mode food (`stats::sm::use_sm_food`,
/// WP-ITEM-21; one call per DST slot as the original, so a second slot meets the running mode); an
/// `HP`/`MP`/`FP` slot restores `adj` through the potion rule (`point_param + adj`, the pool setter clamps);
/// any other DST would be applied as a skill effect (ITEM-20) and is skipped.
pub fn use_food(m: &mut Mover, env: &ItemEnv<'_>, prop: &ItemProp, fx: &mut Fx) -> FoodResult {
    let stats = env.stats();
    if prop.id == II_GEN_FOO_INS_SUPERHOTDOG {
        for dst in [DST_HP, DST_MP, DST_FP] {
            let max = max_of(m, stats, dst);
            set_point_param(m, stats, dst, max, false, &mut fx.out);
        }
        return FoodResult::Used;
    }
    for i in 0..3 {
        let dst = prop.dest_param.get(i).copied().unwrap_or(NULL_ID);
        if dst == NULL_ID {
            continue;
        }
        if prop.circle_time != NULL_ID {
            match crate::stats::sm::use_sm_food(m, env, prop, fx) {
                crate::stats::sm::SmUse::Used => continue,
                _ => return FoodResult::Refused,
            }
        }
        if matches!(dst, DST_HP | DST_MP | DST_FP) {
            let cur = point_param(m, stats, dst);
            let point = prop.adj_param_val.get(i).copied().unwrap_or(0);
            let max = max_of(m, stats, dst);
            let (value, text) = potion_restore(
                dst,
                cur,
                point,
                max,
                prop.ability_min as i32,
                stats.cfg.remove_max_recover_rate,
            );
            if let Some(tid) = text {
                fx.text(tid);
            }
            set_point_param(m, stats, dst, value, false, &mut fx.out);
        }
    }
    FoodResult::Used
}
