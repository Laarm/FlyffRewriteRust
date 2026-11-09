//! The arenas (Plan/21 §8.5; WP-SOCIAL-26; `GuildCombat1to1.cpp:34-387` the arena, `390-675` the rounds and the
//! teleports, `819-844` the pairing, `1127-1174` the war and the close, `1592-1663` the entrance): the pairing into the
//! worlds 241..=244, the entrance and its two teleports, the rounds (wait, war, finish), the red chips, the close wait,
//! the close, the deaths, the logout rule and the sweep of stale combat states.
//!
//! **Timers** are `due` deadlines on the game clock; the manager's slot runs once a second ([`super::process`]).
//! **"Present"** (`IsPossibleUser`) is: online, avatar not deleted, CURRENT guild = the side's guild, standing in the
//! arena's world. The k-th fighter of a side is `lineup[prog]` when it exists.
//!
//! **A round** (`war_wait` -> Wait -> `war` -> War -> `result_check` -> Finish -> `finish` -> `war_wait` ...): a death
//! of a fighter takes his side's life (the round ends when it is 0), a timer that ran out decides by the lives left
//! (equal = draw), an absent fighter loses (both absent = draw). The winner of a round gets `WinChipNum` red chips,
//! delivered into the inventory or, on a full inventory, as a system mail, committed at once in ONE batch. After the
//! last round `close_wait` decides the guild (more round wins; every present member of the winner gets the chips again),
//! writes both tender rows `T -> E` in ONE batch with the chip snapshots and mails, and `close` sends everybody to
//! Madrigal. FIXES of the original: the deaths of anybody but the current fighter re-ran the result check (a death in
//! the entrance could decide round 0 twice, a death after the arena closed paid the chips again); the logout check ran in
//! the up-to-one-second window after a decided round (counted twice); the closing teleport was not forced.

mod close;
mod round;

use close::close;
pub use close::sweep;
pub(crate) use close::{all_close, close_wait};
pub(crate) use round::result_check;
use round::{finish, war, war_wait};
pub use round::{on_logout, on_world_die};

use super::{
    Arena, ArenaState, FIRST_ARENA, Gc1to1State, PlayerResult, Side, character_of, defined,
    enabled, remaining, send_manager_state, send_one, small_caption, text_of,
};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::siege::fight::{live_avatar, name_of, set_gc_state};
use crate::world::ReplaceKind;
use flyff_common::defs::text::{
    TID_GAME_GUILDCOMBAT1TO1_ALLCLOSE, TID_GAME_GUILDCOMBAT1TO1_ENTRANCETIME,
    TID_GAME_GUILDCOMBAT1TO1_NOENTRANCETIME, TID_GAME_GUILDCOMBAT1TO1_NOT_GUILD_MEMBER,
};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{CharacterId, GameMs, GuildId, LayerId, ObjId, WorldId};
use flyff_proto::pb;
use serde_json::json;

/// Where a closed arena sends everybody (`GuildCombat1to1Close`, `TeleportToNpc`): Madrigal, layer 0.
pub const MADRIGAL_SPOT: Vec3 = Vec3::new(6983.0, 0.0, 3326.0);
/// The waiting area of side 0 (centre; `x`, `z` get `xRandom(4) - 2`), angle 0.
pub const WAIT_SPOT_0: Vec3 = Vec3::new(256.0, 90.0, 277.0);
/// The waiting area of side 1 (centre), angle 180.
pub const WAIT_SPOT_1: Vec3 = Vec3::new(272.0, 90.0, 212.0);
/// The fight spot of side 0, angle 0.
pub const WAR_SPOT_0: Vec3 = Vec3::new(260.0, 75.0, 252.0);
/// The fight spot of side 1, angle 180.
pub const WAR_SPOT_1: Vec3 = Vec3::new(260.0, 75.0, 235.0);

impl Arena {
    /// Side `i` (0 or 1; anything else is side 1).
    pub fn side(&self, i: usize) -> &Side {
        if i == 0 {
            &self.sides[0]
        } else {
            &self.sides[1]
        }
    }

