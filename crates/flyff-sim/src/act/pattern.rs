//! Scripted move patterns of flying monsters (`ProcessMovePattern`, `MoverMove.cpp:469-664`;
//! Plan/31 §11.6, PL-003).
//!
//! Counters advance once per frame on both sides and durations use `PATTERN_SEC = 60` frames (the server's
//! `SEC1 = 15` made server patterns four times shorter, FIX PL-003): `++cnt > N` means a segment lasts
//! `N + 1` frames. Pattern 2 is the only one used by data; pattern 1 ("figure 8") is ported literally.

use super::{ActMsg, send_act_msg_cx};
use crate::mover::SimMover;
use crate::num::set_angle_quantized;
use crate::step::{Cx, SimEvent};
use crate::types::*;

/// `PATTERN_SEC`: one pattern "second" in frames.
pub const PATTERN_SEC: i32 = 60;

fn send(m: &mut SimMover, cx: &mut Cx<'_>, msg: ObjMsg) {
    send_act_msg_cx(m, cx, ActMsg::new(msg));
}

/// Stops looking, turning and moving and levels the pitch (the segment-end sequence of the original).
fn level_off(m: &mut SimMover, cx: &mut Cx<'_>, stop: bool) {
    send(m, cx, OBJMSG_STOP_LOOK);
    send(m, cx, OBJMSG_STOP_TURN);
    if stop {
        send(m, cx, OBJMSG_STOP);
    }
    m.angle_x = 0.0;
}

/// Pattern 2 (`m_nMovePattern == 2`).
pub fn process_pattern2(m: &mut SimMover, cx: &mut Cx<'_>) {
    if m.pattern.event == 0 {
        m.pattern.event = 1;
        m.pattern.cnt = 0;
    }
    match m.pattern.event {
        1 => {
            send(m, cx, OBJMSG_FORWARD);
            m.pattern.cnt += 1;
            if m.pattern.cnt > PATTERN_SEC {
                m.angle = set_angle_quantized(m.angle + 135.0);
                m.pattern.cnt = 0;
                m.pattern.event += 1;
            }
        }
        2 => {
            send(m, cx, OBJMSG_FORWARD);
            m.pattern.cnt += 1;
            if m.pattern.cnt > PATTERN_SEC * 2 {
                m.angle = set_angle_quantized(m.angle - 90.0);
                m.pattern.cnt = 0;
                m.pattern.event += 1;
                level_off(m, cx, true);
            }
        }
        3 => {
            send(m, cx, OBJMSG_FORWARD);
            m.pattern.cnt += 1;
            if m.pattern.cnt > PATTERN_SEC * 2 {
                m.angle = set_angle_quantized(m.angle - 45.0);
                m.pattern.cnt = 0;
                m.pattern.event += 1;
                level_off(m, cx, false);
            }
        }
        4 => {
            send(m, cx, OBJMSG_FORWARD);
            m.pattern.cnt += 1;
            if m.pattern.cnt > PATTERN_SEC {
                m.angle = set_angle_quantized(m.angle - 45.0);
                m.pattern.cnt = 0;
                m.pattern.event += 1;
                level_off(m, cx, true);
            }
        }
        5 => {
            send(m, cx, OBJMSG_FORWARD);
            m.pattern.cnt += 1;
            if m.pattern.cnt > PATTERN_SEC * 3 {
                m.angle = set_angle_quantized(m.angle + 45.0);
                m.pattern.cnt = 0;
                m.pattern.event = 99;
                level_off(m, cx, true);
                cx.emit(SimEvent::PatternDone);
            }
        }
        _ => {}
    }
}

/// Pattern 1 ("figure 8"; no data uses it), with `SEC1 = 60`.
pub fn process_pattern1(m: &mut SimMover, cx: &mut Cx<'_>) {
    let sec = PATTERN_SEC;
    if m.pattern.event == 0 {
        m.pattern.event = 1;
        m.pattern.cnt = 0;
    }
    let pulse = |cnt: i32, on: ObjMsg| if cnt & 3 == 0 { on } else { OBJMSG_STOP_TURN };
    match m.pattern.event {
        1 => {
            send(m, cx, OBJMSG_FORWARD);
            let msg = pulse(m.pattern.cnt, OBJMSG_LTURN);
            send(m, cx, msg);
            m.pattern.cnt += 1;
            if m.pattern.cnt > sec * 2 {
                m.pattern.cnt = 0;
                m.pattern.event += 1;
            }
        }
        2 => {
            send(m, cx, OBJMSG_FORWARD);
            let msg = pulse(m.pattern.cnt, OBJMSG_RTURN);
            send(m, cx, msg);
            let half = (sec * 5) / 2;
            if m.pattern.cnt < half {
                send(m, cx, OBJMSG_LOOKUP);
            } else if m.pattern.cnt == half {
                send(m, cx, OBJMSG_STOP_LOOK);
                m.angle_x = 0.0;
            } else {
                send(m, cx, OBJMSG_LOOKDOWN);
            }
            m.pattern.cnt += 1;
            if m.pattern.cnt > sec * 5 {
                m.pattern.cnt = 0;
                m.pattern.event += 1;
                level_off(m, cx, true);
            }
        }
        3 => {
            send(m, cx, OBJMSG_FORWARD);
            let half = (sec * 5) / 2;
            let on = if m.pattern.cnt < half {
                OBJMSG_RTURN
            } else {
                OBJMSG_LTURN
            };
            let msg = pulse(m.pattern.cnt, on);
            send(m, cx, msg);
            m.pattern.cnt += 1;
            if m.pattern.cnt > sec * 5 {
                m.pattern.cnt = 0;
                m.pattern.event += 1;
                level_off(m, cx, false);
            }
        }
        4 => {
            send(m, cx, OBJMSG_FORWARD);
            let msg = pulse(m.pattern.cnt, OBJMSG_LTURN);
            send(m, cx, msg);
            let half = (sec * 5) / 2;
            if m.pattern.cnt < half {
                send(m, cx, OBJMSG_LOOKDOWN);
            } else if m.pattern.cnt == half {
                send(m, cx, OBJMSG_STOP_LOOK);
                m.angle_x = 0.0;
            } else {
                send(m, cx, OBJMSG_LOOKUP);
            }
            m.pattern.cnt += 1;
            if m.pattern.cnt > sec * 5 {
                m.pattern.cnt = 0;
                m.pattern.event = 99;
                level_off(m, cx, true);
                cx.emit(SimEvent::PatternDone);
            }
        }
        _ => {}
    }
}
