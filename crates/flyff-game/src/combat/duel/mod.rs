//! Solo and party duels (Plan/17 §8.6-§8.7, WP-COMBAT-13; `DPSrvr.cpp:1942-2159`, `Mover.cpp:8304-8460,
//! 8706-8725`, `MoverParam.cpp:784-850`, `party.cpp:575-735`, `ActionMoverMsg.cpp:850-895`).
//!
//! | module | content |
//! |---|---|
//! | [`solo`] | the requests of a solo duel (`DuelRequestReq` / `DuelYes` / `DuelNoReq`), the result, the cancel rules |
//! | [`party`] | the party duels (`DuelPartyRequestReq` / `DuelPartyYes` / `DuelPartyNoReq`), start, result, cancel |
//! | [`mod@process`] | the per-tick work: the countdown, the 3 min timeout, the region rule, the vanished opponent |
//! | [`fame`] | the fame tables of the duel results |
//!
//! **State.** A dueling player has `combat.duel = {duel 1|2, state 104..100 | 1 | 300, other, end_at}`; a party
//! duelist also has `social.duel_party` (the opposing party). `state` counts down once a second from 104 (the
//! numbers 3, 2, 1 go to the client), `1` is the fight; damage flows only while the state is 1 on either side
//! ([`super::pvp::is_pvp_target`]). The loser of a duel really dies (Plan/17 §8.6): `last_pk` spares its EXP.
//!
//! **Requests (VR-001, VR-050).** The actor is always the session's player; the ids of the messages only name the
//! *other* player. A request is stored on the requester ([`DuelRequest`]: the invitee, the time, solo or party);
//! `DuelYes` is honoured only from that invitee within `gameplay.duel_request_ttl_s`, so a third player cannot
//! accept it and a late "yes" does nothing.
//!
//! **Parties.** Party duels read the membership through [`PartyDuelView`]: WP-SOCIAL-03 installs the real
//! implementation in `Services.combat.duel.view`; until then [`NoParties`] answers "no such party" and party duels
//! are refused. The tests use a fake.

pub mod fame;
pub mod party;
pub mod process;
pub mod solo;

pub use party::{
    cancel_party_of_leader, on_duel_party_no, on_duel_party_request, on_duel_party_yes,
    on_party_dissolved,
};
pub use process::process;
pub use solo::{
    cancel_solo, duel_result, on_die_message, on_duel_no, on_duel_request, on_duel_yes, sub_duel,
};

pub use crate::combat::state::DuelRequest;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::stats::out::{Aud, Out};
use flyff_common::{CharacterId, GameMs, ObjId, PartyId};
use flyff_proto::pb;
use std::collections::{BTreeMap, BTreeSet};

/// `NEXT_TICK_ENDDUEL`: a duel ends after 3 minutes without a hit.
pub const DUEL_IDLE_MS: u64 = 180_000;
/// `NEXT_TICK_DUEL`: the countdown ticks once a second.
pub const COUNTDOWN_STEP_MS: u64 = 1000;
/// The ProcessActionPoint cadence (3 s) of the "opponent vanished" check.
pub const OPPONENT_CHECK_MS: u64 = 3000;
/// `m_nDuelState` right after the accept: 104, 103, 102, 101, then 100 -> fight.
pub const STATE_COUNTDOWN: u16 = 104;
/// `m_nDuelState` of a party duelist who died: out of the fight.
pub const STATE_OUT: u16 = 300;
/// `m_nDuelState` while fighting.
pub const STATE_FIGHT: u16 = 1;
/// `PROCESS_COUNT * 2`: the invulnerability after a result (2 s).
pub const RESULT_INVULNERABLE_TICKS: i32 = 30;

/// One member of a party as the duels see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuelMember {
    /// The character (`m_aMember[i].m_uPlayerId`).
    pub character: CharacterId,
    /// The avatar when the member is online (`GetUserByPlayerID` valid).
    pub obj: Option<ObjId>,
}

