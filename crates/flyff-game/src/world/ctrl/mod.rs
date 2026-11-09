//! Common ctrls: chests, doors, traps, triggers (`Ctrl`, Plan/15 §14, F-091; WP-WORLD-18).
//!
//! | module | content |
//! |---|---|
//! | this file | the entity ([`Ctrl`], `CCommonCtrl`), the constants of the ctrl data, registration at `_add` / destroy (collider, per-layer keys), the doc-17 seam [`CtrlHooks`] |
//! | [`process`] | `CCommonCtrl::Process` (§14.5): the four sub-frame steps, `_ProcessAction`, `DropItem`, `DropNPC`, `_ProcessTrap`, linked ctrls, the open timer |
//! | [`arrival`] | `CUser::OnMsgArrival` for an `OT_CTRL` (§14.3): the open flow, `CtrlCoolTimeCancel` |
//!
//! # What the server runs
//!
//! * **Animation** is [`flyff_sim::anim::CtrlClock`] (the same state machine the browser's decor entry of a ctrl
//!   runs): `CModel::FrameMove` in object mode, `m_bAniPlay` / `m_fFrameCount`, `CK2_KEEP` re-arm, `CK3_HALF` pause.
//!   Four steps per tick (one per sim frame `4n .. 4n+3`). The shape data (`max_frame`, raw `per_slerp`, the
//!   hit/sound attributes) is the ctrl's [`CtrlModel`](flyff_sim::collision::CtrlModel) of the `FLYK` file; a ctrl
//!   whose model is missing runs a clock without frames (a static trigger such as a warp pad works without one).
//! * **Collision** (§14.7): at `_add` the ctrl registers a collider in its layer's
//!   [`DynamicColliders`](flyff_sim::collision::dynamic::DynamicColliders) with the world matrix of its
//!   `SpawnCtrl` message ([`flyff_sim::collision::dynamic::ctrl_world_mat43`]: the quantised yaw and scale the
//!   client receives) and `seq = ObjId` (spawn order, the same on every client); each tick that moves the
//!   frame re-poses it, and the destroy removes it. The movers of the layer test it in `process_collision`
//!   before the statics (PL-019: each ctrl in its own animated pose).
//! * **Linked ctrls** (§14.6, FIX PL-151): `elem.ctrl_key` registers the ctrl in its **layer's** key map at
//!   `_add` (the first ctrl of a key wins like `map::insert`, and a ctrl only unregisters a key that points
//!   at itself); a trap fires the ctrl its `link_ctrl_key` names in the same layer.

pub mod arrival;
pub mod process;

use super::Worlds;
use super::object::{ObjCore, ObjKind};
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, GameMs, LayerId, ObjId, WorldId};
use flyff_data::worlds::CtrlElemDef;
use flyff_sim::anim::CtrlClock;
use flyff_sim::collision::dynamic::{AddCollider, ctrl_world_mat43};
use std::sync::Arc;

pub use arrival::{CtrlArrival, CtrlQuestPort, NoCtrlQuests, ctrl_cool_time_cancel};

/// `UA_*` open conditions of `CCtrlElem.m_dwSet` and the trap operation types.
pub use flyff_common::defs::define::{
    TOT_NOENDU, TOT_RANDOM, UA_CLASS, UA_GENDER, UA_ITEM, UA_LEVEL, UA_PLAYER_ID, UA_QUEST,
    UA_QUEST_END, UA_TELEPORT,
};
/// `CK1_*`, `CK2_*`, `CK3_*` of `CtrlProp` (`defineItemkind.h`).
pub use flyff_common::defs::itemkind::{
    CK1_CHEST, CK1_DOOR, CK1_GUILD_HOUSE, CK1_HOUSING, CK1_TRIGGER, CK2_FADE, CK2_KEEP, CK3_FULL,
    CK3_HALF,
};
/// `CI_PSYCHICWALL` and `CI_EXPBOX`, the two special ctrl ids.
pub use flyff_common::defs::obj::{CI_EXPBOX, CI_PSYCHICWALL};

/// `COMMONPLACE_ACTION`: play the ctrl animation.
pub const COMMONPLACE_ACTION: u32 = 0;
/// `COMMONPLACE_ALPHA`: fade the ctrl out.
pub const COMMONPLACE_ALPHA: u32 = 1;
/// `COMMONPLACE_QUAKE`: camera shake (BigMuscle's opener sends it to the first player in range,
/// `ai::boss::bigmuscle`; no ctrl sends it).
pub const COMMONPLACE_QUAKE: u32 = 2;
/// `MAX_CTRLDROPITEM`: item slots of a chest.
pub const MAX_CTRLDROPITEM: usize = 4;
/// `MAX_CTRLDROPMOB`: monster slots of a chest.
pub const MAX_CTRLDROPMOB: usize = 3;
/// `MAX_TRAP`: trap skill slots.
pub const MAX_TRAP: usize = 3;
/// `OPEN_COOLTIME`: the timed opening of an exp box, ms.
pub const OPEN_COOLTIME_MS: u64 = 10_000;
/// `FADE` step per sub-frame (`m_nBlendFactorLocal -= 5`).
pub const FADE_STEP: i32 = 5;

