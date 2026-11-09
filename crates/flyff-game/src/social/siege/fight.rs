//! The siege fight (Plan/21 §7.5; WP-SOCIAL-17; `src:_Common/eveschool.cpp` `SetMaintenance` 2207-2410,
//! `SetGuildCombatStart/Close` 2487-2610, `ProcessJoinWar` 2799-2850, `SetPlayerChange` 3286-3398, `OutWar`
//! 633-700, `UserOutGuildCombatResult` 964-1063; `World.cpp` `_OnDie`): the maintenance minute that picks the
//! fighters, the spawn teleports, the start and the close of the fight, the deaths, the early end, the logout of a
//! fighter and the seams of the combat code.
//!
//! **One owner.** [`FightState`] (`SiegeService.fight`, never persisted) holds the line-ups of the admitted guilds
//! ([`GuildLineup`]), the kill log and the queues of the deaths; a fresh one begins with every siege
//! ([`super::result::close_teleport`] clears it). The per-player fight state is
//! `PlayerSocial.guild_combat_state` (0 none, 1 fighting, 2 standby); EVERY change goes through `set_gc_state`
//! and sends `GC_USERSTATE` to the viewers of the mover.
//!
//! **The queue loop of the original** ([`GuildLineup::pull`]): `SetMaintenance` and `SetPlayerChange` pop the
//! front of the waiting queue; a player who is online in world 202 becomes a fighter (`war_count + 1`,
//! `tel_time = now`, `GC_SELECTWARPOS`), any other loses a life and goes to the back of the queue while he has
//! lives, **and is examined again in the same pass**, so an absent player loses ALL his remaining lives unless
//! `MAXWARPLAYER` fighters are reached first (the "loses one life" of WP-SOCIAL-17 is one iteration of this loop).
//!
//! **Deaths.** The death pipeline calls [`on_world_die`] (`DeathHooks::world_die`, `DoDie` step 9) for every dying
//! player with `guild_combat_state != 0`; the entry waits in [`FightState`] until [`drain_deaths`] (right after
//! `combat::tick::finish` in `phase_worlds`, and at the head of [`process_join_war`]) handles it in the order of
//! `CWorld::_OnDie`: `OutWar` (a life lost, the next waiting player pulled), then the kill points, then (PL-109) the
//! server's own revival of the victim in world 202. A fighter who dies without a player killer (a fall, a DoT whose
//! caster is gone) used to leak a `war_count` slot and keep his life in the original; here `OutWar` also runs for
//! him, without points (silent FIX).

use super::lineup::{self, GuildLineup};
use super::{SiegeFacts, SiegeState, SiegeWarState, StopWar};
use crate::buff::set::{BuffKey, BuffKind};
use crate::chat::ingress::notice;
use crate::chat::send_to;
use crate::combat::effects::{Arena, Effects};
use crate::combat::hooks::{CombatHooks, KillCtx, ProcHook};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::{Audience, Outbox};
use crate::presence::PlayerDirectory;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use crate::world::logout::{LogoutHooks, LogoutStep};
use crate::world::{ReplaceKind, Worlds};
use flyff_common::config::EventId;
use flyff_common::defs::skill::SI_RIN_SUP_HOLYCROSS;
use flyff_common::defs::text::{
    TID_GAME_GUILDCOMBAT_JOIN_MSG_DEFENDER, TID_GAME_GUILDCOMBAT_JOIN_MSG_GENERAL,
    TID_GAME_GUILDCOMBAT_JOIN_MSG_MASTER, TID_GAME_GUILDCOMBAT_POINT_DEFENDER,
    TID_GAME_GUILDCOMBAT_POINT_GENERAL, TID_GAME_GUILDCOMBAT_POINT_MASTER,
    TID_GAME_GUILDCOMBAT_POINT_REVIVAL,
};
use flyff_common::defs::world::WI_WORLD_GUILDWAR;
use flyff_common::rng::GameRng;
use flyff_common::{CharacterId, GameMs, GuildId, LayerId, ObjId, UnixTime, WorldId};
use flyff_db::{DbOp, GuildSiegeRecord, SiegeLoad};
use flyff_proto::pb;
use flyff_proto::pb::guild_combat::Sub;
use flyff_rules::social::siege as rules;
use std::collections::BTreeMap;
use std::sync::Arc;

// ---- the state ---------------------------------------------------------------------------------------------------------

/// One kill of the siege log (`__GCGETPOINT`, the nine fields of `GcLogEntry`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KillRecord {
    /// The attacker's guild.
    pub guild_attack: GuildId,
    /// The victim's guild.
    pub guild_defence: GuildId,
    /// The attacker.
    pub player_attack: CharacterId,
    /// The victim.
    pub player_defence: CharacterId,
    /// The points the kill earned.
    pub point: i32,
    /// `bKillDiffernceGuild`: never set (KQ-20).
    pub kill_other_guild: bool,
    /// The attacker is the master of his guild.
    pub master: bool,
    /// The victim is the defender of his guild.
    pub defender: bool,
    /// The attacker was on his last life.
    pub last_life: bool,
}

impl KillRecord {
    /// The `GC_LOG` entry of the record.
    pub fn to_entry(&self) -> pb::guild_combat::GcLogEntry {
        pb::guild_combat::GcLogEntry {
            guild_attack: self.guild_attack.0,
            guild_defence: self.guild_defence.0,
            player_attack: self.player_attack.0,
            player_defence: self.player_defence.0,
            point: self.point,
            kill_other_guild: self.kill_other_guild,
            master: self.master,
            defender: self.defender,
            last_life: self.last_life,
        }
    }
}

