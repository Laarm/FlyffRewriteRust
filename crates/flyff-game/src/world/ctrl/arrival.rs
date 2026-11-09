//! The player reaches a ctrl (`CUser::OnMsgArrival` for an `OT_CTRL`, Plan/15 §14.3;
//! `src:WORLDSERVER/User.cpp:7170-7357`) and the timed-opening cancel (`OnCtrlCoolTimeCancel`,
//! `src:WORLDSERVER/DPSrvr.cpp:748-764`); WP-WORLD-18.
//!
//! The checks run in the legacy order and each refusal ends the call with `ClearTarget` (the client drops the
//! focus):
//!
//! ```text
//! UA_PLAYER_ID  not the exp box's player: TID_GAME_NOT_EXPBOX | already opened | timer running: silent
//!               else arm both timers (10 s) and send ExpBoxCoolTime      (state changes before the later checks)
//! UA_ITEM       no key (and dwSetEndu == 0): TID_ITEMBOX_MESSAGE1 "<item name>"
//! UA_LEVEL      TID_ITEMBOX_MESSAGE2        UA_QUEST   TID_ITEMBOX_MESSAGE4 "<title>"
//! UA_CLASS      TID_ITEMBOX_MESSAGE5        UA_GENDER  TID_ITEMBOX_MESSAGE3
//! -- the trigger is recorded (m_nMoverID) --
//! UA_TELEPORT   REPLACE_NORMAL to the ctrl's world and position, or TID_PK_NO_FLIGHTAREA when flying
//! UA_QUEST_END  advances up to two quests
//! UA_ITEM       consumes dwSetItemCount keys over up to 10 stacks
//! SetActionPlay CommonPlace{ACTION} to the viewers, the clock plays on        then ClearTarget
//! ```

use super::{COMMONPLACE_ACTION, OPEN_COOLTIME_MS, UA_CLASS, UA_GENDER, UA_ITEM, UA_LEVEL};
use super::{UA_PLAYER_ID, UA_QUEST, UA_QUEST_END, UA_TELEPORT};
use crate::ctx::{GameCtx, PlayerRef};
use crate::item::env::ItemCore;
use crate::item::fx::Fx;
use crate::item::ops::{RemoveReason, core as ops};
use crate::movement::MoveConsumers;
use crate::mover::Mover;
use crate::world::Worlds;
use crate::world::phases::FrameCtx;
use crate::world::replace::ReplaceKind;
use flyff_common::defs::text::{
    TID_GAME_NOT_EXPBOX, TID_ITEMBOX_MESSAGE1, TID_ITEMBOX_MESSAGE2, TID_ITEMBOX_MESSAGE3,
    TID_ITEMBOX_MESSAGE4, TID_ITEMBOX_MESSAGE5, TID_PK_NO_FLIGHTAREA,
};
use flyff_common::math::Vec3;
use flyff_common::{GameMs, LayerId, ObjId, SimFrame, WorldId};
use flyff_db::AuditLog;
use flyff_proto::{EncodedMsg, pb};
use flyff_sim::step::SimEvent;
use std::sync::Arc;

/// The quest side of the open conditions (`UA_QUEST`, `UA_QUEST_END`). The quest engine (doc 20) replaces the
/// default, which knows only the persisted quest document.
pub trait CtrlQuestPort: Send {
    /// `GetQuest(id)->m_nState`: the state of the active quest `quest`, `None` when the player does not have it.
    fn state(&self, player: &Mover, quest: u32) -> Option<u32> {
        let book = &player.player()?.quests.doc;
        book.active
            .iter()
            .find(|q| u32::from(q.quest_id) == quest)
            .map(|q| u32::from(q.state))
    }

    /// `QuestProp.m_szTitle` for the `TID_ITEMBOX_MESSAGE4` argument; `None` = no such quest property (the
    /// original then sends no text).
    fn title(&self, _quest: u32) -> Option<String> {
        None
    }

    /// `SetQuest(id, state)` + `AddSetQuest` of an active quest (`UA_QUEST_END`): the default does nothing
    /// (doc 20 owns the quest transitions, [`crate::quest::track::QuestCtrl`]); `now` is the game clock, which
    /// the quest book needs for a restarted minute timer.
    fn set_state(
        &mut self,
        _player: &mut Mover,
        _quest: u32,
        _state: u32,
        _now: GameMs,
        _fx: &mut Fx,
    ) {
    }
}

