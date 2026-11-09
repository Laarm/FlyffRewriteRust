//! `event::combat`: the single event wrapper of the combat hooks (Plan/22 §4.10, KQ-32, WP-EVENT-06).
//!
//! [`EventCombatHooks`] wraps the hook set that is installed (`Game::new` wraps siege's `SiegeCombatHooks`) and
//! answers one question itself: `is_event_spawn_monster(id)` is true for the mover props that an Event.lua spawn
//! event has registered ([`EventSpawnFacts`]) or that the inner hooks name. Every other method of [`CombatHooks`] and
//! [`ProcHook`] is forwarded to the inner hooks, so the seams of the other documents keep working. The impls carry
//! `#[deny(clippy::missing_trait_methods)]`: a method added to the traits later cannot be dropped silently here.
//!
//! This is the only event wrapper of `worlds.movement.combat.hooks` (README ruling 9): a later doc-22 seam that
//! needs live state adds its fact to this type, it does not wrap again.
//!
//! The rule itself lives in the combat code that already asks the hook: a player's hit on such a monster always
//! lands (`roll_attack_result`), the arbiter assigns the damage 1 per hit unless the attacker is in GM one-kill
//! mode (`arbiter.rs`), and the drop has no level gap (`item/drop.rs`).

use super::spawns::EventSpawnFacts;
use crate::ai::monster_skill::MonsterCast;
use crate::combat::effects::{Arena, Effects};
use crate::combat::env::DamageEnv;
use crate::combat::hooks::{ActiveSkill, CombatHooks, KillCtx, MonsterSkillTrigger, ProcHook};
use crate::mover::Mover;
use crate::world::ObjectStore;
use flyff_common::ObjId;
use flyff_data::GameData;
use flyff_data::types::MoverId;
use flyff_rules::combat::{HitType, LinkAttackState};
use std::sync::Arc;

/// The event wrapper of the combat hooks (see the module documentation).
pub struct EventCombatHooks {
    inner: Box<dyn CombatHooks>,
    spawns: Arc<EventSpawnFacts>,
}

impl EventCombatHooks {
    /// Wraps `inner`; `spawns` is [`EventSpawnService::facts`](super::spawns::EventSpawnService::facts).
    pub fn new(inner: Box<dyn CombatHooks>, spawns: Arc<EventSpawnFacts>) -> EventCombatHooks {
        EventCombatHooks { inner, spawns }
    }
}

impl std::fmt::Debug for EventCombatHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EventCombatHooks(..)")
    }
}

#[deny(clippy::missing_trait_methods)]
impl ProcHook for EventCombatHooks {
    fn do_active_skill(&mut self, w: &mut Arena<'_>, fx: &mut Effects, skill: ActiveSkill) {
        self.inner.do_active_skill(w, fx, skill);
    }
}

#[deny(clippy::missing_trait_methods)]
impl CombatHooks for EventCombatHooks {
    fn hit_type_pvp(
        &self,
        att: &Mover,
        def: &Mover,
        is_target: bool,
        reflect: i32,
    ) -> Option<HitType> {
        self.inner.hit_type_pvp(att, def, is_target, reflect)
    }

    fn guild_combat_running(&self) -> bool {
        self.inner.guild_combat_running()
    }

    fn is_guild_combat_target(&self, att: &Mover, def: &Mover) -> bool {
        self.inner.is_guild_combat_target(att, def)
    }

    fn is_event_spawn_monster(&self, mover_id: MoverId) -> bool {
        self.spawns.contains(mover_id) || self.inner.is_event_spawn_monster(mover_id)
    }

    fn event_monster_level_gap(&self, data: &GameData, mover_id: MoverId) -> Option<i32> {
        self.inner.event_monster_level_gap(data, mover_id)
    }

    fn create_monster_attackable(&self, att: &Mover, def: &Mover) -> bool {
        self.inner.create_monster_attackable(att, def)
    }

    fn party_critical(&self, att: &Mover) -> Option<i32> {
        self.inner.party_critical(att)
    }

    fn link_attack_state(&self, att: &Mover) -> LinkAttackState {
        self.inner.link_attack_state(att)
    }

    fn link_attack_state_in(&self, store: &ObjectStore, att: &Mover) -> LinkAttackState {
        self.inner.link_attack_state_in(store, att)
    }

