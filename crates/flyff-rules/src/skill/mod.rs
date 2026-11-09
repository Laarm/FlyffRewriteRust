//! Skill helpers shared by the server and the browser (Plan/17 §6.4, §6.9; WP-COMBAT-01): the cost and the cast
//! arithmetic. The skill-point rule (`sp_cost`, `validate_skill_plan`) is `crate::skill_points`.

pub mod cast;
pub mod cost;
pub mod plan;

pub use cast::{casting_ani_speed, casting_ticks, casting_time};
pub use cost::{refer_value, req_fp, req_mp, skill_point_cost};
pub use plan::{SkillPlanError, validate_skill_plan};