/// A death waiting for [`drain_deaths`] (`CWorld::m_aOnDie`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingDeath {
    /// The player who died.
    pub victim: ObjId,
    /// The player who killed him (`None`: a fall, a DoT whose caster is gone, ...).
    pub attacker: Option<ObjId>,
}

/// A Holy Cross support kill: `(caster of the buff, victim)`.
pub type HolyCross = (ObjId, ObjId);

/// The sending end of the Holy Cross points the combat hook queues for [`drain_deaths`] (the hit frames have no
/// services; the game thread owns the receiving end in [`FightState`]).
pub type HolyCrossQueue = flume::Sender<HolyCross>;

/// What the end of the siege freezes: after the result the bids are gone (`begin_next_siege`), but the line-up
/// boards of the last 40 seconds still need the admitted guilds and the order of the bid list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frozen {
    /// The admitted guilds in rank order.
    pub admitted: Vec<GuildId>,
    /// Every guild of the bid list in first-bid order, cancelled bids included.
    pub order: Vec<GuildId>,
}

/// The fight of the running siege (`SiegeService.fight`; see the module documentation).
#[derive(Debug)]
pub struct FightState {
    /// The line-up of every admitted guild that submitted one.
    pub guilds: BTreeMap<GuildId, GuildLineup>,
    /// The kill log (`m_vecGCGetPoint`).
    pub kills: Vec<KillRecord>,
    pending_deaths: Vec<PendingDeath>,
    holycross: HolyCrossQueue,
    holycross_rx: flume::Receiver<HolyCross>,
    chip_shop_done: bool,
    frozen: Option<Frozen>,
    last_ended: Option<GuildSiegeRecord>,
    started_at: Option<UnixTime>,
    send_items_logged: bool,
}

impl Default for FightState {
    fn default() -> FightState {
        let (holycross, holycross_rx) = flume::unbounded();
        FightState {
            guilds: BTreeMap::new(),
            kills: Vec::new(),
            pending_deaths: Vec::new(),
            holycross,
            holycross_rx,
            chip_shop_done: false,
            frozen: None,
            last_ended: None,
            started_at: None,
            send_items_logged: false,
        }
    }
}

impl FightState {
    /// The state of a starting server: the newest ended siege (its winner is zeroed by a cancelled siege).
    pub fn seeded(load: &SiegeLoad) -> FightState {
        FightState {
            last_ended: load.last_ended.clone(),
            ..FightState::default()
        }
    }

    /// The Holy Cross queue the [`SiegeCombatHooks`] push to.
    pub fn holycross_queue(&self) -> HolyCrossQueue {
        self.holycross.clone()
    }

    /// The line-up of `guild`.
    pub fn lineup(&self, guild: GuildId) -> Option<&GuildLineup> {
        self.guilds.get(&guild)
    }

    /// Deaths waiting for the next drain.
    pub fn pending_len(&self) -> usize {
        self.pending_deaths.len()
    }

    /// `true` once the result ran and the bids are gone.
    pub fn is_frozen(&self) -> bool {
        self.frozen.is_some()
    }

    /// The newest ended siege as the last result left it.
    pub fn last_ended(&self) -> Option<&GuildSiegeRecord> {
        self.last_ended.as_ref()
    }

    /// The siege opened at `at` (`schedule::open` tells; the result needs it to rewrite the row of the siege).
    pub fn mark_started(&mut self, at: UnixTime) {
        self.started_at = Some(at);
    }

    /// When the siege opened (`None` before the first opening of this process).
    pub fn started_at(&self) -> Option<UnixTime> {
        self.started_at
    }

    /// Freezes the admitted guilds and the bid order (the result is about to clear the bids).
    pub(super) fn freeze(&mut self, frozen: Frozen) {
        self.frozen = Some(frozen);
    }

    pub(super) fn set_last_ended(&mut self, rec: GuildSiegeRecord) {
        self.last_ended = Some(rec);
    }

    /// A fresh line-up for the next siege: everything but the memory of the last result and the chip shop.
    pub(super) fn clear_siege(&mut self) {
        self.guilds.clear();
        self.kills.clear();
        self.pending_deaths.clear();
        self.holycross_rx.try_iter().for_each(drop);
        self.frozen = None;
        self.started_at = None;
    }

    fn take_pending(&mut self) -> Vec<PendingDeath> {
        std::mem::take(&mut self.pending_deaths)
    }

    fn take_holycross(&mut self) -> Vec<HolyCross> {
        self.holycross_rx.try_iter().collect()
    }

    pub(super) fn chip_shop_done(&self) -> bool {
        self.chip_shop_done
    }

    pub(super) fn set_chip_shop_done(&mut self, done: bool) {
        self.chip_shop_done = done;
    }

    pub(super) fn send_items_logged(&mut self) -> bool {
        std::mem::replace(&mut self.send_items_logged, true)
    }
}

// ---- helpers shared with lineup.rs and result.rs --------------------------------------------------------------------------

/// `events.guild_combat` (every handler, hook and slot body of the siege returns at once without it).
pub(crate) fn enabled(ctx: &GameCtx<'_>) -> bool {
    ctx.live.event_state(EventId::GuildCombat) != 0
}

