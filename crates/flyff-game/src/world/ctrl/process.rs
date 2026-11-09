//! `CCommonCtrl::Process` (Plan/15 §14.5, §14.4; `src:_Common/CommonCtrl.cpp:311-458, 462-640, 690-830`;
//! WP-WORLD-18): one call per ctrl per tick, after the movers and the ground items of the layer.
//!
//! ```text
//! 1. psychic wall: doc 17 (WorldHooks::process_ctrl); no CtrlProp: nothing
//! 2. delete_at passed -> delete          open timer passed -> the exp box completes (EXP award: DROP)
//! 3. four sub-frames (sim frames 4n .. 4n+3):
//!      attribute HIT at the clock's frame (exact equality, o3d rule) -> _ProcessAction
//!      end frame && trapped -> alpha
//!      CtrlClock::step (FrameMove, frozen frame, CK2_KEEP re-arm, CK3_HALF pause)
//!      CK2_FADE && alpha: blend -= 5, below 0 -> delete
//!      _ProcessTrap
//! ```
//!
//! [`step_ctrl`] is the pure per-ctrl state machine (everything it wants done outside the ctrl goes into a
//! [`CtrlFx`]); `Worlds::process_ctrl` feeds it and applies the effects: `CommonPlace` to the viewers, ground
//! items, monsters, trap skills (doc 17 through [`CtrlHooks`](super::CtrlHooks)), the linked ctrl, the delete,
//! and the re-pose of the ctrl's collider.
//!
//! Draw order of one hit (pinned by the tests): `DropItem`: `total`, then per slot the `3e9` roll and, when the
//! slot hits, the diagonal offset and the angle; `DropNPC`: per monster the offset.

use super::{
    CK2_FADE, COMMONPLACE_ACTION, COMMONPLACE_ALPHA, Ctrl, CtrlSpecial, FADE_STEP,
    MAX_CTRLDROPITEM, MAX_CTRLDROPMOB, MAX_TRAP, TOT_NOENDU, TOT_RANDOM,
};
use crate::ai::MonsterSpawn;
use crate::item::GroundItemMeta;
use crate::item::drop::roll_3e9;
use crate::world::Worlds;
use crate::world::object::{CtrlKey, ObjRef};
use crate::world::phases::PhaseEnv;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{CharacterId, GameMs, LayerId, ObjId, WorldId};
use flyff_data::GameData;
use flyff_data::types::NULL_ID;
use flyff_db::ItemRecord;
use flyff_proto::{EncodedMsg, pb};
use flyff_sim::anim::is_attr_o3d;
use flyff_sim::anim::motion_table::MotionAttr;
use flyff_sim::types::MA_HIT;
use std::sync::Arc;

/// An item a chest drops: where, facing, what, and its loot data.
#[derive(Clone, Debug, PartialEq)]
pub struct DroppedItem {
    /// Position (the ctrl's plus the diagonal offset; the spawn lifts it to the land).
    pub pos: Vec3,
    /// Yaw `0..360`.
    pub angle: f32,
    /// The item (serial 0: numbered by the item job before the next save).
    pub item: ItemRecord,
    /// Owner = the mover that triggered the ctrl, drop time = now.
    pub meta: GroundItemMeta,
}

/// What one [`step_ctrl`] asks the world to do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CtrlFx {
    /// `CommonPlace` kinds to broadcast to the viewers, in order.
    pub broadcasts: Vec<u32>,
    /// Ground items to create.
    pub items: Vec<DroppedItem>,
    /// Monsters to create.
    pub mobs: Vec<MonsterSpawn>,
    /// `(skill, level)` of the trap slots that fire.
    pub trap_skills: Vec<(u32, u32)>,
    /// The key of the ctrl a trap fires (`link_ctrl_key`).
    pub link: Option<String>,
    /// Delete the ctrl.
    pub delete: bool,
    /// `CK2_KEEP` re-armed the ctrl.
    pub rearmed: bool,
}

/// What [`step_ctrl`] reads besides the ctrl.
pub struct StepIn<'a> {
    /// The game clock of the tick.
    pub now: GameMs,
    /// The first sim frame of the tick (`4n`).
    pub first_frame: u64,
    /// The world of the ctrl.
    pub world: WorldId,
    /// The layer of the ctrl.
    pub layer: LayerId,
    /// `CtrlProp.ctrl_kind2`.
    pub kind2: u32,
    /// The hit/sound attributes of the ctrl's model.
    pub attrs: &'a [MotionAttr],
    /// Items and monsters.
    pub data: &'a GameData,
    /// `GetLandHeight(ctrl position)`.
    pub land_height: f32,
}

