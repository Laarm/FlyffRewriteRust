//! `.dyo` placement: NPCs and ctrls with the event-flag filters; `NpcLocator` (Plan/15 §13, WP-WORLD-08;
//! ref/06 §9.6; F-096, F-176, F-298).
//!
//! [`Worlds::load_dyo`] is `CWorld::LoadObject(layer)` (`src:_Common/WorldFile.cpp:296-381`): the records of the
//! folder's parsed `.dyo` ([`WorldStatic::dyo`](super::WorldStatic::dyo), shared, so a layer created later re-uses
//! the parse) in file order:
//!
//! * `OT_MOVER`: the character key is looked up in `character.inc`; [`usable_dyo`] is `IsUsableDYO` / `IsUsableDYO2`
//!   (the event-flag table of 13.2 and the language filter); an accepted record becomes an NPC mover
//!   (`Mover::new_npc`, belligerence from the record, scale and pitch/roll from the base) at the position
//!   `ObjBase::absolute_position(None)` (which already applies the x4 of `OLD_MPU`; **not** multiplied again) and
//!   goes through `spawn_mover` (the add-time clamp). The first mover of a character key of the startup placement
//!   is recorded in the [`NpcLocator`] (the quest helper, ref/06 §9.6);
//! * `OT_CTRL`: an inert [`Ctrl`] with the record's element (WP-WORLD-18 adds its behaviour);
//! * `OT_ITEM` records (none in the shipped data) are skipped with a warning (**DROP**).
//!
//! Flags are read **at placement time** through [`EventFlagQuery`], so changing an event flag at runtime does not
//! add or remove NPCs of existing layers (KEEP); the next layer created reflects the new value.

use super::Worlds;
use super::catalog::WorldStatic;
use super::ctrl::Ctrl;
use crate::live::LiveConfig;
use crate::mover::{Mover, npc_parts};
use flyff_common::config::EventId;
use flyff_common::defs::itemkind::CK1_DOOR;
use flyff_common::defs::lang::LANG_ENG;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{LayerId, WorldId};
use flyff_data::GameData;
use flyff_data::character::NpcDef;
use flyff_data::worlds::CtrlElemDef;
use flyff_formats::objects::{OT_CTRL, OT_ITEM, OT_MOVER};
use std::collections::BTreeMap;

/// What the NPC filters read: the state of an event slot (`g_eLocal.GetState(EVE_*)`).
pub trait EventFlagQuery {
    /// `true` when the event `id` is on.
    fn is_on(&self, id: EventId) -> bool;
}

/// The default [`EventFlagQuery`]: the `[events]` projection in doc 10's `LiveConfig` (03 §5), with the
/// `events.flag.<id>` overrides. Doc 22's `EventFlags` replaces it once it exists.
#[derive(Clone, Copy, Debug)]
pub struct ConfigEventFlags<'a>(pub &'a LiveConfig);

impl EventFlagQuery for ConfigEventFlags<'_> {
    fn is_on(&self, id: EventId) -> bool {
        self.0.event_state(id) != 0
    }
}

/// A fixed set of switched-on events (tests, tools).
#[derive(Clone, Debug, Default)]
pub struct FixedEventFlags(pub std::collections::BTreeSet<u16>);

impl FixedEventFlags {
    /// The set with exactly these events on.
    pub fn with(ids: &[EventId]) -> FixedEventFlags {
        FixedEventFlags(ids.iter().map(|i| i.id()).collect())
    }
}

impl EventFlagQuery for FixedEventFlags {
    fn is_on(&self, id: EventId) -> bool {
        self.0.contains(&id.id())
    }
}

/// The event slot that gates a character key (13.2 table), `None` for an ungated key. Keys compare
/// case-insensitively. `MaFl_HairShop` / `MaFl_FaceOff` are only removed for the TWN and HK languages, never for
/// English (03 S3).
pub fn gating_event(key: &str) -> Option<EventId> {
    let k = key.to_ascii_lowercase();
    match k.as_str() {
        "npc_reward" | "mama_pknpc01" => Some(EventId::Pk),
        "mafl_guildwar" | "mafl_donaris" => Some(EventId::GuildCombat),
        "mafl_annie" | "mafl_amos" => Some(EventId::GuildCombat1to1),
        "mafl_ray" => Some(EventId::Arena),
        "mafl_secretroom_east" | "mada_secretroom_west" => Some(EventId::SecretRoom),
        "mafl_rainbowstart" => Some(EventId::RainbowRace),
        _ => None,
    }
}

