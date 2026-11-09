//! What a death means for PvP (Plan/17 §8.3, §8.8; `Mover.cpp:5679-5960`, `AttackArbiter.cpp:884-900`): `SubPVP`
//! with `GetPVPCase`, `SubPK`, `SubWar`, the fame of a PvP kill and the `m_bLastPK` flag of `DoDie`.
//!
//! These are the functions [`crate::combat::DeathHooks`] installs by default: [`sub_pvp`] (step 2 of `OnDied`),
//! [`pvp_fame`] (step 4) and [`pk_flags`] (`DoDie` step 2).

use super::PvpEnv;
use super::drop::drop_random_item;
use super::hit::{is_arena_target, is_guild_combat_target, is_pvp_target, is_war_target};
use super::pk::{
    is_pink, next_pk_propensity, pk_value_relation, propensity_penalty, propensity_relation,
    set_pk_propensity, set_pk_value,
};
use crate::combat::death::PkFlags;
use crate::combat::duel;
use crate::combat::hooks::KillCtx;
use crate::combat::state::is_chaotic;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::social::hooks::{PvpDeathMode, on_player_killed};
use crate::stats::out::Out;
use flyff_common::ObjId;
use flyff_common::defs::sound::SND_PC_CHAOS;
use flyff_common::defs::text::{TID_GAME_PK_ATTACKER, TID_GAME_PK_DEFENDER};
use flyff_common::defs::world::{WI_WORLD_ARENA, WI_WORLD_GUILDWAR};
use flyff_common::gm::Authority;
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::combat::x_random_range;
use serde_json::json;

/// `PVP_MODE`: what kind of fight a player's death was part of (`GetPVPCase`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PvpCase {
    /// Not PvP: the killer or the victim is an NPC, or `EVE_18` is off. `SubPK` still runs (the chaotic
    /// drop applies to deaths by monsters too).
    None,
    /// A kill outside a duel.
    Pk,
    /// A duel.
    Duel,
    /// The same guild war.
    GuildWar,
}

/// `GetPVPCase(attacker)` (this = `victim`): `EVE_18` off, an NPC on either side -> `None`; the same war id ->
/// `GuildWar`; no duel, or a party duelist already out (state 300) -> `Pk`; else `Duel`.
pub fn get_pvp_case(env: &PvpEnv, victim: &Mover, attacker: Option<&Mover>) -> PvpCase {
    let Some(attacker) = attacker else {
        return PvpCase::None;
    };
    if !env.eve_18 || !attacker.is_player() || !victim.is_player() {
        return PvpCase::None;
    }
    let war = |m: &Mover| m.player().map_or(0, |p| p.social.war);
    if war(victim) != 0 && war(attacker) == war(victim) {
        return PvpCase::GuildWar;
    }
    if victim.combat.duel.duel == 0 || victim.combat.duel.state == 300 {
        return PvpCase::Pk;
    }
    PvpCase::Duel
}

/// `CMover::SubPVP(attacker, reflect)` (Plan/17 §8.8): the duel clean-up of a victim killed by somebody else,
/// then the dispatch on [`get_pvp_case`].
pub fn sub_pvp(s: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let env = PvpEnv::from_services(s, ctx);
    let (v, a) = (k.victim, k.attacker);
    let (Some(victim), Some(attacker)) = (ctx.worlds.mover(v), ctx.worlds.mover(a)) else {
        return;
    };
    // a solo duelist killed by somebody who is not its partner: both duels end (no result)
    match victim.combat.duel.duel {
        1 if attacker.combat.duel.other != Some(v) => duel::cancel_solo(s, ctx, v),
        2 if super::hit::duel_party_id(victim) != super::hit::party_id(attacker)
            || attacker.combat.duel.state == 300 =>
        {
            duel::cancel_party_of_leader(s, ctx, v);
        }
        _ => {}
    }
    let (Some(victim), attacker) = (ctx.worlds.mover(v), ctx.worlds.mover(a)) else {
        return;
    };
    match get_pvp_case(&env, victim, attacker) {
        PvpCase::GuildWar => sub_war(s, ctx, k),
        PvpCase::None | PvpCase::Pk => sub_pk(s, ctx, &env, k),
        PvpCase::Duel => duel::sub_duel(s, ctx, a, v),
    }
}

