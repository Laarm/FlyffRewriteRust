//! Glue of the table loaders that are not driven by the project file: the item extension tables (L40-L44,
//! L48-L55, L57), the social tables (L60, L64-L66, L73, L75), the event tables (L45, L53, L59, L61, L67, L68,
//! L72), `teleport16.inc` (L30), `Client/Guide.inc` (L62) and `WorldDialog.txt` (L70). Each function reads its
//! file through `LoadCtx`, runs the pure parser of its module, stores the result and counts it (Plan/14 §5.3,
//! §9.1 C13-C15, C28-C31).

use crate::dialog_text::parse_dialog_text;
use crate::events::dps::parse_dps_monsters;
use crate::events::environment::parse_environment;
use crate::events::generic_event::parse_generic_events;
use crate::events::minigame::{
    parse_alphabet, parse_five_system, parse_kawibawibo, parse_questions, parse_reassemble,
};
use crate::events::monster_buffs::parse_monster_buffs;
use crate::events::rangda::parse_rangda;
use crate::events::spevent::parse_spevent;
use crate::events::transform_item::parse_transform_items;
use crate::guide::parse_guide_max_seq;
use crate::items_ext::accessory::parse_accessory;
use crate::items_ext::collecting::parse_collecting;
use crate::items_ext::exchange::parse_exchange;
use crate::items_ext::giftbox::{parse_giftbox, parse_pack_items};
use crate::items_ext::item_etc::parse_item_etc;
use crate::items_ext::pet::parse_pet;
use crate::items_ext::random_option::parse_random_option;
use crate::items_ext::server_script::{ServerScriptTargets, apply_server_script};
use crate::items_ext::ticket::{parse_layers, parse_tickets};
use crate::items_ext::ultimate::{parse_gem_ability, parse_ultimate_weapon};
use crate::load::ctx::{LoadCtx, Need};
use crate::social::couple::parse_couple;
use crate::social::guild::{parse_guild_buffs, parse_guild_combat};
use crate::social::honor::parse_honor;
use crate::social::lord::{parse_election, parse_lord_events, parse_lord_skills};
use crate::teleport16::parse_teleport16;

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Reads `path` (a missing file is a Warning) and gives the text with the script tables and the file name.
macro_rules! read {
    ($cx:expr, $path:expr) => {{
        let path: &str = $path;
        match $cx.read_text(path, Need::Warn) {
            Some(text) => {
                let sctx = $cx.script_ctx();
                let file = $cx.file_arc(path);
                Some((text, sctx, file))
            }
            None => None,
        }
    }};
}

/// L30: `teleport16.inc` (F13: no phantom entry).
pub fn load_teleport16(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "teleport16.inc") {
        cx.gd.teleports16 = parse_teleport16(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.teleport16_rows = count(cx.gd.teleports16.len());
    }
}

/// L40: `propGiftbox.inc`.
pub fn load_giftbox(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "propGiftbox.inc") {
        cx.gd.giftboxes = parse_giftbox(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.giftbox_blocks = cx.gd.giftboxes.blocks;
    }
}

/// L41: `propPackItem.inc`.
pub fn load_pack_items(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "propPackItem.inc") {
        cx.gd.pack_items = parse_pack_items(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.pack_blocks = cx.gd.pack_items.blocks;
    }
}

/// L44: `propItemEtc.inc`.
pub fn load_item_etc(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "propItemEtc.inc") {
        cx.gd.item_etc = parse_item_etc(&t, &f, &s, &mut cx.diags);
        let e = &cx.gd.item_etc;
        cx.gd.meta.counts.item_etc = [
            count(e.piercing.len()),
            count(e.set_items.len()),
            count(e.random_opt_items.len()),
        ];
    }
}

/// L45: `propEvent.inc`.
pub fn load_generic_events(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "propEvent.inc") {
        cx.gd.generic_events = parse_generic_events(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.generic_events = count(cx.gd.generic_events.len());
    }
}

/// L48: `pet.inc`.
pub fn load_pet(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "pet.inc") {
        cx.gd.pet = parse_pet(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.pet_kinds = count(cx.gd.pet.avail.len());
    }
}