/// `IsUsableDYO2` (`__CHIPI_DYO`): `SetLang` entries and `SetOutput`. No entries: `output`; an entry equal to
/// `(LANG_ENG, 0)`: `output`; otherwise `!output`.
pub fn language_allows(def: &NpcDef) -> bool {
    if def.languages.is_empty() {
        return def.output;
    }
    if def
        .languages
        .iter()
        .any(|&(lang, sub)| u32::from(lang) == LANG_ENG && sub == 0)
    {
        return def.output;
    }
    !def.output
}

/// `IsUsableDYO(mover)`: the event-flag filter of the character key, then the language filter. A key without an
/// `NpcDef` is accepted.
pub fn usable_dyo(key: &str, def: Option<&NpcDef>, flags: &dyn EventFlagQuery) -> bool {
    let Some(def) = def else {
        return true;
    };
    if let Some(ev) = gating_event(key)
        && !flags.is_on(ev)
    {
        return false;
    }
    language_allows(def)
}

/// `character key (lower-case) -> (WorldId, position)` of the startup placement; the last write wins in
/// placement order (ascending world id, layer 0 then the ticket layers). The quest helper
/// (`QuestHelperReqNpcPos`, doc 20) reads it (15 §13.3).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NpcLocator {
    map: BTreeMap<String, (WorldId, Vec3)>,
}

impl NpcLocator {
    /// Records (or overwrites) the position of a character key.
    pub fn record(&mut self, key: &str, world: WorldId, pos: Vec3) {
        self.map.insert(key.to_ascii_lowercase(), (world, pos));
    }

    /// The recorded place of `key` (case-insensitive).
    pub fn get(&self, key: &str) -> Option<(WorldId, Vec3)> {
        self.map.get(&key.to_ascii_lowercase()).copied()
    }

    /// Number of recorded keys.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `true` when nothing is recorded.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// What one placement did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DyoPlaced {
    /// NPC movers placed.
    pub npcs: u32,
    /// Ctrls placed.
    pub ctrls: u32,
    /// Movers refused by the event or language filter.
    pub filtered: u32,
    /// Records skipped: a mover prop that does not exist, an `OT_ITEM` record, a placement the world refused.
    pub skipped: u32,
}

/// `flyff_formats` ctrl element to the `GameData` type (the two crates keep separate copies, 14 §6.9).
pub fn ctrl_elem(c: &flyff_formats::objects::CtrlElemDef) -> CtrlElemDef {
    CtrlElemDef {
        set: c.set,
        set_item: c.set_item,
        set_level: c.set_level,
        set_quest_num: c.set_quest_num,
        set_flag_num: c.set_flag_num,
        set_gender: c.set_gender,
        set_job: c.set_job,
        set_endu: c.set_endu,
        min_item_num: c.min_item_num,
        max_item_num: c.max_item_num,
        inside_item_kind: c.inside_item_kind,
        inside_item_per: c.inside_item_per,
        mon_res_kind: c.mon_res_kind,
        mon_res_num: c.mon_res_num,
        mon_act_attack: c.mon_act_attack,
        trap_oper_type: c.trap_oper_type,
        trap_random_per: c.trap_random_per,
        trap_delay: c.trap_delay,
        trap_kind: c.trap_kind,
        trap_level: c.trap_level,
        link_ctrl_key: c.link_ctrl_key.to_string_lossy_ascii(),
        ctrl_key: c.ctrl_key.to_string_lossy_ascii(),
        set_quest_num1: c.set_quest_num1,
        set_flag_num1: c.set_flag_num1,
        set_quest_num2: c.set_quest_num2,
        set_flag_num2: c.set_flag_num2,
        set_item_count: c.set_item_count,
        tele_world_id: c.tele_world_id,
        tele_x: c.tele_x,
        tele_y: c.tele_y,
        tele_z: c.tele_z,
    }
}

fn vec3(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}

