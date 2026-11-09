//! The event flow (Plan/22 §11.3; `SecretRoom.cpp:126-250` the manager `Process`, `378-410` the open, `956-1368` the
//! sides): open, rooms, entrance, presence, spawns, kills, fail, win, close-wait, close and the ONE result batch.
//!
//! ```text
//! Close --open (Sun 18:00)--> Open --1 s--> LineUp --LineUpTime--> Entrance --EntranceTime--> WarWait --WarWaitTime--> War
//! War: every side Open -> War (WarTime) -> CloseWait (CloseWaitTime) -> Close; all sides Close -> manager Close
//! ```
//!
//! **Open** copies the script numbers and ranks every side with at least `MinGuildNum` tenders: the ranks beyond
//! `MaxGuildNum` are dropped with the `DropoutReturnRate` refund by system mail to the CURRENT master of the guild
//! (a vanished guild: nothing is paid, an error is logged, the row is still `F`, KEEP as the siege) and the others
//! get the NORMAL groups of the script as their own queue. ONE `secret_room_result` batch (the `F` rows and the mails).
//!
//! **War, per second and side**: `Open` sends the guild list and starts the clock; `War` ends at the deadline or when
//! every guild failed (`NOWINNER`), otherwise checks the masters (a non-failed guild whose CURRENT master is not
//! online in its room fails, a vanished guild too) and spawns `min(MonsterGenNum, front.num)` monsters of EACH guild's
//! OWN queue (PL-102: the legacy shared one counter across the guilds); `CloseWait` sends the monsters of every room
//! away and, for the winner, hands the side to the tax service; `Close` sends the members out. A side that closes
//! in a pass counts as closed at the next second (legacy); when all sides are closed the manager closes (`ALLCLOSE`).
//!
//! **Result persistence** (decision): nothing of the war is written while it runs (KEEP 13 §11). At the manager
//! close ONE `secret_room_result` batch carries every `W` / `L` row, the tax rows of the sides whose winner went
//! to `set_next_guild` (read AT the close) and the new round. A crash before it leaves every ranked tender `T` and
//! the war void.

use super::tender::{area_of, npc_ok, say, say_side, side_name, text_with};
use super::wire::{self, online_obj};
use super::{
    SecretRoomService, SrMngState, SrSideState, SrSpawn, SrTender, SrWarState, deadline_in,
    enabled, is_room_world, passed, remain_ms, room_for,
};
use crate::chat::ingress::notice;
use crate::combat::KillCtx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::event::scripts::SrMonsterKind;
use crate::outbox::Audience;
use crate::presence::PlayerDirectory;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use crate::world::ReplaceKind;
use crate::world::continent::{CONT_NODATA, Continents};
use flyff_common::defs::neuz::MMI_SECRET_ENTRANCE;
use flyff_common::defs::text::{
    TID_GAME_SECRETROOM_ALLCLOSE, TID_GAME_SECRETROOM_CONTOPEN, TID_GAME_SECRETROOM_ISLOSTGUILD,
    TID_GAME_SECRETROOM_NOENTRANCE, TID_GAME_SECRETROOM_NOLINEUPMEMBER,
    TID_GAME_SECRETROOM_NOTOPEN, TID_GAME_SECRETROOM_NOTOPENCONT, TID_GAME_SECRETROOM_NOWINNER,
    TID_GAME_SECRETROOM_RETURNPENYA_POST_SUBSTANCE, TID_GAME_SECRETROOM_RETURNPENYA_POST_TITLE,
    TID_GAME_SECRETROOM_TENDERFAILED, TID_GAME_SECRETROOM_WINNER,
};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{CharacterId, GameMs, GuildId, LayerId, MailId, ObjId, WorldId};
use flyff_db::audit::content as audit;
use flyff_db::{AtomicBatch, SecretRoomTenderRecord, SecretRoomTenderState};
use flyff_rules::consts::NULL_ID;

/// Where a member of a guild still on the normal monsters enters the room (`SetTeleportNormalRoom`).
const NORMAL_SPOT: (i32, i32) = (295, 530);
/// Where a member of a guild on the boss enters (`SetTeleportBossRoom`).
const BOSS_SPOT: (i32, i32) = (600, 310);
/// The height of both spots.
const SPOT_Y: f32 = 102.0;

// ---- sending -------------------------------------------------------------------------------------------------------------

/// `SendNowStateAllMember(cont, state, remain)`: `state` is `Some((state, remain))` for an explicit state, which skips
/// the guilds that failed, or `None` for the manager state with its time, which reaches every line-up member. `cont`
/// limits it to one side. Only online members get it, in rank and line-up order.
fn send_state_all(
    pres: &PlayerDirectory,
    ctx: &mut GameCtx<'_>,
    sr: &SecretRoomService,
    cont: Option<u8>,
    explicit: Option<(i32, u32)>,
    now: GameMs,
) {
    let (state, remain) = explicit.unwrap_or((sr.state.code(), remain_ms(sr.deadline, now)));
    for (k, side) in &sr.sides {
        if cont.is_some_and(|c| c != *k) {
            continue;
        }
        for t in &side.tenders {
            if explicit.is_some() && t.war == SrWarState::Failed {
                continue;
            }
            for c in &t.lineup {
                wire::send_state(pres, ctx, *c, state, remain);
            }
        }
    }
}