/// `GuildCombat{sub}` as a message.
pub(crate) fn gc(sub: Sub) -> pb::GuildCombat {
    pb::GuildCombat { sub: Some(sub) }
}

/// `GuildCombat{sub}` to everybody in the siege world (wire `obj_id` 0).
pub(crate) fn send_world(svc: &Services, ctx: &mut GameCtx<'_>, sub: Sub) {
    ctx.broadcast(
        Some(svc),
        ObjId::NULL,
        Audience::World(WorldId(WI_WORLD_GUILDWAR)),
        gc(sub),
    );
}

/// The avatar of an online character that is not being removed (`IsValidObj( prj.GetUserByID(..) )`).
pub(crate) fn live_avatar(
    presence: &PlayerDirectory,
    worlds: &Worlds,
    c: CharacterId,
) -> Option<ObjId> {
    let obj = presence.get(c)?.online?.obj;
    worlds.core(obj).filter(|core| !core.deleted).map(|_| obj)
}

/// [`live_avatar`] standing in the siege world.
pub(crate) fn live_in_gw(
    presence: &PlayerDirectory,
    worlds: &Worlds,
    c: CharacterId,
) -> Option<ObjId> {
    let obj = live_avatar(presence, worlds, c)?;
    (worlds.core(obj)?.world.0 == WI_WORLD_GUILDWAR).then_some(obj)
}

/// `true` when `obj` stands in the siege world.
pub(crate) fn in_gw(ctx: &GameCtx<'_>, obj: ObjId) -> bool {
    ctx.worlds
        .core(obj)
        .is_some_and(|c| c.world.0 == WI_WORLD_GUILDWAR)
}

/// The character behind an avatar.
pub(crate) fn character(ctx: &GameCtx<'_>, obj: ObjId) -> Option<CharacterId> {
    ctx.worlds
        .player(obj)?
        .player()
        .map(|p| p.ident.character_id)
}

/// The name of a character in the directory.
pub(crate) fn name_of(svc: &Services, c: CharacterId) -> Option<String> {
    svc.presence.get(c).map(|e| e.name.clone())
}

/// The text of `tid` (`prj.GetText`).
pub(crate) fn text_of(ctx: &GameCtx<'_>, tid: u32) -> String {
    ctx.data.texts.get(tid).to_owned()
}

/// `m_nGuildCombatState = state` and `GC_USERSTATE{mover, state}` to everybody who sees the mover (itself included,
/// `AddGuildCombatUserState`).
pub(crate) fn set_gc_state(ctx: &mut GameCtx<'_>, obj: ObjId, state: i32) {
    if let Some(p) = ctx.worlds.player_mut(obj).and_then(|m| m.player_mut()) {
        p.social.guild_combat_state = state;
    }
    ctx.broadcast(
        None,
        obj,
        Audience::NearInclSelf,
        gc(Sub::GcUserState(pb::guild_combat::GcUserState {
            mover_obj_id: obj.0,
            state,
        })),
    );
}

/// `AddDefinedCaption( TRUE, tid, "%d" ...)`: a small caption with one argument to one avatar.
fn caption_to(ctx: &mut GameCtx<'_>, obj: ObjId, tid: u32, arg: String) {
    send_to(
        ctx,
        PlayerRef(obj),
        pb::DefinedText {
            tid: i32::try_from(tid).unwrap_or(i32::MAX),
            args: vec![arg],
            display: pb::defined_text::Display::CaptionSmall as i32,
        },
    );
}

/// The admitted guilds in rank order: the first `MAXJOINGUILDSIZE` of the request ranking, or, once the result
/// cleared the bids, the list it froze.
pub fn admitted(svc: &Services) -> Vec<GuildId> {
    match &svc.social.siege.fight.frozen {
        Some(f) => f.admitted.clone(),
        None => svc.social.siege.admitted(),
    }
}

/// Every guild of the bid list in first-bid order (cancelled bids included), or the frozen copy.
pub fn bid_order(svc: &Services) -> Vec<GuildId> {
    match &svc.social.siege.fight.frozen {
        Some(f) => f.order.clone(),
        None => svc
            .social
            .siege
            .bid_entries()
            .iter()
            .map(|b| b.guild)
            .collect(),
    }
}

/// `IsRequestWarGuild( guild, FALSE )`: the guild is among the admitted.
pub fn is_admitted(svc: &Services, guild: GuildId) -> bool {
    admitted(svc).contains(&guild)
}

// ---- the queue model (pure) --------------------------------------------------------------------------------------------------

/// A player the queue loop pulled into the fight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pulled {
    /// The character.
    pub character: CharacterId,
    /// His avatar.
    pub obj: ObjId,
    /// His chosen gate (`nMap`, 99 = random).
    pub map: i32,
}

impl GuildLineup {
    /// `SetMaintenance`: the waiting queue is every selected player with lives, in line-up order.
    pub fn fill_queue(&mut self) {
        self.queue = self
            .select
            .iter()
            .filter(|s| s.lives > 0)
            .map(|s| s.character)
            .collect();
    }