/// L50: `accessory.inc`.
pub fn load_accessory(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "accessory.inc") {
        cx.gd.accessory = parse_accessory(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.accessories = count(cx.gd.accessory.items.len());
    }
}

/// L51: `randomoption.inc`.
pub fn load_random_option(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "randomoption.inc") {
        cx.gd.random_option = parse_random_option(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.random_option_groups = count(cx.gd.random_option.groups.len());
    }
}

/// L54: `Ultimate_GemAbility.txt` (abilities and the `LEVEL_GEM` ranges) and `Ultimate_UltimateWeapon.txt` (the
/// server probabilities).
pub fn load_ultimate(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "Ultimate_GemAbility.txt") {
        cx.gd.ultimate = parse_gem_ability(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.gem_abilities = count(cx.gd.ultimate.gem_ability.len());
    }
    if let Some((t, _s, f)) = read!(cx, "Ultimate_UltimateWeapon.txt") {
        parse_ultimate_weapon(&t, &f, &mut cx.diags, &mut cx.gd.ultimate);
    }
}

/// L55: `Exchange_Script.txt`.
pub fn load_exchange(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "Exchange_Script.txt") {
        cx.gd.exchange = parse_exchange(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.exchange_menus = count(cx.gd.exchange.menus.len());
    }
}

/// L60: `lordevent.inc` and `lordskill.txt`.
pub fn load_lord(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "lordevent.inc") {
        let (exp, item) = parse_lord_events(&t, &f, &s, &mut cx.diags);
        cx.gd.lord.event_exp = exp;
        cx.gd.lord.event_item = item;
    }
    if let Some((t, s, f)) = read!(cx, "lordskill.txt") {
        cx.gd.lord.skills = parse_lord_skills(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.lord_skills = count(cx.gd.lord.skills.len());
    }
}

/// L75: `election.inc` (the lord election schedule, rates and reward items).
pub fn load_election(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "election.inc") {
        cx.gd.lord.election = parse_election(&t, &f, &s, &mut cx.diags);
    }
}

/// L64: `couple.inc` (F12: the level EXP is divided by `gameplay.couple_rate`).
pub fn load_couple(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "couple.inc") {
        let div = cx.opts.couple_exp_divisor;
        cx.gd.couple = parse_couple(&t, &f, &s, div, &mut cx.diags);
        cx.gd.meta.counts.couple_levels = count(cx.gd.couple.level_exp.len());
    }
}

/// L65: `honorList.txt`.
pub fn load_honor(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "honorList.txt") {
        cx.gd.honor = parse_honor(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.honor_titles = count(cx.gd.honor.titles.len());
    }
}

/// L66: `guildbuff.inc`.
pub fn load_guild_buffs(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "guildbuff.inc") {
        cx.gd.guild.buffs = parse_guild_buffs(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.guild_buffs = count(cx.gd.guild.buffs.len());
    }
}

/// L67: `MonsterBuffs.inc`.
pub fn load_monster_buffs(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "MonsterBuffs.inc") {
        cx.gd.monster_buffs = parse_monster_buffs(&t, &f, &s, &mut cx.diags);
        cx.gd.meta.counts.monster_buff_pairs = count(cx.gd.monster_buffs.len());
    }
}

/// L68: `DPSProp.txt` (empty in this data).
pub fn load_dps_props(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "DPSProp.txt") {
        cx.gd.dps_monsters = parse_dps_monsters(&t, &f, &s, &mut cx.diags);
    }
}

/// L70: `WorldDialog.txt` (windows-1252 by the hint table).
pub fn load_dialog_text(cx: &mut LoadCtx<'_>) {
    if let Some(text) = cx.read_text("WorldDialog.txt", Need::Warn) {
        cx.gd.dialog_text = parse_dialog_text(&text);
        cx.gd.meta.counts.dialog_lines = count(cx.gd.dialog_text.len());
    }
}

/// L49: `collecting.inc` (the collector tables; `s.txt` adds the enchant chances and the loot at L57).
pub fn load_collecting(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "collecting.inc") {
        cx.gd.collecting = parse_collecting(&t, &f, &s, &mut cx.diags);
    }
}