/// `UpdateInfoToAllMember(type, index)`: `info` to the online line-up members of the guilds of `side` that did not fail.
fn send_info_all(
    pres: &PlayerDirectory,
    ctx: &mut GameCtx<'_>,
    sr: &SecretRoomService,
    side: u8,
    info: &flyff_proto::pb::SecretRoomInfo,
) {
    let Some(sd) = sr.sides.get(&side) else {
        return;
    };
    for t in &sd.tenders {
        if t.war == SrWarState::Failed {
            continue;
        }
        for c in &t.lineup {
            wire::send_info(pres, ctx, *c, info.clone());
        }
    }
}

/// The kills needed per war state (`m_mapMonsterNum`) of the run in force.
fn totals(sr: &SecretRoomService) -> [i32; 3] {
    [
        sr.cfg.monster_total(SrMonsterKind::Normal),
        sr.cfg.monster_total(SrMonsterKind::MidBoss),
        sr.cfg.monster_total(SrMonsterKind::Boss),
    ]
}

/// `UpdateInfoToAllMember(SECRETROOM_GUILDLIST)`: the guild list, with the line-up of the receiver's own guild, to the
/// non-failed line-up members of `side`.
pub(super) fn send_guild_list_all(s: &Services, ctx: &mut GameCtx<'_>, side: u8) {
    let sr = &s.event.secret_room;
    let Some(sd) = sr.sides.get(&side) else {
        return;
    };
    let totals = totals(sr);
    for t in &sd.tenders {
        if t.war == SrWarState::Failed {
            continue;
        }
        for c in &t.lineup {
            let guild = s.social.guild.guild_of(*c);
            let info = wire::guild_list(side, &sd.tenders, guild, totals);
            wire::send_info(&s.presence, ctx, *c, info);
        }
    }
}

/// A caption to everybody (`g_DPCoreClient.SendCaption`).
fn caption(s: &Services, ctx: &mut GameCtx<'_>, line: &str) {
    notice::caption(s, ctx, Audience::AllOnline, false, WorldId(0), line);
}

// ---- open ----------------------------------------------------------------------------------------------------------------

/// The result of the ranking of one side at the open: the `F` rows and the refund mails.
struct Dropped {
    rows: Vec<SecretRoomTenderRecord>,
    mails: Vec<MailId>,
    refunds: i64,
}

/// `CSecretRoomMng::SecretRoomOpen` (the wall job `secret_room_open`, also GM `/sropen`): see the module documentation.
/// Does nothing unless flag 24 is on and the manager is `Close`; `true` when at least one side opened.
pub fn open(s: &mut Services, ctx: &mut GameCtx<'_>) -> bool {
    if !enabled(ctx) || s.event.secret_room.state != SrMngState::Close {
        return false;
    }
    let cfg = s.event.configs.secret_room.clone();
    s.event.secret_room.cfg = cfg.clone();
    let sides: Vec<u8> = s.event.secret_room.sides.keys().copied().collect();
    let mut opened = false;
    let mut dropped = Dropped {
        rows: Vec::new(),
        mails: Vec::new(),
        refunds: 0,
    };
    for k in sides {
        let n = s
            .event
            .secret_room
            .sides
            .get(&k)
            .map_or(0, |sd| sd.tenders.len());
        if i64::try_from(n).unwrap_or(i64::MAX) < i64::from(cfg.min_guild_num) {
            continue;
        }
        let line = text_with(ctx, TID_GAME_SECRETROOM_CONTOPEN, &[side_name(ctx, k)]);
        caption(s, ctx, &line);
        opened = true;
        open_side(s, ctx, k, &mut dropped);
    }
    if !opened {
        let line = ctx.data.texts.get(TID_GAME_SECRETROOM_NOTOPEN).to_owned();
        caption(s, ctx, &line);
        return false;
    }
    s.event.secret_room.state = SrMngState::Open;
    s.event.secret_room.deadline = None;
    if !dropped.rows.is_empty() {
        let n = u32::try_from(dropped.rows.len()).unwrap_or(u32::MAX);
        let records: Vec<_> = dropped
            .mails
            .iter()
            .filter_map(|id| {
                s.econ
                    .mail
                    .get(*id)
                    .map(crate::econ::mail::MailEntry::to_record)
            })
            .collect();
        let round = s.event.secret_room.round;
        let batch =
            AtomicBatch::secret_room_result(dropped.rows, records, Vec::new(), Vec::new(), None);
        submit_result(ctx, "SecretRoomOpen", batch, dropped.mails);
        ctx.audit.emit(audit::secret_room_result(
            round,
            "open",
            n,
            n,
            dropped.refunds,
            &[],
        ));
    }
    true
}