/// A scale from a record: a non-finite or non-positive value is 1.
fn sane_scale(s: f32) -> f32 {
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

impl Worlds {
    /// `CWorld::LoadObject(layer)` (15 §13.1): places the accepted records of the world's `.dyo` into
    /// `(world, layer)`; `locate` records the NPC positions in the [`NpcLocator`] (the startup placement only).
    /// Returns what was placed; a missing world or layer places nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn load_dyo(
        &mut self,
        world: WorldId,
        layer: LayerId,
        data: &GameData,
        flags: &dyn EventFlagQuery,
        rng: &mut dyn GameRng,
        locate: bool,
    ) -> DyoPlaced {
        let mut placed = DyoPlaced::default();
        let Some(statics) = self.world(world).map(|w| std::sync::Arc::clone(&w.statics)) else {
            return placed;
        };
        for rec in &statics.dyo.records {
            match rec.obj_type {
                OT_MOVER => {
                    self.place_npc(world, layer, rec, data, flags, rng, locate, &mut placed)
                }
                OT_CTRL => self.place_ctrl(world, layer, rec, data, &mut placed),
                OT_ITEM => {
                    // a `CItem` without an item would crash the original serializer (15 §13.1)
                    tracing::warn!(world = world.0, "dyo: an OT_ITEM record is skipped");
                    placed.skipped += 1;
                }
                _ => {}
            }
        }
        placed
    }

    #[allow(clippy::too_many_arguments)]
    fn place_npc(
        &mut self,
        world: WorldId,
        layer: LayerId,
        rec: &flyff_formats::objects::ObjRecord,
        data: &GameData,
        flags: &dyn EventFlagQuery,
        rng: &mut dyn GameRng,
        locate: bool,
        placed: &mut DyoPlaced,
    ) {
        let Some(ext) = rec.mover() else {
            return;
        };
        let key = ext.character_key.to_string_lossy_ascii();
        let def = if key.is_empty() { None } else { data.npc(&key) };
        if !usable_dyo(&key, def, flags) {
            placed.filtered += 1;
            return;
        }
        let Some(prop) = data.mover(rec.base.index) else {
            tracing::warn!(
                world = world.0,
                prop = rec.base.index,
                key = %key,
                "dyo: the mover prop does not exist; the record is skipped"
            );
            placed.skipped += 1;
            return;
        };
        let pos = vec3(rec.base.absolute_position(None));
        let (mut core, mut sim) = npc_parts(data, prop, pos, rec.base.angle);
        // the record's scale replaces the registry default (`Read()` after `ResetScale()`); the sim carries the same value
        core.scale = sane_scale(rec.base.scale[0]);
        sim.params.scale = core.scale;
        core.angle_x = rec.base.axis[0];
        core.angle_z = rec.base.axis[2];
        let mut mover = Mover::new_npc(core, sim, prop, def, data, rng);
        if let crate::mover::MoverKind::Npc(n) = &mut mover.kind {
            n.belligerence = ext.belligerence;
            if n.character_key.is_empty() {
                n.character_key = key.clone();
            }
        }
        match self.spawn_mover(world, layer, mover) {
            Ok(id) => {
                placed.npcs += 1;
                if locate && !key.is_empty() {
                    // the spawn clamped the height: the located position is the object's
                    let at = self.core(id).map_or(pos, |c| c.pos);
                    self.npc_locator.record(&key, world, at);
                }
            }
            Err(e) => {
                tracing::warn!(world = world.0, key = %key, "dyo: NPC not placed: {e}");
                placed.skipped += 1;
            }
        }
    }

    fn place_ctrl(
        &mut self,
        world: WorldId,
        layer: LayerId,
        rec: &flyff_formats::objects::ObjRecord,
        data: &GameData,
        placed: &mut DyoPlaced,
    ) {
        let Some((_version, elem)) = rec.ctrl() else {
            return;
        };
        let door = data
            .ctrls
            .get(rec.base.index)
            .is_some_and(|p| p.ctrl_kind1 == CK1_DOOR);
        let mut c = Ctrl::new(
            rec.base.index,
            vec3(rec.base.absolute_position(None)),
            rec.base.angle,
            ctrl_elem(elem),
            door,
        );
        c.core.scale = sane_scale(rec.base.scale[0]);
        match self.spawn_ctrl(world, layer, c) {
            Ok(_) => placed.ctrls += 1,
            Err(e) => {
                tracing::warn!(world = world.0, "dyo: ctrl not placed: {e}");
                placed.skipped += 1;
            }
        }
    }
}

/// Data check `C-W02` (15 §13.3, 14 §9.3): every `.dyo` mover whose character key is not empty must resolve in
/// `GameData.npcs`. Returns one warning text per folder and unknown key (the original silently placed such an
/// NPC without a character definition).
pub fn check_c_w02(data: &GameData, catalog: &super::WorldCatalog) -> Vec<String> {
    let mut seen: Vec<*const WorldStatic> = Vec::new();
    let mut out = Vec::new();
    for e in catalog.entries() {
        let ptr = std::sync::Arc::as_ptr(&e.statics);
        if seen.contains(&ptr) {
            continue;
        }
        seen.push(ptr);
        let mut missing: Vec<String> = Vec::new();
        for rec in &e.statics.dyo.records {
            if rec.obj_type != OT_MOVER {
                continue;
            }
            let Some(m) = rec.mover() else { continue };
            let key = m.character_key.to_string_lossy_ascii();
            if !key.is_empty() && data.npc(&key).is_none() && !missing.contains(&key) {
                missing.push(key);
            }
        }
        for key in missing {
            out.push(format!(
                "C-W02: {}: the .dyo mover key '{key}' is not in character.inc",
                e.statics.folder
            ));
        }
    }
    out
}