/// `SubWar(attacker)`: a member of an enemy guild killed a player during a war (`EVE_GUILDWAR`, different
/// guilds): doc 21 scores it ([`on_player_killed`]).
fn sub_war(s: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let env = PvpEnv::from_services(s, ctx);
    if !env.guild_war {
        return;
    }
    let guild = |id: ObjId| {
        ctx.worlds
            .mover(id)
            .and_then(|m| m.player())
            .map(|p| p.social.guild)
    };
    if guild(k.victim) == guild(k.attacker) {
        return;
    }
    on_player_killed(
        s,
        ctx,
        Some(PlayerRef(k.attacker)),
        PlayerRef(k.victim),
        PvpDeathMode::GuildWar,
    );
}

/// `SubPK(attacker, reflect)` (`Mover.cpp:5679-5840`, Plan/17 §8.3): the chaotic item drop of `EVE_PKCOST`, and
/// the kill count and propensity of a player killer.
fn sub_pk(s: &mut Services, ctx: &mut GameCtx<'_>, env: &PvpEnv, k: &KillCtx) {
    if !env.pk {
        return;
    }
    let (v, a) = (k.victim, k.attacker);
    let (Some(victim), Some(attacker)) = (ctx.worlds.mover(v), ctx.worlds.mover(a)) else {
        return;
    };
    // the arena, and the guild-combat worlds while a combat runs, are exempt
    if victim.core.world.0 == WI_WORLD_ARENA {
        return;
    }
    let gw_open = (s.combat.death.gw_open)(s, ctx);
    let in_gw = |m: &Mover| m.core.world.0 == WI_WORLD_GUILDWAR && gw_open;
    if in_gw(victim) || in_gw(attacker) {
        return;
    }
    if victim.is_player()
        && attacker.is_player()
        && (s.combat.death.gc_1to1_player)(s, ctx, PlayerRef(v))
        && (s.combat.death.gc_1to1_player)(s, ctx, PlayerRef(a))
    {
        return;
    }
    // the cost of being chaotic: items fall to the ground (not for staff of authority HELPER and above)
    if env.pk_cost && victim.is_player() && is_chaotic(victim) {
        let helper = victim
            .player()
            .is_some_and(|p| p.ident.authority >= Authority::HELPER);
        if !helper {
            drop_for_chaotic(s, ctx, v);
        }
    }
    let (Some(victim), Some(attacker)) = (ctx.worlds.mover(v), ctx.worlds.mover(a)) else {
        return;
    };
    if !attacker.is_player() || !victim.is_player() {
        return;
    }
    let add = if is_chaotic(attacker) {
        !is_chaotic(victim)
    } else {
        !(is_chaotic(victim) || is_pink(victim))
    };
    if !(add && k.reflect == 0 && a != v) {
        return;
    }
    let (attacker_name, victim_name) = (
        attacker
            .player()
            .map(|p| p.ident.name.clone())
            .unwrap_or_default(),
        victim
            .player()
            .map(|p| p.ident.name.clone())
            .unwrap_or_default(),
    );
    let was_chaotic = is_chaotic(attacker);
    // the messages
    let defender_text =
        crate::chat::ingress::notice::defined_text(TID_GAME_PK_DEFENDER, vec![attacker_name]);
    let attacker_text =
        crate::chat::ingress::notice::defined_text(TID_GAME_PK_ATTACKER, vec![victim_name]);
    let mut vout = Out::new();
    vout.to_self(defender_text);
    vout.flush(ctx, v);
    let mut aout = Out::new();
    if !was_chaotic {
        aout.to_self(pb::PlaySound {
            sound: Some(pb::play_sound::Sound::Id(SND_PC_CHAOS)),
        });
    }
    aout.to_self(attacker_text);
    // the new kill count and propensity
    let (value, propensity) = {
        let Some(att) = ctx.worlds.mover_mut(a) else {
            return;
        };
        let new_value = att.player().map_or(0, |p| p.pk.value).wrapping_add(1);
        set_pk_value(att, new_value);
        let value = att.player().map_or(0, |p| p.pk.value);
        let next = next_pk_propensity(value);
        let old = att.player().map_or(0, |p| p.pk.propensity);
        set_pk_propensity(att, old.wrapping_add(next));
        (value, att.player().map_or(0, |p| p.pk.propensity))
    };
    aout.to_self_subject(pk_value_relation(value));
    aout.near(propensity_relation(propensity));
    aout.flush(ctx, a);
    crate::audit::emit_for(
        ctx,
        PlayerRef(a),
        ev::CHAR_PK,
        json!({ "kind": "kill", "victim": v.0, "pk_value": value, "propensity": propensity }),
    );
}