    fn better_link_attack(&self) -> bool {
        self.inner.better_link_attack()
    }

    fn remove_invisible(&mut self, w: &mut Arena<'_>, fx: &mut Effects, attacker: ObjId) {
        self.inner.remove_invisible(w, fx, attacker);
    }

    fn remove_skill_buff(
        &mut self,
        w: &mut Arena<'_>,
        fx: &mut Effects,
        target: ObjId,
        skill: u32,
    ) {
        self.inner.remove_skill_buff(w, fx, target, skill);
    }

    fn remove_chr_state_buffs(
        &mut self,
        w: &mut Arena<'_>,
        fx: &mut Effects,
        target: ObjId,
        bits: u32,
    ) {
        self.inner.remove_chr_state_buffs(w, fx, target, bits);
    }

    fn do_use_skill(
        &mut self,
        w: &mut Arena<'_>,
        fx: &mut Effects,
        caster: ObjId,
        skill: u32,
        level: u32,
        target: ObjId,
    ) {
        self.inner.do_use_skill(w, fx, caster, skill, level, target);
    }

    fn skill_level(&self, m: &Mover, skill: u32) -> u32 {
        self.inner.skill_level(m, skill)
    }

    fn gold(&self, m: &Mover) -> i32 {
        self.inner.gold(m)
    }

    fn add_gold(&mut self, w: &mut Arena<'_>, fx: &mut Effects, player: ObjId, delta: i32) {
        self.inner.add_gold(w, fx, player, delta);
    }

    fn holycross_gc_point(
        &mut self,
        w: &mut Arena<'_>,
        fx: &mut Effects,
        victim: ObjId,
        attacker: ObjId,
    ) {
        self.inner.holycross_gc_point(w, fx, victim, attacker);
    }

    fn monster_transform(
        &mut self,
        w: &mut Arena<'_>,
        fx: &mut Effects,
        victim: ObjId,
        new_hp: i32,
    ) -> bool {
        self.inner.monster_transform(w, fx, victim, new_hp)
    }

    fn on_hit_recorded(
        &mut self,
        w: &mut Arena<'_>,
        fx: &mut Effects,
        attacker: ObjId,
        victim: ObjId,
        damage: i32,
    ) {
        self.inner.on_hit_recorded(w, fx, attacker, victim, damage);
    }

    fn on_died(&mut self, w: &mut Arena<'_>, fx: &mut Effects, env: &DamageEnv<'_>, k: &KillCtx) {
        self.inner.on_died(w, fx, env, k);
    }

    fn monster_skill_apply(
        &mut self,
        w: &mut Arena<'_>,
        fx: &mut Effects,
        attacker: ObjId,
        target: ObjId,
        trigger: MonsterSkillTrigger,
    ) -> bool {
        self.inner
            .monster_skill_apply(w, fx, attacker, target, trigger)
    }

    fn apply_monster_skill(&mut self, w: &mut Arena<'_>, fx: &mut Effects, cast: &MonsterCast) {
        self.inner.apply_monster_skill(w, fx, cast);
    }

    fn arrow_down(&mut self, w: &mut Arena<'_>, fx: &mut Effects, player: ObjId) {
        self.inner.arrow_down(w, fx, player);
    }

    fn boss_attack_melee(
        &mut self,
        w: &mut Arena<'_>,
        fx: &mut Effects,
        attacker: ObjId,
        state: u32,
        target: ObjId,
    ) -> bool {
        self.inner.boss_attack_melee(w, fx, attacker, state, target)
    }

    fn on_pk_hit(&mut self, w: &mut Arena<'_>, fx: &mut Effects, attacker: ObjId, victim: ObjId) {
        self.inner.on_pk_hit(w, fx, attacker, victim);
    }

    fn on_pvp_hit(&mut self, w: &mut Arena<'_>, attacker: ObjId, victim: ObjId) {
        self.inner.on_pvp_hit(w, attacker, victim);
    }

    fn rainbow_race_entry(&self, m: &Mover) -> bool {
        self.inner.rainbow_race_entry(m)
    }

    fn guild_combat_war_state(&self) -> bool {
        self.inner.guild_combat_war_state()
    }
}
