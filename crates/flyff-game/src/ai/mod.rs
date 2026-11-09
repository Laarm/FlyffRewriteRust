//! Monster and pet AI (Plan/18 §8-§9): the AI framework (WP-AI-06), the target scans (WP-AI-07) and the generic
//! monster brain `CAIMonster` (WP-AI-08, WP-AI-09), plus the [`AiDriver`] plug point the movement pipeline calls
//! and the trivial [`WanderAi`] for tests.
//!
//! | module | content |
//! |---|---|
//! | [`msg`] | [`AiMsg`] / [`AiMsgId`]: the legacy `AIMSG` ids and the typed messages |
//! | [`ctx`] | [`AiCx`] (the plan's `AiCtx`): what a handler sees; [`AiCmd`]: effects on other movers |
//! | [`rng`] | `x_random` / `x_random_f` over the world stream of `AiCx.rng` |
//! | [`scan`] | the four target scans in legacy grid order, the per-tick [`ScanView`] of the players |
//! | [`meteonyker`] | [`MeteonykerAi`](meteonyker::MeteonykerAi): the Meteonyker brain `CAIMeteonyker` (IDLE, RAGE, the special-attack cycle, the retaliation) and its hit handlers |
//! | [`monster`] | [`MonsterAi`]: IDLE, STAND, RAGE, return-home |
//! | [`monster_skill`] | `CMonsterSkill`: the skills a monster casts at its hit frames and the HP-triggered transformation |
//! | [`spawn`] | [`MonsterSpawn`]: monsters the AI creates (summons, transformations) |
//! | [`boss`] | the boss AIs: the common skeleton of the four brains (`CAIClockWorks`, `CAIBigMuscle`, `CAIKrrr`, `CAIBear`), the ClockWorks, the hit handlers (BigMuscle, Krrr, Bear, Meteonyker: M7 work packages that fill its seams) |
//! | [`pet`] | [`PetAi`]: the eat-pet brain `CAIPet` (follow the owner, fetch ground items), its [`PetWorld`] seam to the item system |
//! | [`wander`] | [`WanderAi`]: a trivial wanderer for the movement tests |
//!
//! **Delivery semantics (18 §8.3, ref/12 §8.1).** [`send`] is synchronous: `SetState` switches the state handler
//! and delivers `Init` to the new one in the same call, every other message goes to the current handler.
//! [`post`] is `PostAIMsg`: only `Arrival` is queued (and delivered, FIFO, before `Process` on the mover's next
//! processed tick); everything else is a [`send`]. [`route_messages`] is the per-tick pass: drain the queue, then
//! deliver `Process`. Handlers recurse through `MonsterAi::send` (depth limit 8).
//!
//! **Who calls what.** The movement pipeline ([`crate::movement::MovementHooks`]) calls an installed
//! [`AiDriver`] for every awake non-player mover: [`AiDriver::init`] on the first processed tick (`InitAI`),
//! [`AiDriver::process`] in the AI pass at the head of the tick (frame `4n`) and [`AiDriver::message`] for the
//! sim events at the barrier of their frame (effective from `F + 1`). [`MonsterAiDriver`] maps them onto
//! [`on_added`], [`route_messages`] and [`post`]. Other documents reach an AI through the **mailbox**:
//! [`mail`] queues a message for a mover and the pipeline delivers it at the next AI pass or barrier of the
//! mover's layer (damage and death notifications of doc 17, 18 §8.6).

pub mod boss;
pub mod create_monster;
pub mod ctx;
pub mod meteonyker;
pub mod monster;
pub mod monster_skill;
pub mod msg;
pub mod pet;
pub mod rng;
pub mod scan;
pub mod spawn;
pub mod wander;

pub use ctx::{AiCmd, AiCtx, AiCx, HelperCall, HelperSlot, emote};
pub use monster::attack::{berserk_factor, berserk_factor_of};
pub use monster::{MonsterAi, MonsterState};
pub use msg::{AiMsg, AiMsgId};
pub use pet::{Dismiss, PetAi, PetCall, PetWorld, StubOptions, StubPets};
pub use scan::{
    JOB_ALL, PlayerScan, ScanSpec, ScanView, scan_target, scan_target_next,
    scan_target_over_healer, scan_target_strong,
};
pub use spawn::MonsterSpawn;
pub use wander::WanderAi;

