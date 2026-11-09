//! The action state machine: state-word operations with their end handlers, the action message table,
//! the per-frame state processing and the forced-set family (Plan/31 §5-§7).

pub mod damage;
pub mod force;
pub mod msg_fly;
pub mod msg_ground;
pub mod pattern;
pub mod state_fly;
pub mod state_ground;

use crate::mover::{QueuedAct, SimMover};
use crate::step::{Cx, SimEvent};
use crate::types::*;

pub use damage::{do_damage_fly, set_damaged_motion};
pub use force::{action_force_set, action_force_set2, half_force_set};
pub use msg_ground::send_act_msg_cx;

impl SimMover {
    /// `ResetState(mask)` (`src:_AIInterface/Action.cpp:147-207`): for each group present in `mask` it
    /// clears **all bits of `mask`** (not just that group's) and runs the group's end handler:
    /// MOVE (before the clear): an old `SIT` removes the SIT flag; JUMP and ATK (after the clear, with the
    /// state word as it was on entry): an old `SJUMP4` runs `on_act_end_jump4`, old combo/wand/range attack
    /// states drain the attack queues, old melee/magic skills raise their end events.
    pub fn reset_state(&mut self, cx: &mut Cx<'_>, mask: u32) {
        let dw = self.act.state.bits();
        if mask & OBJSTA_MOVE_ALL != 0 {
            if self.act.state.move_() == OBJSTA_SIT {
                self.act.flags.remove(OBJSTAF_SIT);
            }
            self.act.state.0 &= !mask;
        }
        if mask & OBJSTA_TURN_ALL != 0 {
            self.act.state.0 &= !mask;
        }
        if mask & OBJSTA_LOOK_ALL != 0 {
            self.act.state.0 &= !mask;
        }
        if mask & OBJSTA_JUMP_ALL != 0 {
            self.act.state.0 &= !mask;
            if dw & OBJSTA_JUMP_ALL == OBJSTA_SJUMP4 {
                self.on_act_end_jump4(cx);
            }
        }
        if mask & OBJSTA_ATK_ALL != 0 {
            self.act.state.0 &= !mask;
            self.on_end_attack_state(cx, dw);
        }
        if mask & OBJSTA_DMG_ALL != 0 {
            self.act.state.0 &= !mask;
        }
        if mask & OBJSTA_ACTION_ALL != 0 {
            self.act.state.0 &= !mask;
        }
    }

    /// `ClearState()` = `ResetState(OBJSTA_ALL)`; any bit an end handler set meanwhile is cleared too
    /// (the original logs an error and zeroes the word).
    pub fn clear_state(&mut self, cx: &mut Cx<'_>) {
        self.reset_state(cx, OBJSTA_ALL);
        self.act.state.0 = 0;
    }

    /// `SetState(group_mask, value)`: `ResetState(mask); state |= value`.
    pub fn set_state(&mut self, cx: &mut Cx<'_>, group_mask: u32, value: u32) {
        self.reset_state(cx, group_mask);
        self.act.state.0 |= value;
    }

    /// `SetMoveState`.
    pub fn set_move_state(&mut self, cx: &mut Cx<'_>, v: u32) {
        self.set_state(cx, OBJSTA_MOVE_ALL, v);
    }

    /// `SetTurnState`.
    pub fn set_turn_state(&mut self, cx: &mut Cx<'_>, v: u32) {
        self.set_state(cx, OBJSTA_TURN_ALL, v);
    }

    /// `SetLookState`.
    pub fn set_look_state(&mut self, cx: &mut Cx<'_>, v: u32) {
        self.set_state(cx, OBJSTA_LOOK_ALL, v);
    }

    /// `SetJumpState`.
    pub fn set_jump_state(&mut self, cx: &mut Cx<'_>, v: u32) {
        self.set_state(cx, OBJSTA_JUMP_ALL, v);
    }