/// `CSecretRoomContinent::SecretRoomContOpen`: the side is `Open`, the ranks beyond `MaxGuildNum` are dropped with
/// their refund, the others get the NORMAL groups.
fn open_side(s: &mut Services, ctx: &mut GameCtx<'_>, k: u8, dropped: &mut Dropped) {
    let cfg = s.event.secret_room.cfg.clone();
    let keep = usize::try_from(cfg.max_guild_num.max(0)).unwrap_or(0);
    let normal: Vec<SrSpawn> = cfg
        .monsters
        .iter()
        .filter(|m| m.kind == SrMonsterKind::Normal)
        .map(SrSpawn::of)
        .collect();
    let out: Vec<SrTender> = {
        let Some(sd) = s.event.secret_room.sides.get_mut(&k) else {
            return;
        };
        sd.state = SrSideState::Open;
        sd.deadline = None;
        sd.failed = 0;
        let out = if sd.tenders.len() > keep {
            sd.tenders.split_off(keep)
        } else {
            Vec::new()
        };
        for t in &mut sd.tenders {
            t.war = SrWarState::Normal;
            t.kills = 0;
            t.room = None;
            t.pending = normal.iter().cloned().collect();
        }
        out
    };
    if out.is_empty() {
        return;
    }
    let title = ctx
        .data
        .texts
        .get(TID_GAME_SECRETROOM_RETURNPENYA_POST_TITLE)
        .to_owned();
    let body = ctx
        .data
        .texts
        .get(TID_GAME_SECRETROOM_RETURNPENYA_POST_SUBSTANCE)
        .to_owned();
    for t in out {
        let refund = super::tender::refund_after_fee(t.penya, cfg.dropout_return_rate);
        match s.social.guild.get(t.guild).and_then(|g| g.master()) {
            Some(master) => {
                let (id, _ops) = s.econ.mail.send_system_with_id(
                    master,
                    &title,
                    &body,
                    None,
                    i64::from(refund.max(0)),
                );
                dropped.mails.push(id);
                dropped.refunds += i64::from(refund.max(0));
                if let Some(obj) = online_obj(&s.presence, ctx, master) {
                    notice::defined(
                        ctx,
                        PlayerRef(obj),
                        TID_GAME_SECRETROOM_TENDERFAILED,
                        Vec::new(),
                    );
                }
            }
            None => {
                // KEEP: a vanished guild is not refunded (as the siege); the row is still closed
                tracing::error!(
                    guild = t.guild.0,
                    tender = t.penya,
                    refund,
                    "secret room: the guild of a dropped tender is gone: the refund is lost"
                );
            }
        }
        dropped
            .rows
            .push(t.record(k, SecretRoomTenderState::Failed, t.round));
    }
}

/// Sends `batch` with a ticket that forgets the mails it carried when the commit fails (the database does not have
/// them) and logs a CRITICAL line: the batch is all or nothing, the memory is kept (21 §2.4).
fn submit_result(
    ctx: &mut GameCtx<'_>,
    name: &'static str,
    batch: AtomicBatch,
    mails: Vec<MailId>,
) {
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| match result {
            TicketResult::Committed(_) => {}
            TicketResult::Failed(e) => {
                for id in &mails {
                    svc.econ.mail.remove(*id);
                }
                tracing::error!(
                    error = %e,
                    batch = name,
                    "CRITICAL: a secret-room result batch failed: the memory is kept (a bug, 21 §2.4)"
                );
            }
            other => tracing::error!(?other, batch = name, "a secret-room batch completed unexpectedly"),
        },
    )));
    ctx.persist.atomic(batch.with_ticket(ticket));
}

// ---- the manager states ---------------------------------------------------------------------------------------------------

/// `Open` -> `LineUp` (one second after the open): the clock starts and every online master of a tendering guild of an
/// open side gets `SecretRoomMngState{3, remain}`.
pub(super) fn begin_line_up(s: &mut Services, ctx: &mut GameCtx<'_>, now: GameMs) {
    let sr = &mut s.event.secret_room;
    sr.state = SrMngState::LineUp;
    sr.deadline = Some(deadline_in(now, sr.cfg.line_up_time_ms));
    let remain = remain_ms(sr.deadline, now);
    let masters: Vec<CharacterId> = sr
        .sides
        .values()
        .filter(|sd| sd.state == SrSideState::Open)
        .flat_map(|sd| sd.tenders.iter())
        .filter_map(|t| s.social.guild.get(t.guild).and_then(|g| g.master()))
        .collect();
    for m in masters {
        wire::send_state(&s.presence, ctx, m, SrMngState::LineUp.code(), remain);
    }
}

/// `LineUp` -> `Entrance`: the rooms are assigned (`152 + rank + 8 k`), the masters of the tenders that got a room hear
/// `{101, 0}` (KEEP-QUIRK of `SetSecretRoomWorld`) and every online line-up member the manager state.
pub(super) fn begin_entrance(s: &mut Services, ctx: &mut GameCtx<'_>, now: GameMs) {
    let sr = &mut s.event.secret_room;
    sr.state = SrMngState::Entrance;
    sr.deadline = Some(deadline_in(now, sr.cfg.entrance_time_ms));
    let mut masters = Vec::new();
    for (k, sd) in sr.sides.values_mut().enumerate() {
        for (rank, t) in sd.tenders.iter_mut().enumerate() {
            if let Some(room) = room_for(rank, k) {
                t.room = Some(room);
                if let Some(m) = s.social.guild.get(t.guild).and_then(|g| g.master()) {
                    masters.push(m);
                }
            }
        }
    }
    for m in masters {
        wire::send_state(&s.presence, ctx, m, SrSideState::Close.code(), 0);
    }
    send_state_all(&s.presence, ctx, &s.event.secret_room, None, None, now);
}

