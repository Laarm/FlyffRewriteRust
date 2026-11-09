//! The seams of the damage pipeline (Plan/17 §5, §9.1, §13; WP-COMBAT-04): everything the pipeline needs from
//! work packages that land later or belong to other documents.
//!
//! Every method has a default that answers "nothing happens", so the pipeline is complete and testable before
//! its consumers exist; the owner of a seam installs an implementation of [`CombatHooks`] in
//! [`CombatWorld::hooks`](super::world::CombatWorld::hooks) (`Worlds.movement.combat.hooks`: the hit frames of the
//! world phase have no services, so the seams live where the phase can reach them). A hook that changes the
//! world gets the [`Arena`] and the [`Effects`] of the damage call, so it may draw from the world's random
//! stream in the position of the original call (the order of the draws is part of the contract, 17 §13).
//!
//! The consumers of a death (kill EXP, drops, quests, ...) need the services and are a different seam:
//! [`super::death::DeathHooks`] in [`CombatService::death`].
//!
//! | seam | owner |
//! |---|---|
//! | [`ProcHook::do_active_skill`] (`DoActiveSkill`: stun/poison/dark/bleeding/steal-HP procs, weapon and skill active skills) | WP-SKILL-08 |
//! | `remove_invisible`, `remove_skill_buff`, `remove_chr_state_buffs`, `do_use_skill` (the counter skill) | WP-SKILL-01/02/04/08 |
//! | `on_died` (`OnDied`: PvP, kill EXP, drops, `do_die`) | WP-COMBAT-08 |
//! | `hit_type_pvp` (an override of the native rule), `guild_combat_running` / `is_guild_combat_target` (doc 21's state) | WP-COMBAT-12 (native: [`super::pvp`], [`super::duel`]) |
//! | `is_event_spawn_monster` (KQ-32, `EventCombatHooks`), `event_monster_level_gap` (KQ-32, native: [`crate::event::monsters::level_gap`]) | doc 22 |
//! | `create_monster_attackable` (`CCreateMonster::IsAttackAble`) | doc 19 |
//! | `party_critical`, `link_attack_state`, `holycross_gc_point` | doc 21 |
//! | `monster_transform` (`CMonsterSkill::MonsterTransform`) | an override; doc 18's [`crate::ai::monster_skill::transform_check`] runs after it |
//! | `monster_skill_apply` (an override of the whole `CMonsterSkill::ApplySkill`) | none; doc 18's [`crate::ai::monster_skill::apply`] runs when it answers `false` |
//! | `apply_monster_skill` (the effect of one `CMonsterSkill::ApplySkill` cast) | WP-SKILL-13 (doc 18 picks the skill and the victims) |
//! | `gold`, `add_gold` (Hit of Penya without `no_hop_cost`) | doc 19 |
//! | `on_hit_recorded` (the DPS meter, §10) | WP-COMBAT-14 |
//!
//! **The defaults are the skill engine.** The methods that need the buff engine or the skill start (the procs, the
//! three buff removals, the counter-attack cast and the effect of a monster skill) cannot run where they are
//! called (the world phase has no services), so their default bodies queue a [`SkillJob`] in the [`Effects`] of
//! the call; [`crate::skill::cast::drain_jobs`] runs it right after the call returns in a handler context and
//! after the world phase in a tick. An owner who installs its own hooks overrides them (tests record instead).

use super::effects::{Arena, Effects};
use super::env::DamageEnv;
use crate::mover::Mover;
use crate::skill::cast::SkillJob;
use flyff_common::ObjId;
use flyff_data::GameData;
use flyff_data::types::MoverId;
use flyff_rules::combat::{AttributeTable, HitType, LinkAttackState};
use std::sync::{Arc, OnceLock};

/// One `DoActiveSkill(skill, level, victim, ignore_prob)` request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveSkill {
    /// The mover that casts it (the attacker of the hit).
    pub caster: ObjId,
    /// The victim of the hit.
    pub target: ObjId,
    /// The skill id (`SI_*`).
    pub skill: u32,
    /// The skill level.
    pub level: u32,
    /// The pipeline already rolled the chance (weapon active skills): the skill must not roll again.
    pub ignore_prob: bool,
}

