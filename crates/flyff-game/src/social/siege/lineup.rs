//! The siege line-up and the entry (Plan/21 §7.4; WP-SOCIAL-17; `DPSrvr.cpp:7648-7800` `OnGCSelectPlayer`, `OnGCJoin`,
//! `OnGCTele`, `OnGCSelectMap`; `eveschool.cpp` `JoinObserver` 699-814, `SetSelectMap`, `GuildCombatEnter` 2137-2205,
//! `SetEnter` 2413-2460; `User.cpp` wire builders): the line-up records, the pure builders of the boards and the
//! handlers of `SelectPlayerGuildCombat`, `SelectMapGuildCombat`, `JoinGuildCombat` and `TeleGuildCombat`.
//!
//! **The line-up** (`SelectPlayerGuildCombat`, only in the war state 101 and only for the master and the kingpins of an
//! ADMITTED guild) replaces the whole selection of the guild or changes nothing. FIX (VR-049, PL-096; the original
//! tested the last id it had parsed for every id and added a player twice): every id must be a CURRENT member of the
//! guild, no id may repeat, the list must hold the master or a kingpin who is online, the defender must be in the list
//! (the release-build rule) and survive the filter (online, level 30 or more), and the master is not the defender
//! of a list with more than one player. A refusal changes nothing and still gets the reply with the STORED data; a
//! list longer than `MAXJOINMEMBERSIZE` is ignored without any reply (as the original).
//!
//! **Entry.** `JoinGuildCombat` needs an NPC with `MMI_GUILDWAR_JOIN` near the player (VR-012). The state `Open` lets
//! the player in only during the war state `Enter`; `Close` and `War` let anybody watch from the observer spot (KEEP);
//! `CompleteClose` says it is closed; `GmClose` says nothing. A revival in the siege world sends the player through
//! [`join_observer`] too (`DeathHooks::gw_observer`).

use super::fight::{self, FightState, enabled, live_avatar, name_of, set_gc_state, text_of};
use super::{SiegeConfig, SiegeState, SiegeWarState};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use crate::social::guild::GuildRank;
use crate::world::ReplaceKind;
use flyff_common::defs::neuz::MMI_GUILDWAR_JOIN;
use flyff_common::defs::text::{
    TID_GAME_GUILDCOMBAT_CANNOT_MAKEUP, TID_GAME_GUILDCOMBAT_CANNOT_MAKEUP_FAIL,
    TID_GAME_GUILDCOMBAT_ENTER, TID_GAME_GUILDCOMBAT_NOT_JOIN, TID_GAME_GUILDCOMBAT_NOT_OPEN,
    TID_GAME_GUILDCOMBAT_TELE, TID_GAME_GUILDCOMBAT_WAR_ENTER, TID_GAME_GUILDCOMBAT_WELCOME,
    TID_GAME_GUILDCOMBAT_ZOOM_USE,
};
use flyff_common::defs::world::{WI_WORLD_GUILDWAR, WI_WORLD_MADRIGAL};
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, GameMs, GuildId, LayerId, ObjId, WorldId};
use flyff_proto::pb;
use flyff_proto::pb::guild_combat::Sub;
use flyff_rules::jobs::is_base;
use std::collections::VecDeque;

/// The minimum level of a selected player.
pub const GC_MIN_LEVEL: i32 = 30;
/// `nMap` 99: a random gate.
pub const MAP_RANDOM: i32 = 99;
/// The four entry gates (`JoinWar`): `nMap` 0..=3.
pub const GATES: [Vec3; 4] = [
    Vec3::new(1263.0, 83.0, 1303.0),
    Vec3::new(1310.0, 83.0, 1303.0),
    Vec3::new(1265.0, 83.0, 1254.0),
    Vec3::new(1314.0, 83.0, 1253.0),
];
/// Where an observer stands (`JoinObserver`).
pub const OBSERVER_SPOT: Vec3 = Vec3::new(1361.6, 0.0, 1273.3);
/// Where `TeleGuildCombat` leaves to (Madrigal).
pub const MADRIGAL_LEAVE: Vec3 = Vec3::new(6983.0, 0.0, 3330.0);

// ---- the records -------------------------------------------------------------------------------------------------------------