/// `_ProcessAction`: arms the trap timer and drops the chest's contents (once while untrapped).
fn process_action(c: &mut Ctrl, inp: &StepIn<'_>, rng: &mut dyn GameRng, fx: &mut CtrlFx) {
    if c.trapped {
        return;
    }
    c.trap_at = Some(GameMs(
        inp.now.0.saturating_add(u64::from(c.elem.trap_delay)),
    ));
    // KEEP-QUIRK: a chest with min = max drops nothing
    if c.elem.min_item_num > 0 && c.elem.max_item_num > c.elem.min_item_num {
        drop_items(c, inp, rng, fx);
    }
    drop_npcs(c, inp, rng, fx);
}

/// `DropItem` (15 §14.4): up to `total` of the four slots, each with its own `3e9` chance, scattered along the
/// diagonal within 1.5 m (the same offset on x and z). An empty slot ends the pass (KEEP; the original returned
/// without clearing `max_item_num` then). The chest is one-shot afterwards.
fn drop_items(c: &mut Ctrl, inp: &StepIn<'_>, rng: &mut dyn GameRng, fx: &mut CtrlFx) {
    let (min, max) = (c.elem.min_item_num, c.elem.max_item_num);
    let mut total = i64::from(rng.random_below(max.wrapping_sub(min))) + i64::from(min) + 1;
    for i in 0..MAX_CTRLDROPITEM {
        let kind = c.elem.inside_item_kind[i];
        let Some(prop) = inp.data.item(kind) else {
            return;
        };
        let roll = roll_3e9(rng);
        if c.elem.inside_item_per[i] >= roll && total > 0 {
            let mut item = ItemRecord::new(0, kind, 1, 0);
            item.endurance = prop.endurance as i32; // BOUNDS: a small durability
            total -= 1;
            let d = (rng.random_below(30) as f32 - 15.0) * 0.1;
            let angle = rng.random_below(360) as f32;
            fx.items.push(DroppedItem {
                pos: Vec3::new(c.core.pos.x + d, c.core.pos.y, c.core.pos.z + d),
                angle,
                item,
                meta: GroundItemMeta {
                    owner: c.trigger_mover,
                    holder: None,
                    drop_time: inp.now,
                    from_monster: false,
                    event_monster: None,
                    gold: None,
                    respawned: false,
                },
            });
        }
    }
    c.elem.max_item_num = 0;
}

/// `DropNPC` (15 §14.4): per slot `mon_res_num - mon_act_attack` passive and then `mon_act_attack` aggressive
/// monsters within 1.5 m on the diagonal, standing on the land. An empty slot ends the pass.
fn drop_npcs(c: &mut Ctrl, inp: &StepIn<'_>, rng: &mut dyn GameRng, fx: &mut CtrlFx) {
    for i in 0..MAX_CTRLDROPMOB {
        let kind = c.elem.mon_res_kind[i];
        if inp.data.mover(kind).is_none() {
            return;
        }
        let num = c.elem.mon_res_num[i];
        let attack = c.elem.mon_act_attack[i];
        // `(int)(num - attack)` of two DWORDs: a count above `num` wraps negative and creates nothing
        let base = num.wrapping_sub(attack) as i32;
        let mut spawn = |active: bool, rng: &mut dyn GameRng| {
            let d = (rng.random_below(30) as f32 - 15.0) * 0.1;
            fx.mobs.push(MonsterSpawn {
                mover: kind,
                world: inp.world,
                layer: inp.layer,
                pos: Vec3::new(c.core.pos.x + d, inp.land_height + 0.01, c.core.pos.z + d),
                active_attack: active,
                gold: None,
                owner_ai: None,
            });
        };
        for _ in 0..base.max(0) {
            spawn(false, rng);
        }
        for _ in 0..attack {
            spawn(true, rng);
        }
    }
}

