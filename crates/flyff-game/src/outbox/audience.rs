//! Audiences and their resolution (Plan/15 §8.2, 10 §6.2).
//!
//! WP-OPS-06 defined [`Audience`], the [`AudienceResolver`] trait that [`crate::Outbox::broadcast`] calls and the
//! read-only [`GameView`]; WP-WORLD-05 resolves every variant over the worlds and the session table and adds
//! `broadcast_filtered` as an inherent `impl Outbox` block (15 §8.3).

use super::Outbox;
use crate::ctx::{Services, Worlds};
use crate::session::SessionTable;
use flyff_common::math::Vec3;
use flyff_common::{GuildId, LayerId, ObjId, PartyId, SessionId, WorldId};
use flyff_proto::pb;
use smallvec::SmallVec;

/// Who receives a broadcast (15 §8.2).
#[derive(Clone, Debug, PartialEq)]
pub enum Audience {
    /// The viewers of the subject (`subject.viewers`); a player subject includes itself.
    NearInclSelf,
    /// The same minus the subject.
    NearExclSelf,
    /// Every in-world player of the world, any layer.
    World(WorldId),
    /// Players of `world` (any layer) with `‖pos − center‖² < range²`.
    Radius {
        /// World.
        world: WorldId,
        /// Center.
        center: Vec3,
        /// Range (strict).
        range: f32,
    },
    /// Every online member of the party, any world.
    Party(PartyId),
    /// Every online member of the guild, any world.
    Guild(GuildId),
    /// Every player that is in a world.
    AllOnline,
    /// Every player of one layer.
    Layer(WorldId, LayerId),
    /// `CUserMng::AddShout`: same world and layer within `range` (inclusive); `range == 0` is `AllOnline`.
    Shout {
        /// World.
        world: WorldId,
        /// Layer.
        layer: LayerId,
        /// Center.
        center: Vec3,
        /// Range (inclusive); `0` = everyone.
        range: f32,
    },
    /// `AddWorldMsg(const CRect*)` (PL-118).
    Rect {
        /// Left edge.
        left: i32,
        /// Top edge.
        top: i32,
        /// Right edge.
        right: i32,
        /// Bottom edge.
        bottom: i32,
    },
    /// The listed players (trade pair, duel pair, party subset).
    Players(SmallVec<[ObjId; 8]>),
}

/// Resolution of an [`Audience`] to `(session, player)` pairs (15 §8.2).
pub trait AudienceResolver {
    /// Appends the recipients. Order: viewer-set order for `Near*`, directory order otherwise. Players without a
    /// bound session are skipped.
    fn resolve(
        &self,
        subject: Option<ObjId>,
        aud: &Audience,
        out: &mut SmallVec<[(SessionId, ObjId); 64]>,
    );
}

/// Read-only input of audience resolution: disjoint fields of the game state, so a broadcast can resolve
/// through `&SessionTable` while pushing into `&mut Outbox` (10 §4.1.1). `services` is `None` when the caller is
/// itself a service method; `Party`/`Guild` then resolve to nobody.
pub struct GameView<'a> {
    /// The worlds (viewer sets, doc 15).
    pub worlds: &'a Worlds,
    /// The session table.
    pub sessions: &'a SessionTable,
    /// The services (party, guild membership), when available.
    pub services: Option<&'a Services>,
    /// `fidelity.world_msg_rect_xz` (PL-118): `Audience::Rect` tests `(x, z)` (the default) instead of the
    /// legacy `(x, y)`.
    pub rect_xz: bool,
}

impl GameView<'_> {
    /// The session of a player if it is bound (a lingering avatar has none and receives nothing).
    fn session_of(&self, p: ObjId) -> Option<SessionId> {
        self.sessions.get(p).and_then(|e| e.session)
    }

    /// Every bound player that is an object of a world, in ascending `SessionId` order (the directory order
    /// of 15 §8.2 until the `PlayerDirectory` of doc 21 exists), with its header, filtered by `keep`.
    fn in_world_players(
        &self,
        out: &mut SmallVec<[(SessionId, ObjId); 64]>,
        mut keep: impl FnMut(&crate::world::ObjCore) -> bool,
    ) {
        for (sid, p) in self.sessions.bound() {
            if let Some(core) = self.worlds.player(p).map(|m| &m.core)
                && !core.deleted
                && keep(core)
            {
                out.push((sid, p));
            }
        }
    }
}