    fn side_mut(&mut self, i: usize) -> &mut Side {
        if i == 0 {
            &mut self.sides[0]
        } else {
            &mut self.sides[1]
        }
    }

    /// The k-th fighter of `side`: `lineup[prog]` when the line-up is that long.
    pub fn kth(&self, side: usize) -> Option<CharacterId> {
        self.side(side).lineup.get(self.prog).copied()
    }

    /// The side of `guild`.
    pub fn side_of(&self, guild: GuildId) -> Option<usize> {
        self.sides.iter().position(|s| s.guild == guild)
    }
}

fn arena_clone(svc: &Services, idx: usize) -> Option<Arena> {
    svc.social.gc1to1.arenas.get(idx).cloned()
}

fn arena_mut(svc: &mut Services, idx: usize) -> Option<&mut Arena> {
    svc.social.gc1to1.arenas.get_mut(idx)
}

// ---- who is present --------------------------------------------------------------------------------------------------------

/// The avatar of `c` when it is present for `side` of `a` (`CGuildCombat1to1::IsPossibleUser`).
fn present(
    svc: &Services,
    ctx: &GameCtx<'_>,
    a: &Arena,
    side: usize,
    c: CharacterId,
) -> Option<ObjId> {
    if svc.social.guild.guild_of(c) != Some(a.side(side).guild) {
        return None;
    }
    let obj = live_avatar(&svc.presence, &*ctx.worlds, c)?;
    (ctx.worlds.core(obj)?.world == a.world).then_some(obj)
}

/// The avatar of the k-th fighter of `side` when present; `leaver` counts as absent.
fn present_kth(
    svc: &Services,
    ctx: &GameCtx<'_>,
    a: &Arena,
    side: usize,
    leaver: Option<CharacterId>,
) -> Option<ObjId> {
    let c = a.kth(side)?;
    if leaver == Some(c) {
        return None;
    }
    present(svc, ctx, a, side, c)
}

/// Every present line-up member of both sides: `(side, character, avatar)`, side 0 first, in line-up order.
fn present_members(
    svc: &Services,
    ctx: &GameCtx<'_>,
    a: &Arena,
) -> Vec<(usize, CharacterId, ObjId)> {
    let mut out = Vec::new();
    for side in 0..2 {
        for c in &a.side(side).lineup {
            if let Some(obj) = present(svc, ctx, a, side, *c) {
                out.push((side, *c, obj));
            }
        }
    }
    out
}

// ---- the messages of an arena ------------------------------------------------------------------------------------------------

/// The `Gc1to1NowState` of the arena (`CGuildCombat1to1::SendNowState`, `140-167`): Wait `{101, remaining, prog_count}`,
/// War and CloseWait `{state, remaining}`, Finish and Close `{state, 0}`.
fn arena_state_msg(a: &Arena, now: GameMs) -> pb::Gc1to1NowState {
    let (time_ms, prog_count) = match a.state {
        ArenaState::Wait => (
            remaining(a.due, now),
            i32::try_from(a.prog).unwrap_or(i32::MAX),
        ),
        ArenaState::War | ArenaState::CloseWait => (remaining(a.due, now), 0),
        ArenaState::Finish | ArenaState::Close => (0, 0),
    };
    pb::Gc1to1NowState {
        state: a.state.code(),
        time_ms,
        members: Vec::new(),
        enemy_guild: 0,
        prog_count,
    }
}

/// `SendNowStateAllPlayer`: the arena state to every present line-up member.
fn send_state_all(svc: &Services, ctx: &mut GameCtx<'_>, a: &Arena) {
    let msg = arena_state_msg(a, ctx.clock.game_ms);
    for (_, _, obj) in present_members(svc, ctx, a) {
        send_one(ctx, obj, msg.clone());
    }
}

// ---- the pairing and the entrance ------------------------------------------------------------------------------------------------