/// `_ProcessTrap`: after `trap_delay` the trap fires once (15 §14.5).
fn process_trap(c: &mut Ctrl, inp: &StepIn<'_>, rng: &mut dyn GameRng, fx: &mut CtrlFx) {
    let Some(t) = c.trap_at else { return };
    if t.0 >= inp.now.0 {
        return;
    }
    if inp.kind2 == CK2_FADE {
        fx.broadcasts.push(COMMONPLACE_ALPHA);
    }
    c.trapped = true;
    c.trap_at = None;
    let fire = match c.elem.trap_oper_type {
        TOT_RANDOM => c.elem.trap_random_per >= roll_3e9(rng),
        TOT_NOENDU => c.elem.set_endu == 0,
        _ => false,
    };
    if fire {
        for i in 0..MAX_TRAP {
            let level = c.elem.trap_level[i];
            if level == 0 || level == NULL_ID {
                continue;
            }
            fx.trap_skills.push((c.elem.trap_kind[i], level));
        }
    }
    if !c.elem.link_ctrl_key.is_empty() {
        fx.link = Some(c.elem.link_ctrl_key.clone());
    }
}

/// The four sub-frame steps of one tick for one ctrl (the body of `CCommonCtrl::Process` after the timers).
pub fn step_ctrl(c: &mut Ctrl, inp: &StepIn<'_>, rng: &mut dyn GameRng, fx: &mut CtrlFx) {
    if c.delete_at.is_some_and(|t| inp.now.0 > t.0) {
        fx.delete = true;
        return;
    }
    for i in 0..4_u64 {
        // the attribute of the frame the clock holds (`IsAttrHit`: the exact-equality rule of an .o3d)
        if is_attr_o3d(inp.attrs, c.anim.frame(), MA_HIT).is_some() {
            process_action(c, inp, rng, fx);
        }
        if c.anim.clock.end_frame && c.trapped {
            c.alpha = true;
        }
        let s = c.anim.step(inp.first_frame + i);
        if s.rearmed {
            c.reinit();
            fx.rearmed = true;
        }
        if inp.kind2 == CK2_FADE && c.alpha {
            c.blend -= FADE_STEP;
            if c.blend < 0 {
                c.blend = 0;
                fx.delete = true;
                return;
            }
        }
        process_trap(c, inp, rng, fx);
    }
}

fn place_msg(ctrl: ObjId, kind: u32) -> EncodedMsg {
    EncodedMsg::new(ctrl.0, pb::CommonPlace { kind })
}

impl Worlds {
    /// `CCommonCtrl::SetActionPlay` of ctrl `id` (the open flow, a linked ctrl, a GM or script trigger):
    /// `CommonPlace{ACTION}` to its viewers, the clock plays on. `false` when there is no such ctrl.
    pub fn ctrl_action_play(&mut self, id: ObjId, sink: &mut dyn crate::world::SpawnSink) -> bool {
        let Some(c) = self.store.ctrl_mut(id) else {
            return false;
        };
        c.set_action_play();
        let viewers: Vec<ObjId> = c.core.viewers.iter().copied().collect();
        let msg = place_msg(id, COMMONPLACE_ACTION);
        for v in viewers {
            sink.relay(v, &msg);
        }
        true
    }

    /// `CCommonCtrl::Process` of the ctrl `key` in `(world, layer)`: the open timer, the sub-frame steps and the
    /// effects they asked for.
    pub(crate) fn process_ctrl(
        &mut self,
        world: WorldId,
        layer: LayerId,
        key: CtrlKey,
        statics: &Arc<crate::world::WorldStatic>,
        env: &mut PhaseEnv<'_>,
    ) {
        let data = Arc::clone(&self.movement.shared.data);
        let first_frame = env.tick.0.saturating_mul(4);
        let Some(c) = self.store.ctrls.get_mut(key) else {
            return;
        };
        if c.core.deleted || matches!(c.special, CtrlSpecial::PsychicWall(_)) {
            return;
        }
        // no CtrlProp: nothing runs (`if (pProp == NULL) return`)
        let Some(kind2) = data.ctrls.get(c.core.index).map(|p| p.ctrl_kind2) else {
            return;
        };
        let id = c.core.id;
        // the open timer (an exp box): the completion needs its player; the EXP award is dropped with the boxes
        let timer_done = c.ready_at.is_some_and(|t| env.now.0 > t.0);
        let exp_player = c.exp_player;
        let attrs = self
            .ctrl_models
            .get(c.core.index)
            .map(|m| m.attrs.as_slice())
            .unwrap_or(&[]);
        let land_height = statics.land_height(c.core.pos.x, c.core.pos.z);
        let inp = StepIn {
            now: env.now,
            first_frame,
            world,
            layer,
            kind2,
            attrs,
            data: &data,
            land_height,
        };
        let mut fx = CtrlFx::default();
        let rng = env.rng.world(world);
        step_ctrl(c, &inp, rng, &mut fx);
        let (frame, next) = (c.anim.frame(), c.anim.next_frame());
        if timer_done {
            self.finish_open_timer(id, exp_player, env.sink);
        }
        // re-pose the collider (the clock moved)
        if let Some(l) = self.layer_mut(world, layer)
            && l.dyn_colliders
                .get(id)
                .is_some_and(|d| d.frame != frame || d.next != next)
        {
            Arc::make_mut(&mut l.dyn_colliders).set_pose(id, frame, next);
        }
        self.apply_ctrl_fx(world, layer, id, fx, &data, env);
    }