/// The default [`CtrlQuestPort`]: the persisted quest document, no titles, no transitions.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCtrlQuests;

impl CtrlQuestPort for NoCtrlQuests {}

/// The `MoveConsumers` link of the ctrl arrival: an arrival at a ctrl runs the open flow, every other call goes
/// to the chained consumer (the ground item pickup).
pub struct CtrlArrival {
    core: Arc<ItemCore>,
    audit: AuditLog,
    quests: Box<dyn CtrlQuestPort>,
    inner: Option<Box<dyn MoveConsumers>>,
}

impl std::fmt::Debug for CtrlArrival {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CtrlArrival")
            .field("inner", &self.inner.is_some())
            .finish()
    }
}

impl CtrlArrival {
    /// The ctrl arrival over the shared item environment (the key consumption) and the audit writer.
    pub fn new(core: Arc<ItemCore>, audit: AuditLog) -> CtrlArrival {
        CtrlArrival {
            core,
            audit,
            quests: Box::new(NoCtrlQuests),
            inner: None,
        }
    }

    /// Chains `inner` behind it: it receives every call this consumer does not handle itself.
    pub fn with_inner(mut self, inner: Box<dyn MoveConsumers>) -> CtrlArrival {
        self.inner = Some(inner);
        self
    }

    /// Installs doc 20's quest seam.
    pub fn with_quests(mut self, quests: Box<dyn CtrlQuestPort>) -> CtrlArrival {
        self.quests = quests;
        self
    }
}

impl MoveConsumers for CtrlArrival {
    fn player_arrived_at_obj(&mut self, ctx: &mut FrameCtx<'_>, player: ObjId, target: ObjId) {
        if ctx.store.ctrl(target).is_some() {
            on_player_arrival(
                &self.core,
                &self.audit,
                &mut *self.quests,
                ctx,
                player,
                target,
            );
        } else if let Some(i) = self.inner.as_mut() {
            i.player_arrived_at_obj(ctx, player, target);
        }
    }

    fn player_skill_arrival(&mut self, ctx: &mut FrameCtx<'_>, player: ObjId, target: ObjId) {
        if let Some(i) = self.inner.as_mut() {
            i.player_skill_arrival(ctx, player, target);
        }
    }

    fn sim_event(&mut self, ctx: &mut FrameCtx<'_>, mover: ObjId, frame: SimFrame, ev: &SimEvent) {
        if let Some(i) = self.inner.as_mut() {
            i.sim_event(ctx, mover, frame, ev);
        }
    }

    fn frame_end(&mut self, ctx: &mut FrameCtx<'_>, frame: SimFrame) {
        if let Some(i) = self.inner.as_mut() {
            i.frame_end(ctx, frame);
        }
    }

    fn tail(&mut self, ctx: &mut FrameCtx<'_>, mover: crate::world::object::MoverKey) {
        if let Some(i) = self.inner.as_mut() {
            i.tail(ctx, mover);
        }
    }
}

/// What an open attempt looked at on the player (read once, before anything changes).
struct PlayerFacts {
    character: flyff_common::CharacterId,
    level: i32,
    job: i32,
    sex: u8,
    flying: bool,
}

fn facts(m: &Mover) -> Option<PlayerFacts> {
    let p = m.player()?;
    Some(PlayerFacts {
        character: p.ident.character_id,
        level: m.mv.level,
        job: m.mv.job,
        sex: p.appearance.sex,
        flying: m.sim.act.flags.fly(),
    })
}

/// Sends `fx` to the player through the sink.
fn flush(ctx: &mut FrameCtx<'_>, audit: &AuditLog, player: ObjId, fx: Fx) {
    if let Some(m) = ctx.store.mover(player) {
        fx.flush_sink(&mut *ctx.sink, m, audit);
    }
}

/// `AddClearTarget()`: the refusal tail of every check.
fn refuse(ctx: &mut FrameCtx<'_>, audit: &AuditLog, player: ObjId, mut fx: Fx) {
    fx.out.to_self(pb::ClearTarget {});
    flush(ctx, audit, player, fx);
}

