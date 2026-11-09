//! `teleport16.inc`: the teleporter window of the repack (`CProject::LoadTeleportInfo`, Plan/14 §6.16, F13;
//! src:_Common/Project.cpp:5658-5715).
//!
//! One row per destination: `WORLD X Y Z NAME DESC "image.png" BOSS MINLV MAXLV CLASS COOLDOWN` (17 rows).
//! The server keeps the world and the position, the client the name, description, image, boss, levels,
//! class and cool-down; the loader keeps all of them. **FIX (F13):** the original loop tests `tok !=
//! FINISHED` before reading and so appends a phantom 18th entry; it is not stored. The client rule
//! `max_level < min_level -> MAX_LEGEND_LEVEL` (129) is applied by the bundle export, not here
//! ([`Teleport16::effective_max_level`]).
//!
//! `boss_drops` is the list of boss drops the teleporter window shows. The original fills it in a second
//! `DropItem` branch of `LoadPropMoverEx` that can never run (the first branch consumes the statement), so the
//! list is always empty there; **FIX (no data impact, every row of the shipped file has boss 0):** it is
//! computed with the intended filter by [`fill_boss_drops`] when `propMoverEx.inc` has been read.

use crate::props::item::ItemProp;
use crate::props::mover::MoverProp;
use crate::types::{IdTable, ItemId, MoverId, WorldId};
use flyff_common::defs::itemkind;
pub use flyff_common::defs::job::MAX_LEGEND_LEVEL;
use serde::{Deserialize, Serialize};

/// One teleporter entry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Teleport16 {
    /// The destination world.
    pub world: WorldId,
    /// The destination position.
    pub pos: [f32; 3],
    /// The name (IDS key resolved).
    pub name: String,
    /// The description (IDS key resolved).
    pub desc: String,
    /// The preview image (a bare file name such as `tp_flaris.png`).
    pub image: String,
    /// The boss mover shown in the window (0 = none).
    pub boss: MoverId,
    /// Minimum level.
    pub min_level: u32,
    /// Maximum level as written (the client turns `max < min` into 129).
    pub max_level: u32,
    /// The class column.
    pub class: u32,
    /// The cool-down.
    pub cooldown: u32,
    /// The drops of the boss that the window lists (empty when `boss == 0`); see the module documentation.
    #[serde(default)]
    pub boss_drops: Vec<ItemId>,
}

impl Teleport16 {
    /// The maximum level as the client uses it: `max_level < min_level` means "no upper limit"
    /// ([`MAX_LEGEND_LEVEL`]).
    pub fn effective_max_level(&self) -> u32 {
        if self.max_level < self.min_level {
            MAX_LEGEND_LEVEL
        } else {
            self.max_level
        }
    }
}

/// Whether the teleporter window lists a boss drop (the filter of the original's dead client branch): not
/// eggs, ultimate jewels, quest gems or element cards, not food, potions or refreshers, not armour of level
/// 105 or less (a shield is kept), and weapons only when unique or ultimate.
pub fn lists_boss_drop(p: &ItemProp) -> bool {
    if matches!(
        p.item_kind3,
        itemkind::IK3_EGG | itemkind::IK3_ULTIMATE | itemkind::IK3_GEM | itemkind::IK3_ELECARD
    ) || matches!(
        p.item_kind2,
        itemkind::IK2_FOOD | itemkind::IK2_POTION | itemkind::IK2_REFRESHER
    ) {
        return false;
    }
    if p.item_kind1 == itemkind::IK1_ARMOR
        && p.item_lv <= 105
        && p.item_kind3 != itemkind::IK3_SHIELD
    {
        return false;
    }
    if p.item_kind1 == itemkind::IK1_WEAPON {
        return p.refer_stat1 == itemkind::WEAPON_UNIQUE || p.is_ultimate();
    }
    true
}

/// Fills `boss_drops` of every row from the `DropItem` statements of its boss in `propMoverEx.inc` (call it
/// after L32 and before the `propDropEvent.inc` rows are appended, which the original's branch never saw).
/// Items that do not exist are skipped; duplicates are listed once.
pub fn fill_boss_drops(
    rows: &mut [Teleport16],
    movers: &IdTable<MoverProp>,
    items: &IdTable<ItemProp>,
) {
    for row in rows {
        row.boss_drops.clear();
        if row.boss == 0 {
            continue;
        }
        let Some(boss) = movers.get(row.boss) else {
            continue;
        };
        for di in &boss.ex.drops.items {
            if di.dtype != crate::mover_ex::DropType::Normal {
                continue;
            }
            if let Some(p) = items.get(di.item)
                && lists_boss_drop(p)
                && !row.boss_drops.contains(&di.item)
            {
                row.boss_drops.push(di.item);
            }
        }
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_teleport16;

#[cfg(feature = "load")]
mod parse {
    use super::Teleport16;
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::sync::Arc;

    /// `LoadTeleportInfo` over decoded `text`; a row that runs into the end of the file is dropped.
    pub(crate) fn parse_teleport16(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<Teleport16> {
        let mut out = Vec::new();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        loop {
            let world = sc.get_number();
            if sc.finished() {
                break;
            }
            let pos = [sc.get_float(), sc.get_float(), sc.get_float()];
            sc.get_token();
            let name = sc.token().to_owned();
            sc.get_token();
            let desc = sc.token().to_owned();
            sc.get_token();
            let image = sc.token().to_owned();
            let row = Teleport16 {
                world,
                pos,
                name,
                desc,
                image,
                boss: sc.get_number(),
                min_level: sc.get_number(),
                max_level: sc.get_number(),
                class: sc.get_number(),
                cooldown: sc.get_number(),
                boss_drops: Vec::new(),
            };
            if sc.finished() {
                break;
            }
            out.push(row);
        }
        out
    }
}