/// `Entrance` -> `WarWait`.
pub(super) fn begin_war_wait(s: &mut Services, ctx: &mut GameCtx<'_>, now: GameMs) {
    let sr = &mut s.event.secret_room;
    sr.state = SrMngState::WarWait;
    sr.deadline = Some(deadline_in(now, sr.cfg.war_wait_time_ms));
    send_state_all(&s.presence, ctx, &s.event.secret_room, None, None, now);
}

/// `WarWait` -> `War`: `{6, 0}` to every online line-up member.
pub(super) fn begin_war(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    let sr = &mut s.event.secret_room;
    sr.state = SrMngState::War;
    sr.deadline = None;
    send_state_all(&s.presence, ctx, &s.event.secret_room, None, None, now);
}

/// The `War` manager state, one second: every side that is not closed is processed in map order; a side that closes in
/// this pass counts at the next second. When all sides are closed the manager closes.
pub(super) fn process_war(s: &mut Services, ctx: &mut GameCtx<'_>, now: GameMs) {
    let keys: Vec<u8> = s.event.secret_room.sides.keys().copied().collect();
    let mut closed = 0;
    for k in &keys {
        let state = s.event.secret_room.sides.get(k).map(|sd| sd.state);
        if state == Some(SrSideState::Close) {
            closed += 1;
        } else {
            process_side(s, ctx, *k, now);
        }
    }
    if closed >= keys.len() {
        close_manager(s, ctx, now);
    }
}

// ---- the sides -------------------------------------------------------------------------------------------------------------

/// `CSecretRoomContinent::Process`, one second.
fn process_side(s: &mut Services, ctx: &mut GameCtx<'_>, k: u8, now: GameMs) {
    let Some((state, deadline, failed, n)) = s
        .event
        .secret_room
        .sides
        .get(&k)
        .map(|sd| (sd.state, sd.deadline, sd.failed, sd.tenders.len()))
    else {
        return;
    };
    match state {
        SrSideState::Close => {}
        SrSideState::Open => {
            send_guild_list_all(s, ctx, k);
            let sr = &mut s.event.secret_room;
            let war_time = sr.cfg.war_time_ms;
            if let Some(sd) = sr.sides.get_mut(&k) {
                sd.state = SrSideState::War;
                sd.deadline = Some(deadline_in(now, war_time));
            }
            let remain = war_time;
            send_state_all(
                &s.presence,
                ctx,
                &s.event.secret_room,
                Some(k),
                Some((SrSideState::War.code(), remain)),
                now,
            );
        }
        SrSideState::War => {
            if passed(deadline, now) || usize::try_from(failed).unwrap_or(usize::MAX) >= n {
                let line = text_with(ctx, TID_GAME_SECRETROOM_NOWINNER, &[side_name(ctx, k)]);
                caption(s, ctx, &line);
                close_wait(s, ctx, k, now);
            } else {
                check_masters(s, ctx, k);
                create_monsters(s, ctx, k);
            }
        }
        SrSideState::CloseWait => {
            if passed(deadline, now) {
                close_side(s, ctx, k, now);
            }
        }
    }
}

/// `ProcessInGuildMaster`: every non-failed guild whose CURRENT master is not online inside its room fails (a vanished
/// guild, a master elsewhere, a guild without a room).
fn check_masters(s: &mut Services, ctx: &mut GameCtx<'_>, k: u8) {
    let Some(sd) = s.event.secret_room.sides.get(&k) else {
        return;
    };
    let absent: Vec<GuildId> = sd
        .tenders
        .iter()
        .filter(|t| t.war != SrWarState::Failed)
        .filter(|t| {
            let here = s
                .social
                .guild
                .get(t.guild)
                .and_then(|g| g.master())
                .and_then(|m| online_obj(&s.presence, ctx, m))
                .and_then(|o| ctx.worlds.core(o))
                .map(|c| c.world);
            here.is_none() || here != t.room
        })
        .map(|t| t.guild)
        .collect();
    for guild in absent {
        fail_guild(s, ctx, k, guild);
    }
}

/// `x` or `z` between two corners in any order: `[lo, hi)`, `lo` when they are equal (`xRandom(min, max)`).
fn between(rng: &mut dyn GameRng, a: i32, b: i32) -> i32 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    if hi <= lo {
        lo
    } else {
        let span = u32::try_from(i64::from(hi) - i64::from(lo)).unwrap_or(u32::MAX);
        lo.saturating_add(i32::try_from(rng.random_below(span)).unwrap_or(0))
    }
}

