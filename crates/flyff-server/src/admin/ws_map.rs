//! The connection-side map encoder (Plan/25 §3.15 "Connection task", ADM-D5; framework: WP-ADMIN-05, diff and encode:
//! **WP-ADMIN-07**).
//!
//! The game thread hands a completed [`MapFrame`] to the connection's latest-wins slot. The gateway task takes the
//! newest one, decides whether a *keyframe* is due ([`EncodePlan`]: on subscribe, every 10 s, after a `Resync` or a
//! `MapUpdate`) and asks the subscription's [`MapEncoder`] for the messages. [`new_encoder`] is the factory the
//! gateway calls when a map subscription opens; it returns a [`DiffEncoder`].
//!
//! # What the client receives
//!
//! * A **`MapKeyframe`** replaces the client's picture: every entity as a full record, the bins of a tier-C frame,
//!   the name of every player in the frame. It is sent for the first frame, when the plan asks for one, and whenever
//!   the tier or the `degraded` flag changes (the set of kinds changed, so a delta against the old picture would
//!   mislead).
//! * A **`MapDelta`** changes it:
//!   * `spawns`: a full record for every id the client does not have, and for an id whose `kind`, `sub`, `prop` or
//!     `level` changed (a level-up, a transformed monster); a spawn for a known id *replaces* the record;
//!   * `moves`: packed arrays (`ids`, `x`, `z`, `yaw`, `hp_pct`, `flags`, and `target` in tier A only) for the ids
//!     whose position, yaw, HP, flags or target changed;
//!   * `removes`: the ids that left the frame;
//!   * `bins`: **the full set of bins** in tier C (empty in tiers A and B);
//!   * `names`: the players the client has not been told about yet.
//!
//!   A delta is sent for every frame, also an unchanged one (a few bytes: the client's heartbeat and latency probe).
//! * The encoder only ever diffs against the last frame **it encoded**, and the gateway only encodes frames it
//!   sends, so a congested socket (frames skipped) is handled by construction: the next delta is computed against
//!   the last frame actually sent (25 §11.4).

use flyff_game::admin::map::{EntityKind, MapBin, MapEntity, MapFrame, Tier};
use flyff_proto::admin_pb as pb;
use std::collections::{HashMap, HashSet};

/// The bin edge of a tier-C frame in metres (25 §3.7; the collector's constant).
pub use flyff_game::admin::map::BIN_M;

/// What the gateway decided for one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodePlan {
    /// A full `MapKeyframe` is due (the subscription just opened, 10 s passed since the last one, a `Resync` was
    /// sent, or the encoder asked for it). An encoder may send a keyframe even when this is `false`.
    pub keyframe: bool,
}

/// The messages of one frame.
#[derive(Debug)]
pub struct Encoded {
    /// In wire order: a `MapKeyframe` or a `MapDelta`.
    pub msgs: Vec<pb::AdminServerMsg>,
    /// `true`: the messages contain a keyframe (the gateway restarts its 10 s timer).
    pub keyframe: bool,
}

/// Turns the frames of one map subscription into messages, keeping what it needs to diff (the last frame it sent,
/// the player names the client already has).
pub trait MapEncoder: Send {
    /// Encodes `frame`. Called with the newest frame only; frames the gateway skipped (a congested socket) are never
    /// shown to the encoder, so a delta is always computed against the last frame **actually sent** (25 §11.4).
    fn encode(&mut self, frame: &MapFrame, plan: EncodePlan) -> Encoded;

    /// The client lost its picture (`Resync`, a map update): forget the baseline.
    fn reset(&mut self) {}
}

/// Creates the encoder of a new map subscription: the diffing [`DiffEncoder`].
pub fn new_encoder(sub_id: u32) -> Box<dyn MapEncoder> {
    Box::new(DiffEncoder::new(sub_id))
}

