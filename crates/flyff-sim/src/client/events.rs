//! The JS event buffer (`take_events`, Plan/31 §17.6): which `SimEvent`s leave the module, their kind
//! numbers and payload words ([`crate::wire::EVENT_KINDS`]), and the overflow policy.
//!
//! Layout of the buffer: `[count, dropped]` then `count` records of 8 words
//! `[kind, obj_id, frame_lo, a, b, c, d, e]`; floats are bit-cast. At most
//! [`wire::EVENT_CAPACITY`] records are kept per read; overflow drops the cosmetic events first and counts
//! them in `dropped`.

use crate::step::{HitKind, SimEvent};
use crate::wire::{self, EVENT_CAPACITY, EVENT_HEADER_WORDS, EVENT_WORDS};
use flyff_common::ObjId;

/// The most events kept while nobody reads the buffer (16 frame calls worth).
pub const MAX_PENDING: usize = 16 * EVENT_CAPACITY;

/// One exported event.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ExportEvent {
    /// `wire::EVENT_KINDS` id.
    pub kind: u32,
    /// The mover (or ctrl/decor) the event belongs to.
    pub obj: u32,
    /// Low word of the local frame the event was raised in.
    pub frame: u32,
    /// Payload words `a..e`.
    pub w: [u32; 5],
}

fn kind(name: &str) -> u32 {
    wire::event_kind(name).unwrap_or(0)
}

fn bits(v: f32) -> u32 {
    v.to_bits()
}

/// `HitKind` as the payload word of `AttrHit`.
pub const fn hit_kind_code(k: HitKind) -> u32 {
    match k {
        HitKind::Melee => 0,
        HitKind::MeleeSkill => 1,
        HitKind::MagicSkill => 2,
        HitKind::Wand => 3,
        HitKind::Range => 4,
        HitKind::Special => 5,
        HitKind::FlyMelee => 6,
    }
}

/// Maps a sim event of mover `mover` raised in `frame` to its export record, or `None` for events that stay
/// inside the host (collision responses, queue drains, server-only notifications, ...).
pub fn export(mover: ObjId, e: &SimEvent, frame: u64) -> Option<ExportEvent> {
    let f = frame as u32; // BOUNDS: the low word identifies the frame within the 2.2 years of a u32
    let ev = |name: &str, w: [u32; 5]| {
        Some(ExportEvent {
            kind: kind(name),
            obj: mover.0,
            frame: f,
            w,
        })
    };
    match e {
        SimEvent::AttrSound {
            obj,
            sound,
            x,
            y,
            z,
            positional,
        } => Some(ExportEvent {
            kind: kind("AttrSound"),
            obj: obj.0,
            frame: f,
            w: [
                *sound as u32, // BOUNDS: bit pattern of an i32
                bits(*x),
                bits(*y),
                bits(*z),
                if *positional {
                    wire::SOUND_POSITIONAL
                } else {
                    0
                },
            ],
        }),
        SimEvent::Footstep { obj, x, y, z } => Some(ExportEvent {
            kind: kind("Footstep"),
            obj: obj.0,
            frame: f,
            w: [0, bits(*x), bits(*y), bits(*z), 0],
        }),
        SimEvent::Landed => ev("Landed", [0; 5]),
        SimEvent::JumpCompleted => ev("JumpCompleted", [0; 5]),
        SimEvent::AttrQuake { obj } => Some(ExportEvent {
            kind: kind("AttrQuake"),
            obj: obj.0,
            frame: f,
            w: [0; 5],
        }),
        SimEvent::CameraQuake { frames, scale } => {
            ev("CameraQuake", [*frames as u32, bits(*scale), 0, 0, 0]) // BOUNDS: bit pattern
        }
        SimEvent::DustFx => ev("DustFx", [0; 5]),
        SimEvent::DriftFx => ev("DriftFx", [0; 5]),
        SimEvent::SwordTrail { hit_slow } => ev("SwordTrail", [u32::from(*hit_slow), 0, 0, 0, 0]),
        SimEvent::SwordTrailClear => ev("SwordTrailClear", [0; 5]),
        SimEvent::BossChargeFx { state } => ev("BossChargeFx", [*state, 0, 0, 0, 0]),
        SimEvent::YoyoThrowFx => ev("YoyoThrowFx", [0; 5]),
        SimEvent::CastSpark => ev("CastSpark", [0; 5]),
        SimEvent::SatDown => ev("SatDown", [0; 5]),
        SimEvent::RideChanged { item } => ev(
            "RideChanged",
            [u32::from(item.is_some()), item.unwrap_or(0), 0, 0, 0],
        ),
        SimEvent::RideMissing => ev("RideMissing", [0; 5]),
        SimEvent::RideFrameReset => ev("RideFrameReset", [0; 5]),
        SimEvent::AttrHit {
            kind: k,
            state,
            target,
            hit_index,
            ..
        } => ev(
            "AttrHit",
            [
                hit_kind_code(*k),
                *state,
                target.0,
                *hit_index as u32, // BOUNDS: bit pattern
                0,
            ],
        ),
        SimEvent::LocalAttack { msg, target, flags } => {
            ev("LocalAttack", [*msg, target.0, *flags as u32, 0, 0]) // BOUNDS: bit pattern
        }
        SimEvent::CmdArrived { cmd, target } => ev("CmdArrived", [*cmd, target.0, 0, 0, 0]),
        SimEvent::CmdAborted => ev("CmdAborted", [0; 5]),
        SimEvent::Notice { tid } => ev("Notice", [*tid, 0, 0, 0, 0]),
        SimEvent::CastCancel => ev("CastCancel", [0; 5]),
        SimEvent::LmbRelease => ev("LmbRelease", [0; 5]),
        SimEvent::DestPosChosen { pos, forward } => ev(
            "DestPosChosen",
            [
                u32::from(*forward),
                bits(pos.x),
                bits(pos.y),
                bits(pos.z),
                0,
            ],
        ),
        SimEvent::ChunkMissing { lx, lz } => Some(ExportEvent {
            kind: kind("ChunkMissing"),
            obj: 0,
            frame: f,
            w: [u32::from(*lx), u32::from(*lz), 0, 0, 0],
        }),
        SimEvent::ProjectileHit { id, pos } => Some(ExportEvent {
            kind: kind("ProjectileHit"),
            obj: 0,
            frame: f,
            w: [*id, bits(pos.x), bits(pos.y), bits(pos.z), 0],
        }),
        SimEvent::ProjectileExpired { id } => Some(ExportEvent {
            kind: kind("ProjectileExpired"),
            obj: 0,
            frame: f,
            w: [*id, 0, 0, 0, 0],
        }),
        _ => None,
    }
}