/// The multimap of the pairing for the drawn `keys` (one per tender in rank order): the pairs `(key_i, i / 2)` stable-sorted
/// by key; the j-th pair's `i / 2` is the stage of the j-th tender. `stages_from_keys(&[1, 0, 1, 0]) == [0, 1, 0, 1]`.
pub(crate) fn stages_from_keys(keys: &[u32]) -> Vec<u8> {
    let mut pairs: Vec<(u32, u8)> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| (*k, u8::try_from(i / 2).unwrap_or(u8::MAX)))
        .collect();
    pairs.sort_by_key(|(key, _)| *key); // stable: a multimap keeps the insertion order of equal keys
    pairs.into_iter().map(|(_, stage)| stage).collect()
}

/// `CreateGuildCombat1to1` (`819-844`): the original's multimap reproduced exactly (it is NOT a uniform shuffle). With
/// n tenders (even) and for i = 0..n in rank order a key `xRandom(n)` is drawn from the stream of world 241, the pairs
/// `(key_i, i / 2)` are stable-sorted by key (a multimap keeps the insertion order of equal keys), and the j-th pair's
/// `i / 2` becomes the stage of the j-th tender in rank order. Side 0 of an arena is the first tender of its stage in
/// rank order. Same seed, same pairs.
pub fn pair(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let n = svc.social.gc1to1.tenders.len();
    let Some(count) = u32::try_from(n).ok().filter(|c| *c >= 2) else {
        return;
    };
    let rng = ctx.rng.world(WorldId(FIRST_ARENA));
    let keys: Vec<u32> = (0..n).map(|_| rng.random_below(count)).collect();
    let g = &mut svc.social.gc1to1;
    for (t, stage) in g.tenders.iter_mut().zip(stages_from_keys(&keys)) {
        t.stage = Some(stage);
    }
    let life = g.config.player_life;
    let mut arenas = Vec::new();
    for stage in 0..u8::try_from(n / 2).unwrap_or(u8::MAX) {
        let mut at_stage = g.tenders.iter().filter(|t| t.stage == Some(stage));
        let (Some(first), Some(second)) = (at_stage.next(), at_stage.next()) else {
            continue;
        };
        let side = |t: &super::Tender| Side {
            guild: t.guild,
            lineup: t.lineup.clone(),
            lives: life,
            wins: 0,
            last: PlayerResult::Draw,
        };
        let sides = [side(first), side(second)];
        let total = sides.iter().map(|s| s.lineup.len()).max().unwrap_or(0);
        arenas.push(Arena {
            stage,
            world: WorldId(FIRST_ARENA + u32::from(stage)),
            sides,
            state: ArenaState::Close,
            due: GameMs(0),
            prog: 0,
            total,
            ended: false,
        });
    }
    g.arenas = arenas;
}

/// `GuildCombat1to1EntranceWait` (`1592-1618`): state Entrance for `EntranceWaitTime`; per tender in rank order the
/// online master who is not in the line-up hears `{3, 0}` (KEEP), every online line-up member gets the small caption
/// `ENTRANCETIME` and `{2, remaining, members, enemy}`.
pub fn entrance(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    {
        let g = &mut svc.social.gc1to1;
        g.set_state(Gc1to1State::Entrance);
        g.due = GameMs(now.0.saturating_add(g.config.entrance_wait_time_ms));
    }
    let tenders: Vec<(GuildId, Vec<CharacterId>)> = svc
        .social
        .gc1to1
        .tenders()
        .iter()
        .map(|t| (t.guild, t.lineup.clone()))
        .collect();
    for (guild, lineup) in tenders {
        let master = svc.social.guild.get(guild).and_then(|g| g.master());
        if let Some(m) = master.filter(|m| !lineup.contains(m))
            && let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, m)
        {
            send_manager_state(svc, ctx, obj, m);
        }
        for c in lineup {
            if let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, c) {
                small_caption(ctx, obj, TID_GAME_GUILDCOMBAT1TO1_ENTRANCETIME, Vec::new());
                send_manager_state(svc, ctx, obj, c);
            }
        }
    }
}