/// The proc seam of `ProcessAbnormal` (WP-SKILL-08): the default queues the cast for the skill engine
/// ([`SkillJob::Active`]).
pub trait ProcHook {
    /// `DoActiveSkill(skill, level, target, ignore_prob)` of the hit's attacker.
    fn do_active_skill(&mut self, _w: &mut Arena<'_>, fx: &mut Effects, skill: ActiveSkill) {
        fx.skill_jobs.push(SkillJob::Active(skill));
    }
}

/// What `OnDied` hands to the death pipeline (WP-COMBAT-08): the victim reached 0 HP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KillCtx {
    /// The victim.
    pub victim: ObjId,
    /// The killer.
    pub attacker: ObjId,
    /// `OBJMSG_DAMAGE` or `OBJMSG_DAMAGE_FORCE` (the flying death).
    pub msg: u32,
    /// The `nReflect` of the killing hit.
    pub reflect: i32,
}

/// When a monster skill is rolled (`CMonsterSkill::ApplySkill`'s `ATK_MELEE` / `ATK_RANGE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonsterSkillTrigger {
    /// The melee hit frame.
    Melee,
    /// The ranged hit frame.
    Range,
}

/// The seams (see the module documentation). `Send` because the services live on the game thread.
pub trait CombatHooks: ProcHook + Send {
    /// An **override** of `GetHitType` for two players: `Some(type)` replaces the native rule of
    /// [`super::pvp::hit_type_pvp`] (tests force a type with it); the default `None` lets the native rule decide.
    fn hit_type_pvp(
        &self,
        _att: &Mover,
        _def: &Mover,
        _is_target: bool,
        _reflect: i32,
    ) -> Option<HitType> {
        None
    }

    /// `g_GuildCombatMng.m_nGCState == WAR_WAR_STATE` or `g_GuildCombat1to1Mng.m_nState == GC1TO1_WAR` (doc 21): a
    /// guild combat is being fought. `false` until doc 21 installs its state.
    fn guild_combat_running(&self) -> bool {
        false
    }

    /// `IsGuildCombatTarget(def)` (WP-COMBAT-12): both players are combatants (`guild_combat_state == 1`) of
    /// different guilds while a combat runs ([`CombatHooks::guild_combat_running`]).
    fn is_guild_combat_target(&self, att: &Mover, def: &Mover) -> bool {
        let state = |m: &Mover| {
            m.player()
                .map(|p| (p.social.guild_combat_state, p.social.guild))
        };
        let (Some((sa, ga)), Some((sd, gd))) = (state(att), state(def)) else {
            return false;
        };
        self.guild_combat_running() && sa == 1 && sd == 1 && ga != gd
    }

    /// `prj.m_EventLua.IsEventSpawnMonster(mover id)` (doc 22): such monsters take 1 damage per hand and are always
    /// hit by players.
    fn is_event_spawn_monster(&self, _mover_id: MoverId) -> bool {
        false
    }

    /// `CEventMonster::SetEventMonster(id)` + `GetLevelGap()` (KQ-32): the level gap from which a player misses
    /// the event monster `mover_id`; `None` when it is not an event monster. The default is the rule of WP-EVENT-09
    /// ([`crate::event::monsters::level_gap`] over `EventMonster.lua`, the first entry of a mover winning), like
    /// `create_monster_attackable` defaults to WP-AI-16's; a test hook set overrides it.
    fn event_monster_level_gap(&self, data: &GameData, mover_id: MoverId) -> Option<i32> {
        crate::event::monsters::level_gap(&data.lua.event_monsters, mover_id)
    }

    /// `CCreateMonster::IsAttackAble(user, monster)` (docs 18 §14 and 19): summoned-monster ownership; `false`
    /// turns a player's hit into a miss and clears the victim's attacker and targeter. The default is the rule
    /// of WP-AI-16 ([`crate::ai::create_monster::is_attack_able`]: a summon can only be attacked by its owner and
    /// the owner's party).
    fn create_monster_attackable(&self, att: &Mover, def: &Mover) -> bool {
        crate::ai::create_monster::is_attack_able(att, def)
    }

