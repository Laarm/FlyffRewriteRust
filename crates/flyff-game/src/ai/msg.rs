//! AI messages (Plan/18 §8.2, ref/12 §8.1): the legacy `AIMSG` ids and the typed messages.

use flyff_common::ObjId;

/// The legacy `AIMSG_*` ids (`AIInterface.h:29-52`), kept for traces only.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AiMsgId {
    /// `AIMSG_INIT`: a state was entered (delivered by `SetState`).
    Init = 1,
    /// `AIMSG_PROCESS`: the per-tick pass.
    Process = 2,
    /// `AIMSG_TIMEOUT` (never sent in V15).
    Timeout = 3,
    /// `AIMSG_SETSTATE`.
    SetState = 4,
    /// `AIMSG_SETPROCESS` (never sent in V15).
    SetProcess = 5,
    /// `AIMSG_DSTDAMAGE` (never sent in V15).
    DstDamage = 6,
    /// `AIMSG_DSTDIE`: an enemy died.
    DstDie = 7,
    /// `AIMSG_DAMAGE`: I was hit.
    Damage = 8,
    /// `AIMSG_DIE`: I died.
    Die = 9,
    /// `AIMSG_COLLISION`.
    Collision = 10,
    /// `AIMSG_ARRIVAL`: the destination was reached (the only queued message).
    Arrival = 11,
    /// `AIMSG_NOMOVE` (never sent in V15).
    NoMove = 12,
    /// `AIMSG_BEGINMOVE`.
    BeginMove = 13,
    /// `AIMSG_ATTACK_MELEE`: a melee hit frame (only bosses react).
    AttackMelee = 14,
    /// `AIMSG_END_MELEEATTACK`.
    EndMeleeAttack = 15,
    /// `AIMSG_END_APPEAR`.
    EndAppear = 16,
    /// `AIMSG_INIT_TARGETCLEAR`: I killed my target.
    InitTargetClear = 17,
    /// `AIMSG_EXIT`.
    Exit = 18,
    /// `AIMSG_TARGET_FIND` (never sent in V15).
    TargetFind = 19,
    /// `AIMSG_REATTACK_TIMEOUT` (never sent in V15).
    ReattackTimeout = 20,
    /// `AIMSG_INVALID_TARGET` (never sent in V15).
    InvalidTarget = 21,
}

/// A message for an AI (18 §8.2). Ids never sent in V15 (`Timeout`, `SetProcess`, `DstDamage`, `NoMove`,
/// `TargetFind`, `ReattackTimeout`, `InvalidTarget`) have no variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiMsg {
    /// `AIMSG_INIT`: delivered by [`AiMsg::SetState`] to the new state only.
    Init {
        /// The state that was entered (legacy `p1`).
        state: u32,
        /// The state's parameter (legacy `p2`, an object id; `None` = `NULL_ID`).
        param: Option<ObjId>,
    },
    /// `AIMSG_PROCESS`.
    Process,
    /// `AIMSG_SETSTATE`: switch the state handler and deliver `Init{state, param}` to it.
    SetState {
        /// The new state (`STATE_*`).
        state: u32,
        /// The parameter (legacy `p2`).
        param: Option<ObjId>,
    },
    /// `AIMSG_DSTDIE`: `dead` died (an enemy of mine).
    DstDie {
        /// The dead mover.
        dead: ObjId,
    },
    /// `AIMSG_DAMAGE`.
    Damage {
        /// The attacker.
        attacker: ObjId,
        /// The damage dealt (0 on a miss).
        damage: i32,
    },
    /// `AIMSG_DIE`.
    Die,
    /// `AIMSG_COLLISION`: a wall, an attribute undo or a slope slide.
    Collision,
    /// `AIMSG_ARRIVAL`: the destination was reached.
    Arrival {
        /// The destination object (legacy `p1`); `None` = `NULL_ID`.
        obj: Option<ObjId>,
        /// Legacy `p2 == 1`: a position arrival.
        pos_arrival: bool,
    },
    /// `AIMSG_BEGINMOVE`: `OBJMSG_FORWARD` was accepted.
    BeginMove,
    /// `AIMSG_ATTACK_MELEE`.
    AttackMelee,
    /// `AIMSG_END_MELEEATTACK`.
    EndMeleeAttack,
    /// `AIMSG_END_APPEAR`.
    EndAppear,
    /// `AIMSG_INIT_TARGETCLEAR`.
    InitTargetClear,
    /// `AIMSG_EXIT`.
    Exit,
}

impl AiMsg {
    /// The legacy id of this message.
    pub const fn id(&self) -> AiMsgId {
        match self {
            AiMsg::Init { .. } => AiMsgId::Init,
            AiMsg::Process => AiMsgId::Process,
            AiMsg::SetState { .. } => AiMsgId::SetState,
            AiMsg::DstDie { .. } => AiMsgId::DstDie,
            AiMsg::Damage { .. } => AiMsgId::Damage,
            AiMsg::Die => AiMsgId::Die,
            AiMsg::Collision => AiMsgId::Collision,
            AiMsg::Arrival { .. } => AiMsgId::Arrival,
            AiMsg::BeginMove => AiMsgId::BeginMove,
            AiMsg::AttackMelee => AiMsgId::AttackMelee,
            AiMsg::EndMeleeAttack => AiMsgId::EndMeleeAttack,
            AiMsg::EndAppear => AiMsgId::EndAppear,
            AiMsg::InitTargetClear => AiMsgId::InitTargetClear,
            AiMsg::Exit => AiMsgId::Exit,
        }
    }
}
