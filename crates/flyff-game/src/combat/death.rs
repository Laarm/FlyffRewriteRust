//! Death, corpse timing, revival and the death hooks (Plan/17 §9; WP-COMBAT-08; `AttackArbiter.cpp:851-973`,
//! `Mover.cpp:5249-5557`, `DPSrvr.cpp:994-1220`).
//!
//! **When it runs.** The damage pipeline only leaves a victim at 0 HP and queues a [`KillCtx`]; the real `OnDied`
//! needs the services (kill EXP, drops, quests, ...), which the world phase does not have. [`process_pending`]
//! runs the queue in a service context: right after a handler's damage call, and after the world phase of the
//! tick ([`super::tick::finish`]). Between the killing blow and that moment the victim has 0 HP but no death
//! state; `is_die` (the original's `HP == 0` clause) makes it a corpse for every rule that matters (`hit_type`,
//! the hit handlers, the AI scans), so the delay is invisible but for the death animation, which starts at the
//! next frame after the world phase (at most 3 frames late).
//!
//! **Order** ([`on_died`], §9.1; the hook names are fields of [`DeathHooks`]):
//! `dps_close`, `sub_pvp`, `guild_kill`, `pvp_fame`, kill recovery (direct), `pet_exp` (monster victims),
//! `monster_buffs`, NPC already dead -> delete and stop, `kill_exp`, `drop`, the chaotic propensity loss,
//! `attacker.atk_cnt = 0`, [`do_die`], `attacker.clear_dest_obj()`.
//!
//! **[`do_die`]** (§9.2): the double-death guard, `last_pk` / `guild_combat`, `honor_hunt`, `clear_status`, the sim's
//! STOP and DIE (`DEAD`, or the fly-back `DMG_FLY` for `OBJMSG_DAMAGE_FORCE`, and for server NPCs the 180-frame
//! corpse counter followed by the 60-frame disappear: 240 frames, run by the sim and the movement pipeline),
//! the last-hit mob released, `owner_pets`, `MoverDeath`, `remove_all_enemies` (`DstDie` from the highest id
//! down), the killer's hit target cleared (`AIMSG_INIT_TARGETCLEAR` for a monster killer), `quest_kill`,
//! `remove_buffs` + `RemoveAllSkillInfluence`, `world_die`. There is no `MoverStateNtf` before `MoverDeath`: the
//! original's `bBehavior` is never set, the sim's `DIE` reproduces the pose on every client.
//!
//! **Revival** ([`on_revival_req`], [`on_revival_to_lodestar_req`], §9.4): HP / MP / FP are sent as absolute values
//! (PL-083), `dead_ticks = 75` makes the player invulnerable for 5 s (`hit_type` refuses it), the EXP penalty is
//! the mover-progress `death_penalty::apply_for`.

use super::context::{ActionCx, with_action_at};
use super::effects::SimFx;
use super::hate::remove_all_enemies;
use super::hooks::KillCtx;
use super::state::{is_chaotic, is_die};
use super::{duel, pvp};
use crate::ai::AiMsg;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::ops::RemoveReason;
use crate::stats::StatsCore;
use crate::stats::out::Out;
use crate::stats::pool::{send_pools_absolute, set_fp_silent, set_hp_silent, set_mp_silent};
use crate::world::logout::{LogoutHooks, LogoutStep};
use crate::world::replace::ReplaceKind;
use flyff_common::defs::item::II_SYS_SYS_SCR_RESURRECTION;
use flyff_common::defs::world::{
    WI_DUNGEON_BEAR, WI_DUNGEON_KRRR, WI_DUNGEON_MUSCLE, WI_WORLD_ARENA, WI_WORLD_GUILDWAR,
    WI_WORLD_GUILDWAR1TO1_0, WI_WORLD_GUILDWAR1TO1_1, WI_WORLD_GUILDWAR1TO1_2,
    WI_WORLD_GUILDWAR1TO1_3, WI_WORLD_MADRIGAL,
};
use flyff_common::math::Vec3;
use flyff_common::{LayerId, ObjId, SimFrame, WorldId};
use flyff_proto::pb;
use flyff_rules::combat::{objmsg, x_random_range};
use flyff_rules::stats::c_int;