/// `CreateMonster` (PL-102 FIX): for every non-failed guild with a room and a non-empty queue,
/// `n = min(MonsterGenNum, front.num)` monsters of its OWN front group; the count runs down even for an unknown prop.
fn create_monsters(s: &mut Services, ctx: &mut GameCtx<'_>, k: u8) {
    let gen_num = s.event.secret_room.cfg.monster_gen_num;
    let count = s
        .event
        .secret_room
        .sides
        .get(&k)
        .map_or(0, |sd| sd.tenders.len());
    for i in 0..count {
        let Some((room, front)) = s
            .event
            .secret_room
            .sides
            .get(&k)
            .and_then(|sd| sd.tenders.get(i))
            .filter(|t| t.war != SrWarState::Failed)
            .and_then(|t| t.room.zip(t.pending.front().cloned()))
        else {
            continue;
        };
        if ctx.worlds.world(room).is_none() {
            continue;
        }
        let n = gen_num.min(front.num);
        if ctx.data.mover(front.mover).is_some() {
            for _ in 0..n.max(0) {
                let x = between(ctx.rng.event(), front.rect[0], front.rect[2]);
                let z = between(ctx.rng.event(), front.rect[1], front.rect[3]);
                let spawn = crate::ai::MonsterSpawn {
                    mover: front.mover,
                    world: room,
                    layer: LayerId(0),
                    pos: Vec3::new(x as f32, front.y as f32, z as f32),
                    active_attack: front.red,
                    gold: None,
                    owner_ai: None,
                };
                if crate::ai::spawn::commit(ctx.worlds, ctx.data, ctx.rng.event(), &spawn).is_none()
                {
                    tracing::warn!(
                        mover = front.mover,
                        world = room.0,
                        "secret room: a monster was not created"
                    );
                }
            }
        }
        if let Some(t) = s
            .event
            .secret_room
            .sides
            .get_mut(&k)
            .and_then(|sd| sd.tenders.get_mut(i))
            && let Some(f) = t.pending.front_mut()
        {
            f.num -= n;
            if f.num <= 0 {
                t.pending.pop_front();
            }
        }
    }
}

/// `RemoveAllSecretRoomObj`: deletes every non-player mover of the room (layer 0, members and the add queue).
fn remove_monsters(ctx: &mut GameCtx<'_>, room: Option<WorldId>) {
    let Some(room) = room else { return };
    let ids: Vec<ObjId> = ctx
        .worlds
        .store()
        .ids_in_layer(room, LayerId(0))
        .into_iter()
        .filter(|id| {
            ctx.worlds
                .mover(*id)
                .is_some_and(|m| !m.is_player() && !m.core.deleted)
        })
        .collect();
    for id in ids {
        ctx.worlds.delete(id);
    }
}

// ---- the entrance ----------------------------------------------------------------------------------------------------------

/// Two jitters `xRandom(4) - 2` (-2..=1, the asymmetric KEEP-QUIRK) for x and z.
pub(super) fn jitter(ctx: &mut GameCtx<'_>) -> (i32, i32) {
    let rng = ctx.rng.event();
    let x = i32::try_from(rng.random_below(4)).unwrap_or(0) - 2;
    let z = i32::try_from(rng.random_below(4)).unwrap_or(0) - 2;
    (x, z)
}

/// `SetTeleportNormalRoom` / `SetTeleportBossRoom`: angle 180 and a normal replace to the room, layer 0; `false` when the
/// replace was refused (a flying player).
pub(super) fn teleport_into(ctx: &mut GameCtx<'_>, obj: ObjId, room: WorldId, boss: bool) -> bool {
    let (jx, jz) = jitter(ctx);
    let (bx, bz) = if boss { BOSS_SPOT } else { NORMAL_SPOT };
    set_angle_180(ctx, obj);
    let pos = Vec3::new((bx + jx) as f32, SPOT_Y, (bz + jz) as f32);
    match ctx
        .worlds
        .replace(obj, room, pos, LayerId(0), ReplaceKind::Normal)
    {
        Ok(()) => true,
        Err(e) => {
            tracing::debug!(player = obj.0, room = room.0, error = %e, "secret room: the replace into the room was refused");
            false
        }
    }
}

/// `SetAngle(180.0f)`.
pub(super) fn set_angle_180(ctx: &mut GameCtx<'_>, obj: ObjId) {
    if let Some(m) = ctx.worlds.player_mut(obj) {
        let angle = flyff_sim::num::set_angle_quantized(180.0);
        m.sim.angle = angle;
        m.core.angle = angle;
    }
}

