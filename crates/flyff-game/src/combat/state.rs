//! The combat state of a mover (Plan/17 §1.4; WP-COMBAT-04).
//!
//! **One location per datum.** The attack fields the sim's attack sub-machines set inside `step_frame`
//! (`m_nAtkCnt`, `m_dwAtkFlags`, `m_objidHit`, `m_nMotionHitCount`, `m_nShootCnt`) exist only in `SimMover`
//! (31 §5.1); [`CombatState`] keeps no copy and the functions below read and write the sim fields. Nothing in
//! [`CombatState`] is persisted: cooldowns, pink time, duels, invulnerability and queues are lost on logout
//! (KEEP-QUIRK, `Mover.cpp:483,1519`).

use super::hate::HateList;
use crate::mover::Mover;
use flyff_common::{GameMs, ObjId};
use std::collections::VecDeque;

/// `OBJACT_*`: what the mover is doing for the duration of an attack or skill state (`m_oaAct`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObjAct {
    /// `OBJACT_NONE`.
    #[default]
    None,
    /// `OBJACT_MELEE_ATTACK`.
    MeleeAttack,
    /// `OBJACT_MAGIC_ATTACK` (wand).
    MagicAttack,
    /// `OBJACT_RANGE_ATTACK` (bow).
    RangeAttack,
    /// `OBJACT_SP_ATTACK` (NPC special attack).
    SpAttack,
    /// `OBJACT_USESKILL`.
    UseSkill,
}

/// `m_oaAct` + `m_nAParam[0..4]` + the projectile id (`SetActParam`, `Mover.h`): the parameters the hit-frame
/// handlers read (§3.8, §6.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActParams {
    /// What the mover is doing.
    pub act: ObjAct,
    /// `m_nAParam[0..4]`: target, power / item, level, control (the meaning depends on `act`).
    pub p: [i64; 5],
    /// The projectile allocated at the start of a wand / bow attack.
    pub projectile_id: u32,
}

/// A queued attack message (`m_qMeleeAtkMsg` / `m_qMagicAtkMsg` entry, §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueuedActMsg {
    /// The `OBJMSG_*`.
    pub msg: u32,
    /// The target.
    pub target: ObjId,
    /// `nParam2` (wand queue: 0 = wand, 1 = bow).
    pub p2: i32,
    /// `nParam3` (the charge of a bow shot).
    pub p3: i32,
    /// `nParam4`.
    pub p4: i32,
    /// The projectile allocated at the start.
    pub projectile_id: u32,
}

/// `MAX_ACTMSG`: capacity of a queue.
pub use flyff_rules::combat::MAX_ACTMSG;

/// `m_qMeleeAtkMsg` / `m_qMagicAtkMsg`: a FIFO of at most [`MAX_ACTMSG`] messages; a push beyond that is dropped
/// silently.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActMsgQueue(VecDeque<QueuedActMsg>);

impl ActMsgQueue {
    /// Appends `m`; `false` (nothing stored) when the queue is full.
    pub fn push(&mut self, m: QueuedActMsg) -> bool {
        if self.0.len() >= MAX_ACTMSG {
            return false;
        }
        self.0.push_back(m);
        true
    }

    /// The head.
    pub fn front(&self) -> Option<&QueuedActMsg> {
        self.0.front()
    }

    /// Removes and returns the head.
    pub fn pop(&mut self) -> Option<QueuedActMsg> {
        self.0.pop_front()
    }

    /// Number of queued messages.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` when nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Drops everything (`ClearState`, death).
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// PK timers (`m_dwPKTime`, §8.3). `pink_until` is the end of the pink window (`None` = not pink); only
/// extended, never shortened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PkTimers {
    /// The end of the "pink" window.
    pub pink_until: Option<GameMs>,
}

/// The plan's name for [`PkTimers`]; the persisted `value` / `propensity` / `pk_exp` live in `PlayerExt.pk`.
pub type PkState = PkTimers;

/// A pending duel request of a player (`m_tmDuelRequest`, bound to the invitee: VR-050).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuelRequest {
    /// The player invited.
    pub target: ObjId,
    /// When the request was made.
    pub at: GameMs,
    /// A party duel request (else solo).
    pub party: bool,
}