    /// The queue loop of `SetMaintenance` and `SetPlayerChange` (see the module documentation): `present` answers
    /// with the avatar of a player who is online in the siege world. Stops when `max_war` fighters are reached
    /// (compared with `==`, as the original: a `MAXWARPLAYER` of 0 pulls everybody) or the queue is empty.
    pub fn pull(
        &mut self,
        max_war: i32,
        now: GameMs,
        mut present: impl FnMut(CharacterId) -> Option<ObjId>,
    ) -> Vec<Pulled> {
        let mut pulled = Vec::new();
        while let Some(front) = self.queue.pop_front() {
            match present(front) {
                Some(obj) => {
                    self.war_count += 1;
                    if let Some(s) = self.entry_mut(front) {
                        s.tel_time = Some(now);
                        pulled.push(Pulled {
                            character: front,
                            obj,
                            map: s.map,
                        });
                    }
                }
                None => {
                    let again = self.entry_mut(front).is_some_and(|s| {
                        s.lives -= 1;
                        if s.lives < 0 {
                            s.lives = 0;
                        }
                        s.lives > 0
                    });
                    if again {
                        self.queue.push_back(front);
                    }
                }
            }
            if i64::from(self.war_count) == i64::from(max_war) {
                break;
            }
        }
        pulled
    }

    /// `SetPlayerChange` up to the pull loop: a fighter fell. `false` when the player is no member of the line-up
    /// or waits in the queue ("a queue player has nothing to do with the fight": ignored). Otherwise his teleport
    /// timer is cleared, a fighter slot and a life are lost and he goes to the back of the queue while he has
    /// lives.
    pub fn lose_fighter(&mut self, c: CharacterId) -> bool {
        if self.in_queue(c) {
            return false;
        }
        let Some(s) = self.entry_mut(c) else {
            return false;
        };
        s.tel_time = None;
        s.lives -= 1;
        let again = s.lives > 0;
        if s.lives < 0 {
            s.lives = 0;
        }
        self.war_count = self.war_count.saturating_sub(1);
        if again {
            self.queue.push_back(c);
        }
        true
    }
}

// ---- the phase hooks ------------------------------------------------------------------------------------------------------

/// `WARSTATE 103` (`SetMaintenance`): per admitted guild in rank order the fighters are picked and told, the guild
/// gets its board, every selected player in the siege world gets the box of his role, and a guild with a line-up
/// counts. Fewer than `MINJOINGUILDSIZE` such guilds cancel the siege (`SetGuildCombatCloseWait(TRUE)`: the script
/// jumps to the `GM_CLOSE` row, nothing is persisted or refunded, every bid carries over); a winner who is not among
/// the admitted loses his streak.
pub fn set_maintenance(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if !enabled(ctx) {
        return;
    }
    let now = ctx.clock.game_ms;
    let max_war = svc.social.siege.config.max_war_player;
    let window_s = svc.social.siege.config.max_map_time;
    let min = svc.social.siege.config.min_join_guild_size;
    let win_guild = svc.social.siege.win_guild();
    let texts = [
        text_of(ctx, TID_GAME_GUILDCOMBAT_JOIN_MSG_MASTER),
        text_of(ctx, TID_GAME_GUILDCOMBAT_JOIN_MSG_DEFENDER),
        text_of(ctx, TID_GAME_GUILDCOMBAT_JOIN_MSG_GENERAL),
    ];
    let mut win_continues = false;
    let mut counted = 0_i32;
    for guild in admitted(svc) {
        if Some(guild) == win_guild {
            win_continues = true;
        }
        let Some(master) = svc.social.guild.get(guild).map(|g| g.master()) else {
            continue; // a vanished guild neither fights nor counts
        };
        let mut pulled = Vec::new();
        if let Some(lineup) = svc.social.siege.fight.guilds.get_mut(&guild) {
            lineup.war_count = 0;
            lineup.fill_queue();
            let (presence, worlds) = (&svc.presence, &*ctx.worlds);
            pulled = lineup.pull(max_war, now, |c| live_in_gw(presence, worlds, c));
        }
        let (roles, has_lineup) = {
            let lineup = svc.social.siege.fight.guilds.get(&guild);
            let defender = lineup.and_then(|l| l.defender);
            let roles: Vec<(CharacterId, usize)> = lineup
                .map(|l| {
                    l.select
                        .iter()
                        .map(|s| {
                            let role = if Some(s.character) == master {
                                0
                            } else if Some(s.character) == defender {
                                1
                            } else {
                                2
                            };
                            (s.character, role)
                        })
                        .collect()
                })
                .unwrap_or_default();
            (roles, lineup.is_some_and(|l| !l.select.is_empty()))
        };
        announce_pulled(ctx, window_s, &pulled);
        send_guild_status(svc, ctx, guild);
        for (c, role) in roles {
            if let Some(obj) = live_in_gw(&svc.presence, &*ctx.worlds, c)
                && let Some(text) = texts.get(role)
            {
                notice::diag(ctx, PlayerRef(obj), text);
            }
        }
        if has_lineup {
            counted += 1;
        }
    }
    if counted < min {
        cancel_siege(svc, ctx, win_continues);
    }
}

/// `GC_SELECTWARPOS{map, MAXMAPTIME}` to every pulled player (`AddGCJoinWarWindow`).
fn announce_pulled(ctx: &mut GameCtx<'_>, window_s: i32, pulled: &[Pulled]) {
    for p in pulled {
        super::send_one(
            ctx,
            p.obj,
            Sub::GcSelectWarPos(pb::guild_combat::GcSelectWarPos {
                map: p.map,
                join_time_s: window_s,
            }),
        );
    }
}