    /// Players in a party: the armed one-shot Sphere Circle bonus (`party_size / 2`), else `None` (doc 21). The
    /// default reads the attacker's party link (WP-COMBAT-14).
    fn party_critical(&self, att: &Mover) -> Option<i32> {
        crate::social::party::skills::party_critical(att)
    }

    /// The party facts `CalcLinkAttackDamage` reads (doc 21) from the attacker alone: "no party". The pipeline asks
    /// [`CombatHooks::link_attack_state_in`] (a test overrides either).
    fn link_attack_state(&self, _att: &Mover) -> LinkAttackState {
        LinkAttackState::default()
    }

    /// The same with the object store, which the vanilla rule (`features.better_link_attack = false`) needs for the
    /// members that share the leader's target. The default reads the attacker's party link (WP-COMBAT-14) and
    /// falls back to [`CombatHooks::link_attack_state`] for a player outside a party.
    fn link_attack_state_in(
        &self,
        store: &crate::world::ObjectStore,
        att: &Mover,
    ) -> LinkAttackState {
        let in_party = att.player().is_some_and(|p| p.social.party_link.is_some());
        if in_party {
            crate::social::party::skills::link_attack_state_in(
                store,
                att,
                self.better_link_attack(),
            )
        } else {
            self.link_attack_state(att)
        }
    }

    /// `features.better_link_attack`: the repack default, every member earns the bonus. The pipeline's
    /// `CombatConfig` is the real switch; this answer only shapes the vanilla leader count in
    /// [`CombatHooks::link_attack_state_in`] and is overridden by a hook that owns the configuration.
    fn better_link_attack(&self) -> bool {
        true
    }

    /// `RemoveInvisible()` of the attacker: the `CHS_INVISIBILITY` state and the `SI_ACR_SUP_DARKILLUSION` buff
    /// (WP-SKILL-01). The default queues [`SkillJob::RemoveInvisible`].
    fn remove_invisible(&mut self, _w: &mut Arena<'_>, fx: &mut Effects, attacker: ObjId) {
        fx.skill_jobs
            .push(SkillJob::RemoveInvisible { mover: attacker });
    }

    /// `RemoveBuff(BUFF_SKILL, skill)` of `target` (`SI_ASS_HEAL_PREVENTION`, `SI_RIG_MASTER_BLESSING`). The
    /// default queues [`SkillJob::RemoveSkillBuff`].
    fn remove_skill_buff(
        &mut self,
        _w: &mut Arena<'_>,
        fx: &mut Effects,
        target: ObjId,
        skill: u32,
    ) {
        fx.skill_jobs.push(SkillJob::RemoveSkillBuff {
            mover: target,
            skill,
        });
    }

    /// `RemoveChrStateBuffs(bits)` of `target` (the `CHS_*` counter-attack and Holy Cross states). The default
    /// queues [`SkillJob::RemoveChrState`].
    fn remove_chr_state_buffs(
        &mut self,
        _w: &mut Arena<'_>,
        fx: &mut Effects,
        target: ObjId,
        bits: u32,
    ) {
        fx.skill_jobs.push(SkillJob::RemoveChrState {
            mover: target,
            bits,
        });
    }

    /// `DoUseSkill(skill, level, target, SUT_NORMAL, control = false, cast = 0)` of `caster` (the counter-attack
    /// skill `SI_GEN_ATK_COUNTER`; WP-SKILL-04). The default queues [`SkillJob::Counter`].
    fn do_use_skill(
        &mut self,
        _w: &mut Arena<'_>,
        fx: &mut Effects,
        caster: ObjId,
        skill: u32,
        level: u32,
        target: ObjId,
    ) {
        fx.skill_jobs.push(SkillJob::Counter {
            caster,
            skill,
            level,
            target,
        });
    }

    /// The level of `skill` the mover knows (`GetSkill(skill)` -> `GetSkillLevel`); 0 when it has none
    /// (WP-SKILL; the counter-attack uses `SI_ACR_YOYO_COUNTER`'s level).
    fn skill_level(&self, _m: &Mover, _skill: u32) -> u32 {
        0
    }