/// `GuildCombat1to1War` (`1127-1150`): state War, `{3, 0}` to every online line-up member, then per arena the audit
/// line `gc1to1.war` and the first `war_wait`.
pub fn start_war(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    {
        let g = &mut svc.social.gc1to1;
        g.set_state(Gc1to1State::War);
        g.due = GameMs(0);
    }
    let lineups: Vec<CharacterId> = svc
        .social
        .gc1to1
        .tenders()
        .iter()
        .flat_map(|t| t.lineup.iter().copied())
        .collect();
    for c in lineups {
        if let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, c) {
            send_manager_state(svc, ctx, obj, c);
        }
    }
    for idx in 0..svc.social.gc1to1.arenas.len() {
        if let Some(a) = arena_clone(svc, idx) {
            ctx.audit.emit(flyff_db::AuditEvent::new(
                "gc1to1.war",
                json!({
                    "world": a.world.0,
                    "round": svc.social.gc1to1.round(),
                    "guilds": [a.side(0).guild.0, a.side(1).guild.0],
                }),
            ));
        }
        war_wait(svc, ctx, idx);
    }
}

// ---- the teleports ---------------------------------------------------------------------------------------------------------------

/// `m.sim.angle` of an avatar (`SetAngle`; `Worlds::replace` has no angle).
fn set_angle(ctx: &mut GameCtx<'_>, obj: ObjId, degrees: f32) {
    if let Some(m) = ctx.worlds.player_mut(obj) {
        m.sim.angle = flyff_sim::num::set_angle_quantized(degrees);
    }
}

/// `SetTeleportToWaitStage` (`633-656`): the waiting area of the side (`x`, `z` get `xRandom(4) - 2` each, drawn from the
/// stream of the arena's world, KEEP the asymmetry of `-2..=1`), the angle 0 or 180, state 2 (observer) with its
/// `GcUserState` broadcast. A refused replace is logged and changes nothing (returns `false`).
fn to_wait_stage(ctx: &mut GameCtx<'_>, a: &Arena, side: usize, obj: ObjId) -> bool {
    let rng = ctx.rng.world(a.world);
    let dx = rng.random_below(4) as f32 - 2.0;
    let dz = rng.random_below(4) as f32 - 2.0;
    let (centre, angle) = if side == 0 {
        (WAIT_SPOT_0, 0.0)
    } else {
        (WAIT_SPOT_1, 180.0)
    };
    let at = Vec3::new(centre.x + dx, centre.y, centre.z + dz);
    match ctx
        .worlds
        .replace(obj, a.world, at, LayerId(0), ReplaceKind::Normal)
    {
        Ok(()) => {
            set_angle(ctx, obj, angle);
            set_gc_state(ctx, obj, 2);
            true
        }
        Err(e) => {
            tracing::debug!(error = %e, world = a.world.0, "a GC 1:1 waiting-area teleport was refused");
            false
        }
    }
}

/// `SetTeleportToWarStage` (`659-675`): the fight spot of the side, the angle, state 1 (fighter).
fn to_war_stage(ctx: &mut GameCtx<'_>, a: &Arena, side: usize, obj: ObjId) {
    let (at, angle) = if side == 0 {
        (WAR_SPOT_0, 0.0)
    } else {
        (WAR_SPOT_1, 180.0)
    };
    match ctx
        .worlds
        .replace(obj, a.world, at, LayerId(0), ReplaceKind::Normal)
    {
        Ok(()) => {
            set_angle(ctx, obj, angle);
            set_gc_state(ctx, obj, 1);
        }
        Err(e) => {
            tracing::debug!(error = %e, world = a.world.0, "a GC 1:1 fight-spot teleport was refused");
            // the round still counts the fighter as present; he fights where he stands
            set_gc_state(ctx, obj, 1);
        }
    }
}