/// The cancel of `SetMaintenance` (difference 2 of the task file): a previous winner who is not among the admitted
/// guilds loses win and streak (memory, `GC_WINGUILD`, one `UpsertGuildSiege` that zeroes the row of the last ended
/// siege), then the script is asked for the cancel jump.
fn cancel_siege(svc: &mut Services, ctx: &mut GameCtx<'_>, win_continues: bool) {
    if !win_continues {
        let had_winner = {
            let s = &svc.social.siege;
            s.win_guild().is_some() || s.win_count() != 0
        };
        if had_winner {
            let best = svc.social.siege.best_player();
            svc.social.siege.set_result(None, 0, best);
            // the persisted row of the last ended siege
            let zeroed = svc.social.siege.fight.last_ended.clone().map(|mut rec| {
                rec.winner_guild_id = None;
                rec.winner_streak = 0;
                rec
            });
            if let Some(rec) = zeroed {
                ctx.persist.op(DbOp::UpsertGuildSiege(rec.clone()));
                svc.social.siege.fight.last_ended = Some(rec);
            }
            let sub = Sub::GcWinGuild(pb::guild_combat::GcWinGuild {
                combat_index: i32::try_from(svc.social.siege.siege_id()).unwrap_or(i32::MAX),
                win_guild: 0,
                win_count: 0,
            });
            super::send_all(svc, ctx, sub);
        }
    }
    svc.social.siege.request_stop(StopWar::GmCancel);
}

/// `WARSTATE 104` (`SetGuildCombatStart`): every selected player of an admitted guild with lives who does not wait
/// in the queue and is online (in any world, KEEP) becomes a fighter: state 1.
pub fn start_fight(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if !enabled(ctx) {
        return;
    }
    let mut fighters = Vec::new();
    for guild in admitted(svc) {
        let Some(lineup) = svc.social.siege.fight.guilds.get(&guild) else {
            continue;
        };
        for s in &lineup.select {
            if s.lives > 0
                && !lineup.in_queue(s.character)
                && let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, s.character)
            {
                fighters.push(obj);
            }
        }
    }
    for obj in fighters {
        set_gc_state(ctx, obj, 1);
    }
}

/// `WARSTATE 105` (`SetGuildCombatClose`): every selected player who is online goes back to state 0, each admitted
/// guild gets `+ Σ lives` as revival points (a caption to its online members in the siege world), and the guild
/// board goes to the siege world.
pub fn close_fight(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if !enabled(ctx) {
        return;
    }
    let guilds = admitted(svc);
    for guild in &guilds {
        let Some(lineup) = svc.social.siege.fight.guilds.get(guild) else {
            continue;
        };
        let online: Vec<ObjId> = lineup
            .select
            .iter()
            .filter_map(|s| live_avatar(&svc.presence, &*ctx.worlds, s.character))
            .collect();
        for obj in online {
            set_gc_state(ctx, obj, 0);
        }
    }
    for guild in guilds {
        let Some(lineup) = svc.social.siege.fight.guilds.get_mut(&guild) else {
            continue;
        };
        let revival: i32 = lineup.select.iter().map(|s| s.lives).sum();
        lineup.points += revival;
        for obj in svc.social.guild.online_objs(&svc.presence, guild) {
            if in_gw(ctx, obj) {
                caption_to(
                    ctx,
                    obj,
                    TID_GAME_GUILDCOMBAT_POINT_REVIVAL,
                    revival.to_string(),
                );
            }
        }
    }
    lineup::send_guild_precedence(svc, ctx, lineup::To::World);
}

/// `ProcessJoinWar`, once per second while the siege runs: a fighter whose gate window ran out
/// (`now >= tel_time + MAXMAPTIME * 1000`) is teleported to his gate. The death queue is drained first.
pub fn process_join_war(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if !enabled(ctx) {
        return;
    }
    drain_deaths(svc, ctx);
    let now = ctx.clock.game_ms.0;
    let window = u64::try_from(svc.social.siege.config.max_map_time)
        .unwrap_or(0)
        .saturating_mul(1000);
    let mut due: Vec<(CharacterId, i32)> = Vec::new();
    for guild in admitted(svc) {
        let Some(lineup) = svc.social.siege.fight.guilds.get_mut(&guild) else {
            continue;
        };
        for s in &mut lineup.select {
            if let Some(t) = s.tel_time
                && now >= t.0.saturating_add(window)
            {
                s.tel_time = None;
                due.push((s.character, s.map));
            }
        }
    }
    for (c, map) in due {
        if let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, c) {
            join_war(svc, ctx, obj, map);
        }
    }
}

/// `JoinWar`: the fighter enters through his gate (`map` 0..=3, 99 = one of the four by the world's random stream,
/// drawn at every call like `xRandom(4)` of the original). While the war runs (`WAR_STATE`) the state becomes 1 in the
/// fight phase (104) and 2 in the maintenance minute (103).
pub fn join_war(svc: &mut Services, ctx: &mut GameCtx<'_>, obj: ObjId, map: i32) {
    let draw = ctx.rng.world(WorldId(WI_WORLD_GUILDWAR)).random_below(4) as usize;
    let index = if map == lineup::MAP_RANDOM {
        draw
    } else {
        usize::try_from(map).unwrap_or(0)
    };
    let s = &svc.social.siege;
    if s.state() == SiegeState::War {
        match s.war_state() {
            SiegeWarState::WarWar => set_gc_state(ctx, obj, 1),
            SiegeWarState::Maintenance => set_gc_state(ctx, obj, 2),
            _ => {}
        }
    }
    let gate = lineup::GATES
        .get(index)
        .copied()
        .unwrap_or(lineup::OBSERVER_SPOT);
    if let Err(e) = ctx.worlds.replace(
        obj,
        WorldId(WI_WORLD_GUILDWAR),
        gate,
        LayerId(0),
        ReplaceKind::Normal,
    ) {
        tracing::warn!(error = %e, obj = obj.0, "a siege fighter could not be moved to his gate");
    }
}

