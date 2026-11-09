//! The AI's view of the world while one handler runs (Plan/18 §8.4): [`AiCx`] (the plan's `AiCtx`) and the
//! cross-mover commands [`AiCmd`].
//!
//! A handler never holds two `&mut Mover`. It gets the acting mover mutably plus the read-only things of the
//! layer (the start-of-tick snapshot of the movers, the player grid, the [`ScanView`] of the players, the
//! collision world), and returns effects on *other* movers as [`AiCmd`]s that the movement pipeline applies
//! right after the handler returns, before the next mover is processed (18 §8.3 rule 6).

use super::msg::AiMsg;
use super::scan::ScanView;
use super::spawn::MonsterSpawn;
use crate::movement::{IntentAudience, MoveShared, intents};
use crate::mover::Mover;
use crate::world::grid::Grid;
use crate::world::visibility::SpawnSink;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{GameMs, LayerId, ObjId, SimFrame, Tick, WorldId};
use flyff_data::GameData;
use flyff_data::types::MoverId;
use flyff_proto::{EncodedMsg, pb};
use flyff_sim::collision::world::CollisionWorld;
use flyff_sim::step::{Cx, MoverLookup, SimEnv, SimRules};
use flyff_sim::types::ActResult;
use flyff_sim::{ActMsg, EventMode, SimRole};

/// `DLGEMOT_*`: the emoticons a monster shows (`DialogMsg.h`).
pub mod emote {
    /// `DLGEMOT_ATTACK`: the monster found a target.
    pub const ATTACK: u32 = 0;
    /// `DLGEMOT_SUMMON`.
    pub const SUMMON: u32 = 1;
    /// `DLGEMOT_EVADE`: the monster runs away.
    pub const EVADE: u32 = 2;
    /// `DLGEMOT_HELPER`: the monster calls for help.
    pub const HELPER: u32 = 3;
    /// `DLGEMOT_BERSERK` (never sent).
    pub const BERSERK: u32 = 4;
    /// `DLGEMOT_LOOT` (monster looting is disabled).
    pub const LOOT: u32 = 5;
}

/// An effect of a handler on a mover other than the acting one (18 §8.4). The pipeline applies the commands in
/// order after the handler returned; a command may produce more commands (bounded).
#[derive(Clone, Debug, PartialEq)]
pub enum AiCmd {
    /// Deliver `msg` to `to` as if the original had called `SendAIMsg` on it (helpers set to RAGE, `DstDie`
    /// to enemies, ...). The target may be asleep: synchronous messages still reach it (18 §7).
    Send {
        /// The receiving mover.
        to: ObjId,
        /// The message.
        msg: AiMsg,
    },
    /// `CObj::Delete` (an expired summon, the source of a transform).
    Delete(ObjId),
    /// `CreateObj` + `ADDOBJ` of a monster (a summon, 18 §9.9): created right after the handler returns, linked by
    /// the `_add` of the next tick; the summoner learns the new id.
    Spawn(MonsterSpawn),
    /// The second half of `CMover::RemoveAllEnemies()`: every enemy in `enemies` (the list `who` had, already
    /// cleared on `who`) drops `who` through `RemoveEnemy` (`DstDie` to its AI, its melee target and destination
    /// object cleared when they were `who`), walking the ids from the highest down (18 §9.10).
    RemoveEnemies {
        /// The monster that forgets its enemies.
        who: ObjId,
        /// The enemies it had, ascending.
        enemies: Vec<ObjId>,
    },
    /// `CAIMonster::CallHelper`: the movers around the caller are not visible to a handler, the pipeline picks
    /// them and sends the `SetState{RAGE}` messages (18 §9.9).
    CallHelpers(HelperCall),
    // ---- boss effects (doc 18 §8.4, §10.3, §11, task boss-ai-b) ----
    /// `CWorld::SendDamageAround(&center, flags, attacker, OBJTYPE_PLAYER, item, radius)` raised by a brain (Krrr's
    /// meteors): `attacker` is explicit because the pipeline does not know the acting mover when it drains.
    DamageAround {
        /// The boss that hits.
        attacker: ObjId,
        /// The centre of the area.
        center: Vec3,
        /// The `AF_*` of every hit.
        flags: flyff_rules::combat::AtkFlags,
        /// The attack item (the damage source).
        item: u32,
        /// The radius in metres (strict).
        radius: f32,
    },
    /// `pPlayer->REPLACE(g_uIdofMulti, world, pos, REPLACE_NORMAL, layer)` (the Meteonyker's pull).
    Replace {
        /// The player that is moved.
        player: ObjId,
        /// The world it lands in.
        world: WorldId,
        /// The layer it lands in.
        layer: LayerId,
        /// Where.
        pos: Vec3,
    },
    /// An announcement or a skill of a boss ([`BossFx`](super::boss::fx::BossFx)): queued as a `SkillJob::Boss`
    /// and run at the end of the world phase.
    Boss(super::boss::fx::BossFx),
}

/// One slot of `SetCallHelper(hp, mover, num, party)`: how many helpers it takes (0 = unlimited) and whether the
/// target's party members are the ones to attack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelperSlot {
    /// `m_nCallHelperNum[i]`: at most this many movers answer in the slot (0 = any number, up to 40).
    pub num: i16,
    /// `m_bCallHelperParty[i]`.
    pub party: bool,
}