    /// `SetActionState`.
    pub fn set_action_state(&mut self, cx: &mut Cx<'_>, v: u32) {
        self.set_state(cx, OBJSTA_ACTION_ALL, v);
    }

    /// `CMover::OnActEndJump4()`: the end of the landing animation re-records the destination signs
    /// (`SetDestPos(GetDestPos(), m_bForward, FALSE)`) and lets the ClockWorks bosses reappear.
    pub fn on_act_end_jump4(&mut self, cx: &mut Cx<'_>) {
        if !self.is_empty_dest_pos() {
            let (pos, fwd) = (self.dest.pos, self.dest.forward);
            self.set_dest_pos(pos, fwd);
        }
        if self.params.mover_index == defs::MI_CLOCKWORK1
            || self.params.mover_index == defs::MI_LOADCLOCKWORK
        {
            send_act_msg_cx(self, cx, ActMsg::new(OBJMSG_APPEAR));
        }
    }

    /// `CAction::OnEndAttackState(old_state)` (Plan/31 §6.6): the end of a combo/wand/range attack drains
    /// the queued attack messages (server: not for flying players; client: not for the active mover); the
    /// end of a melee/magic skill raises its end event.
    fn on_end_attack_state(&mut self, cx: &mut Cx<'_>, old: u32) {
        match old & OBJSTA_ATK_ALL {
            OBJSTA_ATK1 | OBJSTA_ATK2 | OBJSTA_ATK3 | OBJSTA_ATK4 | OBJSTA_ATK_MAGIC1
            | OBJSTA_ATK_RANGE1 | OBJSTA_RANGE3 | OBJSTA_RANGE4 => {
                if cx.server() && self.is_player() && self.act.flags.fly() {
                    return;
                }
                if cx.client() && self.is_active {
                    return;
                }
                if self.act.queues.melee.is_empty() && self.act.queues.magic.is_empty() {
                    return;
                }
                if !self.drain_melee_queue(cx) && !self.act.queues.magic.is_empty() {
                    cx.emit(SimEvent::DrainMagicQueue);
                }
            }
            OBJSTA_ATK_MELEESKILL => cx.emit(SimEvent::MeleeSkillEnded),
            OBJSTA_ATK_MAGICSKILL => cx.emit(SimEvent::MagicSkillEnded),
            _ => {}
        }
    }

    /// `CMeleeAtkMsgq::Process`: sends the queued messages in order; a positive result removes the head and
    /// stops (an attack started), `0` stops and keeps the head, a negative result removes the head and
    /// continues. Returns whether an attack started.
    fn drain_melee_queue(&mut self, cx: &mut Cx<'_>) -> bool {
        while let Some(q) = self.act.queues.melee.head() {
            let r = send_act_msg_cx(
                self,
                cx,
                ActMsg {
                    msg: q.msg,
                    p: [q.p1, q.p2, q.p3, 0, 0],
                },
            );
            if r > 0 {
                self.act.queues.melee.pop();
                return true;
            } else if r == 0 {
                return false;
            }
            self.act.queues.melee.pop();
        }
        false
    }

    /// Queues a melee attack message (`m_qMeleeAtkMsg.AddTail`); returns `false` when the queue is full.
    pub fn enqueue_melee(&mut self, a: QueuedAct) -> bool {
        self.act.queues.melee.push(a)
    }
}

/// An action message with its parameters (`SendActMsg(dwMsg, nParam1..nParam5)`, defaults 0).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActMsg {
    /// The message (`OBJMSG_*`).
    pub msg: ObjMsg,
    /// Parameters 1..5.
    pub p: [i32; 5],
}

impl ActMsg {
    /// A message without parameters.
    pub const fn new(msg: ObjMsg) -> ActMsg {
        ActMsg { msg, p: [0; 5] }
    }
    /// A message with parameters.
    pub const fn with(msg: ObjMsg, p: [i32; 5]) -> ActMsg {
        ActMsg { msg, p }
    }
}