// ---- the boards -----------------------------------------------------------------------------------------------------------

/// `GC_GUILDSTATUS` of `guild` to its online members (nothing while the siege is closed).
pub fn send_guild_status(svc: &Services, ctx: &mut GameCtx<'_>, guild: GuildId) {
    lineup::send_guild_status_to(svc, ctx, guild, lineup::To::Guild(guild));
}

// ---- deaths -------------------------------------------------------------------------------------------------------------------

/// `DeathHooks::world_die` (`DoDie` step 9): a dying player with `guild_combat_state != 0` is queued for
/// [`drain_deaths`]; the killer is remembered when he is a player. A player already queued is not queued twice.
pub fn on_world_die(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    if !enabled(ctx) {
        return;
    }
    let Some(state) = ctx
        .worlds
        .player(k.victim)
        .and_then(|m| m.player())
        .map(|p| p.social.guild_combat_state)
    else {
        return;
    };
    if state == 0 {
        return;
    }
    let attacker =
        (k.attacker != k.victim && ctx.worlds.player(k.attacker).is_some()).then_some(k.attacker);
    let fight = &mut svc.social.siege.fight;
    if fight.pending_deaths.iter().any(|d| d.victim == k.victim) {
        return;
    }
    fight.pending_deaths.push(PendingDeath {
        victim: k.victim,
        attacker,
    });
}

/// Handles the queued Holy Cross points and deaths (see the module documentation); a no-op when nothing waits. The
/// chip-shop NPCs are made sure of on the first call of a start.
pub fn drain_deaths(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if !enabled(ctx) {
        return;
    }
    if !svc.social.siege.fight.chip_shop_done() {
        super::result::ensure_chip_shop_npcs(svc, ctx);
    }
    for (caster, victim) in svc.social.siege.fight.take_holycross() {
        kill_points(svc, ctx, caster, victim, true);
    }
    for d in svc.social.siege.fight.take_pending() {
        // (a) OutWar -> SetPlayerChange
        if let Some(c) = character(ctx, d.victim) {
            out_war(svc, ctx, d.victim, c, false);
        }
        // (b) GetPoint
        if let Some(a) = d.attacker {
            kill_points(svc, ctx, a, d.victim, false);
        }
        // (c) PL-109: the server revives the dead fighter, the client never asks (T19)
        let dead_in_gw = ctx.worlds.player(d.victim).is_some_and(|m| {
            crate::combat::state::is_die(m) && m.core.world.0 == WI_WORLD_GUILDWAR
        });
        if dead_in_gw {
            crate::combat::death::on_revival_to_lodestar_req(svc, ctx, PlayerRef(d.victim));
        }
    }
}

/// `OutWar( pUser, NULL, bLogOut )`: a fighter fell or left. `SetPlayerChange` first (a queued player is ignored), the
/// defender who logs out is replaced, then the guild board and the board of the siege world are sent and the early end
/// is looked at. Nothing is sent or decided when nothing changed.
pub fn out_war(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    obj: ObjId,
    character: CharacterId,
    logout: bool,
) {
    let Some(guild) = svc.social.guild.guild_of(character) else {
        return;
    };
    let now = ctx.clock.game_ms;
    let max_war = svc.social.siege.config.max_war_player;
    let window_s = svc.social.siege.config.max_map_time;
    let Some(lineup) = svc.social.siege.fight.guilds.get_mut(&guild) else {
        return;
    };
    let changed = lineup.lose_fighter(character);
    let mut pulled = Vec::new();
    if changed {
        pulled = {
            let (presence, worlds) = (&svc.presence, &*ctx.worlds);
            lineup.pull(max_war, now, |c| live_in_gw(presence, worlds, c))
        };
    }
    if changed {
        set_gc_state(ctx, obj, 0);
        announce_pulled(ctx, window_s, &pulled);
    }
    let replaced = logout && replace_defender(svc, ctx, guild, character);
    if !changed && !replaced {
        return;
    }
    send_guild_status(svc, ctx, guild);
    lineup::send_war_player_list(svc, ctx, lineup::To::World);
    if changed {
        user_out_result(svc, guild);
    }
}

/// The defender who logs out is replaced by a random selected member who is online, is not the master, is not the
/// defender and has lives left (`xRandom` of the world stream). `true` when the defender changed.
fn replace_defender(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    leaver: CharacterId,
) -> bool {
    let Some(lineup) = svc.social.siege.fight.guilds.get(&guild) else {
        return false;
    };
    if lineup.defender != Some(leaver) {
        return false;
    }
    let master = svc.social.guild.get(guild).and_then(|g| g.master());
    let candidates: Vec<CharacterId> = lineup
        .select
        .iter()
        .filter(|s| {
            Some(s.character) != master
                && Some(s.character) != lineup.defender
                && s.lives > 0
                && live_avatar(&svc.presence, &*ctx.worlds, s.character).is_some()
        })
        .map(|s| s.character)
        .collect();
    if candidates.is_empty() {
        return false;
    }
    let pick = ctx
        .rng
        .world(WorldId(WI_WORLD_GUILDWAR))
        .random_below(u32::try_from(candidates.len()).unwrap_or(u32::MAX)) as usize;
    let Some(next) = candidates.get(pick).copied() else {
        return false;
    };
    if let Some(l) = svc.social.siege.fight.guilds.get_mut(&guild) {
        l.defender = Some(next);
    }
    true
}