/// `PROCESS_COUNT * 5`: the invulnerability after a revival, in ticks (5 s).
pub const REVIVAL_INVULNERABLE_TICKS: i32 = 75;
/// The HP fraction of a revival (`__S_9_ADD`): 20 %.
pub const REVIVAL_RATE: f32 = 0.2;
/// The Madrigal revival position of the boss dungeons.
pub const MADRIGAL_BOSS_REVIVAL: Vec3 = Vec3::new(6968.0, 0.0, 3328.8);

/// One death consumer: it gets the services, the game context and the death.
pub type KillHook = fn(&mut Services, &mut GameCtx<'_>, &KillCtx);

/// `m_bLastPK` / `m_bGuildCombat` of a victim (set in `DoDie`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PkFlags {
    /// No EXP penalty at the revival (killed by a player outside the paid PK).
    pub last_pk: bool,
    /// Killed in a guild-combat fight.
    pub guild_combat: bool,
}

/// Decides [`PkFlags`] (WP-COMBAT-12 replaces it).
pub type PkFlagsHook = fn(&Services, &GameCtx<'_>, &KillCtx) -> PkFlags;
/// A question about a running event of another document.
pub type GameStatePredicate = fn(&Services, &GameCtx<'_>) -> bool;
/// A player-level question that needs another document's state.
pub type PlayerPredicate = fn(&Services, &GameCtx<'_>, PlayerRef) -> bool;
/// A player-level action of another document.
pub type PlayerHook = fn(&mut Services, &mut GameCtx<'_>, PlayerRef);
/// The revival destination another document forces (the secret room).
pub type RevivalOverrideHook = fn(&Services, &GameCtx<'_>, PlayerRef) -> Option<(WorldId, Vec3)>;

/// The consumers of a death (§9.1). Every field is a plain function pointer; the default is the wiring of this
/// build ([`DeathHooks::default`]: kill EXP installed, everything else a no-op), [`DeathHooks::noop`] installs
/// nothing at all.
#[derive(Clone, Copy)]
pub struct DeathHooks {
    /// Step 1: the DPS meter closes (`__DPS_METER`, WP-COMBAT-14).
    pub dps_close: KillHook,
    /// Step 2: `SubPVP` (WP-COMBAT-12/13: duel / war / PK bookkeeping).
    pub sub_pvp: KillHook,
    /// Step 3: the guild kill contribution (`gameplay.guild_kill_contri`, doc 21).
    pub guild_kill: KillHook,
    /// Step 4: +1 fame for PvP / war / guild-combat / arena kills (WP-COMBAT-12).
    pub pvp_fame: KillHook,
    /// Step 6: system-pet EXP by kills (doc 19, monster victims only).
    pub pet_exp: KillHook,
    /// Step 8: `MonsterBuffs.inc` (WP-COMBAT-14).
    pub monster_buffs: KillHook,
    /// Step 10: the kill EXP (doc 16: `stats::exp_share::on_monster_killed`).
    pub kill_exp: KillHook,
    /// Step 11: the loot (doc 19). Use [`loot_owner`] for the player the drop belongs to.
    pub drop: KillHook,
    /// `DoDie` step 3: the monster-hunt honor counter (doc 21).
    pub honor_hunt: KillHook,
    /// `DoDie` step 3: stun / poison / dark / bleeding of a dying player are cleared (WP-SKILL-07).
    pub clear_status: KillHook,
    /// `DoDie` step 5: a player killed by an NPC loses a pet life / its pets are removed (doc 19).
    pub owner_pets: KillHook,
    /// `DoDie` step 7: the quest kill counters (doc 20).
    pub quest_kill: KillHook,
    /// `DoDie` step 8: `RemoveCommonBuffs` (WP-SKILL-01).
    pub remove_buffs: KillHook,
    /// `DoDie` step 9: siege `World::on_die`, secret room, CreateMonster, instance kills, Event.lua spawns.
    pub world_die: KillHook,
    /// `DoDie` step 2: `m_bLastPK` / `m_bGuildCombat`.
    pub pk_flags: PkFlagsHook,
    /// The duel part of `OBJMSG_DIE` (right after the death state): the duel of a killed duelist ends
    /// (WP-COMBAT-13).
    pub duel_die: KillHook,
    /// `g_GuildCombatMng.m_nState != CLOSE_STATE` (doc 21): the guild-war world is in use. `false` until doc 21
    /// installs its state.
    pub gw_open: GameStatePredicate,
    /// The secret-room revival destination (doc 22).
    pub secret_room_revival: RevivalOverrideHook,
    /// `g_GuildCombat1to1Mng.IsPossibleUser`: such a player revives with full pools (doc 21).
    pub gc_1to1_player: PlayerPredicate,
    /// `g_GuildCombatMng.JoinObserver`: a guild-war revival joins the observers (doc 21).
    pub gw_observer: PlayerHook,
    /// After any revival: the charged buffs are sent again (PL-082, WP-SKILL-10).
    pub after_revival: PlayerHook,
}

fn no_kill(_: &mut Services, _: &mut GameCtx<'_>, _: &KillCtx) {}
fn no_player_hook(_: &mut Services, _: &mut GameCtx<'_>, _: PlayerRef) {}
fn no_state(_: &Services, _: &GameCtx<'_>) -> bool {
    false
}
fn no_predicate(_: &Services, _: &GameCtx<'_>, _: PlayerRef) -> bool {
    false
}
fn no_override(_: &Services, _: &GameCtx<'_>, _: PlayerRef) -> Option<(WorldId, Vec3)> {
    None
}

/// `m_bLastPK` without the PvP systems: any kill by another player is exempt from the penalty (§9.2 step 2); the
/// paid-PK exception (`events.pk_cost`) arrives with WP-COMBAT-12.
fn default_pk_flags(_s: &Services, ctx: &GameCtx<'_>, k: &KillCtx) -> PkFlags {
    let by_player =
        k.attacker != k.victim && ctx.worlds.mover(k.attacker).is_some_and(|a| a.is_player());
    PkFlags {
        last_pk: by_player,
        guild_combat: false,
    }
}

/// The kill EXP of this build: doc 16's single entry.
fn default_kill_exp(s: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    crate::stats::exp_share::on_monster_killed(s, ctx, k.attacker, k.victim);
}

impl DeathHooks {
    /// No consumer at all: a death gives no EXP, no loot, nothing but the death itself.
    pub fn noop() -> DeathHooks {
        DeathHooks {
            dps_close: no_kill,
            sub_pvp: no_kill,
            guild_kill: no_kill,
            pvp_fame: no_kill,
            pet_exp: no_kill,
            monster_buffs: no_kill,
            kill_exp: no_kill,
            drop: no_kill,
            honor_hunt: no_kill,
            clear_status: no_kill,
            owner_pets: no_kill,
            quest_kill: no_kill,
            remove_buffs: no_kill,
            world_die: no_kill,
            pk_flags: default_pk_flags,
            duel_die: no_kill,
            gw_open: no_state,
            secret_room_revival: no_override,
            gc_1to1_player: no_predicate,
            gw_observer: no_player_hook,
            after_revival: no_player_hook,
        }
    }
}

impl Default for DeathHooks {
    fn default() -> Self {
        DeathHooks {
            dps_close: super::dps::close,
            monster_buffs: super::monster_buffs::apply,
            sub_pvp: pvp::sub_pvp,
            pvp_fame: pvp::pvp_fame,
            pk_flags: pvp::pk_flags,
            duel_die: duel::on_die_message,
            guild_kill: crate::social::guild::contribution::kill_hook,
            honor_hunt: crate::social::honor::on_monster_died,
            kill_exp: default_kill_exp,
            drop: crate::item::drop::drop_hook,
            pet_exp: crate::item::pets::system::pet_exp_hook,
            owner_pets: crate::item::pets::system::owner_pets_hook,
            // doc 20 (task quest-effects): the quest kill counters of `DoDie` step 7
            quest_kill: crate::quest::track::kill_hook,
            remove_buffs: crate::buff::api::on_death_remove_buffs,
            clear_status: crate::buff::api::on_death_clear_status,
            after_revival: crate::skill::resurrect::after_revival,
            // doc 21 (WP-SOCIAL-17): a fighter's death is queued for the siege; a revival in the siege world joins
            // the observers (`gw_open` is installed by `Game::new`)
            // doc 22 (task instances): the dispatcher calls the siege part first, then the event parts behind it
            world_die: crate::event::on_world_die,
            gw_observer: crate::social::siege::lineup::gw_observer,
            // doc 21 §8 (WP-SOCIAL-26): a player in his 1:1 arena revives with full pools
            gc_1to1_player: crate::social::gc1to1::death_participant,
            // doc 22 §11 (task secret-room): a death in a room revives in Madrigal at the revival point of the line-up side
            secret_room_revival: crate::event::secret_room::rules::revival_override,
            ..DeathHooks::noop()
        }
    }
}

/// The player a monster's loot belongs to (`DropItemByDied`, `Mover.cpp:7346-7365`): the enemy with the most
/// damage in the victim's hate list when it is still a valid mover, else the killer.
pub fn loot_owner(ctx: &GameCtx<'_>, k: &KillCtx) -> ObjId {
    ctx.worlds
        .mover(k.victim)
        .and_then(|v| v.combat.hate.max_hate())
        .filter(|id| ctx.worlds.mover(*id).is_some_and(|m| !m.core.deleted))
        .unwrap_or(k.attacker)
}

// ---- the queue -------------------------------------------------------------------------------------------------

/// Runs the queued deaths in a handler context (the effects start at the current frame `4n`).
pub fn process_pending(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let frame = ctx.clock.frame0;
    process_pending_at(s, ctx, frame);
}

/// Runs the queued deaths; their sim effects start at `frame`. The consumers may kill again (a reflected hit,
/// ...): the queue is drained until it stays empty, at most eight rounds.
pub fn process_pending_at(s: &mut Services, ctx: &mut GameCtx<'_>, frame: SimFrame) {
    for _ in 0..8 {
        let batch = std::mem::take(&mut ctx.worlds.movement.combat.queues.deaths);
        if batch.is_empty() {
            return;
        }
        for k in batch {
            on_died(s, ctx, &k, frame);
            ctx.worlds.movement.combat.queues.stats.deaths += 1;
        }
    }
}

// ---- OnDied -------------------------------------------------------------------------------------------------------

/// `CAttackArbiter::OnDied` (§9.1) in a service context. The victim must still be in the world with its hate
/// list; a killer that vanished meanwhile (a logout) is passed on unchanged and skipped where it is read.
pub fn on_died(s: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx, frame: SimFrame) {
    let Some((victim_is_player, victim_dead_state)) = ctx
        .worlds
        .mover(k.victim)
        .map(|v| (v.is_player(), v.sim.act.state.is_die()))
    else {
        return;
    };
    let h = s.combat.death;
    (h.dps_close)(s, ctx, k);
    (h.sub_pvp)(s, ctx, k);
    (h.guild_kill)(s, ctx, k);
    (h.pvp_fame)(s, ctx, k);
    // 5. `AddKillRecovery` of the killer
    add_kill_recovery(s, ctx, k.attacker);
    if !victim_is_player {
        (h.pet_exp)(s, ctx, k);
    }
    (h.monster_buffs)(s, ctx, k);
    // 9. a monster that is already dead is simply removed
    if !victim_is_player && victim_dead_state {
        ctx.worlds.delete(k.victim);
        return;
    }
    (h.kill_exp)(s, ctx, k);
    (h.drop)(s, ctx, k);
    chaotic_death_loss(s, ctx, k);
    if let Some(a) = ctx.worlds.mover_mut(k.attacker) {
        a.sim.atk_cnt = 0;
    }
    do_die(s, ctx, k, frame);
    if let Some(a) = ctx.worlds.mover_mut(k.attacker) {
        a.sim.clear_dest_obj();
    }
}

/// `AddKillRecovery()` of the killer: `+DST_KILL_HP/MP/FP` and the `XI_KILL_RECOVERY` effect (doc 16).
fn add_kill_recovery(s: &mut Services, ctx: &mut GameCtx<'_>, killer: ObjId) {
    let core = s.stats.handle();
    let mut out = Out::new();
    if let Some(m) = ctx.worlds.mover_mut(killer) {
        crate::stats::tick::add_kill_recovery(m, &core, &mut out);
    }
    out.flush(ctx, killer);
}

/// Step 12: a chaotic player who dies (outside the arena) loses `x_random_range(die_min, die_max + 1)` points of
/// propensity (never below 0), told to the viewers (`PkRelation{PROPENSITY}`, §8.3).
fn chaotic_death_loss(s: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let Some(v) = ctx.worlds.mover(k.victim) else {
        return;
    };
    if !v.is_player() || !is_chaotic(v) || v.core.world.0 == WI_WORLD_ARENA {
        return;
    }
    let world = v.core.world;
    let (min, max) = {
        let pk = &s.stats.core().data.pk;
        (pk.die_propensity_min, pk.die_propensity_max)
    };
    let loss = x_random_range(ctx.rng.world(world), min, max.wrapping_add(1));
    let mut out = Out::new();
    if let Some(p) = ctx.worlds.mover_mut(k.victim).and_then(|m| m.player_mut()) {
        let old = p.pk.propensity;
        let new = old.wrapping_sub(loss as u32);
        // `if (old <= new) set 0`: the unsigned subtraction wrapped
        p.pk.propensity = if old <= new { 0 } else { new };
        out.near(pb::PkRelation {
            sub: Some(pb::pk_relation::Sub::Propensity(
                pb::pk_relation::PkPropensity {
                    propensity: p.pk.propensity,
                },
            )),
        });
    }
    out.flush(ctx, k.victim);
}

// ---- DoDie ---------------------------------------------------------------------------------------------------------

/// `CMover::DoDie(attacker, msg)` (§9.2). Returns `false` when the double-death guard refused it.
pub fn do_die(s: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx, frame: SimFrame) -> bool {
    let h = s.combat.death;
    let (victim, attacker) = (k.victim, k.attacker);
    let Some(v) = ctx.worlds.mover(victim) else {
        return false;
    };
    let (victim_is_player, dead_ticks) = (v.is_player(), v.combat.dead_ticks);
    // 1. a player cannot die twice within the 5 s after a revival (the loser of a duel result is invulnerable
    // for 2 s *because* it is about to die: `finishing` lets that death through, Plan/17 §8.6)
    if victim_is_player && dead_ticks > 0 && !v.combat.duel.finishing {
        tracing::warn!(
            victim = victim.0,
            "do_die: a second death within the revival invulnerability"
        );
        return false;
    }
    // 2. m_bLastPK / m_bGuildCombat
    let flags = (h.pk_flags)(s, ctx, k);
    if let Some(p) = ctx.worlds.mover_mut(victim).and_then(|m| m.player_mut()) {
        p.death_flags.last_pk = flags.last_pk;
        p.death_flags.guild_combat = flags.guild_combat;
        // death ends the NPC shop window (19 §7.2)
        p.econ.end_shop_session();
    }
    // 3. the monster-hunt honor, the attack counter, a dying player's abnormal states
    let attacker_is_player = ctx.worlds.mover(attacker).is_some_and(|a| a.is_player());
    if !victim_is_player && attacker_is_player {
        (h.honor_hunt)(s, ctx, k);
    }
    if victim_is_player {
        (h.clear_status)(s, ctx, k);
    }
    let world_of = victim;
    // 4. STOP, DIE; 5. the mob I last hit is released
    with_action_at(s, ctx, world_of, frame, |cx| {
        if let Some(m) = cx.mover_mut(victim) {
            m.sim.atk_cnt = 0;
            // the knock-back death ends in DEAD, never back to life (`m_nHitPoint > 0` is false)
            m.sim.params.hp_positive = false;
        }
        let killer = cx.mover(attacker).map(|a| (attacker, a.sim.angle));
        cx.arb.fx.sim.push((victim, SimFx::Stop));
        cx.arb.fx.sim.push((
            victim,
            SimFx::Die {
                cause: k.msg,
                attacker: killer,
            },
        ));
        // `PostAIMsg(AIMSG_DIE, attacker)` of the DIE message
        cx.arb.fx.mail.push((victim, AiMsg::Die));
        // a monster killer whose attacker was the dying mover forgets it (and its claim)
        if let Some(a) = cx.mover_mut(attacker)
            && !a.is_player()
            && a.combat.id_attacker == Some(victim)
        {
            a.combat.id_attacker = None;
            a.combat.id_targeter = None;
        }
        if victim_is_player {
            release_last_hit(cx, victim);
        }
    });
    if victim_is_player {
        let killed_by_npc = ctx.worlds.mover(attacker).is_none_or(|a| !a.is_player());
        if killed_by_npc {
            (h.owner_pets)(s, ctx, k);
        }
    }
    // the duel part of OBJMSG_DIE: the duel with the killer ends, a party duelist is put out
    (h.duel_die)(s, ctx, k);
    // 6. MoverDeath, RemoveAllEnemies, the killer forgets its target
    with_action_at(s, ctx, world_of, frame, |cx| {
        let attacker_id = cx.mover(attacker).map_or(0, |_| attacker.0);
        cx.arb.fx.near(
            victim,
            pb::MoverDeath {
                attacker: attacker_id,
                attack_msg: k.msg,
            },
        );
        remove_all_enemies(&mut cx.arb.w, &mut cx.arb.fx, victim);
        let attacker_is_npc = cx.mover(attacker).is_some_and(|a| !a.is_player());
        if let Some(a) = cx.mover_mut(attacker) {
            a.sim.act.objid_hit = ObjId::NONE;
        }
        if attacker_is_npc {
            cx.arb.fx.mail.push((attacker, AiMsg::InitTargetClear));
        }
    });
    // 7. the quest kill counters
    if !victim_is_player && attacker_is_player {
        (h.quest_kill)(s, ctx, k);
    }
    // 8. RemoveCommonBuffs, RemoveAllSkillInfluence
    (h.remove_buffs)(s, ctx, k);
    with_action_at(s, ctx, world_of, frame, |cx| {
        cx.arb.fx.near(victim, pb::RemoveAllSkillInfluence {});
    });
    // 9. siege, secret room, CreateMonster, instance dungeon, Event.lua bookkeeping
    crate::ai::create_monster::on_died(s, ctx, k.victim);
    (h.world_die)(s, ctx, k);
    true
}

/// A dying player lets go of the mob it last hit: the mob forgets it as its attacker and its claimer.
fn release_last_hit(cx: &mut ActionCx<'_>, player: ObjId) {
    let Some(last) = cx.mover(player).and_then(|m| m.combat.id_last_hit) else {
        return;
    };
    if let Some(mob) = cx.mover_mut(last) {
        if mob.combat.id_attacker == Some(player) {
            mob.combat.id_attacker = None;
        }
        if mob.combat.id_targeter == Some(player) {
            mob.combat.id_targeter = None;
        }
    }
}

// ---- revival ---------------------------------------------------------------------------------------------------------

/// `GetMaxHitPoint() * rate` as the legacy `(int)` of a `float` product.
fn pool_at(max: i32, rate: f32) -> i32 {
    c_int(max as f32 * rate)
}

/// Sets the revival pools: HP to `rate * max`, MP and FP to `rate * max` (`raise_only`: only when lower), and
/// sends the three absolute values (PL-083). The admin console's resurrection (25 §3.12) uses it too.
pub(crate) fn revive_pools(
    m: &mut crate::mover::Mover,
    core: &StatsCore,
    rate: f32,
    raise_only: bool,
    out: &mut Out,
) {
    use crate::stats::env::{max_fp, max_hp, max_mp};
    let (mh, mm, mf) = (max_hp(m, core), max_mp(m, core), max_fp(m, core));
    set_hp_silent(m, core, pool_at(mh, rate));
    let (mp, fp) = (pool_at(mm, rate), pool_at(mf, rate));
    if !raise_only || m.mv.mp < mp {
        set_mp_silent(m, core, mp);
    }
    if !raise_only || m.mv.fp < fp {
        set_fp_silent(m, core, fp);
    }
    m.sim.params.hp_positive = m.mv.hp > 0;
    send_pools_absolute(m, core, out);
}

/// What both revival requests share: the player is dead (else the request is ignored), the offered resurrection
/// is cleared. Returns the player's world.
fn revival_start(ctx: &mut GameCtx<'_>, who: PlayerRef) -> Option<WorldId> {
    let m = ctx.worlds.player_mut(who.0)?;
    m.combat.resurrection = None;
    is_die(m).then_some(m.core.world)
}

/// `RevivalReq`: revive in place with a Resurrection scroll (`OnRevival`, `DPSrvr.cpp:994-1093`). Ignored unless
/// dead and holding a usable scroll. Chaotic players pay the x0.9 penalty; 20 % pools (100 % in the guild-war
/// world, where the player joins the observers and the scroll is kept); the scroll is consumed otherwise.
pub fn on_revival_req(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(world) = revival_start(ctx, who) else {
        return;
    };
    let core = s.stats.handle();
    let scroll = ctx.worlds.player(who.0).and_then(|m| {
        m.player()?
            .items
            .inventory
            .find(II_SYS_SYS_SCR_RESURRECTION)
            .filter(|(_, it)| !it.is_expired())
            .map(|(_, it)| it.item_id)
    });
    let Some(scroll_id) = scroll else {
        tracing::debug!(player = who.0.0, "RevivalReq without a Resurrection scroll");
        return;
    };
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        m.combat.dead_ticks = REVIVAL_INVULNERABLE_TICKS;
    }
    if ctx.worlds.player(who.0).is_some_and(is_chaotic) {
        crate::stats::death_penalty::apply_for(s, ctx, who, true, 0, true);
    }
    let guild_war = world.0 == WI_WORLD_GUILDWAR;
    let rate = if guild_war { 1.0 } else { REVIVAL_RATE };
    let frame = ctx.clock.frame0;
    let sfx3 = core
        .data
        .item(scroll_id)
        .map(|p| p.sfx_obj[2])
        .filter(|s| *s != flyff_rules::consts::NULL_ID);
    with_action_at(s, ctx, who.0, frame, |cx| {
        cx.arb.fx.sim.push((who.0, SimFx::ClearState));
        cx.arb.fx.near(who.0, pb::RevivalNtf {});
        if !guild_war && let Some(sfx) = sfx3 {
            cx.arb.fx.near(
                who.0,
                pb::CreateSfxObj {
                    sfx_id: sfx,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    flag: false,
                },
            );
        }
    });
    if guild_war {
        (s.combat.death.gw_observer)(s, ctx, who);
    } else {
        crate::item::ops::remove_item_by_id(
            s,
            ctx,
            who,
            II_SYS_SYS_SCR_RESURRECTION,
            Some(1),
            RemoveReason::Consume,
        );
    }
    let mut out = Out::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        revive_pools(m, &core, rate, true, &mut out);
    }
    out.flush(ctx, who.0);
    (s.combat.death.after_revival)(s, ctx, who);
}

/// `RevivalToLodestarReq`: respawn at the revival point (`OnRevivalLodestar`, `DPSrvr.cpp:1095-1220`). The EXP
/// penalty is paid, the pools are 20 % (100 % in the guild-war world and for a 1:1 guild-combat player), the
/// destination is: stay in the guild-war world as an observer; the secret-room revival position; the Madrigal
/// point of the three boss dungeons; the world's revival region (the "Chao" variants for a chaotic player outside
/// the arena); the current place when no region is found.
pub fn on_revival_to_lodestar_req(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(world) = revival_start(ctx, who) else {
        return;
    };
    let core = s.stats.handle();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        m.combat.dead_ticks = REVIVAL_INVULNERABLE_TICKS;
    }
    let _ = crate::stats::death_penalty::apply_for(s, ctx, who, true, 0, false);
    let guild_war = world.0 == WI_WORLD_GUILDWAR;
    let gc1 = matches!(
        world.0,
        WI_WORLD_GUILDWAR1TO1_0
            | WI_WORLD_GUILDWAR1TO1_1
            | WI_WORLD_GUILDWAR1TO1_2
            | WI_WORLD_GUILDWAR1TO1_3
    ) || (s.combat.death.gc_1to1_player)(s, ctx, who);
    let rate = if guild_war || gc1 { 1.0 } else { REVIVAL_RATE };
    let frame = ctx.clock.frame0;
    with_action_at(s, ctx, who.0, frame, |cx| {
        cx.arb.fx.sim.push((who.0, SimFx::ClearState));
        if guild_war {
            cx.arb.fx.near(who.0, pb::RevivalNtf {});
        } else {
            cx.arb.fx.near(who.0, pb::RevivalToLodestarNtf {});
        }
    });
    let mut out = Out::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        revive_pools(m, &core, rate, false, &mut out);
    }
    out.flush(ctx, who.0);
    if guild_war {
        (s.combat.death.gw_observer)(s, ctx, who);
        (s.combat.death.after_revival)(s, ctx, who);
        return;
    }
    // where to
    let (pos, layer, chaotic) = match ctx.worlds.player(who.0) {
        Some(m) => (m.core.pos, m.core.layer, is_chaotic(m)),
        None => return,
    };
    let dest: Option<(WorldId, Vec3, LayerId, ReplaceKind)> =
        if let Some((w, p)) = (s.combat.death.secret_room_revival)(s, ctx, who) {
            Some((w, p, LayerId(0), ReplaceKind::Normal))
        } else if matches!(
            world.0,
            WI_DUNGEON_MUSCLE | WI_DUNGEON_KRRR | WI_DUNGEON_BEAR
        ) {
            Some((
                WorldId(WI_WORLD_MADRIGAL),
                MADRIGAL_BOSS_REVIVAL,
                LayerId(0),
                ReplaceKind::Normal,
            ))
        } else {
            let arena = world.0 == WI_WORLD_ARENA;
            let settings = ctx.worlds.world(world).map(|w| w.statics.settings.clone());
            settings
                .and_then(|st| {
                    ctx.worlds
                        .revival
                        .select(world, &st, pos, chaotic && !arena)
                })
                .map(|p| (p.world, p.pos, LayerId(0), ReplaceKind::Force))
        };
    let (w, p, l, kind) = dest.unwrap_or((world, pos, layer, ReplaceKind::Force));
    if let Err(e) = ctx.worlds.replace(who.0, w, p, l, kind) {
        tracing::debug!(player = who.0.0, error = %e, "lodestar revival: the replace was refused");
    }
    (s.combat.death.after_revival)(s, ctx, who);
}

// ---- the logout of a dead player -------------------------------------------------------------------------------

/// `combat::death::on_logout_dead` (§9.4): a player that leaves the game dead pays the death penalty of the
/// table and revives with 30..80 % pools at the revival point (doc 16's `logout_while_dead`; the position is
/// doc 15's `select_revival`). No message is sent. Installed as `LogoutStep::DeadLogout` by [`CombatLogout`].
pub fn on_logout_dead(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    crate::stats::death_penalty::logout_while_dead(s, ctx, who);
}

/// The combat cleanups of the logout (15 §18.2): the enemies of a leaving player drop it, and a dead player pays
/// its penalty ([`on_logout_dead`], chained after `StatsLogout` in the server wiring or used instead of it).
pub struct CombatLogout {
    inner: Option<Box<dyn LogoutHooks>>,
}

impl CombatLogout {
    /// The combat logout hooks alone.
    pub fn new() -> CombatLogout {
        CombatLogout { inner: None }
    }

    /// Chains `inner` behind them (it receives every step and answers the predicates).
    pub fn with_inner(mut self, inner: Box<dyn LogoutHooks>) -> CombatLogout {
        self.inner = Some(inner);
        self
    }
}

impl Default for CombatLogout {
    fn default() -> Self {
        CombatLogout::new()
    }
}

impl LogoutHooks for CombatLogout {
    fn step(
        &mut self,
        step: LogoutStep,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        player: PlayerRef,
    ) {
        match step {
            LogoutStep::RemoveAllEnemies | LogoutStep::RemoveAllEnemiesGuildWar => {
                let frame = ctx.clock.frame0;
                with_action_at(svc, ctx, player.0, frame, |cx| {
                    remove_all_enemies(&mut cx.arb.w, &mut cx.arb.fx, player.0);
                });
                // the claims the player held end with it
                super::target::release_claims(ctx.worlds.store_mut(), player.0);
            }
            _ => {}
        }
        if let Some(i) = self.inner.as_mut() {
            i.step(step, svc, ctx, player);
        }
    }

    fn secret_room_position(&self, ctx: &GameCtx<'_>, player: PlayerRef) -> Option<Vec3> {
        self.inner
            .as_ref()
            .and_then(|i| i.secret_room_position(ctx, player))
    }

    fn is_instance_world(&self, world: WorldId) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|i| i.is_instance_world(world))
    }

    fn is_chaotic(&self, ctx: &GameCtx<'_>, player: PlayerRef) -> bool {
        match self.inner.as_ref() {
            Some(i) => i.is_chaotic(ctx, player),
            None => ctx.player(player).is_some_and(is_chaotic),
        }
    }
}

/// `OBJMSG_DAMAGE` / `OBJMSG_DAMAGE_FORCE` of a [`KillCtx`].
pub fn is_forced(k: &KillCtx) -> bool {
    k.msg == objmsg::DAMAGE_FORCE
}