/// One selected player of a line-up (`__JOINPLAYER`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected {
    /// The player.
    pub character: CharacterId,
    /// `nlife`: lives left.
    pub lives: i32,
    /// `nPoint`: the points he scored.
    pub points: i32,
    /// `nMap`: the gate he chose, 0..=3, or 99 for a random one.
    pub map: i32,
    /// `dwTelTime`: when the gate window opened (`None`: no teleport pending).
    pub tel_time: Option<GameMs>,
}

/// The line-up of one admitted guild (`__GuildCombatMember` without the bid).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuildLineup {
    /// The selected players in line-up order.
    pub select: Vec<Selected>,
    /// `m_uidDefender`.
    pub defender: Option<CharacterId>,
    /// `lspFifo`: the players waiting for a place in the fight.
    pub queue: VecDeque<CharacterId>,
    /// `nWarCount`: the fighters at the moment.
    pub war_count: u32,
    /// `nGuildPoint`.
    pub points: i32,
}

impl GuildLineup {
    /// The selected player `c`.
    pub fn entry(&self, c: CharacterId) -> Option<&Selected> {
        self.select.iter().find(|s| s.character == c)
    }

    /// The selected player `c`, mutably.
    pub fn entry_mut(&mut self, c: CharacterId) -> Option<&mut Selected> {
        self.select.iter_mut().find(|s| s.character == c)
    }

    /// `true` when `c` waits in the queue.
    pub fn in_queue(&self, c: CharacterId) -> bool {
        self.queue.contains(&c)
    }

    /// Σ lives of all selected players.
    pub fn total_lives(&self) -> i32 {
        self.select.iter().map(|s| s.lives).sum()
    }

    /// Σ points of all selected players.
    pub fn player_points(&self) -> i32 {
        self.select.iter().map(|s| s.points).sum()
    }

    /// `true` when somebody still has lives.
    pub fn has_lives(&self) -> bool {
        self.select.iter().any(|s| s.lives > 0)
    }
}

/// Who a board goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum To {
    /// Everybody in the siege world.
    World,
    /// The online members of a guild.
    Guild(GuildId),
    /// One avatar.
    One(ObjId),
}

fn deliver(svc: &Services, ctx: &mut GameCtx<'_>, to: To, sub: Sub) {
    match to {
        To::World => fight::send_world(svc, ctx, sub),
        To::Guild(g) => super::send_guild(svc, ctx, g, sub),
        To::One(obj) => super::send_one(ctx, obj, sub),
    }
}

// ---- the pure builders ------------------------------------------------------------------------------------------------------

/// `GC_SELECTPLAYER`: the answer to the line-up window and to a submission. `window` echoes the request; the stored
/// defender and ids follow only for an admitted guild and a window request (the legacy layout).
pub fn select_player_msg(
    cfg: &SiegeConfig,
    admitted: bool,
    window: bool,
    lineup: Option<&GuildLineup>,
) -> Sub {
    let mut m = pb::guild_combat::GcSelectPlayer {
        max_join_member: cfg.max_join_member_size,
        max_war_player: cfg.max_war_player,
        request_war: admitted,
        ..pb::guild_combat::GcSelectPlayer::default()
    };
    if admitted {
        m.window = window;
        if window {
            m.defender = lineup.and_then(|l| l.defender).map_or(0, |d| d.0);
            m.ids = lineup
                .map(|l| l.select.iter().map(|s| s.character.0).collect())
                .unwrap_or_default();
        }
    }
    Sub::GcSelectPlayer(m)
}

/// `GC_GUILDSTATUS` (`AddGCGuildStatus`): the players of a guild with their lives, the fighters first (in line-up
/// order), then the waiting ones (in queue order), then the eliminated (in reverse line-up order, as the original's
/// `push_back` while it walked the selection backwards). `max_war_player` is sent only while somebody waits.
pub fn guild_status_msg(cfg: &SiegeConfig, lineup: Option<&GuildLineup>) -> Sub {
    let Some(l) = lineup else {
        return Sub::GcGuildStatus(pb::guild_combat::GcGuildStatus::default());
    };
    let life = |c: CharacterId, lives: i32| pb::guild_combat::GcLifeEntry {
        id_player: c.0,
        lives,
    };
    let mut players = Vec::with_capacity(l.select.len());
    for s in &l.select {
        if s.lives > 0 && !l.in_queue(s.character) {
            players.push(life(s.character, s.lives));
        }
    }
    for c in &l.queue {
        players.push(life(*c, l.entry(*c).map_or(0, |s| s.lives)));
    }
    for s in l.select.iter().rev() {
        if s.lives <= 0 {
            players.push(life(s.character, s.lives));
        }
    }
    Sub::GcGuildStatus(pb::guild_combat::GcGuildStatus {
        max_war_player: if l.queue.is_empty() {
            0
        } else {
            cfg.max_war_player
        },
        players,
    })
}