/// `UserOutGuildCombatResult`: a guild that has no player with lives left ends the siege when exactly ONE other
/// admitted guild still has a selected player with lives (`StopWar::Early`; the script jumps to the result).
fn user_out_result(svc: &mut Services, guild: GuildId) {
    let fight = &svc.social.siege.fight;
    let Some(own) = fight.guilds.get(&guild) else {
        return;
    };
    if own.select.iter().any(|s| s.lives > 0) {
        return;
    }
    let mut alive = 0;
    for other in admitted(svc) {
        if other == guild {
            continue;
        }
        if fight
            .guilds
            .get(&other)
            .is_some_and(|l| l.select.iter().any(|s| s.lives > 0))
        {
            alive += 1;
            if alive == 2 {
                return; // two or more: the siege goes on
            }
        }
    }
    if alive == 1 {
        svc.social.siege.request_stop(StopWar::Early);
    }
}

/// `GetPoint`: the attacker scores for the death of the victim. Both must be in the tables of their guilds, else
/// nothing happens. 2 points, +1 master, +1 defender, +1 on his last life (his own count, after the victim's death
/// was booked), +1 for a different guild than the last kill NEVER (KQ-20); `support` (Holy Cross) is exactly 1.
pub fn kill_points(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    attacker: ObjId,
    victim: ObjId,
    support: bool,
) {
    let (Some(a_char), Some(d_char)) = (character(ctx, attacker), character(ctx, victim)) else {
        return;
    };
    let (Some(a_guild), Some(d_guild)) = (
        svc.social.guild.guild_of(a_char),
        svc.social.guild.guild_of(d_char),
    ) else {
        return;
    };
    let master = svc.social.guild.is_master(a_char);
    let fight = &mut svc.social.siege.fight;
    let (last_life, defender) = {
        let Some(ae) = fight.guilds.get(&a_guild).and_then(|l| l.entry(a_char)) else {
            return;
        };
        let Some(dl) = fight.guilds.get(&d_guild) else {
            return;
        };
        if dl.entry(d_char).is_none() {
            return;
        }
        (ae.lives == 1, dl.defender == Some(d_char))
    };
    let point = rules::kill_points(master, defender, last_life, support);
    if let Some(l) = fight.guilds.get_mut(&a_guild) {
        l.points += point;
        if let Some(e) = l.entry_mut(a_char) {
            e.points += point;
        }
    }
    let record = KillRecord {
        guild_attack: a_guild,
        guild_defence: d_guild,
        player_attack: a_char,
        player_defence: d_char,
        point,
        kill_other_guild: false,
        master,
        defender,
        last_life,
    };
    fight.kills.push(record.clone());

    // the captions to the guild of the attacker and, for a defender, to the guild of the victim
    let tid = if master {
        TID_GAME_GUILDCOMBAT_POINT_MASTER
    } else {
        TID_GAME_GUILDCOMBAT_POINT_GENERAL
    };
    for obj in svc.social.guild.online_objs(&svc.presence, a_guild) {
        if in_gw(ctx, obj) {
            caption_to(ctx, obj, tid, point.to_string());
        }
    }
    if defender && let Some(name) = svc.social.guild.get(a_guild).map(|g| g.name.clone()) {
        for obj in svc.social.guild.online_objs(&svc.presence, d_guild) {
            if in_gw(ctx, obj) {
                caption_to(
                    ctx,
                    obj,
                    TID_GAME_GUILDCOMBAT_POINT_DEFENDER,
                    format!("\"{name}\""),
                );
            }
        }
    }
    // GC_LOG_REALTIME: skipped when a name is unknown
    if let (Some(an), Some(dn)) = (name_of(svc, a_char), name_of(svc, d_char)) {
        send_world(
            svc,
            ctx,
            Sub::GcLogRealtime(pb::guild_combat::GcLogRealtime {
                guild_attack: a_guild.0,
                guild_defence: d_guild.0,
                attacker_name: an,
                defender_name: dn,
                player_attack: a_char.0,
                player_defence: d_char.0,
                point,
                kill_other_guild: false,
                master,
                defender,
                last_life,
            }),
        );
    }
    lineup::send_guild_precedence(svc, ctx, lineup::To::World);
    lineup::send_player_precedence(svc, ctx, lineup::To::World);
}

// ---- the logout -----------------------------------------------------------------------------------------------------------------

/// The logout hook of the siege (`LogoutStep::SiegeOutWar`, `events.guild_combat`): while the war state is the
/// maintenance minute or the fight, the leaver is taken out of the fight like a death without points
/// (`OutWar( pUser, NULL, TRUE )`); a defender who leaves is replaced. Every other step and predicate is
/// forwarded to `inner`.
#[derive(Default)]
pub struct SiegeLogout {
    inner: Option<Box<dyn LogoutHooks>>,
}

impl SiegeLogout {
    /// The siege's hook alone.
    pub fn new() -> SiegeLogout {
        SiegeLogout { inner: None }
    }

