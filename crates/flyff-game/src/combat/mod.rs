//! Combat (Plan/17): the per-mover combat state, the hate list, the damage pipeline `CAttackArbiter` and the
//! seams its later work packages plug into.
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`state`] | `CombatState`, `ActParams`, the attack queues, the accessors over the sim-owned attack fields | COMBAT-04 |
//! | [`hate`] | `HateList`, `remove_enemy`, `remove_all_enemies` | COMBAT-04 |
//! | [`effects`] | `Arena` (the world of one call), `Effects` (messages, AI mail, sim calls), delivery | COMBAT-04 |
//! | [`mod@env`] | `DamageEnv` (rule tables, switches, events), `ArbiterCx` | COMBAT-04 |
//! | [`view`] | the `Mover` -> `flyff_rules::combat::Combatant` adapter, the worn-item reader | COMBAT-04 |
//! | [`mod@hit_type`] | `hit_type`, `is_attackable_npc` | COMBAT-04 (PvP: COMBAT-12) |
//! | [`pvp`] | the PvP hit types, region inspection, pink / chaotic / propensity, `SubPVP`, the PK cost drop | COMBAT-12 |
//! | [`duel`] | solo and party duels: requests, countdown, results, cancel rules | COMBAT-13 |
//! | [`hooks`] | `CombatHooks` / `ProcHook` / `KillCtx`, `CombatService` (`Services.combat`) | COMBAT-04 |
//! | [`arbiter`] | `process_damage`, `send_damage[_force\|_around[_point]]`, `on_damage_msg`, ... | COMBAT-04 |
//!
//! The pure formulas are `flyff_rules::combat`; this module owns the state around them. The attack
//! requests, the hit-frame handlers, the projectiles, death and revival (WP-COMBAT-05 .. 09) build on it.

pub mod action;
pub mod arbiter;
pub mod context;
pub mod death;
pub mod dps;
pub mod duel;
pub mod effects;
pub mod env;
pub mod hate;
pub mod hit_type;
pub mod hooks;
pub mod lodelight;
pub mod monster_buffs;
pub mod phase;
pub mod projectile;
pub mod projectile_kind;
pub mod pvp;
pub mod request;
pub mod skillhit;
pub mod state;
pub mod target;
pub mod tick;
pub mod view;
pub mod world;

pub use arbiter::{
    ApplyMask, AroundHit, AroundSpec, DamageRequest, around_candidates, damage_around_hits,
    is_steal, on_damage_msg, pre_process_damage, process_damage, send_damage, send_damage_around,
    send_damage_force, skill_attacked, with_arbiter,
};
pub use context::{ActionCx, StartRules, with_action, with_action_phase};
pub use death::{CombatLogout, DeathHooks, loot_owner};
pub use effects::{Arena, Effects, Emit, SimCtx, SimFx, apply_sim_fx_game};
pub use env::{ArbiterCx, DamageEnv};
pub use hate::{HateEntry, HateList, HitInfo, remove_all_enemies, remove_enemy};
pub use hit_type::{HitCheck, hit_type, hit_type_ex, is_attackable_npc};
pub use hooks::{
    ActiveSkill, AttrCache, CombatHooks, CombatService, KillCtx, MonsterSkillTrigger, NoHooks,
    ProcHook,
};
pub use phase::CombatConsumers;
pub use projectile::{HitPayload, ProjectileManager, ShotRequest, spawn_shot};
pub use skillhit::{NoSkillHits, SkillHitHooks};
pub use state::{
    ActMsgQueue, ActParams, CombatState, DpsMeter, DpsTrack, DuelRequest, DuelState, ObjAct,
    PkState, PkTimers, QueuedActMsg, ResurrectionOffer, atk_cnt, is_chaotic, is_die,
    is_invulnerable, is_live, is_peaceful, set_atk_cnt,
};
pub use view::{WornItem, attack_info, combatant, worn_item};
pub use world::{ActionConfig, CombatEnv, CombatQueues, CombatStats, CombatWorld};
