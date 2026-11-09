//! Mover statistics (Plan/16 §1.3): the DST runtime, the derived-stat glue, regeneration and the per-second
//! timer, stat points and the mode word.
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`dst`] | `DstState`: the aggregate, the per-source ledger, bit reference counts, override stacks, the emissions | MOVER-05 |
//! | [`mod@env`] | `StatsCore` / `StatsService` (rule tables, switches, game data), `inputs`, the pool getters, `refresh_sim_params` | MOVER-04/05 |
//! | [`out`] | `Out`: the messages a stat function produces, flushed through the outbox or the world sink | MOVER-05 |
//! | [`pool`] | `set_point_param`, `inc_*`, the silent setters, `send_pools_absolute`, `emit_gold` | MOVER-05 |
//! | [`source`] | `apply_source` / `remove_source`: the mover-level face of the ledger | MOVER-05 |
//! | [`points`] | `inc_stat_level`, `modify_status`, `restat_*`: stat points and their messages | MOVER-07 |
//! | [`modes`] | `set_mode_bits`, `clear_mode_bits`, `emit_modify_mode`, the tutorial state | MOVER-12 |
//! | [`fame_fuel`] | `set_fame`, `report_fuel` (every 16 ticks), `emit_flight_level` | MOVER-21 |
//! | [`fashion`] | `HideCoatReq`, the render-mask and equip-denial toggles, `set_state_mode`, `set_state_not_mode`, scale and disguise messages | MOVER-17 |
//! | [`jobs`] | `add_change_job`, `init_level`, `change_job`, the `UpdateJob` handler (fast job change) | MOVER-18 |
//! | [`skills`] | `init_skill_exp`, `master_skill_up`, `legend_up_check`, `legend_skill_up` | MOVER-19 |
//! | [`handlers`] | the `IncStatLevel` / `ModifyStatus` / `TutorialState` / `ExpStop` entry points | MOVER-07/08/12 |
//! | [`tick`] | `process_recovery`, `add_kill_recovery`, `second_timer`, `StatsHooks` (the tick tail) | MOVER-06 |
//! | [`progression`] | `add_experience`, `grant_experience`, the level-up sequence, `set_level`, `add_gp`, `add_skill_points`, `add_fxp`, `toggle_exp_stop` | MOVER-08 |
//! | [`exp_share`] | `on_monster_killed`: the damage-ledger walk, `exp_solo`, the special-boss split `around_exp` | MOVER-09 |
//! | [`death_penalty`] | `apply`, `apply_for`, `logout_while_dead`, `StatsLogout` (the `DeadLogout` hook) | MOVER-10 |

pub mod death_penalty;
pub mod dst;
pub mod env;
pub mod equip;
pub mod equip_sets;
pub mod exp_share;
pub mod fame_fuel;
pub mod fashion;
pub mod handlers;
pub mod jobs;
pub mod join;
pub mod modes;
pub mod out;
pub mod points;
pub mod pool;
pub mod progression;
pub mod skills;
pub mod sm;
pub mod source;
pub mod tick;

pub use dst::{DstCtx, DstEmit, DstOp, DstSource, DstState};
pub use env::{ExpSwitches, StatsCore, StatsService};
pub use out::{Aud, Out, OutMsg};