/// The item loss of a dying chaotic player (`EVE_PKCOST`): `x_random_range(inven_min, inven_max + 1)` random
/// bag items, then `x_random_range(equip_min, equip_max + 1)` worn ones; a round stops at the first miss.
fn drop_for_chaotic(s: &mut Services, ctx: &mut GameCtx<'_>, victim: ObjId) {
    let Some(propensity) = ctx
        .worlds
        .mover(victim)
        .and_then(|m| m.player())
        .map(|p| p.pk.propensity)
    else {
        return;
    };
    let Some(row) = propensity_penalty(&s.stats.core().data.pk, propensity).cloned() else {
        return;
    };
    let world = ctx.worlds.core(victim).map(|c| c.world);
    let Some(world) = world else { return };
    let n_inven = x_random_range(
        ctx.rng.world(world),
        row.inven_drop_min,
        row.inven_drop_max.wrapping_add(1),
    );
    for _ in 0..n_inven.max(0) {
        if !drop_random_item(s, ctx, victim, true, false) {
            break;
        }
    }
    let n_equip = x_random_range(
        ctx.rng.world(world),
        row.equip_drop_min,
        row.equip_drop_max.wrapping_add(1),
    );
    for _ in 0..n_equip.max(0) {
        if !drop_random_item(s, ctx, victim, false, true) {
            break;
        }
    }
}

/// `__PVP_POINTS_PER_KILL` (`AttackArbiter.cpp:884-900`, step 4 of `OnDied`): +1 fame for the killer of a duel,
/// war, guild-combat or arena target (`SetFame` to the viewers).
pub fn pvp_fame(s: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let env = PvpEnv::from_services(s, ctx);
    if !env.pvp_points_per_kill {
        return;
    }
    let (Some(victim), Some(attacker)) = (ctx.worlds.mover(k.victim), ctx.worlds.mover(k.attacker))
    else {
        return;
    };
    if !(attacker.is_player() && victim.is_player()) {
        return;
    }
    let hooks = &*ctx.worlds.movement.combat.hooks;
    let pvp = is_pvp_target(attacker, victim)
        || is_war_target(&env, attacker, victim)
        || is_guild_combat_target(attacker, victim, hooks)
        || is_arena_target(attacker, victim);
    if !pvp {
        return;
    }
    let fame = {
        let Some(p) = ctx
            .worlds
            .mover_mut(k.attacker)
            .and_then(|m| m.player_mut())
        else {
            return;
        };
        p.progression.fame = p.progression.fame.wrapping_add(1);
        p.progression.fame
    };
    let mut out = Out::new();
    out.near(pb::SetFame { fame });
    out.flush(ctx, k.attacker);
    crate::audit::emit_for(
        ctx,
        PlayerRef(k.attacker),
        ev::CHAR_PK,
        json!({ "kind": "pvp_point", "victim": k.victim.0, "fame": fame }),
    );
}

/// `m_bLastPK` / `m_bGuildCombat` of `DoDie` (Plan/17 §9.2 step 2): killed by another player, the victim keeps its
/// EXP - except in a PK fight with `EVE_PKCOST` outside the arena, where the death costs EXP like any other.
pub fn pk_flags(s: &Services, ctx: &GameCtx<'_>, k: &KillCtx) -> PkFlags {
    let env = PvpEnv::from_services(s, ctx);
    let (Some(victim), Some(attacker)) = (ctx.worlds.mover(k.victim), ctx.worlds.mover(k.attacker))
    else {
        return PkFlags::default();
    };
    if !(attacker.is_player() && k.attacker != k.victim) {
        return PkFlags::default();
    }
    let last_pk = match get_pvp_case(&env, victim, Some(attacker)) {
        PvpCase::Pk => !(env.pk_cost && victim.core.world.0 != WI_WORLD_ARENA),
        _ => true,
    };
    let guild_combat = (victim.core.world.0 == WI_WORLD_GUILDWAR
        && (s.combat.death.gw_open)(s, ctx))
        || (s.combat.death.gc_1to1_player)(s, ctx, PlayerRef(k.victim));
    PkFlags {
        last_pk,
        guild_combat,
    }
}