/// `Gc1to1TeleportToNpc` (`SetTeleportToNPC`, `1621-1632`): state not Entrance -> `NOENTRANCETIME`; FIX (silent): only a
/// LIVING line-up member (the original teleported anybody for free); then Madrigal (6983, 0, 3326), layer 0.
pub fn on_teleport_to_npc(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    if svc.social.gc1to1.state() != Gc1to1State::Entrance {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_NOENTRANCETIME,
            Vec::new(),
        );
        return;
    }
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    let member = svc
        .social
        .guild
        .guild_of(c)
        .and_then(|g| svc.social.gc1to1.tender_of(g))
        .is_some_and(|t| t.lineup.contains(&c));
    let alive = ctx
        .player(who)
        .is_some_and(|m| !crate::combat::state::is_die(m));
    if !member || !alive {
        return;
    }
    if let Err(e) = ctx.worlds.replace(
        who.0,
        WorldId(WI_WORLD_MADRIGAL),
        MADRIGAL_SPOT,
        LayerId(0),
        ReplaceKind::Normal,
    ) {
        tracing::debug!(error = %e, "a GC 1:1 teleport to the NPC was refused");
    }
}

/// `Gc1to1TeleportToStage` (`SetTeleportToStage`, `1635-1663`): state not Entrance -> `NOENTRANCETIME`; not a line-up
/// member -> `NOT_GUILD_MEMBER`; dead -> silent (FIX); then the waiting area of the side, the common buffs removed and a
/// party left.
pub fn on_teleport_to_stage(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    if svc.social.gc1to1.state() != Gc1to1State::Entrance {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_NOENTRANCETIME,
            Vec::new(),
        );
        return;
    }
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    let guild = svc.social.guild.guild_of(c);
    let member = guild
        .and_then(|g| svc.social.gc1to1.tender_of(g))
        .is_some_and(|t| t.lineup.contains(&c));
    if !member {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_NOT_GUILD_MEMBER,
            Vec::new(),
        );
        return;
    }
    if ctx.player(who).is_none_or(crate::combat::state::is_die) {
        return;
    }
    let Some(guild) = guild else {
        return;
    };
    let Some(idx) = svc.social.gc1to1.arena_of(guild) else {
        return;
    };
    let Some(a) = arena_clone(svc, idx) else {
        return;
    };
    let Some(side) = a.side_of(guild) else {
        return;
    };
    if !to_wait_stage(ctx, &a, side, who.0) {
        return;
    }
    crate::buff::api::remove_common(svc, ctx, who.0);
    if svc.social.party.party_of(c).is_some() {
        let name = name_of(svc, c).unwrap_or_default();
        let _ = crate::social::party::remove_member(svc, ctx, c, &name);
    }
}

/// The arenas, once a second while the manager is in War (`Process` of every arena); when every arena is Close the big
/// caption `ALLCLOSE` goes to everybody and `all_close` runs in the same call (`1000-1024`).
pub fn step_all(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    for idx in 0..svc.social.gc1to1.arenas.len() {
        step(svc, ctx, idx);
    }
    if svc
        .social
        .gc1to1
        .arenas
        .iter()
        .all(|a| a.state == ArenaState::Close)
    {
        let line = text_of(ctx, TID_GAME_GUILDCOMBAT1TO1_ALLCLOSE);
        super::big_caption_all(svc, ctx, &line);
        all_close(svc, ctx);
    }
}

/// `CGuildCombat1to1::Process` (`70-122`).
fn step(svc: &mut Services, ctx: &mut GameCtx<'_>, idx: usize) {
    let Some(a) = arena_clone(svc, idx) else {
        return;
    };
    let now = ctx.clock.game_ms;
    match a.state {
        ArenaState::Close => {}
        ArenaState::Wait => {
            if now >= a.due {
                if let Some(m) = arena_mut(svc, idx) {
                    m.due = GameMs(0);
                }
                war(svc, ctx, idx);
            }
        }
        ArenaState::War => {
            if now >= a.due {
                if let Some(m) = arena_mut(svc, idx) {
                    m.due = GameMs(0);
                }
                result_check(svc, ctx, idx, None);
            }
        }
        ArenaState::Finish => finish(svc, ctx, idx),
        ArenaState::CloseWait => {
            if now >= a.due {
                close(svc, ctx, idx);
            }
        }
    }
}