/// `GC_WARPLAYERLIST` (`SerializeGCWarPlayerList`): one entry per admitted guild in rank order: the defender and every
/// selected player with 1 (fighting: lives left and not waiting) or 0 (waiting or out).
pub fn war_player_list_msg(admitted: &[GuildId], fight: &FightState) -> Sub {
    let guilds = admitted
        .iter()
        .map(|g| match fight.lineup(*g) {
            Some(l) => pb::guild_combat::GcWarGuild {
                defender: l.defender.map_or(0, |d| d.0),
                players: l
                    .select
                    .iter()
                    .map(|s| pb::guild_combat::GcWarPlayer {
                        id_player: s.character.0,
                        fighting: s.lives > 0 && !l.in_queue(s.character),
                    })
                    .collect(),
            },
            None => pb::guild_combat::GcWarGuild::default(),
        })
        .collect();
    Sub::GcWarPlayerList(pb::guild_combat::GcWarPlayerList { guilds })
}

/// `GC_GUILDPRECEDENCE` (`AddGCGuildPrecedence`): every guild of the bid list in first-bid order; `send` is false for
/// a guild without selected players, `"Not Guild"` stands for a vanished one.
pub fn guild_precedence_msg(
    order: &[GuildId],
    fight: &FightState,
    name: impl Fn(GuildId) -> Option<String>,
) -> Sub {
    let entries = order
        .iter()
        .map(
            |g| match fight.lineup(*g).filter(|l| !l.select.is_empty()) {
                Some(l) => pb::guild_combat::GcGuildPoint {
                    send: true,
                    guild_name: name(*g).unwrap_or_else(|| "Not Guild".to_owned()),
                    point: l.points,
                },
                None => pb::guild_combat::GcGuildPoint::default(),
            },
        )
        .collect();
    Sub::GcGuildPrecedence(pb::guild_combat::GcGuildPrecedence { entries })
}