/// The animation state of a ctrl: `CModel::FrameMove` in object mode plus `m_bAniPlay` / `m_fFrameCount`, the
/// `CK2_KEEP` re-arm and the `CK3_HALF` pause (`flyff_sim::anim::CtrlClock`, shared with the browser).
pub type CtrlAnim = CtrlClock;

/// The state of a psychic wall (doc 17 fills it and runs it, `skill::ctrl`; the ctrl only carries it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PsychicWallState {
    /// The caster whose skill created the wall (`m_idAttacker`).
    pub caster: Option<ObjId>,
    /// The skill (`m_pSkillProp`) and the level of its add-skill row (`m_pAddSkillProp`).
    pub skill: u32,
    /// See [`PsychicWallState::skill`].
    pub level: u32,
    /// `m_bControl`: the caster may hurt players outside a duel (a GM's forced PvP).
    pub control: bool,
    /// `m_tmStart`: when the wall was created.
    pub born: GameMs,
    /// `m_nLife`: the contacts so far; the wall breaks at `level / 2`.
    pub life: i32,
}

/// What a ctrl does beyond the common behaviour.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CtrlSpecial {
    /// A chest, door, trap or trigger (everything this module runs).
    #[default]
    Common,
    /// The psychic wall (`CI_PSYCHICWALL`): processed by doc 17 through [`WorldHooks::process_ctrl`](super::WorldHooks).
    PsychicWall(Box<PsychicWallState>),
}

/// An `OT_CTRL` object (15 §14.1; `CCommonCtrl`).
#[derive(Clone, Debug)]
pub struct Ctrl {
    /// The object header; `core.index` is the `CtrlProp` id.
    pub core: ObjCore,
    /// The configuration record (`CCtrlElem`); `max_item_num` mutates (one-shot drops).
    pub elem: CtrlElemDef,
    /// The animation state.
    pub anim: CtrlAnim,
    /// `m_dwTrapProcessTime` (`0xFFFFFFFF` = `None`).
    pub trap_at: Option<GameMs>,
    /// `m_bTrap`.
    pub trapped: bool,
    /// `m_bAlpha`: fading out.
    pub alpha: bool,
    /// `m_nBlendFactorLocal` (255 .. 0).
    pub blend: i32,
    /// `m_nMoverID`: the mover that triggered the ctrl last (the owner of its drops, the target of its trap skills).
    pub trigger_mover: Option<ObjId>,
    /// `m_dwDelete`.
    pub delete_at: Option<GameMs>,
    /// `m_dwCtrlReadyTime`: the open timer.
    pub ready_at: Option<GameMs>,
    /// `m_idExpPlayer` (exp boxes only: the dead path).
    pub exp_player: Option<CharacterId>,
    /// `m_bAction`.
    pub action_done: bool,
    /// Special behaviour.
    pub special: CtrlSpecial,
    /// `CtrlProp.ctrl_kind1 == CK1_DOOR`: a door may be buried below the terrain, so the add-time height clamp
    /// skips it (15 §5.3).
    pub door: bool,
}

impl Ctrl {
    /// A detached ctrl of prop `index` at `pos` facing `angle` (the constructor and `Init`): not trapped, not
    /// playing, opaque, nothing pending. The clock is a placeholder until the `_add` registration builds it
    /// from the model.
    pub fn new(index: u32, pos: Vec3, angle: f32, elem: CtrlElemDef, door: bool) -> Ctrl {
        let mut core = ObjCore::new(ObjId::NULL, ObjKind::Ctrl, index, WorldId(0), LayerId(0));
        core.pos = pos;
        core.angle = angle;
        Ctrl {
            core,
            elem,
            anim: CtrlClock::new(0, 0.5, false, false),
            trap_at: None,
            trapped: false,
            alpha: false,
            blend: 255,
            trigger_mover: None,
            delete_at: None,
            ready_at: None,
            exp_player: None,
            action_done: false,
            special: if index == CI_PSYCHICWALL {
                CtrlSpecial::PsychicWall(Box::default())
            } else {
                CtrlSpecial::Common
            },
            door,
        }
    }

    /// `CCommonCtrl::Init` for everything but the clock (the `CK2_KEEP` re-arm): the trap, the fade, the owner of
    /// the last trigger, the timers and the exp-box fields go back to their start values.
    pub fn reinit(&mut self) {
        self.trapped = false;
        self.trap_at = None;
        self.alpha = false;
        self.blend = 255;
        self.trigger_mover = None;
        self.delete_at = None;
        self.ready_at = None;
        self.exp_player = None;
        self.action_done = false;
    }

    /// `SetActionPlay` (clock part): the animation plays on from where it holds.
    pub fn set_action_play(&mut self) {
        self.anim.set_action_play();
    }

