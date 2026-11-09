//! The messages a stat function produces (Plan/16 §4.7, §17).
//!
//! The functions of `stats` mutate one `&mut Mover` and must not borrow the worlds while they send, so they
//! append `(audience, message)` pairs to an [`Out`]; the caller flushes it afterwards, either through the
//! [`GameCtx`] (handlers, timers: the `Outbox` resolves the audience) or through the world driver's
//! [`SpawnSink`] (the tick tail of a mover, where only `relay(player, bytes)` exists). Both preserve the order
//! of the appends, which is the order of the legacy sends.

use crate::ctx::{GameCtx, PlayerRef};
use crate::mover::Mover;
use crate::outbox::Audience;
use crate::world::SpawnSink;
use flyff_common::ObjId;
use flyff_proto::{EncodedMsg, pb};

/// Who receives one message of an [`Out`] (the subject is the mover the function ran on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aud {
    /// `NearInclSelf`: the viewers of the subject, a player subject included; the wire `obj_id` is the subject.
    NearInclSelf,
    /// `NearExclSelf`.
    NearExclSelf,
    /// The subject's own session only, wire `obj_id = 0` (the legacy `NULL_ID` header of `SETSTATE`,
    /// `SET_TUTORIAL_STATE`, `SETEXPERIENCE`, ...).
    SelfZero,
    /// The subject's own session only, wire `obj_id` = the subject (`SetPointParam{DST_GOLD}`, PL-075).
    SelfSubject,
}

/// One queued message.
#[derive(Clone, Debug, PartialEq)]
pub struct OutMsg {
    /// The audience.
    pub aud: Aud,
    /// The message.
    pub body: pb::server_msg::Msg,
}

/// An ordered list of messages produced by a stat function.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Out {
    msgs: Vec<OutMsg>,
}

impl Out {
    /// An empty list.
    pub fn new() -> Out {
        Out::default()
    }

    /// Appends a message for `aud`.
    pub fn push(&mut self, aud: Aud, body: impl Into<pb::server_msg::Msg>) {
        self.msgs.push(OutMsg {
            aud,
            body: body.into(),
        });
    }

    /// Appends a `NearInclSelf` message.
    pub fn near(&mut self, body: impl Into<pb::server_msg::Msg>) {
        self.push(Aud::NearInclSelf, body);
    }

    /// Appends a `NearExclSelf` message.
    pub fn near_excl(&mut self, body: impl Into<pb::server_msg::Msg>) {
        self.push(Aud::NearExclSelf, body);
    }

    /// Appends a message for the subject's own session with wire `obj_id` 0.
    pub fn to_self(&mut self, body: impl Into<pb::server_msg::Msg>) {
        self.push(Aud::SelfZero, body);
    }

    /// Appends a message for the subject's own session with wire `obj_id` = the subject.
    pub fn to_self_subject(&mut self, body: impl Into<pb::server_msg::Msg>) {
        self.push(Aud::SelfSubject, body);
    }

    /// The queued messages in order.
    pub fn iter(&self) -> impl Iterator<Item = &OutMsg> {
        self.msgs.iter()
    }

    /// Number of queued messages.
    pub fn len(&self) -> usize {
        self.msgs.len()
    }

    /// `true` when nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.msgs.is_empty()
    }

    /// Takes the queued messages, leaving the list empty.
    pub fn take(&mut self) -> Vec<OutMsg> {
        std::mem::take(&mut self.msgs)
    }

    /// Sends everything through the outbox: `NearInclSelf` / `NearExclSelf` resolve the viewer set of `subject`,
    /// the self audiences go to the subject's bound session (a lingering avatar has none and receives nothing).
    pub fn flush(self, ctx: &mut GameCtx<'_>, subject: ObjId) {
        for OutMsg { aud, body } in self.msgs {
            match aud {
                Aud::NearInclSelf => ctx.broadcast(None, subject, Audience::NearInclSelf, body),
                Aud::NearExclSelf => ctx.broadcast(None, subject, Audience::NearExclSelf, body),
                Aud::SelfZero | Aud::SelfSubject => {
                    let header = if aud == Aud::SelfZero { 0 } else { subject.0 };
                    if let Some(sid) = ctx.session_of(PlayerRef(subject)) {
                        ctx.outbox.to_session(sid, header, body);
                    }
                }
            }
        }
    }

    /// Sends everything through the world driver's sink (the tick tail, where the mover is already borrowed):
    /// the viewer set of `m` in its order (a player not yet its own viewer is added), encoded once per message.
    pub fn flush_sink(self, sink: &mut dyn SpawnSink, m: &Mover) {
        let id = m.core.id;
        for OutMsg { aud, body } in self.msgs {
            match aud {
                Aud::NearInclSelf | Aud::NearExclSelf => {
                    let enc = EncodedMsg::new(id.0, body);
                    for &v in &m.core.viewers {
                        if aud == Aud::NearInclSelf || v != id {
                            sink.relay(v, &enc);
                        }
                    }
                    if aud == Aud::NearInclSelf && m.is_player() && !m.core.viewers.contains(&id) {
                        sink.relay(id, &enc);
                    }
                }
                Aud::SelfZero | Aud::SelfSubject => {
                    if m.is_player() {
                        let header = if aud == Aud::SelfZero { 0 } else { id.0 };
                        sink.relay(id, &EncodedMsg::new(header, body));
                    }
                }
            }
        }
    }
}