/// Sends every frame as a full `MapKeyframe` (the simple encoder; tests and tools use it as a reference).
#[derive(Debug, Default)]
pub struct KeyframeEncoder;

impl MapEncoder for KeyframeEncoder {
    fn encode(&mut self, frame: &MapFrame, _plan: EncodePlan) -> Encoded {
        Encoded {
            msgs: vec![pb::AdminServerMsg {
                msg: Some(pb::admin_server_msg::Msg::MapKeyframe(keyframe(frame))),
            }],
            keyframe: true,
        }
    }
}

/// What the client holds after the last message: its entities by id.
#[derive(Debug)]
struct Baseline {
    tier: Tier,
    degraded: bool,
    entities: HashMap<u32, MapEntity>,
}

/// The diffing encoder of one map subscription (see the module documentation).
#[derive(Debug)]
pub struct DiffEncoder {
    sub_id: u32,
    last: Option<Baseline>,
    /// The players whose name the client has (pruned to the players still in the frame).
    names_sent: HashSet<u32>,
}

impl DiffEncoder {
    /// An encoder for subscription `sub_id`; its first frame is a keyframe.
    pub fn new(sub_id: u32) -> DiffEncoder {
        DiffEncoder {
            sub_id,
            last: None,
            names_sent: HashSet::new(),
        }
    }

    fn keyframe(&mut self, frame: &MapFrame) -> Encoded {
        self.names_sent = frame.player_names.iter().map(|(id, _)| *id).collect();
        self.last = Some(Baseline {
            tier: frame.tier,
            degraded: frame.degraded,
            entities: frame.entities.iter().map(|e| (e.id, *e)).collect(),
        });
        let mut k = keyframe(frame);
        k.sub_id = self.sub_id;
        Encoded {
            msgs: vec![pb::AdminServerMsg {
                msg: Some(pb::admin_server_msg::Msg::MapKeyframe(k)),
            }],
            keyframe: true,
        }
    }
}

/// `true` when the record the client holds must be replaced (a field a move does not carry changed).
fn identity_changed(old: &MapEntity, new: &MapEntity) -> bool {
    old.kind != new.kind || old.sub != new.sub || old.prop != new.prop || old.level != new.level
}

/// `true` when a move must be sent for an entity the client already has.
fn moved(old: &MapEntity, new: &MapEntity, with_target: bool) -> bool {
    old.x.to_bits() != new.x.to_bits()
        || old.z.to_bits() != new.z.to_bits()
        || old.yaw != new.yaw
        || old.hp_pct != new.hp_pct
        || old.flags != new.flags
        || (with_target && old.target != new.target)
}

impl MapEncoder for DiffEncoder {
    fn encode(&mut self, frame: &MapFrame, plan: EncodePlan) -> Encoded {
        let Some(base) = self.last.as_ref() else {
            return self.keyframe(frame);
        };
        if plan.keyframe || base.tier != frame.tier || base.degraded != frame.degraded {
            return self.keyframe(frame);
        }
        let with_target = frame.tier == Tier::A;
        let mut spawns = Vec::new();
        let mut moves = pb::MapMoves::default();
        let mut now: HashMap<u32, MapEntity> = HashMap::with_capacity(frame.entities.len());
        for e in &frame.entities {
            match base.entities.get(&e.id) {
                None => spawns.push(spawn(e)),
                Some(old) if identity_changed(old, e) => spawns.push(spawn(e)),
                Some(old) if moved(old, e, with_target) => {
                    moves.ids.push(e.id);
                    moves.x.push(e.x);
                    moves.z.push(e.z);
                    moves.yaw.push(u32::from(e.yaw));
                    moves.hp_pct.push(u32::from(e.hp_pct));
                    moves.flags.push(u32::from(e.flags.0));
                    if with_target {
                        moves.target.push(e.target);
                    }
                }
                Some(_) => {}
            }
            now.insert(e.id, *e);
        }
        let mut removes: Vec<u32> = base
            .entities
            .keys()
            .filter(|id| !now.contains_key(id))
            .copied()
            .collect();
        removes.sort_unstable();
        // the names of players the client has not seen; the set follows the players in the frame
        let mut names = Vec::new();
        for (id, name) in &frame.player_names {
            if self.names_sent.insert(*id) {
                names.push(pb::PlayerName {
                    id: *id,
                    name: name.clone(),
                });
            }
        }
        self.names_sent
            .retain(|id| now.get(id).is_some_and(|e| e.kind == EntityKind::Player));
        let delta = pb::MapDelta {
            sub_id: self.sub_id,
            tick: frame.tick,
            tier: tier_number(frame.tier),
            degraded: frame.degraded,
            moves: (!moves.ids.is_empty()).then_some(moves),
            spawns,
            removes,
            bins: frame.bins.iter().map(bin).collect(),
            names,
        };
        self.last = Some(Baseline {
            tier: frame.tier,
            degraded: frame.degraded,
            entities: now,
        });
        Encoded {
            msgs: vec![pb::AdminServerMsg {
                msg: Some(pb::admin_server_msg::Msg::MapDelta(delta)),
            }],
            keyframe: false,
        }
    }