/// `GC_PLAYERPRECEDENCE` (`AddGCPlayerPrecedence`): the points of every selected player, per guild of the bid list.
pub fn player_precedence_msg(order: &[GuildId], fight: &FightState) -> Sub {
    let guilds = order
        .iter()
        .map(|g| pb::guild_combat::GcPlayerPoints {
            players: fight
                .lineup(*g)
                .map(|l| {
                    l.select
                        .iter()
                        .map(|s| pb::guild_combat::GcPlayerPointEntry {
                            id_player: s.character.0,
                            job: 0,
                            point: s.points,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();
    Sub::GcPlayerPrecedence(pb::guild_combat::GcPlayerPrecedence { guilds })
}

// ---- the senders (nothing while the siege is closed, except the war list) ---------------------------------------------------

/// `GC_WARPLAYERLIST` to `to`.
pub fn send_war_player_list(svc: &Services, ctx: &mut GameCtx<'_>, to: To) {
    let sub = war_player_list_msg(&fight::admitted(svc), &svc.social.siege.fight);
    deliver(svc, ctx, to, sub);
}

/// `GC_GUILDSTATUS` of `guild` to `to` (nothing while the siege is closed).
pub fn send_guild_status_to(svc: &Services, ctx: &mut GameCtx<'_>, guild: GuildId, to: To) {
    if svc.social.siege.state() == SiegeState::Close {
        return;
    }
    let sub = guild_status_msg(
        &svc.social.siege.config,
        svc.social.siege.fight.lineup(guild),
    );
    deliver(svc, ctx, to, sub);
}

/// `GC_GUILDPRECEDENCE` to `to` (nothing while the siege is closed).
pub fn send_guild_precedence(svc: &Services, ctx: &mut GameCtx<'_>, to: To) {
    if svc.social.siege.state() == SiegeState::Close {
        return;
    }
    let order = fight::bid_order(svc);
    let sub = guild_precedence_msg(&order, &svc.social.siege.fight, |g| {
        svc.social.guild.get(g).map(|x| x.name.clone())
    });
    deliver(svc, ctx, to, sub);
}

/// `GC_PLAYERPRECEDENCE` to `to` (nothing while the siege is closed).
pub fn send_player_precedence(svc: &Services, ctx: &mut GameCtx<'_>, to: To) {
    if svc.social.siege.state() == SiegeState::Close {
        return;
    }
    let order = fight::bid_order(svc);
    let sub = player_precedence_msg(&order, &svc.social.siege.fight);
    deliver(svc, ctx, to, sub);
}

// ---- SelectPlayerGuildCombat -----------------------------------------------------------------------------------------------

/// What a valid submission stores.
struct Plan {
    defender: CharacterId,
    kept: Vec<CharacterId>,
}

/// The checks of a submission (module documentation); `None` = refused.
fn validate(
    svc: &Services,
    ctx: &GameCtx<'_>,
    guild: GuildId,
    ids: &[u32],
    defender: u32,
) -> Option<Plan> {
    let g = svc.social.guild.get(guild)?;
    let mut seen = Vec::with_capacity(ids.len());
    for id in ids {
        let c = CharacterId(*id);
        if g.member(c).is_none() || seen.contains(&c) {
            return None;
        }
        seen.push(c);
    }
    let level_ok = |c: CharacterId| {
        live_avatar(&svc.presence, &*ctx.worlds, c)
            .and_then(|o| ctx.worlds.player(o))
            .is_some_and(|m| m.mv.level >= GC_MIN_LEVEL)
    };
    let online = |c: CharacterId| live_avatar(&svc.presence, &*ctx.worlds, c).is_some();
    // the master or a kingpin of the list is online
    let leader_online = seen.iter().any(|c| {
        g.member(*c)
            .is_some_and(|m| matches!(m.rank, GuildRank::Master | GuildRank::Kingpin))
            && online(*c)
    });
    let defender = CharacterId(defender);
    if !leader_online || !seen.contains(&defender) {
        return None;
    }
    if seen.len() > 1 && g.master() == Some(defender) {
        return None;
    }
    let kept: Vec<CharacterId> = seen.iter().copied().filter(|c| level_ok(*c)).collect();
    if !kept.contains(&defender) {
        return None; // FIX: the original stored a defender the filter had dropped
    }
    Some(Plan { defender, kept })
}

/// `OnGCSelectPlayer`: see the module documentation. The answer is always `GC_SELECTPLAYER` with the stored line-up;
/// after a submission the guild gets `GC_GUILDSTATUS` and everybody of the guild `GC_WARPLAYERLIST`.
pub fn select_player(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::SelectPlayerGuildCombat,
) {
    if !enabled(ctx) {
        return;
    }
    let Some(c) = fight::character(ctx, who.0) else {
        return;
    };
    if svc.social.siege.war_state() != SiegeWarState::NotEnterCount {
        notice::defined(ctx, who, TID_GAME_GUILDCOMBAT_CANNOT_MAKEUP, Vec::new());
        return;
    }
    let guild = svc
        .social
        .guild
        .guild_of(c)
        .filter(|g| fight::is_admitted(svc, *g));
    let Some(guild) = guild else {
        notice::defined(
            ctx,
            who,
            TID_GAME_GUILDCOMBAT_CANNOT_MAKEUP_FAIL,
            Vec::new(),
        );
        return;
    };
    let rank = svc
        .social
        .guild
        .get(guild)
        .and_then(|g| g.member(c))
        .map(|m| m.rank);
    if !matches!(rank, Some(GuildRank::Master | GuildRank::Kingpin)) {
        return;
    }
    if !m.window {
        let max = usize::try_from(svc.social.siege.config.max_join_member_size).unwrap_or(0);
        if m.ids.len() > max {
            return; // silent, no reply
        }
        if let Some(plan) = validate(svc, ctx, guild, &m.ids, m.defender) {
            let lives = svc.social.siege.config.max_player_life;
            let select = plan
                .kept
                .iter()
                .map(|c| Selected {
                    character: *c,
                    lives,
                    points: 0,
                    map: MAP_RANDOM,
                    tel_time: None,
                })
                .collect();
            svc.social.siege.fight.guilds.insert(
                guild,
                GuildLineup {
                    select,
                    defender: Some(plan.defender),
                    ..GuildLineup::default()
                },
            );
            tracing::info!(
                guild = guild.0,
                players = plan.kept.len(),
                "siege line-up stored"
            );
        }
    }
    let sub = select_player_msg(
        &svc.social.siege.config,
        true,
        m.window,
        svc.social.siege.fight.lineup(guild),
    );
    super::send_one(ctx, who.0, sub);
    if !m.window {
        send_guild_status_to(svc, ctx, guild, To::Guild(guild));
        send_war_player_list(svc, ctx, To::Guild(guild));
    }
}

/// `OnGCSelectMap` / `SetSelectMap`: the selected player chooses his gate; 0..=3 and 99 are valid, anything else counts
/// as 99 (FIX T22). Only a selected player's own entry is touched.
pub fn select_map(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, map: i32) {
    if !enabled(ctx) {
        return;
    }
    let Some(c) = fight::character(ctx, who.0) else {
        return;
    };
    let Some(guild) = svc.social.guild.guild_of(c) else {
        return;
    };
    let map = if matches!(map, 0..=3 | MAP_RANDOM) {
        map
    } else {
        MAP_RANDOM
    };
    if let Some(e) = svc
        .social
        .siege
        .fight
        .guilds
        .get_mut(&guild)
        .and_then(|l| l.entry_mut(c))
    {
        e.map = map;
    }
}

// ---- entry, observer, leaving ---------------------------------------------------------------------------------------------------

/// `OnGCJoin`: a player near an entry NPC (`MMI_GUILDWAR_JOIN`, VR-012) enters.
pub fn join_guild_combat(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    let Some((world, layer, pos)) = ctx
        .worlds
        .player(who.0)
        .map(|m| (m.core.world, m.core.layer, m.core.pos))
    else {
        return;
    };
    if ctx
        .worlds
        .close_npc(MMI_GUILDWAR_JOIN, world, layer, pos)
        .is_none()
    {
        return;
    }
    enter(svc, ctx, who);
}

/// `GuildCombatEnter` (also GM `/gcin`): see the module documentation. After a successful entry the player gets the
/// time to the next phase.
pub fn enter(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    let s = &svc.social.siege;
    let (state, war) = (s.state(), s.war_state());
    let entered = match state {
        SiegeState::Open if war != SiegeWarState::Enter => {
            notice::defined(ctx, who, TID_GAME_GUILDCOMBAT_NOT_JOIN, Vec::new());
            false
        }
        SiegeState::Open | SiegeState::Close | SiegeState::War => join_observer(svc, ctx, who),
        SiegeState::CompleteClose => {
            notice::defined(ctx, who, TID_GAME_GUILDCOMBAT_NOT_OPEN, Vec::new());
            false
        }
        SiegeState::GmClose => false,
    };
    if entered {
        send_next_time(svc, ctx, who);
    }
}

/// `AddGuildCombatNextTime( m_dwTime - GetTickCount(), GuildCombatProcess[m_nProcessGo - 1].dwParam )`: the time to
/// the due row and the war state it leads to, when a timer runs.
fn send_next_time(svc: &Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let s = &svc.social.siege;
    let Some(last) = s.step().checked_sub(1) else {
        return;
    };
    if s.step_due().0 == 0 {
        return;
    }
    let param = s.script().row(last).map_or(0, |r| r.param);
    let ms = u32::try_from(s.step_due().0.saturating_sub(ctx.clock.game_ms.0)).unwrap_or(u32::MAX);
    super::send_one(
        ctx,
        who.0,
        Sub::GcNextTimeState(pb::guild_combat::GcNextTimeState {
            time_ms: ms,
            state: param,
        }),
    );
}

/// `JoinObserver`: a vagrant is refused; the player loses his common buffs and stands at the observer spot (layer
/// 0). A selected player (admitted guild, lives left) becomes a standby fighter (state 2) and, in the war states 102
/// to 105, leaves his party. The boards of his guild and of the siege follow. `true` when he entered.
pub fn join_observer(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) -> bool {
    let Some((job, character)) = ctx
        .worlds
        .player(who.0)
        .and_then(|m| m.player().map(|p| (m.mv.job, p.ident.character_id)))
    else {
        return false;
    };
    let core = svc.stats.handle();
    if is_base(&core.rules, job) {
        let line = text_of(ctx, TID_GAME_GUILDCOMBAT_WAR_ENTER);
        notice::diag(ctx, who, &line);
        return false;
    }
    crate::buff::api::remove_common(svc, ctx, who.0);
    ctx.broadcast(
        None,
        who.0,
        Audience::NearInclSelf,
        pb::RemoveAllSkillInfluence {},
    );
    if let Err(e) = ctx.worlds.replace(
        who.0,
        WorldId(WI_WORLD_GUILDWAR),
        OBSERVER_SPOT,
        LayerId(0),
        ReplaceKind::Normal,
    ) {
        tracing::warn!(error = %e, "a siege observer could not be moved to the observer spot");
    }
    for tid in [TID_GAME_GUILDCOMBAT_WELCOME, TID_GAME_GUILDCOMBAT_ZOOM_USE] {
        let line = text_of(ctx, tid);
        notice::text(ctx, who, &line);
    }
    let war = svc.social.siege.war_state();
    if war.code() <= SiegeWarState::Enter.code() {
        send_war_player_list(svc, ctx, To::One(who.0));
    }
    let guild = svc.social.guild.guild_of(character);
    let selected = guild.is_some_and(|g| {
        fight::is_admitted(svc, g)
            && svc
                .social
                .siege
                .fight
                .lineup(g)
                .and_then(|l| l.entry(character))
                .is_some_and(|s| s.lives > 0)
    });
    if selected {
        set_gc_state(ctx, who.0, 2);
        if (SiegeWarState::Enter.code()..=SiegeWarState::WarClose.code()).contains(&war.code()) {
            leave_party(svc, ctx, character);
        }
    }
    match guild {
        Some(g) => send_guild_status_to(svc, ctx, g, To::One(who.0)),
        // a player without a guild still gets the (empty) board, as the original's `AddGCGuildStatus( 0, pUser )`
        None if svc.social.siege.state() != SiegeState::Close => {
            let sub = guild_status_msg(&svc.social.siege.config, None);
            super::send_one(ctx, who.0, sub);
        }
        None => {}
    }
    send_guild_precedence(svc, ctx, To::One(who.0));
    send_player_precedence(svc, ctx, To::One(who.0));
    true
}

/// `SendGCRemoveParty`: the player leaves his party (the core's `OnGCRemoveParty` names the party leader).
pub(crate) fn leave_party(svc: &mut Services, ctx: &mut GameCtx<'_>, c: CharacterId) {
    let Some(leader) = svc
        .social
        .party
        .party_record_of(c)
        .and_then(|p| p.leader_id())
    else {
        return;
    };
    let leader_name = name_of(svc, leader).unwrap_or_default();
    crate::social::party::remove_member(svc, ctx, c, &leader_name);
}

/// `DeathHooks::gw_observer`: a player who revives in the siege world joins the observers.
pub fn gw_observer(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    let _ = join_observer(svc, ctx, who);
}

/// `OnGCTele` (FIX: only in the siege world and only for the living): the player leaves for Madrigal (6983, 0, 3330),
/// when the revival region `flaris` exists.
pub fn tele_out(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let _ = svc;
    if !enabled(ctx) {
        return;
    }
    let inside = ctx
        .worlds
        .player(who.0)
        .is_some_and(|m| m.core.world.0 == WI_WORLD_GUILDWAR && !crate::combat::state::is_die(m));
    if !inside {
        return;
    }
    if ctx
        .worlds
        .revival
        .revival_pos(WorldId(WI_WORLD_MADRIGAL), "flaris")
        .is_none()
    {
        return;
    }
    if let Err(e) = ctx.worlds.replace(
        who.0,
        WorldId(WI_WORLD_MADRIGAL),
        MADRIGAL_LEAVE,
        LayerId(0),
        ReplaceKind::Normal,
    ) {
        tracing::debug!(error = %e, "a siege player could not leave for Madrigal");
    }
}

/// `WARSTATE 102` (`SetEnter`): the entry opens. `TID_GAME_GUILDCOMBAT_ENTER` goes to everybody as a system notice and
/// a small caption, and every ONLINE selected player of the admitted guilds (lives are not looked at) gets the offer
/// to teleport (`GC_TELE`).
pub fn set_enter(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if !enabled(ctx) {
        return;
    }
    let line = text_of(ctx, TID_GAME_GUILDCOMBAT_ENTER);
    notice::system(svc, ctx, &line);
    notice::caption(svc, ctx, Audience::AllOnline, true, WorldId(0), &line);
    let tele = text_of(ctx, TID_GAME_GUILDCOMBAT_TELE);
    let mut targets = Vec::new();
    for guild in fight::admitted(svc) {
        if let Some(l) = svc.social.siege.fight.lineup(guild) {
            for s in &l.select {
                if let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, s.character) {
                    targets.push(obj);
                }
            }
        }
    }
    for obj in targets {
        super::send_one(
            ctx,
            obj,
            Sub::GcTele(pb::guild_combat::GcTele { text: tele.clone() }),
        );
    }
}