/// The duel state of a player (`m_nDuel`, `m_nDuelState`, ...; Plan/17 §8.6, WP-COMBAT-13). The opposing party
/// of a party duel (`m_idDuelParty`) is `PlayerSocial.duel_party`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DuelState {
    /// 0 none, 1 solo, 2 party.
    pub duel: u8,
    /// 104 -> 100 countdown, 1 fighting, 300 out.
    pub state: u16,
    /// The opponent of a solo duel.
    pub other: Option<ObjId>,
    /// The end of the duel (`m_dwTickEndDuel`): 3 minutes after the last hit.
    pub end_at: GameMs,
    /// The next step of the countdown (`m_dwTickDuel`, a 1 s grid).
    pub next_tick: GameMs,
    /// The next check for a vanished opponent (the 3 s tick of `ProcessActionPoint`).
    pub next_ap: GameMs,
    /// A party duel: the leader of the own party (its idle timer is the one that counts).
    pub leader: Option<ObjId>,
    /// The loser of a duel result that is about to die: `do_die` lets the death through although the result
    /// already made the player invulnerable for 2 s.
    pub finishing: bool,
    /// The request this player made and waits for (VR-050).
    pub pending: Option<DuelRequest>,
}

/// An offered resurrection (§9.4; `m_Resurrection_Data`): the caster and the skill row the accept applies
/// (`pSkillProp` / `pAddSkillProp`: the skill and the level of the cast).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResurrectionOffer {
    /// The caster.
    pub caster: ObjId,
    /// The skill id (`SI_ASS_HEAL_RESURRECTION`).
    pub skill: u32,
    /// The level of the cast (the add-skill row of the accept).
    pub level: u32,
}

/// One player of a monster's damage meter (`__DPSPLAYER`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DpsEntry {
    /// `m_idPlayer`.
    pub id_player: u32,
    /// The avatar the player had when it was listed (a player that logs in again is listed anew).
    pub obj: ObjId,
    /// `m_szName`.
    pub name: String,
    /// `nDPS`: the damage dealt since the player was listed (what mode 0 sends).
    pub dps: i32,
    /// `nDPSTotal`.
    pub total: i32,
}

/// `__DPS_METER` tracking of a monster (§10): `Some` on a boss, an instance-dungeon super monster or a monster of
/// `DPSProp.txt` (`m_bIsDPSMover`), with the players that hit it in insertion order (`m_aDPSPlayer`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DpsTrack {
    /// The tracked players in insertion order.
    pub players: Vec<DpsEntry>,
    /// A value or the list changed since the last mode-0 send (the rewrite sends only then, Plan/17 §10).
    pub dirty: bool,
}

/// The older name of [`DpsTrack`] (WP-MOVER-02's stand-in).
pub type DpsMeter = DpsTrack;

/// `CombatState` (§1.4): everything of the damage pipeline that lives on a mover and is not a sim field.
#[derive(Clone, Debug, PartialEq)]
pub struct CombatState {
    /// `m_nDead`: post-death / post-duel invulnerability in ticks, decremented every tick (§3.9).
    pub dead_ticks: i32,
    /// `m_oaAct` and its parameters.
    pub act: ActParams,
    /// `m_qMeleeAtkMsg`.
    pub melee_queue: ActMsgQueue,
    /// `m_qMagicAtkMsg` (wand and bow).
    pub magic_queue: ActMsgQueue,
    /// `m_nReflexDmg`: the Reflex Hit accumulator.
    pub reflex_dmg: i32,
    /// NPC: the last player attacker (`m_idAttacker`, the exp-ratio rule).
    pub id_attacker: Option<ObjId>,
    /// NPC: the claim owner (`m_idTargeter`, `SETTARGET` and kill-steal).
    pub id_targeter: Option<ObjId>,
    /// `m_idLastHitMover`.
    pub id_last_hit: Option<ObjId>,
    /// Players: the remembered target (`m_idSetTarget`: party skills, the skill queue).
    pub id_set_target: Option<ObjId>,
    /// NPC: `m_fExpRatio` (1.0). **INERT legacy field**: written by `on_attacked`, read by nothing (KEEP
    /// write-only; kill EXP does not use it, 16 §9.3). Nothing may start reading it without a ledger row.
    pub exp_ratio: f32,
    /// `m_nOverHeal` in ticks (450 = 30 s); consumer: the AI's target scan.
    pub over_heal_ticks: u32,
    /// `m_idEnemies`: the **only** enemy list (doc 16's `EnemyLedger` is replaced by it).
    pub hate: HateList,
    /// PK timers.
    pub pk: PkState,
    /// Duel state (players).
    pub duel: DuelState,
    /// A resurrection offered by a caster.
    pub resurrection: Option<ResurrectionOffer>,
    /// `Some` while a DPS meter tracks this mover.
    pub dps: Option<DpsTrack>,
    /// NPC: replaced by `CMonsterSkill::MonsterTransform` and waiting for its deferred delete (the `IsDelete()` the
    /// original sets at once; a second hit of the same barrier must not transform it again).
    pub transformed: bool,
}

