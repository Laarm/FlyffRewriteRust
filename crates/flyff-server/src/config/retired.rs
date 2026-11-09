//! Retired and renamed key names (Plan/10 §8.4, stage1-review §8, 03 §4.10). A retired key in the file, the
//! environment or an override produces a warning naming the canonical key; its value is ignored.
//!
//! Withdrawn draft names that never shipped (`net.trust_forwarded_for`, `assets.startup_threads`) are NOT
//! listed: they are plain unknown keys.

/// `(retired key, canonical key)`; `None` = the setting has no replacement (always on, or dropped).
pub static RETIRED_KEYS: &[(&str, Option<&str>)] = &[
    // stage1-review §8 "Retired key names".
    ("net.unauth_timeout_s", Some("net.timeout_hello_s")),
    ("net.not_in_world_timeout_s", Some("net.timeout_entering_s")),
    ("net.max_outbound_bytes", Some("net.outbound_queue_bytes")),
    ("session.logout_delay_s", Some("gameplay.timewait_close_s")),
    ("net.reattach_window_s", Some("gameplay.timewait_close_s")),
    ("features.inventory_size", Some("features.inventory_168")),
    ("features.perm_extra_bags", Some("features.perm_extrabags")),
    ("features.add_shop_item", Some("features.addshopitem")),
    ("gameplay.max_stack", Some("gameplay.max_pack_item")),
    ("gameplay.cs_item_price", Some("gameplay.all_cs_price")),
    ("gameplay.pet_item_price", Some("gameplay.all_pet_price")),
    ("rates.guild_exp_divisor", Some("gameplay.guild_rate")),
    ("rates.couple_exp_divisor", Some("gameplay.couple_rate")),
    ("rates.exp", Some("rates.monster_exp")),
    // ref/23 §7.12 proposals (03 §4.10).
    ("rates.drop", Some("rates.item_drop")),
    ("rates.gold", Some("rates.gold_drop")),
    ("rates.spawn", Some("gameplay.increased_spawn")),
    ("rates.pet_base_exp", Some("gameplay.pet_base_exp")),
    ("gameplay.max_lord_events", Some("gameplay.max_lord_event")),
    ("gameplay.name_min_len", Some("gameplay.min_name_letters")),
    ("gameplay.name_max_len", Some("gameplay.max_name_letters")),
    (
        "gameplay.guild_kill_contribution",
        Some("gameplay.guild_kill_contri"),
    ),
    ("gameplay.max_block", Some("gameplay.max_block_value")),
    ("gameplay.max_block_gw", Some("gameplay.max_block_value_gw")),
    ("gameplay.max_speed", Some("gameplay.set_max_speed")),
    ("gameplay.max_cast_speed", Some("gameplay.set_max_cast")),
    ("gameplay.max_crit", Some("gameplay.set_max_crit")),
    ("gm.admin_title", Some("gameplay.admin_title")),
    ("gm.gm_title", Some("gameplay.gm_title")),
    ("gm.name_color", Some("gameplay.gm_name_color")),
    ("server.welcome_messages", Some("gameplay.welcome_messages")),
    ("client.enchant_wait_s", Some("gameplay.enchant_time_s")),
    ("world.respawn", Some("events.respawn")),
    ("events.adult_18", Some("events.eve_18")),
    // ref/11 §13.4.
    (
        "features.improved_block",
        Some("features.improved_block_calculation"),
    ),
    ("features.dmg_correct_block", Some("features.dmgcorrect")),
    ("features.yoyo_splash", Some("features.splash_autoattacks")),
    (
        "features.no_arrows_consumed",
        Some("features.no_arrows_poster"),
    ),
    ("features.bow_dmg_propjob", Some("features.bowdmg_propjob")),
    ("features.pk_enabled", Some("events.pk")),
    ("features.steal_allowed", Some("events.steal")),
    // ref/13 port notes.
    (
        "features.pet_level_by_kills",
        Some("features.sys_petleveling_mobkill"),
    ),
    ("features.auto_perin", Some("features.auto_perin_converter")),
    ("gameplay.max_pack", Some("gameplay.max_pack_item")),
    // ref/14 §A.5.
    ("rates.party_exp_rate", Some("rates.party_exp")),
    ("gameplay.guild_pxp_divisor", Some("gameplay.guild_rate")),
    ("gameplay.couple_exp_divisor", Some("gameplay.couple_rate")),
    (
        "gameplay.guild_rejoin_cooldown_s",
        Some("features.remove_guild_cooldown"),
    ),
    (
        "gameplay.couple_propose_cooldown_s",
        Some("features.no_couple_cd"),
    ),
    (
        "gameplay.same_sex_couples",
        Some("features.gay_and_lesbian"),
    ),
    ("gameplay.party_finder", Some("features.party_find")),
    // ref/16 P4: VR-060 is always on, there is no switch.
    ("dialog.strict_validation", None),
    // ref/02 port 15.
    ("login.bank_pin_on_enter", Some("auth.second_password")),
];

/// The canonical replacement of a retired key; `Some(None)` when the key is retired without a
/// replacement, `None` when it is not retired.
pub fn retired(key: &str) -> Option<Option<&'static str>> {
    RETIRED_KEYS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, c)| *c)
}
