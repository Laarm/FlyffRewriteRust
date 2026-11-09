//! MP / FP costs and skill-point costs (Plan/17 §6.4; `MoverAttack.cpp:2318-2336`).

use crate::data::RulesData;
use crate::dst::DstTable;
use crate::stats;
use flyff_data::props::item::ItemProp;

pub use crate::combat::skillpower::refer_value;

/// `GetReqMp(n)`: the sentinel `9_999_999` ("all MP") is unchanged, else `n - n * param(MP_DEC_RATE, 0) / 100`.
pub fn req_mp(dst: &DstTable, n: i32) -> i32 {
    stats::req_mp(dst, n)
}

/// `GetReqFp(n)`: like [`req_mp`] with `DST_FP_DEC_RATE`.
pub fn req_fp(dst: &DstTable, n: i32) -> i32 {
    stats::req_fp(dst, n)
}

/// The skill points one level of `skill` costs (`Constant.inc`: vagrant, expert and pro skills pay 1, 2 and 3 by
/// default; masters and heroes pay nothing): the tier is the skill's `item_kind1` job type. The rule for whole
/// plans is `crate::skill_points::validate_skill_plan`.
pub fn skill_point_cost(d: &RulesData, skill: &ItemProp) -> u32 {
    use crate::consts::job::{JTYPE_BASE, JTYPE_EXPERT, JTYPE_PRO};
    match skill.item_kind1 {
        JTYPE_BASE => d.sp_cost[0],
        JTYPE_EXPERT => d.sp_cost[1],
        JTYPE_PRO => d.sp_cost[2],
        _ => 0,
    }
}