use crate::movement::MovementState;
use crate::mover::Mover;
use flyff_common::ObjId;
use flyff_data::GameData;
use flyff_data::props::mover::MoverProp;
use flyff_rules::consts::defs::neuz::{
    AII_BEAR, AII_BIGMUSCLE, AII_CLOCKWORKS, AII_KRRR, AII_METEONYKER, AII_MONSTER, AII_PET,
};
use std::collections::VecDeque;

/// The boss AIs (ClockWorks, BigMuscle, Krrr, Bear/Mushmoot: M7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BossKind {
    /// `AII_CLOCKWORKS`.
    ClockWorks,
    /// `AII_BIGMUSCLE`.
    BigMuscle,
    /// `AII_KRRR`.
    Krrr,
    /// `AII_BEAR`.
    Bear,
}

/// The kind of AI a mover prop selects (`dwAI`, 18 §8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiKind {
    /// No AI: shops, quest givers, players, eggs (`AII_EGG` is client only), `AII_VER2_*` (dropped).
    None,
    /// `AII_MONSTER`: `CAIMonster` (guards, event monsters and Rangda included).
    Monster,
    /// A boss AI.
    Boss(BossKind),
    /// `AII_METEONYKER`.
    Meteonyker,
    /// `AII_PET`: the eat-pet AI.
    Pet,
}

/// `CObj::SetAIInterface` selection by `dwAI` (18 §8.1 table).
pub const fn kind_of_ai(dw_ai: u32) -> AiKind {
    match dw_ai {
        AII_MONSTER => AiKind::Monster,
        AII_CLOCKWORKS => AiKind::Boss(BossKind::ClockWorks),
        AII_BIGMUSCLE => AiKind::Boss(BossKind::BigMuscle),
        AII_KRRR => AiKind::Boss(BossKind::Krrr),
        AII_BEAR => AiKind::Boss(BossKind::Bear),
        AII_METEONYKER => AiKind::Meteonyker,
        AII_PET => AiKind::Pet,
        _ => AiKind::None,
    }
}

/// The AI kind of a mover prop (`MoverProp.ai`).
pub fn kind_of(prop: &MoverProp) -> AiKind {
    kind_of_ai(prop.ai)
}

/// The brain of a mover: `CAIMonster`, the eat-pet brain `CAIPet`, the boss brains (`CAIClockWorks`, `CAIBigMuscle`,
/// `CAIKrrr`, `CAIBear`) and the Meteonyker brain `CAIMeteonyker`; a mover whose prop selects no AI
/// ([`AiKind::None`]) gets [`AiBrain::None`].
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AiBrain {
    /// No brain.
    #[default]
    None,
    /// `CAIMonster`.
    Monster(Box<MonsterAi>),
    /// `CAIPet` (18 §12, WP-AI-15): driven by `pet::pass`, never by the generic [`AiDriver`].
    Pet(Box<PetAi>),
    /// The boss AIs (18 §10, WP-AI-18): ClockWorks, BigMuscle, Krrr, Bear.
    Boss(Box<boss::BossAi>),
    /// `CAIMeteonyker` (18 §11, WP-AI-21): `MI_DU_METEONYKER2/3/4`.
    Meteonyker(Box<meteonyker::MeteonykerAi>),
}

/// `MonsterExt.ai`: the AI of one mover (18 §8.1 `AiSlot`).
#[derive(Clone, Debug, PartialEq)]
pub struct AiSlot {
    /// `dwAI` (`AII_*`): the sleep rule exempts `AII_PET`.
    pub kind: u32,
    /// `m_dwAIInterfaceState`: the initial state of the respawn line (2 IDLE, 8 STAND; 15 sets it).
    pub state: u32,
    brain: AiBrain,
    queue: VecDeque<AiMsg>,
}

/// The default `m_dwAIInterfaceState` (`SetAIInterface(type, state = 2)`).
pub const AI_STATE_DEFAULT: u32 = 2;

/// An AI message queue longer than this drops new `Arrival`s (an unobserved mover is not processed, so its
/// queue would otherwise grow; a processed one drains it every tick).
const QUEUE_CAP: usize = 64;

impl Default for AiSlot {
    fn default() -> Self {
        AiSlot::new(0, AI_STATE_DEFAULT)
    }
}

