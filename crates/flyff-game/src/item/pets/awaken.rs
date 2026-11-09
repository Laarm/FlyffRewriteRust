//! The stat bonus of a summoned pet while a scroll rewrites the lines of its awakening
//! (`ResetDestParamRandomOptExtension` / `SetDestParamRandomOptExtension` of `CMover`; Plan/19 §12.9, §13).
//!
//! The upgrade handlers of WP-ITEM-11 re-roll the random option of a pet item that is out (a system pet in
//! its egg slot, an eat pet) and need the bonus of the OLD lines gone before the change and the bonus of the NEW
//! lines applied after it. Both functions are installed as [`UpgradeHooks::pet_options_reset`] and
//! [`UpgradeHooks::pet_options_set`] by [`crate::item::ItemService::new`]; before the integration of B7 nobody
//! installed them and the lines of a summoned pet were only refreshed by the next summon.
//!
//! * a system pet keeps its lines under [`DstSource::Pet`] (`system::summon`), next to the `BUFF_PET` entry of
//!   its kind, which a re-roll does not touch;
//! * an eat pet keeps them under [`EAT_PET_SOURCE`] (`eat::on_summoned`).
//!
//! Both are ledger sources, so "reset" reverts exactly what was applied and "set" applies whatever the item
//! holds now; an item that is not out (or no longer holds lines) is left alone.
//!
//! [`UpgradeHooks::pet_options_reset`]: crate::item::upgrade::UpgradeHooks::pet_options_reset
//! [`UpgradeHooks::pet_options_set`]: crate::item::upgrade::UpgradeHooks::pet_options_set

use super::eat::EAT_PET_SOURCE;
use super::system;
use crate::buff::core::BuffCx;
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::upgrade::UpgradeCx;
use crate::mover::Mover;
use crate::stats::dst::DstSource;
use crate::stats::source::{apply_source, remove_source};
use flyff_rules::consts::NULL_CHGPARAM;

/// Which of the two summoned pets of `m` the item `id` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Out {
    /// The summoned eat pet.
    Eat,
    /// The summoned system pet.
    System,
}

fn out(m: &Mover, id: StorageId) -> Option<Out> {
    let items = &m.player()?.items;
    if items.summoned_eat_pet.is_some_and(|(i, _)| i == id) {
        Some(Out::Eat)
    } else if items.pet_obj_id == Some(id) {
        Some(Out::System)
    } else {
        None
    }
}

/// `ResetDestParamRandomOptExtension`: the bonus of the lines the pet item holds now goes (before they are
/// replaced).
pub fn options_reset(m: &mut Mover, cx: &UpgradeCx<'_>, id: StorageId, fx: &mut Fx) {
    let bcx = BuffCx::new(*cx.env, cx.now);
    match out(m, id) {
        Some(Out::Eat) => remove_source(m, bcx.stats(), EAT_PET_SOURCE, true, &mut fx.out),
        Some(Out::System) => remove_source(m, bcx.stats(), DstSource::Pet, true, &mut fx.out),
        None => {}
    }
}

/// `SetDestParamRandomOptExtension`: the bonus of the lines the pet item holds now (after they changed).
pub fn options_set(m: &mut Mover, cx: &UpgradeCx<'_>, id: StorageId, fx: &mut Fx) {
    let bcx = BuffCx::new(*cx.env, cx.now);
    match out(m, id) {
        Some(Out::Eat) => {
            // once: a second call without a reset in between must not stack the lines
            if m.mv.dst.has(EAT_PET_SOURCE) {
                return;
            }
            let lines: Vec<(u32, i32, i32)> = m
                .player()
                .and_then(|p| p.items.inventory.get_at_id(id))
                .map(|it| {
                    it.random_option
                        .lines()
                        .map(|l| (u32::from(l.dst), i32::from(l.adj), NULL_CHGPARAM))
                        .collect()
                })
                .unwrap_or_default();
            if !lines.is_empty() {
                apply_source(m, bcx.stats(), EAT_PET_SOURCE, &lines, true, &mut fx.out);
            }
        }
        Some(Out::System) => system::summon::apply_lines(m, &bcx, id, fx),
        None => {}
    }
}

/// Is `id` the item of a summoned pet (`IsUsing` of the awakening code)?
pub fn is_out(m: &Mover, id: StorageId) -> bool {
    out(m, id).is_some()
}