/// `SecretRoomEntrance` (menu 87, `SetTeleportSecretRoom` + `SetTeleportSecretRoomCont`): the manager must be in
/// `Entrance`, `WarWait` or `War` (`NOENTRANCE`); the side at the player's position must not be `Close`
/// (`NOTOPENCONT("\"side\"")`); his guild must have a tender there (silent otherwise), he must be in its line-up
/// (`NOLINEUPMEMBER`) and the guild must not have failed (`ISLOSTGUILD`). Then he is put into the guild's room (the boss
/// spot when the guild fights the boss), his common buffs are removed and he leaves his party.
pub fn on_entrance(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !npc_ok(ctx, who, MMI_SECRET_ENTRANCE) {
        return;
    }
    if !matches!(
        s.event.secret_room.state,
        SrMngState::Entrance | SrMngState::WarWait | SrMngState::War
    ) {
        say(ctx, who, TID_GAME_SECRETROOM_NOENTRANCE);
        return;
    }
    let side = area_of(ctx, who);
    if side == CONT_NODATA {
        return;
    }
    let open = s
        .event
        .secret_room
        .side(side)
        .is_some_and(|sd| sd.state != SrSideState::Close);
    if !open {
        say_side(ctx, who, TID_GAME_SECRETROOM_NOTOPENCONT, side);
        return;
    }
    let Some(c) = crate::social::guild::character_of(ctx, who) else {
        return;
    };
    let Some(tender) = s
        .social
        .guild
        .guild_of(c)
        .and_then(|g| s.event.secret_room.side(side).and_then(|sd| sd.tender(g)))
    else {
        return;
    };
    if !tender.has_member(c) {
        say(ctx, who, TID_GAME_SECRETROOM_NOLINEUPMEMBER);
        return;
    }
    if tender.war == SrWarState::Failed {
        say(ctx, who, TID_GAME_SECRETROOM_ISLOSTGUILD);
        return;
    }
    let (room, boss) = (tender.room, tender.war == SrWarState::Boss);
    let Some(room) = room else {
        // a rank beyond the rooms (a larger `MaxGuildNum` than the stride): nothing to enter
        return;
    };
    if !teleport_into(ctx, who.0, room, boss) {
        return; // a flying player stays where he is, with his buffs and his party
    }
    crate::buff::api::remove_common(s, ctx, who.0);
    crate::social::siege::lineup::leave_party(s, ctx, c);
}

// ---- kills and failures ----------------------------------------------------------------------------------------------------

/// `DeathHooks::world_die` (`DoDie` step 9, `Mover.cpp:5527-5535`): in a room world, while the manager is in `War`,
/// a monster killed by a player counts for the guild whose room it is (`on_monster_killed`); a player who is the
/// master of his guild fails it (`fail_by_master`).
pub fn on_world_die(s: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    if s.event.secret_room.state != SrMngState::War {
        return;
    }
    let Some((world, is_player, prop, character)) = ctx.worlds.mover(k.victim).map(|m| {
        (
            m.core.world,
            m.is_player(),
            m.prop_id(),
            m.player().map(|p| p.ident.character_id),
        )
    }) else {
        return;
    };
    if !is_room_world(world) {
        return;
    }
    if is_player {
        if let Some(c) = character
            && s.social.guild.is_master(c)
            && let Some(guild) = s.social.guild.guild_of(c)
        {
            fail_by_master(s, ctx, guild);
        }
    } else if ctx.worlds.player(k.attacker).is_some() {
        on_monster_killed(s, ctx, world, prop);
    }
}

/// `SetSecretRoomKill`: a monster of prop `mover` died in room `world` by a player. It counts when the guild of that room
/// fights a state whose groups hold the prop; `kills` grows, the side's non-failed members hear `KillCount`, and when
/// `kills` reaches the total of the state the guild moves on (see [`advance`]).
fn on_monster_killed(s: &mut Services, ctx: &mut GameCtx<'_>, world: WorldId, mover: u32) {
    let sides: Vec<u8> = s.event.secret_room.sides.keys().copied().collect();
    for k in sides {
        let sr = &s.event.secret_room;
        let Some(sd) = sr.sides.get(&k).filter(|sd| sd.state == SrSideState::War) else {
            continue;
        };
        let Some(idx) = sd.tenders.iter().position(|t| t.room == Some(world)) else {
            continue;
        };
        let t = &sd.tenders[idx];
        // the legacy read garbage for a war state other than the three monster classes
        let Some(kind) = t.war.kind() else { return };
        if mover == NULL_ID
            || !sr
                .cfg
                .monsters
                .iter()
                .any(|m| m.kind == kind && m.mover == mover)
        {
            return;
        }
        let kills = {
            let Some(t) = s
                .event
                .secret_room
                .sides
                .get_mut(&k)
                .and_then(|sd| sd.tenders.get_mut(idx))
            else {
                return;
            };
            t.kills += 1;
            t.kills
        };
        let info = wire::kill_count(k, idx, kills);
        send_info_all(&s.presence, ctx, &s.event.secret_room, k, &info);
        advance(s, ctx, k, idx);
        return;
    }
}

