//! Load normalisation (Plan/13 §17.4, WP-DB-07): `apply_character_load`.
//!
//! The steps, in the order of 13 §17.4:
//!
//! * step 1: JSON upgrade and container validation: done by `flyff-db` before the `CharacterLoad` reaches the game;
//! * step 2: item sanitation with the props: an item of an unknown prop, or a stack count outside `1..=pack_max`, gets
//!   `count = 1` (legacy `CheckValidItem`), logged; the numeric repairs of the row (level, EXP, pools, gold, job)
//!   are `Mover::new_player`'s ([`LoadRepair`]); the `RemoveItemIK3()` rule is doc 19's [`LoadHook`];
//! * step 3: quest normalisation: doc 20's [`LoadHook`];
//! * step 4: GM-only mode bits are stripped from non-GMs (VR-085, 23 §2.5, `ModeFlags::GM_ONLY`);
//! * step 5: position sanity: the join placement (15 §9.1, `world::join`);
//! * steps 6-9: buff caps (17), housing furniture expiry (22), pending removals and grants (13 §18), the notes (21):
//!   hooks of the owning docs, run through [`LoadHook::after_load`] in registration order.

use crate::input::Authority;
use crate::item::ItemContainer;
use crate::mover::{LoadRepair, Mover, NewPlayer};
use flyff_common::GameMs;
use flyff_common::gm::ModeFlags;
use flyff_common::rng::GameRng;
use flyff_data::GameData;
use flyff_db::CharacterLoad;

/// A normalisation step owned by another doc (items 19, quests 20, buffs 17, housing 22, grants/removals 13).
pub trait LoadHook: Send {
    /// Called with the freshly built player, after the steps above. May change any part of it.
    fn after_load(&mut self, player: &mut Mover, load: &CharacterLoad, gd: &GameData);
}

/// What the normalisation did, for the join's log and the audit event `persist.repair`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadReport {
    /// The numeric repairs of `Mover::new_player`.
    pub repairs: Vec<LoadRepair>,
    /// Items whose count was reset to 1 (unknown prop or count out of range).
    pub items_repaired: u32,
    /// GM-only mode bits that were stripped (VR-085).
    pub gm_modes_stripped: u32,
}

/// The tunables of the load normalisation.
#[derive(Clone, Copy, Debug)]
pub struct LoadOptions {
    /// `gameplay.max_pack_item`.
    pub max_pack: u16,
}

impl Default for LoadOptions {
    fn default() -> Self {
        LoadOptions { max_pack: 9999 }
    }
}

/// Step 2 on one container: `true` props are looked up in `gd.items` only when the table is loaded.
fn sanitize_container(c: &mut ItemContainer, gd: &GameData, max_pack: u16) -> u32 {
    let check_props = !gd.items.is_empty();
    let mut n = 0;
    for (_, it) in c.iter_mut() {
        let unknown = check_props && gd.item(it.item_id).is_none();
        let out_of_range = it.count < 1 || it.count > max_pack;
        if (unknown || out_of_range) && it.count != 1 {
            tracing::warn!(
                item = it.item_id,
                count = it.count,
                unknown,
                "load: item count reset to 1"
            );
            it.count = 1;
            n += 1;
        }
    }
    n
}

/// `persist::load::apply_character_load`: builds the player of a `CharacterLoad` and applies the load
/// normalisation. The caller (the join) places it in a world.
pub fn apply_character_load(
    load: &CharacterLoad,
    authority: Authority,
    gd: &GameData,
    now: GameMs,
    rng: &mut dyn GameRng,
    opts: LoadOptions,
    hooks: &mut [Box<dyn LoadHook>],
) -> (NewPlayer, LoadReport) {
    let mut built = Mover::new_player(load, authority, gd, now, rng);
    let mut report = LoadReport {
        repairs: std::mem::take(&mut built.repairs),
        ..LoadReport::default()
    };
    // 2: item sanitation
    if let Some(p) = built.mover.player_mut() {
        report.items_repaired += sanitize_container(&mut p.items.inventory, gd, opts.max_pack);
        for pocket in &mut p.items.pockets {
            report.items_repaired += sanitize_container(&mut pocket.items, gd, opts.max_pack);
        }
        for bank in p.items.banks.iter_mut().flatten() {
            report.items_repaired += sanitize_container(&mut bank.items, gd, opts.max_pack);
        }
    }
    // 4: VR-085
    let before = ModeFlags(built.mover.mv.mode);
    let after = before.strip_gm_only_for(authority);
    if after != before {
        report.gm_modes_stripped = before.bits() & !after.bits();
        built.mover.mv.mode = after.bits();
        built.mover.sim.params.mode = after.bits();
        tracing::info!(
            character = load.identity.id.0,
            stripped = format_args!("{:#x}", report.gm_modes_stripped),
            "load: GM-only mode bits stripped from a non-GM"
        );
    }
    for h in hooks.iter_mut() {
        h.after_load(&mut built.mover, load, gd);
    }
    built.repairs = report.repairs.clone();
    (built, report)
}