impl AiSlot {
    /// The AI of a mover with `dwAI = kind` that starts in respawn-line state `state`.
    pub fn new(kind: u32, state: u32) -> AiSlot {
        let brain = match kind_of_ai(kind) {
            AiKind::Monster => AiBrain::Monster(Box::default()),
            AiKind::Pet => AiBrain::Pet(Box::default()),
            AiKind::Boss(k) => AiBrain::Boss(Box::new(boss::BossAi::new(k))),
            AiKind::Meteonyker => AiBrain::Meteonyker(Box::default()),
            AiKind::None => AiBrain::None,
        };
        AiSlot {
            kind,
            state,
            brain,
            queue: VecDeque::new(),
        }
    }

    /// The kind of AI.
    pub fn ai_kind(&self) -> AiKind {
        kind_of_ai(self.kind)
    }

    /// The monster brain, if this is a monster.
    pub fn monster(&self) -> Option<&MonsterAi> {
        match &self.brain {
            AiBrain::Monster(b) => Some(b),
            _ => None,
        }
    }

    /// The monster brain, mutably (tests set up scenarios through it).
    pub fn monster_mut(&mut self) -> Option<&mut MonsterAi> {
        match &mut self.brain {
            AiBrain::Monster(b) => Some(b),
            _ => None,
        }
    }

    /// Number of queued messages (only `Arrival` is ever queued).
    pub fn queued(&self) -> usize {
        self.queue.len()
    }
}

/// `CObj::SetAIInterface` for a mover built from its prop: the slot of `m`, `None` when the prop has no AI
/// (NPCs, players, eggs).
pub fn attach(m: &Mover, data: &GameData) -> Option<AiSlot> {
    let prop = m.prop(data)?;
    match kind_of(prop) {
        AiKind::None => None,
        _ => Some(AiSlot::new(prop.ai, AI_STATE_DEFAULT)),
    }
}

/// Runs `f` with the monster brain of `m` taken out of its slot (so the handler can hold the mover mutably next
/// to its own state). `None` for a mover without a monster brain, and while the brain is out (a re-entrant
/// call of a free function from inside a handler; handlers recurse through `MonsterAi::send`).
pub(crate) fn with_brain<R>(
    m: &mut Mover,
    f: impl FnOnce(&mut MonsterAi, &mut Mover, u32) -> R,
) -> Option<R> {
    let ext = m.monster_mut()?;
    let line_state = ext.ai.state;
    // only a monster brain is taken out: a pet's brain is not this function's to drop
    if !matches!(ext.ai.brain, AiBrain::Monster(_)) {
        return None;
    }
    let AiBrain::Monster(mut brain) = std::mem::take(&mut ext.ai.brain) else {
        return None;
    };
    let r = f(&mut brain, m, line_state);
    if let Some(ext) = m.monster_mut() {
        ext.ai.brain = AiBrain::Monster(brain);
    }
    Some(r)
}

/// [`with_brain`] for the boss brain (WP-AI-18): runs `f` with the boss brain of `m` taken out of its slot.
/// `None` for a mover without a boss brain, and while the brain is out.
pub(crate) fn with_boss_brain<R>(
    m: &mut Mover,
    f: impl FnOnce(&mut boss::BossAi, &mut Mover, u32) -> R,
) -> Option<R> {
    let ext = m.monster_mut()?;
    let line_state = ext.ai.state;
    if !matches!(ext.ai.brain, AiBrain::Boss(_)) {
        return None;
    }
    let AiBrain::Boss(mut brain) = std::mem::take(&mut ext.ai.brain) else {
        return None;
    };
    let r = f(&mut brain, m, line_state);
    if let Some(ext) = m.monster_mut() {
        ext.ai.brain = AiBrain::Boss(brain);
    }
    Some(r)
}

/// [`with_brain`] for the Meteonyker brain (WP-AI-21): runs `f` with the brain of `m` taken out of its slot.
/// `None` for a mover without a Meteonyker brain, and while the brain is out.
pub(crate) fn with_meteonyker_brain<R>(
    m: &mut Mover,
    f: impl FnOnce(&mut meteonyker::MeteonykerAi, &mut Mover, u32) -> R,
) -> Option<R> {
    let ext = m.monster_mut()?;
    let line_state = ext.ai.state;
    if !matches!(ext.ai.brain, AiBrain::Meteonyker(_)) {
        return None;
    }
    let AiBrain::Meteonyker(mut brain) = std::mem::take(&mut ext.ai.brain) else {
        return None;
    };
    let r = f(&mut brain, m, line_state);
    if let Some(ext) = m.monster_mut() {
        ext.ai.brain = AiBrain::Meteonyker(brain);
    }
    Some(r)
}