/// L52: `ticket.inc` and `layer.inc`.
pub fn load_tickets(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "ticket.inc") {
        parse_tickets(&t, &f, &s, &mut cx.diags, &mut cx.gd.tickets);
    }
    if let Some((t, s, f)) = read!(cx, "layer.inc") {
        parse_layers(&t, &f, &s, &mut cx.diags, &mut cx.gd.tickets);
    }
    cx.gd.meta.counts.tickets = count(cx.gd.tickets.tickets.len());
    cx.gd.meta.counts.ticket_layers = count(cx.gd.tickets.layers.len());
}

/// L53: the four mini-game scripts (and the private question table of the alphabet game).
pub fn load_minigames(cx: &mut LoadCtx<'_>) {
    let mut games = std::mem::take(&mut cx.gd.minigames);
    let questions = match cx.read_text("MiniGame_Alphabet.txt.txt", Need::Warn) {
        Some(t) => {
            let f = cx.file_arc("MiniGame_Alphabet.txt.txt");
            parse_questions(&t, &f, &mut cx.diags)
        }
        None => std::collections::BTreeMap::new(),
    };
    if let Some((t, s, f)) = read!(cx, "MiniGame_Alphabet.txt") {
        parse_alphabet(&t, &f, &s, &questions, &mut cx.diags, &mut games);
    }
    if let Some((t, s, f)) = read!(cx, "MiniGame_KawiBawiBo.txt") {
        parse_kawibawibo(&t, &f, &s, &mut cx.diags, &mut games);
    }
    if let Some((t, _s, f)) = read!(cx, "MiniGame_FiveSystem.txt") {
        parse_five_system(&t, &f, &mut cx.diags, &mut games);
    }
    if let Some((t, s, f)) = read!(cx, "MiniGame_Reassemble.txt") {
        parse_reassemble(&t, &f, &s, &mut cx.diags, &mut games);
    }
    cx.gd.meta.counts.minigame_words = count(games.alphabet.len());
    cx.gd.minigames = games;
}

/// L57: `s.txt`, the server-only probabilities that patch the accessory, collecting and pet tables (F10; loaded
/// after `pet.inc`, `collecting.inc` and `accessory.inc`).
pub fn load_server_script(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "s.txt") {
        let gd = &mut cx.gd;
        let stats = apply_server_script(
            &t,
            &f,
            &s,
            &mut cx.diags,
            ServerScriptTargets {
                accessory: &mut gd.accessory,
                collecting: &mut gd.collecting,
                pet: &mut gd.pet,
            },
        );
        gd.meta.counts.s_txt_entries = count(
            (stats.accessory_probability
                + stats.collecting_enchant
                + stats.collecting_items
                + stats.pet_levelup_rows
                + stats.pet_add_life) as usize,
        );
    }
}

/// L59: `EnvironmentEffect.txt` (the season table; one byte in this data set).
pub fn load_environment(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "EnvironmentEffect.txt") {
        cx.gd.environment = parse_environment(&t, &f, &s, &mut cx.diags);
    }
}

/// L61: `randomeventmonster.inc` (the Rangda monsters) and `transformitem.inc` (the egg transformations).
pub fn load_rangda_transform(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "randomeventmonster.inc") {
        cx.gd.rangda = parse_rangda(&t, &f, &s, &mut cx.diags);
    }
    if let Some((t, s, f)) = read!(cx, "transformitem.inc") {
        cx.gd.transform_items = parse_transform_items(&t, &f, &s, &mut cx.diags);
    }
}

/// L62: `Client/Guide.inc`, of which the server keeps the highest `LEVEL` sequence number.
pub fn load_guide(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "Client/Guide.inc") {
        cx.gd.guide_max_seq = parse_guide_max_seq(&t, &f, &s, &mut cx.diags);
    }
}

/// L72: `spevent.txt` (the Christmas drop weights).
pub fn load_spevent(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "spevent.txt") {
        cx.gd.xmas_event = parse_spevent(&t, &f, &s, &mut cx.diags);
    }
}

/// L73: `GuildCombat.txt` (the guild siege settings and state script).
pub fn load_guild_combat(cx: &mut LoadCtx<'_>) {
    if let Some((t, s, f)) = read!(cx, "GuildCombat.txt") {
        cx.gd.guild.combat = parse_guild_combat(&t, &f, &s, &mut cx.diags);
    }
}