/// `CUser::OnMsgArrival` for an `OT_CTRL` target (see the module docs for the order).
pub fn on_player_arrival(
    core: &ItemCore,
    audit: &AuditLog,
    quests: &mut dyn CtrlQuestPort,
    ctx: &mut FrameCtx<'_>,
    player: ObjId,
    target: ObjId,
) {
    let Some(ctrl) = ctx.store.ctrl(target).filter(|c| !c.core.deleted) else {
        return;
    };
    let elem = ctrl.elem.clone();
    let Some(me) = ctx.store.mover(player).and_then(facts) else {
        return;
    };
    let mut fx = Fx::new();

    // ---- the exp box's own player and its cool time ----
    if elem.set & UA_PLAYER_ID != 0 {
        let Some(c) = ctx.store.ctrl(target) else {
            return;
        };
        if c.exp_player != Some(me.character) {
            fx.text(TID_GAME_NOT_EXPBOX);
            return refuse(ctx, audit, player, fx);
        }
        if c.action_done || c.ready_at.is_some() {
            return refuse(ctx, audit, player, fx);
        }
        let until = GameMs(ctx.now.0.saturating_add(OPEN_COOLTIME_MS));
        if let Some(c) = ctx.store.ctrl_mut(target) {
            c.ready_at = Some(until);
        }
        if let Some(p) = ctx.store.mover_mut(player).and_then(|m| m.player_mut()) {
            p.ctrl_ready = Some((target, until));
        }
        fx.out.to_self(pb::ExpBoxCoolTime {
            ctrl_id: target.0,
            time_ms: OPEN_COOLTIME_MS as u32, // BOUNDS: 10 000
        });
    }

    // ---- the key ----
    if elem.set & UA_ITEM != 0 {
        let blocked = ctx
            .store
            .mover(player)
            .and_then(|m| m.player())
            .is_some_and(|p| {
                let inv = &p.items.inventory;
                match inv.find(elem.set_item) {
                    Some((_, key)) if key.is_usable() => {
                        // `nItemCount < (int)dwSetItemCount && dwSetEndu == 0`
                        i64::from(inv.count_of(elem.set_item))
                            < i64::from(elem.set_item_count as i32)
                            && elem.set_endu == 0
                    }
                    _ => elem.set_endu == 0,
                }
            });
        if blocked {
            let name = core
                .data
                .item(elem.set_item)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            fx.defined(TID_ITEMBOX_MESSAGE1, vec![format!("\"{name}\"")]);
            return refuse(ctx, audit, player, fx);
        }
    }

    // ---- level, quest, class, gender ----
    if elem.set & UA_LEVEL != 0 && i64::from(me.level) < i64::from(elem.set_level as i32) {
        fx.text(TID_ITEMBOX_MESSAGE2);
        return refuse(ctx, audit, player, fx);
    }
    if elem.set & UA_QUEST != 0 {
        let state = ctx
            .store
            .mover(player)
            .and_then(|m| quests.state(m, elem.set_quest_num));
        if state != Some(elem.set_flag_num) {
            if let Some(title) = quests.title(elem.set_quest_num) {
                fx.defined(TID_ITEMBOX_MESSAGE4, vec![format!("\"{title}\"")]);
            }
            return refuse(ctx, audit, player, fx);
        }
    }
    if elem.set & UA_CLASS != 0 {
        // `GetJob() != m_bSetJob[GetJob()]`: the flag at the player's own job index holds that job
        let ok = usize::try_from(me.job)
            .ok()
            .and_then(|j| elem.set_job.get(j))
            .is_some_and(|&v| i64::from(v) == i64::from(me.job));
        if !ok {
            fx.text(TID_ITEMBOX_MESSAGE5);
            return refuse(ctx, audit, player, fx);
        }
    }
    if elem.set & UA_GENDER != 0 && u32::from(me.sex) != elem.set_gender {
        fx.text(TID_ITEMBOX_MESSAGE3);
        return refuse(ctx, audit, player, fx);
    }

    // ---- accepted: the trigger is recorded ----
    if let Some(c) = ctx.store.ctrl_mut(target) {
        c.trigger_mover = Some(player);
    }

    if elem.set & UA_TELEPORT != 0 {
        if me.flying {
            fx.text(TID_PK_NO_FLIGHTAREA);
        } else {
            // `(float)dwTeleX`: the number, not the bits; `nTempLayer` is the default layer
            let pos = Vec3::new(elem.tele_x as f32, elem.tele_y as f32, elem.tele_z as f32);
            ctx.replace(
                player,
                WorldId(elem.tele_world_id),
                LayerId(0),
                pos,
                ReplaceKind::Normal,
            );
        }
    }
    if elem.set & UA_QUEST_END != 0 {
        for (num, flag) in [
            (elem.set_quest_num1, elem.set_flag_num1),
            (elem.set_quest_num2, elem.set_flag_num2),
        ] {
            if num == 0 {
                continue;
            }
            let has = ctx
                .store
                .mover(player)
                .is_some_and(|m| quests.state(m, num).is_some());
            if has && let Some(m) = ctx.store.mover_mut(player) {
                quests.set_state(m, num, flag, ctx.now, &mut fx);
            }
        }
    }

    // ---- the keys are consumed ----
    if elem.set & UA_ITEM != 0 {
        consume_keys(
            core,
            ctx,
            player,
            elem.set_item,
            elem.set_item_count,
            &mut fx,
        );
    }

    // ---- SetActionPlay: the texts and item updates go first, then the effect, then ClearTarget ----
    flush(ctx, audit, player, fx);
    play_action(ctx, target);
    refuse(ctx, audit, player, Fx::new());
}