/// `InitAI` (18 §9.3): records the spawn position and angle and switches to the base state. Idempotent; every
/// entry point calls it first, so a message that reaches an AI before its first processed tick still works.
pub fn on_added(m: &mut Mover, cx: &mut AiCx<'_>) {
    if boss::is_boss(m) {
        return boss::init_ai(m, cx);
    }
    if meteonyker::is_meteonyker(m) {
        return meteonyker::init_ai(m, cx);
    }
    with_brain(m, |brain, m, line| brain.init_ai(m, cx, line));
}

/// `RouteMessage()` (18 §8.3 rule 3): delivers every queued message in FIFO order, then `Process`.
pub fn route_messages(m: &mut Mover, cx: &mut AiCx<'_>) {
    if boss::is_boss(m) {
        return boss::route(m, cx);
    }
    if meteonyker::is_meteonyker(m) {
        return meteonyker::route(m, cx);
    }
    with_brain(m, |brain, m, line| {
        brain.init_ai(m, cx, line);
        while let Some(msg) = m.monster_mut().and_then(|e| e.ai.queue.pop_front()) {
            brain.send(m, cx, msg);
        }
        brain.send(m, cx, AiMsg::Process);
    });
}

/// `SendAIMsg`: synchronous delivery to the mover's AI (see the module docs).
pub fn send(m: &mut Mover, msg: AiMsg, cx: &mut AiCx<'_>) {
    if boss::is_boss(m) {
        return boss::send(m, msg, cx);
    }
    if meteonyker::is_meteonyker(m) {
        return meteonyker::send(m, msg, cx);
    }
    with_brain(m, |brain, m, line| {
        brain.init_ai(m, cx, line);
        brain.send(m, cx, msg);
    });
}

/// `PostAIMsg` (`__AI_0509`): `Arrival` is queued for the next [`route_messages`], every other message is a
/// [`send`].
pub fn post(m: &mut Mover, msg: AiMsg, cx: &mut AiCx<'_>) {
    if matches!(msg, AiMsg::Arrival { .. }) {
        if let Some(ext) = m.monster_mut()
            && matches!(
                ext.ai.brain,
                AiBrain::Monster(_) | AiBrain::Pet(_) | AiBrain::Boss(_) | AiBrain::Meteonyker(_)
            )
            && ext.ai.queue.len() < QUEUE_CAP
        {
            ext.ai.queue.push_back(msg);
        }
    } else {
        send(m, msg, cx);
    }
}

/// Queues `msg` for mover `to` in the mailbox of the movement pipeline. This is how docs 15-22 send AI
/// messages without holding an [`AiCx`]: the pipeline delivers it as [`post`] at the next AI pass or barrier
/// of the mover's layer, so its effects are stamped like a synchronous send at that point (18 §8.6).
pub fn mail(movement: &mut MovementState, to: ObjId, msg: AiMsg) {
    movement.ai_mail.push((to, msg));
}

/// The brain plug point (18 §8): called by the movement pipeline for every awake non-player mover. Every
/// method defaults to doing nothing.
pub trait AiDriver: Send {
    /// `InitAI`: the first time the pipeline processes `m` (record the spawn position, choose the initial
    /// state). Called right before the first [`AiDriver::process`].
    fn init(&mut self, _m: &mut Mover, _cx: &mut AiCx<'_>) {}

    /// The per-tick AI pass of one awake mover (18 §3.3 step 4), before frame `4n`.
    fn process(&mut self, _m: &mut Mover, _cx: &mut AiCx<'_>) {}

    /// A sim event or a mailed message for the AI (18 §3.4, §8.6): delivered at the barrier of the frame it
    /// happened in (effective from `F + 1`), or at the head of the AI pass.
    fn message(&mut self, _m: &mut Mover, _msg: AiMsg, _cx: &mut AiCx<'_>) {}
}

/// The AI that does nothing (the default).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoAi;

impl AiDriver for NoAi {}

/// The driver of the monster brains: every awake non-player mover with a [`MonsterAi`] runs `CAIMonster`.
/// Stateless: all state lives in the movers (`MonsterExt.ai`), so replays and clones are exact.
#[derive(Clone, Copy, Debug, Default)]
pub struct MonsterAiDriver;

impl AiDriver for MonsterAiDriver {
    fn init(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        on_added(m, cx);
    }

    fn process(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        route_messages(m, cx);
    }

    fn message(&mut self, m: &mut Mover, msg: AiMsg, cx: &mut AiCx<'_>) {
        post(m, msg, cx);
    }
}