    /// `GetGold()` of a player (doc 19); only the vanilla Hit of Penya (`!features.no_hop_cost`) asks.
    fn gold(&self, _m: &Mover) -> i32 {
        0
    }

    /// `AddGold(delta)` (doc 19).
    fn add_gold(&mut self, _w: &mut Arena<'_>, _fx: &mut Effects, _player: ObjId, _delta: i32) {}

    /// The guild-combat point of a Holy-Cross kill (`__HOLYCROSS_SUPPORT_GW`; doc 21): `caster` is the
    /// attacker of the Holy Cross buff on `victim`.
    fn holycross_gc_point(
        &mut self,
        _w: &mut Arena<'_>,
        _fx: &mut Effects,
        _victim: ObjId,
        _attacker: ObjId,
    ) {
    }

    /// `CMonsterSkill::MonsterTransform(victim, new_hp)` (`MoverEx.transform`): `true` when the victim was
    /// replaced (the damage call then answers 0 and sends nothing more). An override: the pipeline asks doc 18's
    /// [`crate::ai::monster_skill::transform_check`] after this hook.
    fn monster_transform(
        &mut self,
        _w: &mut Arena<'_>,
        _fx: &mut Effects,
        _victim: ObjId,
        _new_hp: i32,
    ) -> bool {
        false
    }

    /// The DPS bookkeeping of step 9 (`m_bIsDPSMover`; WP-COMBAT-14): `damage` is the dealt damage. The default
    /// feeds the damage meter of a tracked monster ([`super::dps::record_hit`]).
    fn on_hit_recorded(
        &mut self,
        w: &mut Arena<'_>,
        fx: &mut Effects,
        attacker: ObjId,
        victim: ObjId,
        damage: i32,
    ) {
        super::dps::record_hit(w, fx, attacker, victim, damage);
    }

    /// `OnDied` (§9.1) as the damage pipeline sees it: the victim reached 0 HP. The pipeline itself queues the
    /// [`KillCtx`] for [`super::death::process_pending`] (the real `OnDied`, which needs the services); this seam
    /// only observes the moment (tests record it).
    fn on_died(
        &mut self,
        _w: &mut Arena<'_>,
        _fx: &mut Effects,
        _env: &DamageEnv<'_>,
        _k: &KillCtx,
    ) {
    }

    /// `CMonsterSkill::ApplySkill(attacker, target, trigger)` as a whole (`MoverActEvent.cpp` `OnAttackMelee` /
    /// `OnAttackRange`): an **override**. When it answers `true` the hit frame treats the skill as fired and doc
    /// 18's table roll ([`crate::ai::monster_skill::apply`]) is skipped; the default (`false`) lets the AI roll.
    /// An owner that only applies the effect implements [`CombatHooks::apply_monster_skill`] instead.
    fn monster_skill_apply(
        &mut self,
        _w: &mut Arena<'_>,
        _fx: &mut Effects,
        _attacker: ObjId,
        _target: ObjId,
        _trigger: MonsterSkillTrigger,
    ) -> bool {
        false
    }

    /// One cast of `CMonsterSkill::ApplySkill` (`MoverActEvent.cpp` `OnAttackMelee` / `OnAttackRange`, Plan/17
    /// §3.8, §6.15; WP-SKILL-13): doc 18 ([`crate::ai::monster_skill::apply`]) rolls the skill of
    /// `MonsterSkill.lua` and the victims; this seam applies the skill to one victim
    /// (`CMover::ApplySkill(caster, skillProp, addProp, ignore_prob)`, with the duration of the cast and without
    /// the skill's refer targets). Called once per victim, in window order.
    fn apply_monster_skill(
        &mut self,
        _w: &mut Arena<'_>,
        fx: &mut Effects,
        cast: &crate::ai::monster_skill::MonsterCast,
    ) {
        fx.skill_jobs.push(SkillJob::Monster(*cast));
    }

    /// `ArrowDown(1)`: a bow shot of a player uses one arrow (only without `features.no_arrows_poster`, the
    /// repack default consumes none; doc 19's item removal).
    fn arrow_down(&mut self, _w: &mut Arena<'_>, _fx: &mut Effects, _player: ObjId) {}

    /// The melee hit frame of a boss model (ClockWorks, BigMuscle, Krrr, Mushmoot, Meteonyker): an **override**
    /// seam. The native handlers are the brains' ([`crate::ai::boss::hits::on_attack_melee`], WP-AI-18..21), which
    /// [`super::action::on_attack_melee`] calls when this hook answers `false`; `true` means the owner handled the
    /// swing itself and the native handler is skipped. The default (`false`) leaves it to the brains.
    fn boss_attack_melee(
        &mut self,
        _w: &mut Arena<'_>,
        _fx: &mut Effects,
        _attacker: ObjId,
        _state: u32,
        _target: ObjId,
    ) -> bool {
        false
    }

    /// `OnAttacked` for a PK hit, **after** the native bookkeeping ([`super::pvp::pk::on_pk_hit`]: the attacker
    /// becomes pink): an observer seam.
    fn on_pk_hit(
        &mut self,
        _w: &mut Arena<'_>,
        _fx: &mut Effects,
        _attacker: ObjId,
        _victim: ObjId,
    ) {
    }

    /// `OnAttacked` for a duel hit, **after** the native bookkeeping ([`super::duel::reset_duel_time`]): an
    /// observer seam.
    fn on_pvp_hit(&mut self, _w: &mut Arena<'_>, _attacker: ObjId, _victim: ObjId) {}

    // ---- the questions of the skill start (`DoUseSkill`, Plan/17 §6.3; WP-SKILL-04) ----

    /// `CRainbowRaceMng::IsEntry(player)` (step 7, only without `features.rm_hero_skill`; doc 22).
    fn rainbow_race_entry(&self, m: &Mover) -> bool {
        // ---- rainbow race (doc 22 §14, task rainbow-race): the runtime mirror of `RainbowRaceService::is_entry` ----
        m.player().is_some_and(|p| p.misc.rainbow_entrant)
    }

    /// `g_GuildCombatMng.m_nState == WAR_STATE` (step 9: the Resurrection skill in the guild-war world is only
    /// allowed during the war; doc 21).
    fn guild_combat_war_state(&self) -> bool {
        false
    }
}

/// The hook set that does nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoHooks;

impl ProcHook for NoHooks {}
impl CombatHooks for NoHooks {}

/// The `ItemUpgrade.lua` attribute table, built once on first use from the loaded game data (the shipped table
/// when the script is not loaded).
#[derive(Debug, Default)]
pub struct AttrCache(OnceLock<Arc<AttributeTable>>);

impl AttrCache {
    /// The table.
    pub fn get(&self, data: &GameData) -> &AttributeTable {
        self.handle(data)
    }

    /// The table as a shared handle (the world phases keep one in their per-tick snapshot).
    pub fn handle(&self, data: &GameData) -> &Arc<AttributeTable> {
        self.0.get_or_init(|| {
            Arc::new(
                data.lua
                    .item_upgrade
                    .as_ref()
                    .map(AttributeTable::from_config)
                    .unwrap_or_default(),
            )
        })
    }

    /// A cache holding `t` (tests).
    pub fn with(t: AttributeTable) -> AttrCache {
        let c = OnceLock::new();
        let _ = c.set(Arc::new(t));
        AttrCache(c)
    }
}

/// The combat service (`Services.combat`): the death consumers and the lazily built element table. The fields
/// are public so that a caller can borrow them separately from the rest of the services. The damage seams
/// ([`CombatHooks`]) live in [`CombatWorld`](super::world::CombatWorld).
#[derive(Default)]
pub struct CombatService {
    /// The element table cache.
    pub attr: AttrCache,
    /// The consumers of a death (kill EXP, drops, guild, quests, ...; Plan/17 §9.1).
    pub death: super::death::DeathHooks,
    /// The duels: the party membership seam, the players visited every tick, the counters (WP-COMBAT-13).
    pub duel: super::duel::DuelService,
}

impl std::fmt::Debug for CombatService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CombatService(..)")
    }
}