    fn reset(&mut self) {
        self.last = None;
        self.names_sent.clear();
    }
}

/// The protocol number of a tier.
pub fn tier_number(t: Tier) -> i32 {
    match t {
        Tier::A => pb::Tier::A as i32,
        Tier::B => pb::Tier::B as i32,
        Tier::C => pb::Tier::C as i32,
    }
}

/// The index of a tier in the metrics (`flyff_admin_map_subscriptions{tier}`, 0 = A).
pub fn tier_index(t: Tier) -> usize {
    match t {
        Tier::A => 0,
        Tier::B => 1,
        Tier::C => 2,
    }
}

/// The protocol number of an entity kind.
pub fn kind_number(k: EntityKind) -> i32 {
    match k {
        EntityKind::Player => pb::EntityKind::Player as i32,
        EntityKind::Monster => pb::EntityKind::Monster as i32,
        EntityKind::Npc => pb::EntityKind::Npc as i32,
        EntityKind::Pet => pb::EntityKind::Pet as i32,
        EntityKind::Item => pb::EntityKind::Item as i32,
        EntityKind::Ctrl => pb::EntityKind::Ctrl as i32,
    }
}

/// One entity as a `MapEntitySpawn` record.
pub fn spawn(e: &MapEntity) -> pb::MapEntitySpawn {
    pb::MapEntitySpawn {
        id: e.id,
        kind: kind_number(e.kind),
        sub: u32::from(e.sub),
        prop: e.prop,
        level: u32::from(e.level),
        x: e.x,
        z: e.z,
        yaw: u32::from(e.yaw),
        hp_pct: u32::from(e.hp_pct),
        flags: u32::from(e.flags.0),
        target: e.target,
    }
}

/// One bin as a `MapBin` record.
pub fn bin(b: &MapBin) -> pb::MapBin {
    pb::MapBin {
        bx: u32::from(b.bx),
        bz: u32::from(b.bz),
        normal: u32::from(b.normal),
        captain: u32::from(b.captain),
        asleep: u32::from(b.asleep),
    }
}

/// The full `MapKeyframe` of a frame.
pub fn keyframe(f: &MapFrame) -> pb::MapKeyframe {
    pb::MapKeyframe {
        sub_id: f.sub_id,
        tick: f.tick,
        tier: tier_number(f.tier),
        degraded: f.degraded,
        bin_m: if f.tier == Tier::C { BIN_M } else { 0 },
        entities: f.entities.iter().map(spawn).collect(),
        bins: f.bins.iter().map(bin).collect(),
        names: f
            .player_names
            .iter()
            .map(|(id, name)| pb::PlayerName {
                id: *id,
                name: name.clone(),
            })
            .collect(),
    }
}