/// The party facts the duels read (the membership trait of Plan/17 WP-COMBAT-12/13; implemented by doc 21's party
/// service, WP-SOCIAL-03, which installs itself in `Services.combat.duel.view`).
pub trait PartyDuelView: Send {
    /// The members of `party` in member order (index 0 is the leader); `None` when there is no such party.
    fn members(&self, svc: &Services, party: PartyId) -> Option<Vec<DuelMember>>;

    /// The name the opposing side sees: `m_sParty` of an advanced party; `None` for a plain party (the duel then
    /// shows the leader's name).
    fn advanced_name(&self, _svc: &Services, _party: PartyId) -> Option<String> {
        None
    }
}

/// No party service: every party is unknown, party duels are refused.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoParties;

impl PartyDuelView for NoParties {
    fn members(&self, _svc: &Services, _party: PartyId) -> Option<Vec<DuelMember>> {
        None
    }
}

/// Counters of the duel system (tests, metrics).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DuelStats {
    /// Solo duels that started.
    pub solo_started: u64,
    /// Party duels that started.
    pub party_started: u64,
    /// Duels that ended with a result.
    pub results: u64,
    /// Duels cancelled or timed out.
    pub cancelled: u64,
    /// Requests or answers ignored (forged, expired, wrong player).
    pub ignored: u64,
}

/// The duel service (`Services.combat.duel`): the party membership seam, the players that need the per-tick work
/// and the party pairs.
pub struct DuelService {
    /// The party membership; [`NoParties`] until doc 21 installs its implementation.
    pub view: Box<dyn PartyDuelView>,
    /// The players that are (or were a moment ago) in a duel: only they are visited every tick.
    active: BTreeSet<ObjId>,
    /// The party duels: party -> the party it duels (`CParty::m_idDuelParty`; doc 21 reads it for the party wire).
    pairs: BTreeMap<PartyId, PartyId>,
    /// The counters.
    pub stats: DuelStats,
}

impl Default for DuelService {
    fn default() -> Self {
        DuelService {
            view: Box::new(NoParties),
            active: BTreeSet::new(),
            pairs: BTreeMap::new(),
            stats: DuelStats::default(),
        }
    }
}

impl std::fmt::Debug for DuelService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DuelService")
            .field("active", &self.active)
            .field("pairs", &self.pairs)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl DuelService {
    /// `CParty::m_idDuelParty`: the party `party` duels, if it does.
    pub fn opposing(&self, party: PartyId) -> Option<PartyId> {
        self.pairs.get(&party).copied()
    }

    /// The players visited by the per-tick work (ascending).
    pub fn active(&self) -> impl Iterator<Item = ObjId> + '_ {
        self.active.iter().copied()
    }

    pub(crate) fn track(&mut self, id: ObjId) {
        self.active.insert(id);
    }

    pub(crate) fn untrack(&mut self, id: ObjId) {
        self.active.remove(&id);
    }

    pub(crate) fn pair(&mut self, a: PartyId, b: PartyId) {
        self.pairs.insert(a, b);
        self.pairs.insert(b, a);
    }

    pub(crate) fn unpair(&mut self, a: PartyId, b: Option<PartyId>) {
        self.pairs.remove(&a);
        if let Some(b) = b {
            self.pairs.remove(&b);
        }
    }
}

// ---- helpers shared by the submodules -------------------------------------------------------------------------------

/// The character id of a player.
pub(crate) fn character_of(m: &Mover) -> CharacterId {
    m.player().map_or(CharacterId(0), |p| p.ident.character_id)
}

/// The avatar of an online character (`GetUserByPlayerID`).
pub(crate) fn avatar_of(s: &Services, ctx: &GameCtx<'_>, character: CharacterId) -> Option<ObjId> {
    let live = |id: &ObjId| {
        ctx.worlds
            .mover(*id)
            .is_some_and(|m| m.is_player() && !m.core.deleted && character_of(m) == character)
    };
    // the player directory first (O(log n)), the session table as the fallback
    s.presence
        .get(character)
        .and_then(|e| e.online.map(|o| o.obj))
        .filter(live)
        .or_else(|| {
            ctx.sessions
                .iter()
                .find(|e| e.character == character)
                .map(|e| e.player)
                .filter(live)
        })
}