    /// The wire form of the ctrl's yaw: `(i16)(angle * 10)` (`ObjCommon.angle_x10`).
    pub fn angle_x10(&self) -> i32 {
        i32::from((self.core.angle * 10.0) as i16)
    }

    /// The wire form of the ctrl's scale: `(u16)(scale * 100)` (`ObjCommon.scale_x100`).
    pub fn scale_x100(&self) -> u32 {
        (self.core.scale * 100.0) as u16 as u32
    }

    /// The LH world matrix (`Mat43`) of the ctrl's collider: what the client builds from `SpawnCtrl`.
    pub fn world_mat43(&self) -> [f32; 12] {
        ctrl_world_mat43(self.core.pos, self.angle_x10(), self.scale_x100())
    }
}

/// The seams of the ctrl code that doc 17 implements.
pub trait CtrlHooks: Send {
    /// `_CreateTrapSkill` for one slot: apply `skill` at `level` from the ctrl (`WUI_NOW` skills) or to the
    /// mover that triggered it, and broadcast `DoApplyUseSkill`. The validity of the skill and its level against
    /// the expert maximum is the hook's; the default does nothing.
    fn trap_skill(
        &mut self,
        _worlds: &mut Worlds,
        _ctrl: ObjId,
        _trigger: Option<ObjId>,
        _skill: u32,
        _level: u32,
    ) {
    }
}

/// The hooks that do nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCtrlHooks;

impl CtrlHooks for NoCtrlHooks {}

/// The holder of the installed [`CtrlHooks`] (`Worlds` is `Debug`, trait objects are not).
pub(crate) struct CtrlHookSlot(pub Option<Box<dyn CtrlHooks>>);

impl Default for CtrlHookSlot {
    fn default() -> Self {
        CtrlHookSlot(Some(Box::new(NoCtrlHooks)))
    }
}

impl std::fmt::Debug for CtrlHookSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CtrlHookSlot(..)")
    }
}

impl Worlds {
    /// Installs doc 17's trap-skill seam.
    pub fn set_ctrl_hooks(&mut self, h: Box<dyn CtrlHooks>) {
        self.ctrl_hooks.0 = Some(h);
    }

    /// The ctrl of `key` in `(world, layer)` (`g_MapStrToObjId.find`, per layer).
    pub fn ctrl_by_key(&self, world: WorldId, layer: LayerId, key: &str) -> Option<ObjId> {
        self.layer(world, layer)?.ctrl_keys.get(key).copied()
    }

    /// The registration of a ctrl at its `_add` (15 §14.7, §14.6): builds its clock from the model and the
    /// `CtrlProp` kinds, registers its collider in the layer and its key in the layer's key map.
    pub(crate) fn ctrl_added(&mut self, world: WorldId, layer: LayerId, id: ObjId) {
        let data = Arc::clone(&self.movement.shared.data);
        let models = Arc::clone(&self.ctrl_models);
        let Some(c) = self.store.ctrl_mut(id) else {
            return;
        };
        let prop = data.ctrls.get(c.core.index);
        let keep = prop.is_some_and(|p| p.ctrl_kind2 == CK2_KEEP);
        let half = prop.is_some_and(|p| p.ctrl_kind3 == CK3_HALF);
        let model = models.get(c.core.index);
        c.anim = match model {
            Some(m) => CtrlClock::new(m.max_frame, m.per_slerp_raw, keep, half),
            None => CtrlClock::new(0, 0.5, keep, half),
        };
        c.anim.autoplay =
            prop.is_some_and(|p| p.ctrl_kind1 == CK1_HOUSING || p.ctrl_kind1 == CK1_GUILD_HOUSE);
        let world_mat = flyff_sim::pose::quat::mat44_from_43(&c.world_mat43());
        let add = AddCollider {
            id,
            model: c.core.index,
            world: world_mat,
            aabb: None,
            seq: u64::from(id.0),
            frame: c.anim.frame(),
            next: c.anim.next_frame(),
        };
        let key = c.elem.ctrl_key.clone();
        if let Some(l) = self.layer_mut(world, layer) {
            // a registration for an unknown model is counted by the set (`without_model`) and has no collider
            Arc::make_mut(&mut l.dyn_colliders).add(add);
            if !key.is_empty() {
                l.ctrl_keys.entry(key).or_insert(id);
            }
        }
    }

    /// The unregistration at destroy: the collider and the key (only when the key points at this ctrl).
    pub(crate) fn ctrl_removed(&mut self, world: WorldId, layer: LayerId, id: ObjId) {
        let key = self
            .store
            .ctrl(id)
            .map(|c| c.elem.ctrl_key.clone())
            .unwrap_or_default();
        if let Some(l) = self.layer_mut(world, layer) {
            if l.dyn_colliders.get(id).is_some() {
                Arc::make_mut(&mut l.dyn_colliders).remove(id);
            }
            if !key.is_empty() && l.ctrl_keys.get(&key) == Some(&id) {
                l.ctrl_keys.remove(&key);
            }
        }
    }
}