impl AudienceResolver for GameView<'_> {
    /// Resolves the audiences of 15 §8.2. `Near*` walks the viewer set of the subject in its insertion order
    /// (a player subject includes itself), the others the bound sessions in ascending `SessionId` order.
    /// Players without a bound session are skipped. `Party` and `Guild` need doc 21's membership (the services).
    fn resolve(
        &self,
        subject: Option<ObjId>,
        aud: &Audience,
        out: &mut SmallVec<[(SessionId, ObjId); 64]>,
    ) {
        match aud {
            Audience::AllOnline => out.extend(self.sessions.bound()),
            Audience::Players(list) => {
                for &p in list {
                    if let Some(sid) = self.session_of(p) {
                        out.push((sid, p));
                    }
                }
            }
            Audience::NearInclSelf | Audience::NearExclSelf => {
                let Some(subject) = subject else {
                    debug_assert!(false, "a Near* audience needs a subject");
                    return;
                };
                let excl = matches!(aud, Audience::NearExclSelf);
                if let Some(core) = self.worlds.core(subject) {
                    for &p in &core.viewers {
                        if excl && p == subject {
                            continue;
                        }
                        if let Some(sid) = self.session_of(p) {
                            out.push((sid, p));
                        }
                    }
                }
            }
            Audience::World(w) => self.in_world_players(out, |c| c.world == *w),
            Audience::Layer(w, l) => self.in_world_players(out, |c| c.world == *w && c.layer == *l),
            Audience::Radius {
                world,
                center,
                range,
            } => {
                let r2 = range * range;
                self.in_world_players(out, |c| c.world == *world && dist2(c.pos, *center) < r2);
            }
            Audience::Shout {
                world,
                layer,
                center,
                range,
            } => {
                if *range > 0.0 {
                    let r2 = range * range;
                    self.in_world_players(out, |c| {
                        c.world == *world && c.layer == *layer && dist2(c.pos, *center) <= r2
                    });
                } else {
                    // `AddShout` with range 0 is "everybody that is in a world"
                    self.in_world_players(out, |_| true);
                }
            }
            Audience::Rect {
                left,
                top,
                right,
                bottom,
            } => {
                let xz = self.rect_xz;
                self.in_world_players(out, |c| {
                    let x = c.pos.x as i32;
                    let a = if xz { c.pos.z as i32 } else { c.pos.y as i32 };
                    (*left..*right).contains(&x) && (*top..*bottom).contains(&a)
                });
            }
            Audience::Party(id) => {
                // the online members in party order (21 §2.8); a lingering avatar has no session and is skipped
                if let Some(s) = self.services {
                    for o in s.social.party_members_online(*id) {
                        if let Some(sid) = self.session_of(o) {
                            out.push((sid, o));
                        }
                    }
                }
            }
            Audience::Guild(id) => {
                // the online members in `CharacterId` order (21 §2.8); a lingering avatar has no session
                if let Some(s) = self.services {
                    for o in s.social.guild.online_objs(&s.presence, *id) {
                        if let Some(sid) = self.session_of(o) {
                            out.push((sid, o));
                        }
                    }
                }
            }
        }
    }
}

/// `‖a - b‖²` in `f32` (3-D).
fn dist2(a: Vec3, b: Vec3) -> f32 {
    let d = a - b;
    d.x * d.x + d.y * d.y + d.z * d.z
}

impl Outbox {
    /// Resolves `aud` like [`Outbox::broadcast`], keeps the recipients for which `keep(session, player)` is
    /// true, encodes the message **once** (and only when at least one recipient remains) and pushes it through
    /// the same internal path as `broadcast`, so staging inside a handler scope applies unchanged (15 §8.3).
    ///
    /// The filter receives each recipient in resolution order. It must not mutate game state; the owners of
    /// filters are the feature docs (vendor viewers, the pet filter, chat blocks, hidden GMs).
    pub fn broadcast_filtered<R: AudienceResolver + ?Sized, F: FnMut(SessionId, ObjId) -> bool>(
        &mut self,
        r: &R,
        subject: ObjId,
        aud: Audience,
        body: impl Into<pb::server_msg::Msg>,
        mut keep: F,
    ) {
        self.guard.check();
        let near = matches!(aud, Audience::NearInclSelf | Audience::NearExclSelf);
        debug_assert!(
            !(near && subject == ObjId::NULL),
            "Near* broadcast without a subject"
        );
        if near && subject == ObjId::NULL {
            return;
        }
        let mut recipients: SmallVec<[(SessionId, ObjId); 64]> = SmallVec::new();
        r.resolve(
            Some(subject).filter(|s| *s != ObjId::NULL),
            &aud,
            &mut recipients,
        );
        recipients.retain(|&mut (sid, p)| self.live(sid) && keep(sid, p));
        if recipients.is_empty() {
            return;
        }
        let m = self.encode(subject.0, body.into());
        for (sid, _) in recipients {
            self.push_one(sid, &m);
        }
    }
}
