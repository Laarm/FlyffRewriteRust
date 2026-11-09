//! THE key table of the five sections owned by Plan/03 (WP-CORE-90): one row per key of §4.2–§4.7 and
//! §5.2, in plan order. One invocation of `config_tables!` generates the section structs, their
//! defaults, `KEY_REGISTRY`, the `SimFlags` and client projections and `RulesConfig` (see
//! `macros.rs`). To add a key add one row here; nothing else needs a second list.
//!
//! Row syntax: `name: Type = default ; Class Side client|- reserved|- ; "legacy" ; "owner" ; ledger ; "doc" ;`
//! (`client` = the C flag, `reserved` = accepted but not implemented). `SimFlags` collects the rows whose
//! side is `Sim`.

// The key descriptions are plain text for the generated docs; `[section]` in them is not a link.
#![allow(rustdoc::broken_intra_doc_links)]

use super::fidelity::HitAttrRule;
use super::registry::{KeyClass, KeyMeta, KeySide};

config_tables! {
    features Features ClientFeatures {
        additional_target_info: bool = true; Startup Client client -; "ServerCommon.h:4 __ADDITIONAL_TARGET_INFO"; "45-client-game-screens"; None; "target bar extra info and job icons";
        addshopitem: bool = true; Startup Server - -; "ServerCommon.h:5 __ADDSHOPITEM"; "14-game-data-and-vfs/19-items-and-economy"; Some("KQ-10"); "AddShopItem(slot, item[, price]) in character.inc honoured; price overrides dwCost globally (KQ-10)";
        around_buff: bool = true; Startup Server - -; "ServerCommon.h:6 __AROUND_BUFF"; "23-gm-admin-chat"; None; "GM /arb buffs players within 7 m";
        auto_perin_converter: bool = true; Startup Server - -; "ServerCommon.h:7 __AUTO_PERIN_CONVERTER"; "19-items-and-economy"; None; "gold > 2 100 000 000 → 21 Perins";
        auto_skill_max: bool = true; Startup Both client -; "ServerCommon.h:8 __AUTO_SKILL_MAX"; "16-movers-stats-progression"; None; "job skills set to dwExpertMax on job change";
        better_armor_bonus: bool = true; Startup Both client -; "ServerCommon.h:9 __BETTER_ARMOR_BONUS"; "16-movers-stats-progression"; None; "armour +N bonus 4th column = DST_ATKPOWER_RATE (not DST_ADDMAGIC)";
        better_berserk: bool = true; Startup Both client -; "ServerCommon.h:10 __BETTER_BERSERK"; "17-combat-skills-buffs"; None; "skills usable while Berserk";
        better_buff_display: bool = true; Startup Client client -; "ServerCommon.h:11 __BETTER_BUFF_DISPLAY"; "45-client-game-screens"; None; "buff timers as H/M/S";
        better_holycross: bool = true; Startup Server - -; "ServerCommon.h:12 __BETTER_HOLYCROSS"; "17-combat-skills-buffs"; None; "Holy Cross (CHS_DOUBLE) not consumed by a hit";
        better_clockwork_alpha: bool = true; Startup Both client -; "ServerCommon.h:13 __BETTER_CLOCKWORK_ALPHA"; "18-movement-and-ai/41-client-rendering/44-client-audio"; None; "ClockWorks boss tweaks (no level penalty; ATK3 handling); client: MI_LOADCLOCKWORK laser/charge visuals, landing quake and sound path (18 Q14, 44 OQ11)";
        better_couplebuffs: bool = true; Startup Server - -; "ServerCommon.h:14 __BETTER_COUPLEBUFFS"; "21-social-systems"; None; "Miracle couple buff stats from propItem; hard-coded ×1.05 drop removed";
        better_impowerweapon: bool = true; Startup Both client -; "ServerCommon.h:15 __BETTER_IMPOWERWEAPON"; "17-combat-skills-buffs"; None; "Impower Weapon without element weapon";
        better_link_attack: bool = true; Startup Server - -; "ServerCommon.h:16 __BETTER_LINK_ATTACK"; "17-combat-skills-buffs/21-social-systems"; None; "Link Attack ×1.8 for every member";
        better_movement: bool = true; Startup Sim client -; "ServerCommon.h:17 __BETTER_MOVEMENT"; "31-shared-sim"; None; "active mover keeps its destination after a collision/jump (MoverActEvent.cpp:2000)";
        better_safe_upgrades: bool = true; Startup Both client -; "ServerCommon.h:18 __BETTER_SAFE_UPGRADES"; "19-items-and-economy/45-client-game-screens"; None; "auto safe-upgrade window, PRIME_SMELT 0x70057054, u16 item ids";
        bigger_pocket: bool = true; InitOnly Both client -; "ServerCommon.h:19 __BIGGER_POCKET"; "19-items-and-economy/13-persistence"; None; "pockets 18/42/42 (vanilla 6/24/24)";
        bowdmg_propjob: bool = true; Startup Both client -; "ServerCommon.h:20 __BOWDMG_PROPJOB"; "16-movers-stats-progression"; None; "bow ATK = (DEX−12)·fMeleeBOW + Lv·1.2 (propJob BOW column)";
        buff_pang_update: bool = true; Startup Server - -; "ServerCommon.h:21 __BUFF_PANG_UPDATE"; "17-combat-skills-buffs"; None; "NPC buffer gives class-specific 30 min buffs";
        clean_start_menu: bool = true; Startup Client client -; "ServerCommon.h:22 __CLEAN_START_MENU"; "42-client-ui-framework"; None; "cleaner start menu";
        damage_space: bool = true; Startup Client client -; "ServerCommon.h:23 __DAMAGE_SPACE"; "41-client-rendering"; None; "thousands separators on damage numbers";
        darkvador: bool = true; Startup Client client -; "ServerCommon.h:24 __DARKVADOR"; "16-movers-stats-progression/45-client-game-screens"; Some("KQ-33"); "the client shout code is compiled (16 C16-4): for every processed mover wearing II_ARM_M/F_CHR_DARKVADER01HAT, when its frame counter satisfies count & 131071 == 0 and rand(100) > 50, the **local** client sends /s + TID_GAME_FATHER/TID_GAME_MOTHER as its own player (KQ-33); false ⇒ never";
        dps_meter: bool = true; Startup Both client -; "ServerCommon.h:25 __DPS_METER"; "17-combat-skills-buffs/45-client-game-screens"; None; "DPS meter (DPS_UPDATE 0x8903, DPSProp.txt)";
        drop_awaked_items: bool = true; Startup Server - -; "ServerCommon.h:26 __DROP_AWAKED_ITEMS"; "19-items-and-economy"; None; "gear dropped by monsters is pre-awakened/blessed";
        exp_stop: bool = true; Startup Both client -; "ServerCommon.h:27 __EXP_STOP"; "16-movers-stats-progression"; None; "player toggle MODE_EXPUP_STOP (EXP_STOP 0xAA000010)";
        farm_bot: bool = false; Startup Client client reserved; "ServerCommon.h:28 __FARM_BOT"; "45-client-game-screens"; Some("PL-141"); "farm-bot window — not built in v1 (PL-141, PD-04); true ⇒ startup warning";
        faster_projectiles: bool = true; Startup Both client -; "ServerCommon.h:29 __FASTER_PROJECTILES"; "17-combat-skills-buffs/41-client-rendering"; None; "projectile speeds 0.4/0.7/0.45 per frame (else 0.2/…/0.25): server projectile sim + client visuals";
        fast_jobchange: bool = true; Startup Both client -; "ServerCommon.h:30 __FAST_JOBCHANGE"; "16-movers-stats-progression/45-client-game-screens"; None; "instant job change (UPDATE_JOB 0xAA000001)";
        fast_login: bool = true; Startup Client client -; "ServerCommon.h:31 __FAST_LOGIN"; "45-client-game-screens"; None; "skip login-window animation";
        ftool: bool = false; Startup Client client reserved; "ServerCommon.h:33 __FTOOL"; "45-client-game-screens"; Some("PL-140"); "F-Tool key auto-presser — not built in v1 (PL-140, PD-04)";
        gay_and_lesbian: bool = true; Startup Server - -; "ServerCommon.h:34 __GAY_AND_LESBIAN"; "21-social-systems"; None; "same-sex couples allowed";
        guild_buffs: bool = true; Startup Both client -; "ServerCommon.h:35 __GUILD_BUFFS"; "21-social-systems"; None; "guild-level buffs from guildbuff.inc";
        guild_color: bool = true; Startup Client client -; "ServerCommon.h:36 __GUILD_COLOR"; "41-client-rendering"; None; "guild name colour by guild level";
        heal_over_time: bool = true; Startup Server - -; "ServerCommon.h:37 __HEAL_OVER_TIME"; "17-combat-skills-buffs"; None; "DST_HEAL +adj every 3 s (vanilla: set every 6 s)";
        hide_fashion: bool = true; Startup Both client -; "ServerCommon.h:38 __HIDE_FASHION"; "19-items-and-economy"; None; "hide 5 costume parts (HIDE_COAT 0xAA000007 / 0x8900)";
        hide_pets: bool = true; Startup Client client -; "ServerCommon.h:39 __HIDE_PETS"; "45-client-game-screens"; None; "option to hide pets and make them untargetable";
        holycross_support_gw: bool = true; Startup Server - -; "ServerCommon.h:40 __HOLYCROSS_SUPPORT_GW"; "21-social-systems"; None; "Holy Cross caster gets 1 GC point when the target dies";
        ignore_npc_dialogue: bool = true; Startup Client client -; "ServerCommon.h:41 __IGNORE_NPC_DIALOGUE"; "45-client-game-screens"; None; "option to hide NPC speech";
        improved_block_calculation: bool = true; Startup Both client -; "ServerCommon.h:42 __IMPROVED_BLOCK_CALCULATION"; "17-combat-skills-buffs"; None; "block = BLOCK_MELEE + BLOCK_RANGE capped at gameplay.max_block_value; monsters never block";
        improved_pets: bool = true; Startup Sim client -; "ServerCommon.h:43 __IMPROVED_PETS"; "18-movement-and-ai/19-items-and-economy"; None; "pets copy owner speed; pet fSpeed ×2.8 at load; equipping a ride dismisses pets";
        improved_spiritbomb: bool = true; Startup Server - -; "ServerCommon.h:44 __IMPROVED_SPIRITBOMB"; "17-combat-skills-buffs"; None; "Spirit Bomb without MP-percent scaling";
        improved_status: bool = true; Startup Client client -; "ServerCommon.h:45 __IMPROVED_STATUS"; "45-client-game-screens"; None; "status-bar formatting";
        improved_stat_scroll: bool = true; Startup Server - -; "ServerCommon.h:46 __IMPROVED_STAT_SCROLL"; "19-items-and-economy"; None; "higher stat-scroll chances; stat-option drop 25 %";
        improved_ulti_gems: bool = true; Startup Client client -; "ServerCommon.h:47 __IMPROVED_ULTI_GEMS"; "45-client-game-screens"; None; "UI shows only main stats on ultimate gems (data-driven)";
        inventory_168: bool = true; InitOnly Both client -; "ServerCommon.h:48 __INVENTORY_168"; "19-items-and-economy/13-persistence"; None; "168 bag slots (vanilla 42)";
        inventory_sorting: bool = true; Startup Both client -; "ServerCommon.h:49 __INVENTORY_SORTING"; "19-items-and-economy"; None; "inventory sort (SORT_INVENTORY 0xAA000002)";
        item_count_tooltip: bool = true; Startup Client client -; "ServerCommon.h:50 __ITEM_COUNT_TOOLTIP"; "42-client-ui-framework"; None; "item amount in tooltips";
        magic_steal_hp: bool = true; Startup Server - -; "ServerCommon.h:51 __MAGIC_STEAL_HP"; "17-combat-skills-buffs"; None; "DST_MELEE_STEALHP also for magic and skills";
        max_awake_color: bool = true; Startup Client client -; "ServerCommon.h:52 __MAX_AWAKE_COLOR"; "42-client-ui-framework"; None; "max awakening shown red";
        model_view: bool = true; Startup Client client -; "ServerCommon.h:53 __MODEL_VIEW"; "45-client-game-screens"; None; "item model viewer (right click)";
        monster_buffs: bool = true; Startup Server - -; "ServerCommon.h:54 __MONSTER_BUFFS"; "17-combat-skills-buffs"; None; "party buffs after special kills (MonsterBuffs.inc)";
        multiple_item_picker: bool = true; Startup Both client -; "ServerCommon.h:55 __MULTIPLE_ITEM_PICKER"; "19-items-and-economy"; None; "pickup collects identical stackables within 30 m (packet 0xAA000006 is a server no-op)";
        navigator_indicator: bool = true; Startup Client client -; "ServerCommon.h:56 __NAVIGATOR_INDICATOR"; "45-client-game-screens"; None; "minimap markers for bosses/giants";
        new_font_color: bool = true; Startup Client client -; "ServerCommon.h:57 __NEW_FONT_COLOR"; "42-client-ui-framework"; None; "font colours";
        new_skill_attribute: bool = true; Startup Both client -; "ServerCommon.h:58 __NEW_SKILL_ATTRIBUTE"; "17-combat-skills-buffs"; None; "RM Blessing grants a DST_IGNORE_DMG shield";
        no_arrows_poster: bool = true; Startup Both client -; "ServerCommon.h:59 __NO_ARROWS_POSTER"; "17-combat-skills-buffs/19-items-and-economy"; None; "arrows, charms, posters not consumed";
        no_asal_mp: bool = true; Startup Server - -; "ServerCommon.h:60 __NO_ASAL_MP"; "17-combat-skills-buffs"; None; "Asalraalaikum uses the normal magic formula, no MP drain";
        no_couple_cd: bool = true; Startup Server - -; "ServerCommon.h:61 __NO_COUPLE_CD"; "21-social-systems"; None; "couple proposal cooldown 0 (vanilla 86 400 s)";
        no_guidesystem: bool = true; Startup Client client -; "ServerCommon.h:62 __NO_GUIDESYSTEM"; "45-client-game-screens"; None; "guide pang removed (guide windows not built)";
        no_hop_cost: bool = true; Startup Server - -; "ServerCommon.h:63 __NO_HOP_COST"; "17-combat-skills-buffs"; None; "Hit of Penya costs no penya";
        no_party_gw_join: bool = true; Startup Server - -; "ServerCommon.h:64 __NO_PARTY_GW_JOIN"; "21-social-systems"; None; "no party join in GW/GC1:1 maps or GC state 1";
        no_pet_life: bool = true; Startup Both client -; "ServerCommon.h:65 __NO_PET_LIFE"; "19-items-and-economy"; None; "pets never die (GetLife() = 1)";
        no_piercing: bool = true; Startup Both client -; "ServerCommon.h:66 __NO_PIERCING"; "19-items-and-economy"; Some("PL-070"); "gear not pierceable; card effects on gear not applied (see PL-070)";
        no_quake: bool = true; Startup Client client -; "ServerCommon.h:67 __NO_QUAKE"; "43-client-input-camera"; None; "no camera shake";
        no_quest_button: bool = true; Startup Client client -; "ServerCommon.h:68 __NO_QUEST_BUTTON"; "43-client-input-camera"; None; "no Q hotkey";
        no_yoyo_knockback: bool = true; Startup Server - -; "ServerCommon.h:69 __NO_YOYO_KNOCKBACK"; "17-combat-skills-buffs"; None; "dwDmgShift knock-back disabled for all skills";
        only_armor_awakes: bool = true; Startup Both client -; "ServerCommon.h:70 __ONLY_ARMOR_AWAKES"; "19-items-and-economy"; None; "only helmet/suit/gloves/boots awakeable";
        party_exp_solo: bool = true; Startup Server - -; "ServerCommon.h:71 __PARTY_EXP_SOLO"; "21-social-systems"; None; "solo kills by party members still add party points";
        party_find: bool = true; Startup Both client -; "ServerCommon.h:72 __PARTY_FIND"; "21-social-systems/45-client-game-screens"; None; "party finder (0xAA000008/9, 0x8901/0x8902)";
        perm_extrabags: bool = true; Startup Both client -; "ServerCommon.h:73 __PERM_EXTRABAGS"; "19-items-and-economy"; None; "all pockets permanent and free";
        pet_auto_level_up: bool = true; Startup Server - -; "ServerCommon.h:74 __PET_AUTO_LEVEL_UP"; "19-items-and-economy"; None; "\"pet tamer's mistake\" re-levels the pet immediately";
        pet_filter: bool = true; Startup Both client -; "ServerCommon.h:75 __PET_FILTER"; "19-items-and-economy/18-movement-and-ai"; None; "pet loot filter (PETFILTER 0xAA000011)";
        player_lordevent: bool = true; Startup Both client -; "ServerCommon.h:76 __PLAYER_LORDEVENT"; "22-events-and-content"; None; "any player may start lord events (max gameplay.max_lord_event)";
        pvp_points_per_kill: bool = true; Startup Server - -; "ServerCommon.h:77 __PVP_POINTS_PER_KILL"; "17-combat-skills-buffs"; None; "+1 fame per GW / GC / arena kill";
        quick_sell: bool = true; Startup Both client -; "ServerCommon.h:78 __QUICK_SELL"; "19-items-and-economy"; None; "quick sell (QUICKSELL 0xAA000003)";
        randomoption_retry: bool = true; Startup Both client -; "ServerCommon.h:79 __RANDOMOPTION_RETRY"; "19-items-and-economy"; None; "Scroll of Augmentation (0xAA000004)";
        reactivate_eatpet: bool = true; Startup Server - -; "ServerCommon.h:80 __REACTIVATE_EATPET"; "19-items-and-economy"; None; "eat pet removed before teleport, re-summoned after";
        remove_bt_exp: bool = true; Startup Client client -; "ServerCommon.h:81 __REMOVE_BT_EXP"; "45-client-game-screens"; None; "BT EXP % hidden in character status";
        remove_guild_cooldown: bool = true; Startup Server - -; "ServerCommon.h:82 __REMOVE_GUILD_COOLDOWN"; "21-social-systems"; None; "no 2-day guild rejoin cooldown";
        remove_max_recover_rate: bool = true; Startup Both client -; "ServerCommon.h:83 __REMOVE_MAX_RECOVER_RATE"; "16-movers-stats-progression"; None; "potions always heal their full value";
        rm_hero_skill: bool = true; Startup Both client -; "ServerCommon.h:84 __RM_HERO_SKILL"; "17-combat-skills-buffs"; None; "RM hero skill casts all assist/RM buffs at max level";
        run_on_charselect: bool = true; Startup Client client -; "ServerCommon.h:85 __RUN_ON_CHARSELECT"; "45-client-game-screens"; None; "run animation on character select";
        select_lord: bool = true; Startup Client client -; "ServerCommon.h:87 __SELECT_LORD"; "45-client-game-screens"; None; "lord vote UI shows vote counts";
        servers_authorize: bool = true; Startup Server - -; "ServerCommon.h:88 __SERVERS_AUTHORIZE"; "12-lobby-accounts-sessions/23-gm-admin-chat"; Some("PL-036"); "GM grants from the config list merged with the DB (PL-036)";
        shuffle_drop: bool = true; Startup Server - -; "ServerCommon.h:89 __SHUFFLE_DROP"; "19-items-and-economy"; None; "monster drop table iterated in random order";
        speed_cast: bool = true; Startup Sim client -; "ServerCommon.h:90 __SPEED_CAST"; "17-combat-skills-buffs/31-shared-sim"; None; "DST_SPELL_RATE also speeds melee/range skill animations";
        splash_autoattacks: bool = false; Startup Both client -; "ServerCommon.h:91 __SPLASH_AUTOATTACKS"; "17-combat-skills-buffs/44-client-audio"; None; "yo-yo autoattacks hit everything within 5 m of the target at full damage (the repack's splash); default off by owner decision (bug #007): every weapon autoattack hits only its target; client: while on, no generic hit sound for yo-yo autoattacks (44 OQ11)";
        statistics: bool = true; Startup Client client -; "ServerCommon.h:92 __STATISTICS"; "45-client-game-screens"; None; "extra stats in the character window";
        sys_petleveling_mobkill: bool = true; Startup Server - -; "ServerCommon.h:93 __SYS_PETLEVELING_MOBKILL"; "19-items-and-economy"; None; "pets gain gameplay.pet_base_exp per kill, auto-hatch; per-minute pet EXP and egg feeding disabled";
        targetsave: bool = true; Startup Client client -; "ServerCommon.h:94 __TARGETSAVE"; "43-client-input-camera"; None; "target persists after a kill (AoE)";
        teleportation_system: bool = true; Startup Both client -; "ServerCommon.h:95 __TELEPORTATION_SYSTEM"; "15-world-and-visibility/45-client-game-screens"; None; "teleport window from teleport16.inc (TELE16 0xAA000005)";
        trails: bool = true; Startup Client client -; "ServerCommon.h:96 __TRAILS"; "41-client-rendering"; None; "colourful character trails";
        update_taskbar: bool = true; Startup Client client -; "ServerCommon.h:97 __UPDATE_TASKBAR"; "42-client-ui-framework"; None; "centred taskbar menu";
        v21_levelup_sfx: bool = true; Startup Client client -; "ServerCommon.h:98 __V21_LEVELUP_SFX"; "41-client-rendering"; None; "job-coloured level-up SFX";
        wand_autoattacks: bool = true; Startup Both client -; "ServerCommon.h:99 __WAND_AUTOATTACKS"; "17-combat-skills-buffs"; None; "wand ATK = (INT−12)·fMagicWAND + Lv·1.2, charge multiplier 1.0";
        welcome_notice: bool = true; Startup Server - -; "ServerCommon.h:100 __WELCOME_NOTICE"; "23-gm-admin-chat"; None; "gameplay.welcome_messages sent at login";
        elemental_upgrade_bonus: bool = false; Startup Server - reserved; "ServerCommon.h:102 __ELEMENTAL_UPGRADE_BONUS"; "03-scope-and-parity-ledger"; None; "Reserved, not implemented in v1: inactive in this repack";
        lootname_and_amount: bool = false; Startup Server - reserved; "ServerCommon.h:103 __LOOTNAME_AND_AMOUNT"; "03-scope-and-parity-ledger"; None; "Reserved, not implemented in v1: inactive";
        monster_raid: bool = false; Startup Server - reserved; "ServerCommon.h:104 __MONSTER_RAID"; "03-scope-and-parity-ledger"; None; "Reserved, not implemented in v1: F-188 DEFER; MonsterRaid.lua, AutoRaid.lua";
        new_buff_format: bool = false; Startup Server - reserved; "ServerCommon.h:105 __NEW_BUFF_FORMAT"; "03-scope-and-parity-ledger"; None; "Reserved, not implemented in v1: inactive";
        no_bbag: bool = false; Startup Server - reserved; "ServerCommon.h:106 __NO_BBAG"; "03-scope-and-parity-ledger"; None; "Reserved, not implemented in v1: inactive";
        remove_pve_block: bool = false; Startup Server - reserved; "ServerCommon.h:107 __REMOVE_PVE_BLOCK"; "03-scope-and-parity-ledger"; None; "Reserved, not implemented in v1: inactive";
        af_magic_fix: bool = true; Startup Server - -; "ServerCommon.h:109 __AF_MAGIC_FIX"; "17-combat-skills-buffs"; None; "wand crit bonus uses the attacker's DST_CRITICAL_BONUS";
        attack_range_code: bool = true; Startup Sim client -; "ServerCommon.h:110 __ATTACK_RANGE_CODE"; "31-shared-sim"; None; "bow approach range = AR_WAND (15 m) instead of the item range (MoverMsg.cpp:477)";
        dmgcorrect: bool = true; Startup Server - -; "ServerCommon.h:111 __DMGCORRECT"; "17-combat-skills-buffs"; None; "block outcome table (PvP block 75 % ⇒ ×0; monster→player ×0.7)";
        element_fix: bool = true; Startup Server - -; "ServerCommon.h:112 __ELEMENT_FIX"; "17-combat-skills-buffs"; None; "same-element bonus only when skill element ≠ none";
        fix_get_hp_mp_fp_point: bool = true; Startup Both client -; "ServerCommon.h:113 __FIX_GET_HP_MP_FP_POINT"; "16-movers-stats-progression"; None; "HP/MP/FP getters clamp to current max";
        fix_ranger_roller: bool = true; Startup Sim client -; "ServerCommon.h:114 __FIX_RANGER_ROLLER"; "31-shared-sim"; None; "stop before a ranged attack; abort when out of range (MoverMove.cpp:233-246, MoverSkill.cpp:3323)";
        hop_scaling_fix: bool = true; Startup Server - -; "ServerCommon.h:116 __HOP_SCALING_FIX"; "17-combat-skills-buffs"; None; "Hit of Penya without extra ×destData scaling";
        wrong_pw_fix: bool = true; Startup Server - -; "ServerCommon.h:123 __WRONG_PW_FIX"; "12-lobby-accounts-sessions"; Some("PL-031"); "wrong-password lock 1 s / 5 s instead of 15 s / 15 min (PL-031)";
        yoyo_model_fix: bool = true; Startup Client client -; "ServerCommon.h:124 __YOYO_MODEL_FIX"; "41-client-rendering"; None; "yo-yo SFX uses the equipped weapon mesh";
    }
    gameplay Gameplay ClientGameplay {
        max_pack_item: u32 = 9_999; InitOnly Both client -; "ServerCommon.h:154 __MAX_PACK_ITEM"; "14-game-data-and-vfs/19-items-and-economy"; None; "every prop with dwPackMax > 1 gets this stack size at load";
        increased_spawn: f32 = 1.0; Startup Server - -; "ServerCommon.h:153 __INCREASED_SPAWN"; "15-world-and-visibility"; None; "multiplies each respawn region's max count, capped at 80";
        max_lord_event: u32 = 2; Startup Both client -; "ServerCommon.h:155"; "22-events-and-content"; None; "concurrent lord events";
        min_name_letters: u32 = 4; Startup Both client -; "ServerCommon.h:156"; "23-gm-admin-chat/12-lobby-accounts-sessions"; None; "name length minimum (VR-070)";
        max_name_letters: u32 = 16; Startup Both client -; "ServerCommon.h:157"; "23-gm-admin-chat/12-lobby-accounts-sessions"; None; "name length maximum";
        enchant_time_s: u32 = 1; Startup Both client -; "ServerCommon.h:158 ENCHANT_TIME"; "45-client-game-screens"; None; "client wait before an upgrade is sent (cosmetic)";
        guild_kill_contri: u32 = 25; Startup Server - -; "ServerCommon.h:160"; "21-social-systems"; None; "guild contribution (PXP) per kill by a member";
        couple_rate: u32 = 200; Startup Both client -; "ServerCommon.h:161 __COUPLE_RATE"; "21-social-systems"; None; "divisor of every couple-level EXP requirement";
        guild_rate: u32 = 200; Startup Both client -; "ServerCommon.h:162 __GUILD_RATE"; "21-social-systems"; None; "divisor of every guild-level PXP requirement";
        pet_base_exp: f32 = 50.0; Startup Server - -; "ServerCommon.h:163"; "19-items-and-economy"; None; "base pet EXP per kill (features.sys_petleveling_mobkill)";
        max_block_value: u32 = 75; Startup Both client -; "ServerCommon.h:165"; "17-combat-skills-buffs"; None; "block cap %";
        max_block_value_gw: u32 = 75; Startup Both client -; "ServerCommon.h:166"; "17-combat-skills-buffs"; None; "block cap % in guild war";
        set_max_speed: u32 = 150; Startup Both client -; "ServerCommon.h:167"; "16-movers-stats-progression"; None; "speed adjustment cap %";
        set_max_cast: u32 = 100; Startup Both client -; "ServerCommon.h:168"; "16-movers-stats-progression/45-client-game-screens"; None; "cast-speed cap (display only in original)";
        set_max_crit: u32 = 100; Startup Both client -; "ServerCommon.h:169"; "17-combat-skills-buffs"; None; "crit chance cap";
        all_cs_price: u32 = 25_000_000; Startup Server - -; "ServerCommon.h:171"; "14-game-data-and-vfs"; None; "dwCost of IK3_HAT/CLOTH/GLOVE/SHOES at load";
        all_pet_price: u32 = 25_000_000; Startup Server - -; "ServerCommon.h:172"; "14-game-data-and-vfs"; None; "dwCost of IK3_PET at load";
        welcome_messages: Vec<String> = vec!["Welcome #1".to_owned(), "Welcome #2".to_owned()]; Startup Server - -; "ServerCommon.h:148–149"; "23-gm-admin-chat"; None; "chat lines at login (features.welcome_notice)";
        gm_title: String = "[Gm]".to_owned(); Startup Both client -; "ServerCommon.h:146"; "41-client-rendering"; None; "name suffix for GM ≥ L";
        admin_title: String = "[Admin]".to_owned(); Startup Both client -; "ServerCommon.h:143"; "41-client-rendering"; None; "name suffix for admin P";
        gm_name_color: String = "#FF4500".to_owned(); Startup Both client -; "ServerCommon.h:145 0xFFFF4500"; "41-client-rendering"; None; "GM name colour";
        admin_name_color: String = "#FF4500".to_owned(); Startup Both client -; "ServerCommon.h:142"; "41-client-rendering"; None; "admin name colour";
        shout_min_level: u32 = 0; Startup Both client -; "ServerCommon.h:152 __SHOUT_LEVEL 15 applies only to TWN/HK"; "23-gm-admin-chat"; Some("KQ-12"); "minimum level to shout (0 = none, ENG parity, KQ-12)";
        shout_range: f32 = 255.0; Startup Server - -; "CUserMng::AddShout (ref/24 §7.2)"; "23-gm-admin-chat"; None; "shout radius, same world+layer; server-wide with a shout item";
        npc_menu_range: f32 = 32.0; Startup Server - -; "MAX_LEN_MOVER_MENU 1024 (squared, 3-D)"; "20-npc-dialog-quests-scripting"; None; "NPC service range (VR-012)";
        vendor_npc_min_dist: f32 = 3.0; Startup Server - -; "MAX_NPC_RADIUS 9 (squared, XZ)"; "19-items-and-economy"; None; "vendor exclusion radius (VR-013)";
        npc_script_throttle_ms: u32 = 400; Startup Server - -; "__QUEST_1208"; "20-npc-dialog-quests-scripting"; None; "NPC script request throttle (VR-061)";
        duel_request_ttl_s: u32 = 10; Startup Server - -; "__HACK_1130"; "17-combat-skills-buffs"; None; "duel invitation lifetime (VR-050)";
        invite_ttl_s: u32 = 60; Startup Server - -; "new (original had no invite records)"; "21-social-systems"; None; "party/guild/friend/campus/war invite lifetime (VR-040..044)";
        bank_pin_max_failures: u32 = 3; Startup Server - -; "new (original unlimited)"; "19-items-and-economy"; Some("PL-061"); "wrong PINs before lock; 0 = unlimited (= original) (PL-061)";
        bank_pin_lock_s: u32 = 900; Startup Server - -; "new"; "19-items-and-economy"; None; "lock duration";
        upgrade_min_interval_ms: u32 = 600; Startup Server - -; "new (original: client pacing only)"; "19-items-and-economy"; Some("PL-076"); "min interval between upgrade attempts (PL-076)";
        move_guard_dist_sq: f32 = 1_000_000.0; Startup Server - -; "DPSrvr.cpp:2336"; "18-movement-and-ai"; None; "1 km jump guard (VR-080)";
        timewait_close_s: u32 = 10; Startup Both client -; "TIMEWAIT_CLOSE"; "12-lobby-accounts-sessions"; None; "logout delay outside safe zones";
        chat_max_chars: u32 = 120; Startup Both client -; "client SetStrNull(120)"; "23-gm-admin-chat"; None; "chat line cut (VR-071)";
        chat_drop_bytes: u32 = 260; Startup Server - -; "server ≥260 drop"; "23-gm-admin-chat"; None; "oversize lines dropped";
        chat_same_text_ms: u32 = 2_000; Startup Both client -; "m_timerDobe"; "23-gm-admin-chat"; None; "identical-line window";
        chat_warn_ms: u32 = 700; Startup Both client -; "WARNING_MILLISECOND"; "23-gm-admin-chat"; None; "fast-line warning (not sent)";
        chat_warn2_ms: u32 = 1_000; Startup Both client -; "WARNING_2_MILLISECOND"; "23-gm-admin-chat"; None; "second warning tier (sent)";
        chat_shortcut_warn_ms: u32 = 3_000; Startup Both client -; "SHORTCUT_WARNING_MILLISECOND"; "23-gm-admin-chat"; None; "macro-line window";
        chat_ban_points: u32 = 5; Startup Both client -; "BANNING_POINT"; "23-gm-admin-chat"; None; "warnings before chat ban";
        chat_ban2_points: u32 = 2; Startup Both client -; "BANNING_2_POINT"; "23-gm-admin-chat"; None; "tier-2 warnings before ban";
        chat_ban_ms: u32 = 180_000; Startup Both client -; "BANNING_MILLISECOND"; "23-gm-admin-chat"; None; "chat ban duration";
        shout_same_text_ms: u32 = 3_000; Startup Both client -; "client shout rule"; "23-gm-admin-chat"; None; "identical shout window";
        shout_limit_count: u32 = 5; Startup Both client -; "IDS_SHOUTLIMIT_COUNT"; "23-gm-admin-chat"; None; "shouts per window (only without a shout item at ENG)";
        shout_limit_window_ms: u32 = 5_000; Startup Both client -; "IDS_SHOUTLIMIT_SECOND"; "23-gm-admin-chat"; None; "fixed window";
        grant_poll_s: u32 = 30; Startup Server - -; "new (original: at login only)"; "13-persistence"; Some("PL-055"); "pending-grant delivery to online players every N s (PL-055); 0 = original (poll job P12 not registered, grants delivered only at enter-world)";
        campus_requires_quest: bool = false; Startup Server - -; "ref/14 §12.1 (QUEST_SCE_MDRIGALTEACHER5 required for a campus master; true = original)"; "21-social-systems"; Some("PL-097"); "false: the required-quest list of the campus master check is treated as empty (quest data is empty, D1; PL-097, PD-22). Same pattern as guild.create_requires_quest, kept in [gameplay] so no new section is needed (supersedes the [social] section of 10 §8.5, Open question 14)";
        gc1to1_min_join_player: u32 = 0; Startup Server - -; "GuildCombat1to1.cpp:804-806 (test builds without __MAINSERVER used 1)"; "21-social-systems"; None; "0 = MinJoinPlayer of GuildCombat1TO1.lua; n >= 1 overrides it (test servers, D-M7)";
    }
    rates Rates - {
        monster_exp: Option<f32> = None; Runtime Server - -; "prj.m_fMonsterExpRate, GM rate command"; "16-movers-stats-progression"; None; "monster EXP multiplier Unset = Constant.inc formula{} (1.0 for ENG).";
        item_drop: Option<f32> = None; Runtime Server - -; "m_fItemDropRate"; "19-items-and-economy"; None; "item drop multiplier Unset = 1.0.";
        gold_drop: Option<f32> = None; Runtime Server - -; "m_fGoldDropRate"; "19-items-and-economy"; None; "gold drop multiplier Unset = 1.0.";
        monster_hit: Option<f32> = None; Runtime Server - -; "m_fMonsterHitRate"; "17-combat-skills-buffs"; None; "monster hit-rate multiplier Unset = 1.0.";
        shop_cost: Option<f32> = None; Runtime Server - -; "m_fShopCost (__SHOP_COST_RATE)"; "19-items-and-economy"; None; "NPC shop price multiplier Unset = 1.0.";
        party_exp: f32 = 10.0; Runtime Server - -; "CoreServer.ini PartyExpRate 10 (code default 1.0)"; "21-social-systems"; None; "party-level EXP per kill: (monLv/25+1)·10·rate";
        monster_aggressive: f32 = 0.2; Runtime Server - -; "m_fMonsterAggressiveRate (default 0.2)"; "15-world-and-visibility"; Some("KQ-14"); "Madrigal aggressive override (KQ-14)";
    }
    events EventDefaults - {
        eve_18: bool = true; Runtime Server - -; "WorldServer.ini token 18; EventId Eve18 = 1"; "17-combat-skills-buffs"; None; "\"18+ server\": PvP/PK case computation, duel/PK menu entries; not settable by /localevent";
        guild_bank: bool = true; Runtime Server - -; "WorldServer.ini token GUILDBANK; EventId GuildInventory = 2"; "21-social-systems/22-events-and-content"; None; "guild bank, guild-house furniture set/remove/reset and tender results, guild-bank cleanup";
        guild_war: bool = true; Runtime Server - -; "WorldServer.ini token GUILDWAR; EventId GuildWar = 3"; "21-social-systems"; None; "guild wars (IsWarTarget, war deaths, duel/PK refusal between war members)";
        wormon: bool = true; Runtime Server - -; "WorldServer.ini token WORMON; EventId Wormon = 9"; "22-events-and-content"; None; "\"Wormon server\": IsWormonServer() for NPC scripts; /ctd acts only when 0";
        respawn: u8 = 1; Runtime Server - -; "WorldServer.ini token respawn n; EventId Respawn = 10"; "15-world-and-visibility"; None; "0 ⇒ no monster respawn at all";
        pk: bool = false; Runtime Server - -; "WorldServer.ini token PK; EventId Pk = 11"; "17-combat-skills-buffs"; None; "PK server: non-duel PvP, chaotic logic, PK NPCs, RANK_GUARD spawns, angel-buff items (KQ-17)";
        pk_cost: bool = false; Runtime Server - -; "WorldServer.ini token PKCOST; EventId PkCost = 12"; "17-combat-skills-buffs"; None; "PK penalties (wanted reward, chaotic gold drop)";
        steal: bool = true; Runtime Server - -; "WorldServer.ini token STEAL; EventId Steal = 13"; "17-combat-skills-buffs"; None; "**true ⇒ kill-steal protection OFF**";
        guild_combat: bool = true; Runtime Server - -; "WorldServer.ini token GUILDCOMBAT; EventId GuildCombat = 15"; "21-social-systems"; None; "guild siege: GuildCombat.txt, NPCs MaFl_GuildWar/MaFl_Donaris, process";
        drop_item_remove: bool = true; Runtime Server - -; "WorldServer.ini token DROPITEM_REMOVE; EventId DropItemRemove = 16"; "19-items-and-economy"; None; "**player \"drop\" destroys the item** (server, C1) + client warning texts";
        recommend: bool = true; Runtime Server - -; "WorldServer.ini token RECOMMEND; EventId Recommend = 21"; "22-events-and-content"; None; "level-up recommendation report; backend dropped (F-284), flag kept for scripts";
        guild_combat_1to1: bool = false; Runtime Server - -; "WorldServer.ini token GUILDCOMBAT1TO1; EventId GuildCombat1to1 = 22"; "21-social-systems"; None; "1:1 guild combat process and NPCs MaFl_Annie/MaFl_Amos";
        arena: bool = true; Runtime Server - -; "WorldServer.ini token ARENA; EventId Arena = 23"; "17-combat-skills-buffs/15-world-and-visibility"; None; "arena NPC MaFl_Ray spawned";
        secret_room: bool = true; Runtime Server - -; "WorldServer.ini token SECRETROOM; EventId SecretRoom = 24"; "22-events-and-content"; None; "secret-room process and NPCs";
        rainbow_race: bool = false; Runtime Server - -; "WorldServer.ini token RAINBOWRACE; EventId RainbowRace = 25"; "22-events-and-content"; None; "rainbow-race process and NPC MaFl_RainbowStart";
    }
    fidelity Fidelity ClientFidelity {
        hit_attr_rule: HitAttrRule = HitAttrRule::Legacy; Startup Sim client -; "original: \"legacy\""; "31-shared-sim/17-combat-skills-buffs"; Some("KQ-01"); "Which animation frames count as a hit: \"legacy\" = floor(frameCur) rule (original), \"window\" = all attributes in (old, cur].";
        player_melee_can_miss: bool = false; Startup Server - -; "original: false (repack)"; "17-combat-skills-buffs"; Some("KQ-03"); "Player melee autoattacks never miss (the repack ignores the AF_MISS roll)";
        remote_extrap_cap_frames: u32 = 37; Startup Client client -; "original: (ghost replay, n/a)"; "40-client-architecture"; Some("PL-007"); "Remote mover display: extrapolation from the timestamped state with the shared sim (cap 37 frames) + the original /8 render smoothing (D4)";
        fix_vis_slot_expansion: bool = true; Startup Both client -; "original: false"; "19-items-and-economy"; Some("PL-070"); "Vis-pet slot expansion: vis-pet slots expandable up to 9";
        flying_drop_overflow_to_ground: bool = true; Startup Server - -; "original: false"; "19-items-and-economy"; Some("PL-071"); "Flying-monster drops: when full, spawned on the ground at the monster — table drops and event drops alike (19 §6.2 step 2, §24.2 N6)";
        gold_update_self_only: bool = true; Startup Server - -; "original: false"; "19-items-and-economy"; Some("PL-075"); "Gold privacy: sent to the owner only";
        party_exp_loop_fix: bool = true; Startup Server - -; "original: false"; "21-social-systems"; Some("PL-081"); "Party EXP distribution: loop continues; carry-over fixed";
        duel_target_summon_check: bool = true; Startup Server - -; "original: false"; "17-combat-skills-buffs"; Some("PL-084"); "Duel request check: the target's summon state is checked";
        instance_enter_immediate: bool = true; Startup Server - -; "original: false"; "22-events-and-content"; Some("PL-101"); "Instance first entry: instance created synchronously, teleport immediately";
        chat_filter_server_side: bool = true; Startup Server client -; "original: false"; "23-gm-admin-chat"; Some("PL-111"); "Profanity filter: applied by the server to the same channels with the same WordChange rules";
        unknown_command_as_chat: bool = false; Startup Server - -; "original: true"; "23-gm-admin-chat"; Some("PL-112"); "Unknown commands: reply \"Unknown command\" (same reply when unauthorised)";
        item_gm_commands: bool = true; Startup Server - -; "original: false"; "23-gm-admin-chat"; Some("PL-114"); "Item-triggered GM commands: whitelist (dis, nodis, ud, ud2, noud, createguild) executed with item privilege (PD-11)";
        world_msg_rect_xz: bool = true; Startup Server - -; "original: false"; "15-world-and-visibility"; Some("PL-118"); "Rect world messages: tests (x, z)";
        persist_gm_overrides: bool = true; Startup Server - -; "original: false"; "10-server-runtime"; Some("PL-056"); "GM runtime changes: persisted in runtime_override (D2)";
        quest_condition_fixes: bool = true; Startup Server client -; "original: false"; "20-npc-dialog-quests-scripting"; Some("PL-159"); "Quest condition fixes: fixed as listed in the \"Fix\" column of 20 §9.4–9.5; the client condition display follows the same rule";
    }
}

/// The typed browser projection of every key with the C flag (Plan/03 §4.1), one nested struct per
/// section (`[rates]` and `[events]` have no client keys). The carrier is `GET /api/bootstrap`
/// (doc 12/24); `web/src/gen/config/clientFeatures.ts` is generated from the same registry.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClientFeatureConfig {
    /// `[features]` keys with the C flag.
    pub features: ClientFeatures,
    /// `[gameplay]` keys with the C flag.
    pub gameplay: ClientGameplay,
    /// `[fidelity]` keys with the C flag.
    pub fidelity: ClientFidelity,
}

impl From<&RulesConfig> for ClientFeatureConfig {
    fn from(rules: &RulesConfig) -> Self {
        Self {
            features: ClientFeatures::from(&rules.features),
            gameplay: ClientGameplay::from(&rules.gameplay),
            fidelity: ClientFidelity::from(&rules.fidelity),
        }
    }
}

impl Default for ClientFeatureConfig {
    fn default() -> Self {
        Self::from(&RulesConfig::default())
    }
}