/// The pending events and the last assembled buffer.
#[derive(Clone, Debug, Default)]
pub struct EventBuffer {
    pending: Vec<ExportEvent>,
    words: Vec<u32>,
    dropped_carry: u32,
}

impl EventBuffer {
    /// An empty buffer.
    pub fn new() -> EventBuffer {
        EventBuffer::default()
    }

    /// Queues an event for the next [`EventBuffer::take`]. A host that never reads (a stalled page) cannot
    /// grow the queue without bound: beyond [`MAX_PENDING`] events the new ones are counted as dropped.
    pub fn push(&mut self, e: ExportEvent) {
        if self.pending.len() >= MAX_PENDING {
            self.dropped_carry = self.dropped_carry.saturating_add(1);
            return;
        }
        self.pending.push(e);
    }

    /// Number of events waiting.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Forgets the waiting events (leaving a world).
    pub fn clear(&mut self) {
        self.pending.clear();
        self.dropped_carry = 0;
    }

    /// Assembles `[count, dropped, records...]` from the waiting events (at most [`EVENT_CAPACITY`]; the
    /// cosmetic ones go first on overflow, then the newest) and empties the queue. The returned words stay
    /// valid until the next call.
    pub fn take(&mut self) -> &[u32] {
        let mut dropped = std::mem::take(&mut self.dropped_carry);
        if self.pending.len() > EVENT_CAPACITY {
            let mut excess = self.pending.len() - EVENT_CAPACITY;
            let cosmetic = |k: u32| {
                wire::EVENT_KINDS
                    .iter()
                    .find(|d| d.id == k)
                    .is_some_and(|d| d.cosmetic)
            };
            let mut kept = Vec::with_capacity(EVENT_CAPACITY);
            for e in self.pending.drain(..) {
                if excess > 0 && cosmetic(e.kind) {
                    excess -= 1;
                    dropped += 1;
                } else {
                    kept.push(e);
                }
            }
            if kept.len() > EVENT_CAPACITY {
                dropped += (kept.len() - EVENT_CAPACITY) as u32; // BOUNDS: far below u32::MAX
                kept.truncate(EVENT_CAPACITY);
            }
            self.pending = kept;
        }
        self.words.clear();
        self.words
            .reserve(EVENT_HEADER_WORDS + self.pending.len() * EVENT_WORDS);
        self.words.push(self.pending.len() as u32); // BOUNDS: at most 256
        self.words.push(dropped);
        for e in self.pending.drain(..) {
            self.words.extend_from_slice(&[e.kind, e.obj, e.frame]);
            self.words.extend_from_slice(&e.w);
        }
        &self.words
    }

    /// The words of the last [`EventBuffer::take`].
    pub fn words(&self) -> &[u32] {
        &self.words
    }
}