/// The recursion of `SetSecretRoomContKill`: while the kills reach the total of the state, they reset and the guild moves
/// on - Normal to MidBoss (the queue becomes the mid-boss groups), MidBoss to Boss (the queue becomes the boss and
/// every online line-up member inside a room goes to the boss spot), Boss to Win (`WINNER` caption, close-wait). Each
/// change sends `WarState{index, war}`; a state with a total of 0 is passed at once.
fn advance(s: &mut Services, ctx: &mut GameCtx<'_>, k: u8, idx: usize) {
    loop {
        let sr = &s.event.secret_room;
        let Some(sd) = sr.sides.get(&k).filter(|sd| sd.state == SrSideState::War) else {
            return;
        };
        let Some(t) = sd.tenders.get(idx) else { return };
        let Some(kind) = t.war.kind() else { return };
        if t.kills < sr.cfg.monster_total(kind) {
            return;
        }
        let war = t.war;
        let guild = t.guild;
        let lineup = t.lineup.clone();
        let next = match war {
            SrWarState::Normal => SrWarState::MidBoss,
            SrWarState::MidBoss => SrWarState::Boss,
            _ => SrWarState::Win,
        };
        let queue: Vec<SrSpawn> = match next {
            SrWarState::MidBoss | SrWarState::Boss => {
                let wanted = next.kind();
                sr.cfg
                    .monsters
                    .iter()
                    .filter(|m| Some(m.kind) == wanted)
                    .map(SrSpawn::of)
                    .collect()
            }
            _ => Vec::new(),
        };
        if let Some(t) = s
            .event
            .secret_room
            .sides
            .get_mut(&k)
            .and_then(|sd| sd.tenders.get_mut(idx))
        {
            t.kills = 0;
            t.war = next;
            if next != SrWarState::Win {
                t.pending = queue.into_iter().collect();
            }
        }
        let info = wire::war_state(k, idx, next);
        send_info_all(&s.presence, ctx, &s.event.secret_room, k, &info);
        match next {
            SrWarState::Boss => {
                let room = s
                    .event
                    .secret_room
                    .sides
                    .get(&k)
                    .and_then(|sd| sd.tenders.get(idx))
                    .and_then(|t| t.room);
                for c in lineup {
                    let inside = online_obj(&s.presence, ctx, c).filter(|o| {
                        ctx.worlds
                            .core(*o)
                            .is_some_and(|core| is_room_world(core.world))
                    });
                    if let (Some(obj), Some(room)) = (inside, room) {
                        let _ = teleport_into(ctx, obj, room, true);
                    }
                }
            }
            SrWarState::Win => {
                let name = s
                    .social
                    .guild
                    .get(guild)
                    .map(|g| g.name.clone())
                    .unwrap_or_default();
                let line = text_with(ctx, TID_GAME_SECRETROOM_WINNER, &[side_name(ctx, k), name]);
                caption(s, ctx, &line);
                let now = ctx.clock.game_ms;
                close_wait(s, ctx, k, now);
                return;
            }
            _ => {}
        }
    }
}

/// `SetFailGuild` (the master of `guild` died inside a room): every side in `War` fails the guild.
pub(super) fn fail_by_master(s: &mut Services, ctx: &mut GameCtx<'_>, guild: GuildId) {
    if s.event.secret_room.state != SrMngState::War {
        return;
    }
    let sides: Vec<u8> = s
        .event
        .secret_room
        .sides
        .iter()
        .filter(|(_, sd)| sd.state == SrSideState::War)
        .map(|(k, _)| *k)
        .collect();
    for k in sides {
        fail_guild(s, ctx, k, guild);
    }
}

/// `SetContFailGuild` (a second fail of the same guild is a no-op: FIX of the double count): the guild is `Failed` and
/// counted, the other members of the side hear `WarState`; each online line-up member of the failed guild inside a room
/// is sent to Madrigal at the revival point of the side, hears `WarState` and `{101, 0}`; the monsters of its room go.
pub(super) fn fail_guild(s: &mut Services, ctx: &mut GameCtx<'_>, k: u8, guild: GuildId) {
    let sr = &mut s.event.secret_room;
    let Some(sd) = sr.sides.get_mut(&k) else {
        return;
    };
    let Some(idx) = sd.index_of(guild) else {
        return;
    };
    if sd.tenders[idx].war == SrWarState::Failed {
        return;
    }
    sd.tenders[idx].war = SrWarState::Failed;
    sd.failed += 1;
    let (room, lineup) = (sd.tenders[idx].room, sd.tenders[idx].lineup.clone());
    let info = wire::war_state(k, idx, SrWarState::Failed);
    send_info_all(&s.presence, ctx, &s.event.secret_room, k, &info);
    for c in lineup {
        let Some(obj) = online_obj(&s.presence, ctx, c) else {
            continue;
        };
        let inside = ctx
            .worlds
            .core(obj)
            .is_some_and(|core| is_room_world(core.world));
        if inside {
            send_to_madrigal(ctx, obj, k);
        }
        wire::send_info(&s.presence, ctx, c, info.clone());
        wire::send_state(&s.presence, ctx, c, SrSideState::Close.code(), 0);
    }
    remove_monsters(ctx, room);
}

/// `REPLACE(WI_WORLD_MADRIGAL, GetRevivalPos(cont))`, layer 0.
pub(super) fn send_to_madrigal(ctx: &mut GameCtx<'_>, obj: ObjId, side: u8) {
    if let Err(e) = ctx.worlds.replace(
        obj,
        WorldId(WI_WORLD_MADRIGAL),
        Continents::revival_pos(side),
        LayerId(0),
        ReplaceKind::Normal,
    ) {
        tracing::debug!(player = obj.0, error = %e, "secret room: the replace to Madrigal was refused");
    }
}

// ---- close-wait, close ------------------------------------------------------------------------------------------------------