/// The key loop of `OnMsgArrival`: up to 10 rounds, each takes from the first stack of the key item.
fn consume_keys(
    core: &ItemCore,
    ctx: &mut FrameCtx<'_>,
    player: ObjId,
    item: u32,
    count: u32,
    fx: &mut Fx,
) {
    let env = core.env(flyff_common::UnixTime(0));
    let mut remaining = i64::from(count as i32); // `int nItemCount = dwSetItemCount`
    for _ in 0..10 {
        if remaining <= 0 {
            break;
        }
        let Some(m) = ctx.store.mover_mut(player) else {
            return;
        };
        let Some((id, have, usable)) = m
            .player()
            .and_then(|p| p.items.inventory.find(item))
            .map(|(id, it)| (id, i64::from(it.count), it.is_usable()))
        else {
            // `GetAtItemId` found nothing: the loop spins without effect
            continue;
        };
        if !usable {
            continue;
        }
        let take = remaining.min(have);
        ops::remove_item(
            m,
            &env,
            id,
            take as u16, // BOUNDS: at most the stack's u16 count
            RemoveReason::Other("ctrl_key"),
            fx,
        );
        remaining -= take;
    }
}

/// `CCommonCtrl::SetActionPlay` on the server: `CommonPlace{ACTION}` to the viewers and the clock plays on.
fn play_action(ctx: &mut FrameCtx<'_>, target: ObjId) {
    let Some(c) = ctx.store.ctrl_mut(target) else {
        return;
    };
    c.set_action_play();
    let viewers: Vec<ObjId> = c.core.viewers.iter().copied().collect();
    let msg = EncodedMsg::new(
        target.0,
        pb::CommonPlace {
            kind: COMMONPLACE_ACTION,
        },
    );
    for v in viewers {
        ctx.sink.relay(v, &msg);
    }
}

/// `OnCtrlCoolTimeCancel`: the player aborts a timed opening ([`Worlds::cancel_ctrl_cool_time`]).
pub fn ctrl_cool_time_cancel(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    ctx.worlds.cancel_ctrl_cool_time(who.0);
}

impl Worlds {
    /// `OnCtrlCoolTimeCancel`: the player's timer clears, then the ctrl it was opening (when it still exists)
    /// forgets its timer and its "done" flag. No message goes back to the client.
    pub fn cancel_ctrl_cool_time(&mut self, player: ObjId) {
        let Some(ready) = self
            .player_mut(player)
            .and_then(|m| m.player_mut())
            .and_then(|p| p.ctrl_ready.take())
        else {
            return;
        };
        if let Some(c) = self.store.ctrl_mut(ready.0).filter(|c| !c.core.deleted) {
            c.ready_at = None;
            c.action_done = false;
        }
    }
}