    /// The open-timer completion (15 §14.3): when the exp box's player is online the timers clear on both sides,
    /// `ExpBoxCoolTimeCancel` goes to the player, the box is done and fades.
    fn finish_open_timer(
        &mut self,
        id: ObjId,
        exp_player: Option<CharacterId>,
        sink: &mut dyn crate::world::SpawnSink,
    ) {
        let Some(cid) = exp_player else { return };
        let player = self
            .store
            .movers
            .values()
            .find(|m| m.player().is_some_and(|p| p.ident.character_id == cid))
            .map(|m| m.core.id);
        let Some(pid) = player else { return };
        if let Some(p) = self.store.mover_mut(pid).and_then(|m| m.player_mut()) {
            p.ctrl_ready = None;
        }
        if let Some(c) = self.store.ctrl_mut(id) {
            c.ready_at = None;
            c.action_done = true;
            c.alpha = true;
        }
        sink.relay(pid, &EncodedMsg::new(0, pb::ExpBoxCoolTimeCancel {}));
    }

    fn apply_ctrl_fx(
        &mut self,
        world: WorldId,
        layer: LayerId,
        id: ObjId,
        fx: CtrlFx,
        data: &Arc<GameData>,
        env: &mut PhaseEnv<'_>,
    ) {
        let CtrlFx {
            broadcasts,
            items,
            mobs,
            trap_skills,
            link,
            delete,
            ..
        } = fx;
        if !broadcasts.is_empty() {
            let viewers: Vec<ObjId> = self
                .store
                .ctrl(id)
                .map(|c| c.core.viewers.iter().copied().collect())
                .unwrap_or_default();
            for kind in broadcasts {
                let msg = place_msg(id, kind);
                for v in &viewers {
                    env.sink.relay(*v, &msg);
                }
            }
        }
        for it in items {
            if let Err(e) = self.spawn_ground_item(world, layer, it.pos, it.angle, it.item, it.meta)
            {
                tracing::warn!(ctrl = id.0, ?e, "a chest drop was refused");
            }
        }
        for m in mobs {
            crate::ai::spawn::commit(self, data, env.rng.world(world), &m);
        }
        if !trap_skills.is_empty() {
            let trigger = self.store.ctrl(id).and_then(|c| c.trigger_mover);
            if let Some(mut hooks) = self.ctrl_hooks.0.take() {
                for (skill, level) in trap_skills {
                    hooks.trap_skill(self, id, trigger, skill, level);
                }
                self.ctrl_hooks.0 = Some(hooks);
            }
        }
        if let Some(key) = link {
            self.fire_linked_ctrl(world, layer, &key, env.sink);
        }
        if delete {
            self.delete(id);
        }
    }

    /// The tail of `_ProcessTrap`: the ctrl `key` names (in this layer: PL-151) plays unless it already
    /// moved off frame 0.
    fn fire_linked_ctrl(
        &mut self,
        world: WorldId,
        layer: LayerId,
        key: &str,
        sink: &mut dyn crate::world::SpawnSink,
    ) {
        let Some(target) = self.ctrl_by_key(world, layer, key) else {
            return;
        };
        let ready = self.store.ctrl(target).is_some_and(|c| {
            !c.core.deleted && c.anim.frame() == 0.0 // `m_fFrameCurrent != 0.0f` returns
        });
        if ready {
            self.ctrl_action_play(target, sink);
        }
    }

    /// Whether the layer's ctrl lookup finds a live ctrl for `key` (tests, diagnostics).
    pub fn live_ctrl_for_key(&self, world: WorldId, layer: LayerId, key: &str) -> Option<ObjId> {
        let id = self.ctrl_by_key(world, layer, key)?;
        match self.store.lookup(id) {
            Some(ObjRef::Ctrl(k)) if self.store.ctrls.get(k).is_some_and(|c| !c.core.deleted) => {
                Some(id)
            }
            _ => None,
        }
    }
}