    /// The same, forwarding to `inner` too.
    pub fn with_inner(inner: Box<dyn LogoutHooks>) -> SiegeLogout {
        SiegeLogout { inner: Some(inner) }
    }
}

impl std::fmt::Debug for SiegeLogout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SiegeLogout")
            .field("inner", &self.inner.is_some())
            .finish()
    }
}

impl LogoutHooks for SiegeLogout {
    fn step(
        &mut self,
        step: LogoutStep,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        player: PlayerRef,
    ) {
        if step == LogoutStep::SiegeOutWar {
            siege_out_war(svc, ctx, player);
        }
        if let Some(i) = self.inner.as_mut() {
            i.step(step, svc, ctx, player);
        }
    }

    fn secret_room_position(
        &self,
        ctx: &GameCtx<'_>,
        player: PlayerRef,
    ) -> Option<flyff_common::math::Vec3> {
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
            None => ctx
                .player(player)
                .and_then(|m| m.player())
                .is_some_and(|p| p.pk.propensity > 0),
        }
    }
}

/// The body of [`SiegeLogout`].
fn siege_out_war(svc: &mut Services, ctx: &mut GameCtx<'_>, player: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    if !matches!(
        svc.social.siege.war_state(),
        SiegeWarState::Maintenance | SiegeWarState::WarWar
    ) {
        return;
    }
    if let Some(c) = character(ctx, player.0) {
        out_war(svc, ctx, player.0, c, true);
    }
}

// ---- the combat seams -------------------------------------------------------------------------------------------------------

/// The `CombatHooks` of the siege (installed in `Game::new`): `IsGuildCombatTarget` needs the war state 104 or the War
/// state of the 1:1 guild combat (`guild_combat_running`; the trait's default rule then asks for both fighters in state 1
/// of different guilds, so only the two current fighters of an arena can hit each other), the
/// Resurrection skill of the siege world needs the siege state `War` (`guild_combat_war_state`), and a Holy Cross
/// kill queues a point for the CASTER of the buff (`holycross_gc_point`; the original awards him, not the hitter).
pub struct SiegeCombatHooks {
    facts: Arc<SiegeFacts>,
    holycross: HolyCrossQueue,
    // ---- guild combat 1:1 (doc 21 §8, task gc11)
    gc1to1: Option<Arc<crate::social::gc1to1::Gc1to1Facts>>,
}

impl SiegeCombatHooks {
    /// The hooks over the facts of the service and its Holy Cross queue.
    pub fn new(facts: Arc<SiegeFacts>, holycross: HolyCrossQueue) -> SiegeCombatHooks {
        SiegeCombatHooks {
            facts,
            holycross,
            gc1to1: None,
        }
    }

    /// The same, also reading the state of the 1:1 guild combat (`IsGuildCombatTarget` is true in its War as well,
    /// `MoverAttack.cpp:2183-2200`).
    #[must_use]
    pub fn with_gc1to1(
        mut self,
        facts: Arc<crate::social::gc1to1::Gc1to1Facts>,
    ) -> SiegeCombatHooks {
        self.gc1to1 = Some(facts);
        self
    }
}

impl ProcHook for SiegeCombatHooks {}

impl CombatHooks for SiegeCombatHooks {
    fn guild_combat_running(&self) -> bool {
        self.facts.war_state() == SiegeWarState::WarWar
            || self.gc1to1.as_ref().is_some_and(|f| f.war())
    }

    fn guild_combat_war_state(&self) -> bool {
        self.facts.state() == SiegeState::War
    }

    fn holycross_gc_point(
        &mut self,
        w: &mut Arena<'_>,
        _fx: &mut Effects,
        victim: ObjId,
        _hitter: ObjId,
    ) {
        let caster = w
            .store
            .mover(victim)
            .and_then(|m| {
                m.buffs
                    .get(BuffKey::new(BuffKind::Skill, SI_RIN_SUP_HOLYCROSS))
            })
            .and_then(|b| b.attacker);
        if let Some(caster) = caster {
            // the receiver lives in `FightState` as long as the game does; a failed send only means a stopped game
            let _ = self.holycross.send((caster, victim));
        }
    }
}

impl std::fmt::Debug for SiegeCombatHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SiegeCombatHooks(..)")
    }
}

// ---- the login ----------------------------------------------------------------------------------------------------------------------

/// The second contributor of [`JoinStep::GuildCombat`] (after siege-a's five messages, `DPDatabaseClient.cpp:1156-1160`):
/// while the siege is not closed the player's guild board (`GC_GUILDSTATUS`, empty for a player without a guild) and the
/// list of the fighters (`GC_WARPLAYERLIST`). The re-attach replay writes them too.
pub struct WarLoginStep;

impl JoinContributor for WarLoginStep {
    fn step(&self) -> JoinStep {
        JoinStep::GuildCombat
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let s = &cx.svc.social.siege;
        if s.state() == SiegeState::Close {
            return;
        }
        let guild = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .and_then(|p| cx.svc.social.guild.guild_of(p.ident.character_id));
        let status = lineup::guild_status_msg(&s.config, guild.and_then(|g| s.fight.lineup(g)));
        let list = lineup::war_player_list_msg(&admitted(cx.svc), &s.fight);
        out.to_session(cx.session, 0, gc(status));
        out.to_session(cx.session, 0, gc(list));
    }
}