/// A live player mover (`IsValidObj`).
pub(crate) fn valid_player<'a>(ctx: &'a GameCtx<'_>, id: ObjId) -> Option<&'a Mover> {
    ctx.worlds
        .mover(id)
        .filter(|m| m.is_player() && !m.core.deleted)
}

/// Sends one message to the session of `to` with the wire object of `aud`.
pub(crate) fn tell(
    ctx: &mut GameCtx<'_>,
    to: ObjId,
    aud: Aud,
    body: impl Into<pb::server_msg::Msg>,
) {
    let mut out = Out::new();
    out.push(aud, body);
    out.flush(ctx, to);
}

/// A `DefinedText` to `to`.
pub(crate) fn text(ctx: &mut GameCtx<'_>, to: ObjId, tid: u32, args: Vec<String>) {
    crate::chat::ingress::notice::defined(ctx, PlayerRef(to), tid, args);
}

/// `AddSetDuel(m)`: the duel fields of `m` (the wire object of the message is the receiver, 0).
pub(crate) fn set_duel_msg(m: &Mover) -> pb::SetDuel {
    pb::SetDuel {
        mover_obj_id: m.id().0,
        duel: i32::from(m.combat.duel.duel),
        duel_state: i32::from(m.combat.duel.state),
        id_duel_other: m.combat.duel.other.map_or(u32::MAX, |o| o.0),
        id_duel_party: m.player().map_or(0, |p| p.social.duel_party),
    }
}

/// `SetDuel` of `about` to the session of `to`.
pub(crate) fn send_set_duel(ctx: &mut GameCtx<'_>, to: ObjId, about: ObjId) {
    let Some(m) = ctx.worlds.mover(about) else {
        return;
    };
    let msg = set_duel_msg(m);
    tell(ctx, to, Aud::SelfZero, msg);
}

/// `ClearDuel()`: no duel, no opponent, state 0; the idle timer restarts.
pub(crate) fn clear_duel(m: &mut Mover, now: GameMs) {
    let d = &mut m.combat.duel;
    d.duel = 0;
    d.other = None;
    d.state = 0;
    d.end_at = GameMs(now.0 + DUEL_IDLE_MS);
    d.leader = None;
    d.finishing = false;
}

/// `ClearDuelParty()`: no duel, no opposing party, state 0.
pub(crate) fn clear_duel_party(m: &mut Mover, now: GameMs) {
    clear_duel(m, now);
    if let Some(p) = m.player_mut() {
        p.social.duel_party = 0;
    }
}

/// `ReSetDuelTime(attacker, defender)` (`MoverAttack.cpp:1810-1840`): a hit between duel opponents restarts the
/// 3 minutes. Solo: both players. Party: the leaders of both parties (their ids are kept in the duel state, so the
/// damage pipeline needs no party service).
pub fn reset_duel_time(
    store: &mut crate::world::object::ObjectStore,
    now: GameMs,
    a: ObjId,
    d: ObjId,
) {
    let end = GameMs(now.0 + DUEL_IDLE_MS);
    let (Some(am), Some(dm)) = (store.mover(a), store.mover(d)) else {
        return;
    };
    let (ad, dd) = (am.combat.duel, dm.combat.duel);
    if dd.duel == 1 && ad.duel == 1 && dd.other == Some(a) && ad.other == Some(d) {
        for id in [a, d] {
            if let Some(m) = store.mover_mut(id) {
                m.combat.duel.end_at = end;
            }
        }
        return;
    }
    let party = |m: &Mover| m.player().and_then(|p| p.social.party).map_or(0, |x| x.0);
    let duel_party = |m: &Mover| m.player().map_or(0, |p| p.social.duel_party);
    if dd.duel == 2 && ad.duel == 2 && duel_party(dm) == party(am) && duel_party(am) == party(dm) {
        let mut ids = vec![a, d];
        ids.extend(ad.leader);
        ids.extend(dd.leader);
        for id in ids {
            if let Some(m) = store.mover_mut(id) {
                m.combat.duel.end_at = end;
            }
        }
    }
}