/// `SetContCloseWait`: the side waits `CloseWaitTime` (`{104, remain}` to the non-failed members, `failed` back to 0);
/// the monsters of every room go; the winner's side is handed to the tax service (`set_next_guild`, memory, now) and the
/// winner's master gets the rate window; every tender becomes a pending result row (`W` the winner, else `L`).
fn close_wait(s: &mut Services, ctx: &mut GameCtx<'_>, k: u8, now: GameMs) {
    let sr = &mut s.event.secret_room;
    let wait = sr.cfg.close_wait_time_ms;
    let Some(sd) = sr.sides.get_mut(&k) else {
        return;
    };
    sd.state = SrSideState::CloseWait;
    sd.deadline = Some(deadline_in(now, wait));
    sd.failed = 0;
    let rooms: Vec<(Option<WorldId>, SrWarState, GuildId, SecretRoomTenderRecord)> = sd
        .tenders
        .iter()
        .map(|t| {
            let state = if t.war == SrWarState::Win {
                SecretRoomTenderState::Won
            } else {
                SecretRoomTenderState::Lost
            };
            (t.room, t.war, t.guild, t.record(k, state, t.round))
        })
        .collect();
    send_state_all(
        &s.presence,
        ctx,
        &s.event.secret_room,
        Some(k),
        Some((SrSideState::CloseWait.code(), wait)),
        now,
    );
    for (room, war, guild, row) in rooms {
        remove_monsters(ctx, room);
        if war == SrWarState::Win {
            let _ = s.event.tax.set_next_guild(k, Some(guild));
            if !s.event.secret_room.pending_tax_sides.contains(&k) {
                s.event.secret_room.pending_tax_sides.push(k);
            }
            crate::event::tax::open_rate_window(s, ctx, k, guild);
        }
        s.event.secret_room.pending_result.push(row);
    }
}

/// `SetContClose`: the side is `Close`, the non-failed members hear `{101, 0}` and those inside a room are sent to
/// Madrigal at the revival point of the side; the tenders are cleared.
fn close_side(s: &mut Services, ctx: &mut GameCtx<'_>, k: u8, now: GameMs) {
    let Some(sd) = s.event.secret_room.sides.get_mut(&k) else {
        return;
    };
    sd.state = SrSideState::Close;
    sd.deadline = None;
    send_state_all(
        &s.presence,
        ctx,
        &s.event.secret_room,
        Some(k),
        Some((SrSideState::Close.code(), 0)),
        now,
    );
    let members: Vec<CharacterId> = s
        .event
        .secret_room
        .sides
        .get(&k)
        .map(|sd| {
            sd.tenders
                .iter()
                .filter(|t| t.war != SrWarState::Failed)
                .flat_map(|t| t.lineup.iter().copied())
                .collect()
        })
        .unwrap_or_default();
    for c in members {
        if let Some(obj) = online_obj(&s.presence, ctx, c)
            && ctx
                .worlds
                .core(obj)
                .is_some_and(|core| is_room_world(core.world))
        {
            send_to_madrigal(ctx, obj, k);
        }
    }
    if let Some(sd) = s.event.secret_room.sides.get_mut(&k) {
        sd.tenders.clear();
        sd.failed = 0;
    }
}

/// The manager closes (all sides closed): `Close`, `ALLCLOSE`, the round moves on, and ONE `secret_room_result` batch
/// carries every `W` / `L` row, the tax rows of the sides whose winner went to the tax service (read now) and
/// `RaiseCounter(secret_room_round, round + 1)`.
fn close_manager(s: &mut Services, ctx: &mut GameCtx<'_>, now: GameMs) {
    let sr = &mut s.event.secret_room;
    sr.state = SrMngState::Close;
    sr.deadline = None;
    send_state_all(&s.presence, ctx, &s.event.secret_room, None, None, now);
    let line = ctx.data.texts.get(TID_GAME_SECRETROOM_ALLCLOSE).to_owned();
    caption(s, ctx, &line);

    let sr = &mut s.event.secret_room;
    let rows = std::mem::take(&mut sr.pending_result);
    let tax_sides = std::mem::take(&mut sr.pending_tax_sides);
    let round = sr.round;
    sr.round = round.saturating_add(1);
    let next_round = sr.round;
    let mut owners = Vec::new();
    let mut details = Vec::new();
    for k in tax_sides {
        let (owner, rows) = s.event.tax.rows_of(k);
        owners.push(owner);
        details.extend(rows);
    }
    let winners: Vec<_> = rows
        .iter()
        .filter(|r| r.state == SecretRoomTenderState::Won)
        .map(|r| (r.continent, r.guild_id))
        .collect();
    let total = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    let lost = u32::try_from(
        rows.iter()
            .filter(|r| r.state == SecretRoomTenderState::Lost)
            .count(),
    )
    .unwrap_or(u32::MAX);
    let batch =
        AtomicBatch::secret_room_result(rows, Vec::new(), owners, details, Some(next_round));
    submit_result(ctx, "SecretRoomClose", batch, Vec::new());
    ctx.audit.emit(audit::secret_room_result(
        round, "close", total, lost, 0, &winners,
    ));
}
