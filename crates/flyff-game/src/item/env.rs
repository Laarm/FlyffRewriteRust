//! The read-only environment of the item functions (Plan/19 §23): the loaded game data, the stats
//! environment and the switches this area reads.
//!
//! [`ItemCore`] is built once (`Game::new`) and shared: `Services.items` holds a handle for the handlers, the
//! pickup hook of the world phase owns another (the tick tail has no `Services`, like `StatsHooks`). The
//! runtime-class switch `events.drop_item_remove` is not here: it is read from `LiveConfig` per call.

use crate::stats::StatsCore;
use flyff_common::UnixTime;
use flyff_common::config::RulesConfig;
use flyff_data::GameData;
use std::sync::Arc;

/// The switches of `[features]`, `[gameplay]` and `[fidelity]` the item code reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemConfig {
    /// `features.only_armor_awakes`.
    pub only_armor_awakes: bool,
    /// `gameplay.max_pack_item`.
    pub max_pack_item: u32,
    /// `features.auto_perin_converter`.
    pub auto_perin_converter: bool,
    /// `features.multiple_item_picker`.
    pub multiple_item_picker: bool,
    /// `features.improved_pets`.
    pub improved_pets: bool,
    /// `features.inventory_sorting`.
    pub inventory_sorting: bool,
    /// `features.perm_extrabags`.
    pub perm_extrabags: bool,
    /// `features.no_piercing`.
    pub no_piercing: bool,
    /// `fidelity.gold_update_self_only` (PL-075).
    pub gold_self_only: bool,
    /// `fidelity.flying_drop_overflow_to_ground` (PL-071).
    pub flying_drop_overflow_to_ground: bool,
    /// `features.new_skill_attribute`: Master's Blessing grants a `DST_IGNORE_DMG` shield (17 §6.10).
    pub new_skill_attribute: bool,
    /// `features.better_impowerweapon`: Impower Weapon needs no element weapon (17 §7.6).
    pub better_impowerweapon: bool,
    /// `features.buff_pang_update`: the NPC buffer gives the class buffs (17 §7.11).
    pub buff_pang_update: bool,
    /// `fidelity.item_gm_commands`: the item-triggered GM commands run (PL-114).
    pub item_gm_commands: bool,
    /// `features.better_safe_upgrades`: the repack smelt materials (`IK3_SMELT_*`) take part in `Enchant` and
    /// `SmeltSafetyReq` (Plan/19 §12.1).
    pub better_safe_upgrades: bool,
    /// `features.improved_stat_scroll`: the higher tiers of the random stat scroll (Plan/19 §12.6.1).
    pub improved_stat_scroll: bool,
    /// `features.randomoption_retry`: the Scroll of Augmentation works (Plan/19 §12.6.7).
    pub randomoption_retry: bool,
    /// `gameplay.upgrade_min_interval_ms`: the pacing of the upgrade attempts (PL-076).
    pub upgrade_min_interval_ms: u32,
    /// `features.no_pet_life`: a system pet's life reads 1 and it never dies (19 §13.2).
    pub no_pet_life: bool,
    /// `features.pet_auto_level_up`: the tamer's mistake levels the pet up again at once (19 §13.4).
    pub pet_auto_level_up: bool,
    /// `features.sys_petleveling_mobkill`: the pets gain EXP by kills; the per-minute EXP and the feeding of
    /// eggs are off (19 §13.3).
    pub sys_petleveling_mobkill: bool,
    /// `features.reactivate_eatpet`: a killed owner's pets are dismissed (19 §13.5).
    pub reactivate_eatpet: bool,
    /// `features.pet_filter`: the pet loot filter (19 §6.8).
    pub pet_filter: bool,
    /// `gameplay.pet_base_exp`: the base pet EXP per kill.
    pub pet_base_exp: f32,
    /// `fidelity.fix_vis_slot_expansion` (PL-070): the vis key adds a slot.
    pub fix_vis_slot_expansion: bool,
}

impl ItemConfig {
    /// The projection of the configuration.
    pub fn from_rules(r: &RulesConfig) -> ItemConfig {
        ItemConfig {
            only_armor_awakes: r.features.only_armor_awakes,
            max_pack_item: r.gameplay.max_pack_item,
            auto_perin_converter: r.features.auto_perin_converter,
            multiple_item_picker: r.features.multiple_item_picker,
            improved_pets: r.features.improved_pets,
            inventory_sorting: r.features.inventory_sorting,
            perm_extrabags: r.features.perm_extrabags,
            no_piercing: r.features.no_piercing,
            gold_self_only: r.fidelity.gold_update_self_only,
            flying_drop_overflow_to_ground: r.fidelity.flying_drop_overflow_to_ground,
            new_skill_attribute: r.features.new_skill_attribute,
            better_impowerweapon: r.features.better_impowerweapon,
            buff_pang_update: r.features.buff_pang_update,
            item_gm_commands: r.fidelity.item_gm_commands,
            better_safe_upgrades: r.features.better_safe_upgrades,
            improved_stat_scroll: r.features.improved_stat_scroll,
            randomoption_retry: r.features.randomoption_retry,
            upgrade_min_interval_ms: r.gameplay.upgrade_min_interval_ms,
            no_pet_life: r.features.no_pet_life,
            pet_auto_level_up: r.features.pet_auto_level_up,
            sys_petleveling_mobkill: r.features.sys_petleveling_mobkill,
            reactivate_eatpet: r.features.reactivate_eatpet,
            pet_filter: r.features.pet_filter,
            pet_base_exp: r.gameplay.pet_base_exp,
            fix_vis_slot_expansion: r.fidelity.fix_vis_slot_expansion,
        }
    }
}