/// A call for help (`CAIMonster::CallHelper`, `AIMonster.cpp:1061-1160`): the data the pipeline needs to pick the
/// helpers around `pos` and to choose their targets.
#[derive(Clone, Debug, PartialEq)]
pub struct HelperCall {
    /// The caller (excluded from the helpers).
    pub caller: ObjId,
    /// The caller's mover prop (`GetIndex()`, compared when `help_who == 2`).
    pub caller_mover: MoverId,
    /// The caller's position (the window is centred here).
    pub pos: Vec3,
    /// `nRange`: metres; the window of the scan is `nRange / MPU` link units.
    pub n_range: i32,
    /// `m_bHelpWho`: 1 anybody, 2 only movers of the caller's own kind.
    pub help_who: u8,
    /// `m_nCallHelperMax` slots (5 once a `Helper` AI exists).
    pub slots: Vec<HelperSlot>,
    /// `m_dwIdTarget`: the caller's target.
    pub target: Option<ObjId>,
    /// The party id of the target (0 = none), read when the call was made.
    pub target_party: u32,
}

/// What an AI needs to drive its own mover: the clock, the world RNG, the collision world, the sim calls
/// (`send_act`) with the right frame in the environment, and the read-only layer data. Built by the pipeline
/// for one call (18 §8.4's `AiCtx`).
pub struct AiCx<'a> {
    /// The tick being processed.
    pub tick: Tick,
    /// The game clock of the tick (`GameMs`, PL-012): every AI timer compares against this.
    pub now: GameMs,
    /// The frame the AI acts in: `4n` in the AI pass, `F + 1` while a sim event of frame `F` is delivered.
    pub frame: SimFrame,
    /// The world of the layer being processed.
    pub world: WorldId,
    /// The layer being processed.
    pub layer: LayerId,
    /// `IsInstanceDungeon(world)`: no leashes, wider `ScanTargetNext`.
    pub instance: bool,
    /// The world's random stream (`RngSet::world(world)`, 18 §8.5): the only RNG an AI may use.
    pub rng: &'a mut dyn GameRng,
    /// The game-rule predicates of the sim.
    pub rules: &'a mut dyn SimRules,
    /// The static data of the movement pipeline (also the loaded `GameData`).
    pub shared: &'a MoveShared,
    /// The collision world of the mover's world.
    pub collision: &'a CollisionWorld,
    /// The dynamic ctrl colliders of the layer.
    pub dynamic: &'a flyff_sim::collision::dynamic::DynamicColliders,
    /// Other movers as of the start of the tick (or the frame).
    pub lookup: &'a dyn MoverLookup,
    /// The player grid of the layer (legacy order, 15 §6).
    pub grid: &'a Grid,
    /// The players of the layer as the scans see them (start of the tick).
    pub scan: &'a ScanView,
    /// Where messages for the viewers of the acting mover go (emoticons): the session queues.
    pub sink: &'a mut dyn SpawnSink,
    /// Effects on other movers (applied by the pipeline after the handler returns).
    pub cmds: &'a mut Vec<AiCmd>,
}

/// The plan's name for [`AiCx`].
pub type AiCtx<'a> = AiCx<'a>;

impl AiCx<'_> {
    /// The loaded game data (props, `propMoverEx`).
    pub fn data(&self) -> &GameData {
        &self.shared.data
    }

    /// `SendActMsg(msg)` on `m` at [`AiCx::frame`] (18 §3.5): the sim call with this context's environment.
    pub fn send_act(&mut self, m: &mut Mover, msg: ActMsg) -> ActResult {
        self.with_cx(m, |sim, cx| flyff_sim::act::send_act_msg_cx(sim, cx, msg))
    }

    /// Runs `f` with `m`'s sim and the sim context of this call (environment of [`AiCx::frame`], rules, the
    /// world RNG, `m`'s event buffer): the way docs 17/18 call `set_damaged_motion`, `die`, `reset_state` ...
    /// on a mover from the AI pass or the barrier. Events raised go to `m.sync.events` and reach the barrier.
    pub fn with_cx<R>(
        &mut self,
        m: &mut Mover,
        f: impl FnOnce(&mut flyff_sim::SimMover, &mut Cx<'_>) -> R,
    ) -> R {
        let env = SimEnv {
            frame: self.frame,
            role: SimRole::Server,
            flags: self.shared.flags,
            world: self.collision,
            dynamic: self.dynamic,
            motions: &self.shared.motions,
            lookup: self.lookup,
            mode: EventMode::Normal,
        };
        let mut cx = Cx::new(&env, &mut *self.rules, &mut *self.rng, &mut m.sync.events);
        f(&mut m.sim, &mut cx)
    }

    /// Queues `msg` for mover `to` (applied after the current handler returns).
    pub fn send(&mut self, to: ObjId, msg: AiMsg) {
        self.cmds.push(AiCmd::Send { to, msg });
    }

    /// `g_UserMng.AddCreateSfxObj(m, sfx, x, y, z)`: the effect `sfx` at `pos` (or on `m` when `pos` is zero) to
    /// everybody who sees `m`.
    pub fn sfx(&mut self, m: &Mover, sfx: u32, pos: Vec3) {
        let msg = EncodedMsg::new(
            m.core.id.0,
            crate::world::fx::create_sfx_msg(sfx, pos, false),
        );
        intents::deliver(&mut *self.sink, m, IntentAudience::Viewers, &msg);
    }

    /// `g_UserMng.AddDlgEmoticon(m, n)`: a `ChatNtf` with the text `"!<n>"` from `m` to its viewers (21's
    /// message; the client shows it as a balloon).
    pub fn emote(&mut self, m: &Mover, n: u32) {
        let msg = EncodedMsg::new(
            m.core.id.0,
            pb::ChatNtf {
                text: format!("!{n}"),
            },
        );
        intents::deliver(&mut *self.sink, m, IntentAudience::Viewers, &msg);
    }
}