impl Default for CombatState {
    fn default() -> Self {
        CombatState {
            dead_ticks: 0,
            act: ActParams::default(),
            melee_queue: ActMsgQueue::default(),
            magic_queue: ActMsgQueue::default(),
            reflex_dmg: 0,
            id_attacker: None,
            id_targeter: None,
            id_last_hit: None,
            id_set_target: None,
            exp_ratio: 1.0,
            over_heal_ticks: 0,
            hate: HateList::default(),
            pk: PkState::default(),
            duel: DuelState::default(),
            resurrection: None,
            dps: None,
            transformed: false,
        }
    }
}

// ---- accessors over the sim-owned fields (§1.4) --------------------------------------------------------------------

/// `m_nAtkCnt` (the sim field `sim.atk_cnt`).
pub fn atk_cnt(m: &Mover) -> i32 {
    m.sim.atk_cnt
}

/// Sets `m_nAtkCnt`.
pub fn set_atk_cnt(m: &mut Mover, n: i32) {
    m.sim.atk_cnt = n;
}

/// `m_dwAtkFlags` (`sim.act.atk_flags`).
pub fn atk_flags(m: &Mover) -> u32 {
    m.sim.act.atk_flags
}

/// `m_objidHit` (`sim.act.objid_hit`); `None` for `NULL_ID`.
pub fn obj_hit(m: &Mover) -> Option<ObjId> {
    let id = m.sim.act.objid_hit;
    (id != ObjId::NONE).then_some(id)
}

/// `m_nMotionHitCount` (`sim.act.motion_hit_count`).
pub fn motion_hit_count(m: &Mover) -> i32 {
    m.sim.act.motion_hit_count
}

/// `m_nShootCnt` (`sim.act.shoot_cnt`).
pub fn shoot_cnt(m: &Mover) -> i32 {
    m.sim.act.shoot_cnt
}

/// `IsDie()` (`Mover.h:1286`): the action state is a death state **or** the stored HP is 0.
pub fn is_die(m: &Mover) -> bool {
    m.sim.act.state.is_die() || m.mv.hp == 0
}

/// `IsLive()` (`Mover.h:1287`): sic, the original reads "not in a death state **or** HP above 0".
pub fn is_live(m: &Mover) -> bool {
    !m.sim.act.state.is_die() || m.mv.hp > 0
}

/// `m_nDead > 0`: invulnerable after a revival or a duel.
pub fn is_invulnerable(m: &Mover) -> bool {
    m.combat.dead_ticks > 0
}

/// `IsPeaceful()`: the belligerence of the NPC is `BELLI_PEACEFUL`.
pub fn is_peaceful(m: &Mover) -> bool {
    let belli = match &m.kind {
        crate::mover::MoverKind::Monster(e) => e.belligerence,
        crate::mover::MoverKind::Npc(e) => e.belligerence,
        crate::mover::MoverKind::Player(_) => return false,
    };
    belli == flyff_rules::consts::dst::BELLI_PEACEFUL
}

/// `IsChaotic()` (§8.3): `pk_propensity > 0`.
pub fn is_chaotic(m: &Mover) -> bool {
    m.player().is_some_and(|p| p.pk.propensity > 0)
}