impl Default for ItemConfig {
    /// The repack defaults.
    fn default() -> Self {
        ItemConfig::from_rules(&RulesConfig::default())
    }
}

/// Everything the item functions read that is not on the mover.
#[derive(Debug)]
pub struct ItemCore {
    /// The loaded game data (empty in tests without data).
    pub data: Arc<GameData>,
    /// The stat functions' environment (DST application, gold messages).
    pub stats: Arc<StatsCore>,
    /// The switches.
    pub cfg: ItemConfig,
}

impl ItemCore {
    /// The environment of a running game.
    pub fn new(data: Arc<GameData>, stats: Arc<StatsCore>, rules: &RulesConfig) -> ItemCore {
        ItemCore {
            data,
            stats,
            cfg: ItemConfig::from_rules(rules),
        }
    }

    /// A call environment at wall time `now`.
    pub fn env(&self, now: UnixTime) -> ItemEnv<'_> {
        ItemEnv { core: self, now }
    }
}

impl Default for ItemCore {
    /// No game data, default switches (the `Services` placeholder before `Game::new` replaces it).
    fn default() -> Self {
        ItemCore {
            data: Arc::default(),
            stats: Arc::default(),
            cfg: ItemConfig::default(),
        }
    }
}

/// The game data and switches of one call plus the wall time (keep times are absolute unix seconds).
#[derive(Clone, Copy, Debug)]
pub struct ItemEnv<'a> {
    /// The shared environment.
    pub core: &'a ItemCore,
    /// The wall clock of the tick.
    pub now: UnixTime,
}

impl<'a> ItemEnv<'a> {
    /// The loaded game data.
    pub fn data(&self) -> &'a GameData {
        &self.core.data
    }

    /// The stat functions' environment.
    pub fn stats(&self) -> &'a StatsCore {
        &self.core.stats
    }

    /// The switches.
    pub fn cfg(&self) -> &'a ItemConfig {
        &self.core.cfg
    }
}

/// The item environment as a game service (`Services.items`): a cheap handle to the shared [`ItemCore`].
#[derive(Clone, Debug, Default)]
pub struct ItemService {
    core: Arc<ItemCore>,
    /// The instant-bank NPCs alive and when they go (`ProcInstantBanker`, 180 s).
    instant_banks: Vec<(flyff_common::GameMs, flyff_common::ObjId)>,
    /// The branches of the upgrade handlers that other work packages install (ITEM-13: the safe ultimate
    /// enchant, ITEM-18: the stat bonus of summoned pets). Empty until they do.
    pub upgrade_hooks: super::upgrade::UpgradeHooks,
}

impl ItemService {
    /// A service over `core`.
    pub fn new(core: Arc<ItemCore>) -> ItemService {
        ItemService {
            core,
            instant_banks: Vec::new(),
            upgrade_hooks: super::upgrade::UpgradeHooks {
                // ITEM-13: the Shining Orichalcum branch of the safe smelt
                ultimate_safe: Some(super::ultimate::safe_enchant_hook),
                // ITEM-18: the stat bonus of a summoned pet follows the re-rolled lines of its awakening
                pet_options_reset: Some(super::pets::awaken::options_reset),
                pet_options_set: Some(super::pets::awaken::options_set),
            },
        }
    }

    /// Remembers an instant-bank NPC that disappears at `due`.
    pub fn track_instant_bank(&mut self, due: flyff_common::GameMs, id: flyff_common::ObjId) {
        self.instant_banks.push((due, id));
    }

    /// The instant-bank NPCs whose time is up (removed from the list).
    pub fn take_due_instant_banks(
        &mut self,
        now: flyff_common::GameMs,
    ) -> Vec<flyff_common::ObjId> {
        let mut gone = Vec::new();
        self.instant_banks.retain(|&(due, id)| {
            if now > due {
                gone.push(id);
                false
            } else {
                true
            }
        });
        gone
    }

    /// The environment.
    pub fn core(&self) -> &ItemCore {
        &self.core
    }

    /// A shared handle to the environment (the world hooks keep one).
    pub fn handle(&self) -> Arc<ItemCore> {
        Arc::clone(&self.core)
    }
}
