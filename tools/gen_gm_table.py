#!/usr/bin/env python3 -I
"""Generates crates/flyff-game/src/gm/table.rs from the command tables of Plan/23 §4.2-§4.9 (WP-GM-04).

    python3 -I tools/gen_gm_table.py --plan /path/to/Plan/23-gm-admin-chat.md [--check]

The plan tables are the source of truth for the 219 legacy rows and the new commands N01-N16; the repository adds N17
(`/content`, `REPO_NEW` below). Which rows have a
handler in this repository is the HANDLERS dict below (row number or "N01" -> Rust path relative to
`crate::gm`); every other `PORT` row is emitted as `Deferred("<WP>")`, which behaves like an unknown command
until its work package lands (23 §12). Rerun the script after adding a handler and commit the result; the unit
test `gm_table_complete` pins the totals.
"""
import re
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / 'crates/flyff-game/src/gm/table.rs'

# row -> handler path (a `fn(&mut CmdCtx, &mut CmdArgs) -> CmdResult`), relative to `crate::gm` unless it starts
# with `crate::`.
HANDLERS = {
    3: 'crate::chat::route::commands::cmd_whisper',
    4: 'crate::chat::route::commands::cmd_say',
    6: 'crate::chat::route::commands::cmd_shout',
    7: 'crate::chat::route::commands::cmd_party',
    9: 'crate::chat::route::commands::cmd_guild',
    10: 'crate::social::party::handlers::cmd_invite',
    11: 'crate::social::guild::handlers::cmd_invite',
    12: 'crate::social::campus::cmd_invite',
    13: 'crate::chat::privacy::cmd_trade_agree',
    14: 'crate::chat::privacy::cmd_trade_refuse',
    15: 'crate::chat::privacy::cmd_whisper_agree',
    16: 'crate::chat::privacy::cmd_whisper_refuse',
    17: 'crate::chat::privacy::cmd_messenger_agree',
    18: 'crate::chat::privacy::cmd_messenger_refuse',
    19: 'crate::chat::privacy::cmd_party_agree',
    20: 'crate::chat::privacy::cmd_party_refuse',
    21: 'crate::chat::privacy::cmd_connect_agree',
    22: 'crate::chat::privacy::cmd_connect_refuse',
    23: 'crate::chat::privacy::cmd_shout_agree',
    24: 'crate::chat::privacy::cmd_shout_refuse',
    25: 'crate::chat::privacy::cmd_ignore',
    26: 'crate::chat::privacy::cmd_unignore',
    28: 'cmd::move_::teleport',
    31: 'cmd::move_::summon',
    29: 'cmd::mode::invisible',
    30: 'cmd::mode::no_invisible',
    33: 'cmd::admin::out',
    34: 'cmd::mode::talk',
    35: 'cmd::mode::notalk',
    37: 'cmd::mode::mute',
    43: 'cmd::chat::system',
    44: 'cmd::progress::pvp_param',
    45: 'cmd::progress::pk_param',
    46: 'cmd::mode::undying',
    47: 'cmd::mode::undying2',
    48: 'cmd::mode::no_undying',
    49: 'cmd::mode::onekill',
    50: 'cmd::mode::no_onekill',
    51: 'cmd::char_::aroundkill',
    52: 'cmd::char_::stat',
    53: 'cmd::progress::level',
    54: 'cmd::progress::init_skill_exp',
    55: 'cmd::progress::skill_level',
    56: 'cmd::progress::skill_level_all',
    62: 'cmd::progress::change_job',
    63: 'cmd::mode::freeze',
    64: 'cmd::mode::no_freeze',
    65: 'cmd::party::party_level',
    66: 'cmd::social::guild_stat',
    67: 'cmd::social::create_guild',
    69: 'cmd::siege::gc_in',
    70: 'cmd::siege::gc_open',
    71: 'cmd::siege::gc_close',
    72: 'cmd::siege::gc_next',
    74: 'cmd::spawn::createnpc',
    77: 'crate::gm::rates::gamesetting',
    78: 'cmd::spawn::rmvnpc',
    79: 'cmd::progress::disguise',
    80: 'cmd::progress::no_disguise',
    81: 'cmd::items::resist_item',
    82: 'cmd::char_::jobname',
    83: 'cmd::char_::getgold',
    84: 'cmd::create::createitem',
    85: 'cmd::items::create_item2',
    88: 'crate::gm::rates::reload_constant',
    90: 'cmd::items::piercing',
    91: 'cmd::pets::pet_level',
    92: 'cmd::pets::pet_exp',
    94: 'cmd::pets::pet',
    99: 'cmd::items::refine_accessory',
    100: 'cmd::items::refine_collector',
    101: 'cmd::items::gen_random_option_cmd',
    102: 'cmd::items::init_random_option',
    103: 'cmd::items::set_random_option',
    104: 'cmd::pets::set_pet_name',
    106: 'cmd::social::propose',
    107: 'cmd::social::refuse',
    108: 'cmd::social::couple',
    109: 'cmd::social::decouple',
    110: 'cmd::social::clear_propose',
    111: 'cmd::social::next_couple_level',
    112: 'cmd::progress::remove_buff',
    113: 'cmd::social::honor_title_set',
    120: 'cmd::social::set_player_name',
    121: 'cmd::social::set_guild_name',
    125: 'cmd::mode::gm_item',
    126: 'cmd::mode::gm_not_item',
    127: 'cmd::mode::gm_attack',
    128: 'cmd::mode::gm_not_attack',
    129: 'cmd::mode::gm_community',
    130: 'cmd::mode::gm_not_community',
    131: 'cmd::mode::gm_observe',
    132: 'cmd::mode::gm_not_observe',
    133: 'cmd::items::escape_reset',
    138: 'cmd::siege::gc_request',
    139: 'cmd::siege::gc_cancel',
    144: 'cmd::create::inven_clear',
    145: 'cmd::mode::exp_up_stop',
    154: 'cmd::progress::angel_exp',
    162: 'cmd::items::item_level',
    186: 'cmd::items::remove_total_gold_cmd',
    187: 'cmd::items::set_tutorial_state',
    217: 'cmd::social::remove_campus_member',
    218: 'cmd::social::update_campus_point',
    219: 'cmd::create::inven_remove',
    'N01': 'cmd::info::help',
    'N02': 'cmd::move_::where_cmd',
    'N03': 'cmd::move_::goto',
    'N04': 'cmd::char_::heal',
    'N05': 'cmd::char_::kill',
    'N06': 'cmd::create::item_search',
    'N07': 'cmd::admin::setauth',
    'N08': 'cmd::admin::ban',
    'N09': 'cmd::admin::unban',
    'N10': 'cmd::admin::kickall',
    'N11': 'cmd::admin::save',
    'N12': 'cmd::admin::shutdown',
    'N13': 'crate::gm::rates::rates',
    'N15': 'cmd::mode::unmute',
    # ---- gm-events-quests (WP-GM-25)
    75: 'cmd::event::event_list',
    76: 'cmd::event::event_info',
    118: 'cmd::event::local_event',
    153: 'cmd::event::call_the_roll',
    163: 'cmd::event::coupon',
    87: 'cmd::reload::load_script',
    95: 'cmd::reload::lua',
    'N14': 'cmd::reload::reload',
    57: 'cmd::quest::begin_quest',
    58: 'cmd::quest::end_quest',
    59: 'cmd::quest::remove_quest',
    60: 'cmd::quest::remove_all_quest',
    61: 'cmd::quest::remove_complete_quest',
    86: 'cmd::quest::quest_state',
    135: 'cmd::quest::set_guild_quest',
    # ---- gm-content-control (WP-GM-26)
    96: 'cmd::content::gc1to1::gc1to1_open',
    97: 'cmd::content::gc1to1::gc1to1_close',
    98: 'cmd::content::gc1to1::gc1to1_next',
    166: 'cmd::content::secret_room::sr_open',
    167: 'cmd::content::secret_room::sr_next',
    171: 'cmd::content::secret_room::sr_close',
    178: 'cmd::content::lord::election_process',
    179: 'cmd::content::lord::election_begin_candidacy',
    180: 'cmd::content::lord::election_begin_vote',
    181: 'cmd::content::lord::election_end_vote',
    184: 'cmd::content::lord::lord_event_initialize',
    188: 'cmd::content::tax::tax_apply_now',
    193: 'cmd::content::layer::create_layer',
    194: 'cmd::content::layer::delete_layer',
    195: 'cmd::content::layer::layer',
    197: 'cmd::content::rainbow::rr_open',
    198: 'cmd::content::rainbow::rr_next',
    199: 'cmd::content::rainbow::rr_pass',
    209: 'cmd::content::housing::housing_gm_remove_all',
    211: 'cmd::content::quiz::quiz_event_open',
    213: 'cmd::content::quiz::quiz_state_next',
    214: 'cmd::content::quiz::quiz_event_close',
    'N17': 'cmd::content::control::content',
}

# Hand-written one-line help for the commands that have a handler; the others get the first clause of the table text.
HELP = {
    3: 'Whisper to a player: /w <name> <text>',
    4: 'Send a messenger instant message: /say <name> <text>',
    6: 'Shout to everybody nearby: /s <text>',
    7: 'Party chat: /p <text>',
    9: 'Guild chat: /g <text>',
    10: 'Invite a player to your party: /partyinvite <name>',
    28: 'Teleport to a player, to world x z, or to a world region key',
    31: 'Summon a player to your position',
    33: 'Kick a player out of the game',
    43: 'Send a server-wide notice',
    44: 'Set your fame: /p_Param <fame>',
    45: 'Set your PK value and propensity: /PKParam <pkValue> <propensity> (-1 keeps a value)',
    53: 'Rebuild your character as a job and level: /lv <job> <level> [1 master / 2 hero]',
    54: 'Reset your skills and get the skill points back',
    55: 'Set the level of one of your skills: /slv <skillId> <level>',
    56: 'Set all your job skills to their maximum level',
    62: 'Take over the skills of a job without changing level or stats: /cjob <job>',
    65: 'Set the level and points of your own party: /plv <level> [points=100]',
    79: 'Look like a monster: /dis <moverId or name>',
    80: 'End the disguise',
    112: 'Remove all your buffs and premium timers',
    154: 'Add angel experience: /aexp <n>',
    'N01': 'List the commands you may use, or show the usage of one',
    'N02': 'Show where a player is',
    'N03': 'Teleport inside the current world: /goto <x> <z> or <x> <y> <z>',
}

HELP.update({
    51: 'One hit on every monster within 64 m (kills with /ok); needs a weapon',
    52: 'Set a stat: /stat <str|sta|dex|int|gp|all|restate> <n>',
    74: 'Spawn monsters at your position: /cn <moverId or name> [count] [aggressive 0/1] [dps 0/1]',
    77: 'Show the game settings (rates, skill point costs); a GM sees all rates',
    78: 'Remove an NPC or monster by object id: /rn <objId>',
    88: 'Send the game settings to everybody again (Constant.inc needs a restart)',
    82: 'List the expert and professional jobs and their level ranges',
    83: 'Add gold: /gg <n>',
    84: 'Create an item: /ci <itemId or name> [num] [charged 0/1]',
    144: 'Remove every item of your inventory, equipment included',
    219: 'Remove every usable item that is not worn and not a pet',
    29: 'Become invisible to players below GM level',
    30: 'Become visible again',
    34: 'Let a muted-by-mode player talk again: /talk <name>',
    35: 'Silence a player (say, shout, whisper): /notalk <name>',
    37: 'Mute a player for some seconds: /mute <name> <seconds> (0 unmutes)',
    46: 'Take no damage',
    47: 'Take damage but never die',
    48: 'End /undying and /undying2',
    49: 'Every hit kills',
    50: 'End /onekill',
    63: 'Freeze a player in place (client side): /freeze <name>',
    64: 'Unfreeze a player: /nofreeze <name>',
    125: 'Cannot pick up or drop items',
    126: 'End /gmitem',
    127: 'Cannot attack',
    128: 'End /gmattck',
    129: 'Community mode (no server effect)',
    130: 'End /gmcommunity',
    131: 'Observe mode: no items, no attacks',
    132: 'End /gmobserve',
    145: 'Toggle: no experience from monsters',
    81: 'Set element, element level and +N of a bag item: /ritem <objId> <elem 0-5> <elemLv> <upgr>',
    85: 'Create an item with a stat bonus: /ci2 <item> <randomOptItemId>',
    90: 'Set the socket count of a bag item: /pier <objId> <n 0-10>',
    91: 'Level up the summoned pet when its EXP is full',
    92: 'Fill the EXP of the summoned pet (not grade S)',
    94: 'Rewrite the pet of a player: /pet <player> <kind> <exp%> <abilityDigits> <life>',
    99: 'Set the upgrade level of the accessory in bag slot 0: /ra [n]',
    100: 'Set the level of the collector in bag slot 0: /rc [n 0-5]',
    101: 'Roll the awakening or blessing of the item in bag slot 0 again',
    102: 'Clear the awakening or blessing of the item in bag slot 0',
    103: 'Set the random option lines of bag slot 0: /sro <dst> <adj> [<dst> <adj> ...]',
    104: 'Rename your summoned pet: /setpetname <name>',
    133: 'Reset the escape cooldown of a player id, or of yourself: /EscapeReset [idPlayer]',
    162: 'Level-down of the equipment in bag slot 0: /il [1 = -5, 2 = -10, else none]',
    186: 'Remove penya and Perins from your bag, Perins first: /rtg <amount>',
    187: 'Set your tutorial step: /sts <n>',
    66: 'Change your own guild: /gstat logo <n> | pxp <n> | penya <n> | notice <text>',
    67: 'Create a guild with yourself as its only member and master: /cg <name>',
    69: 'Enter the guild siege as through the NPC',
    70: 'Open the guild siege (only while it is closed)',
    71: 'Close the guild siege: /gcclose [0 = with results, other = cancelled]',
    72: 'Skip the current timer of the guild siege',
    106: 'Propose a couple to a player: /propose <name>',
    107: 'Refuse the pending couple proposal',
    108: 'Accept the pending couple proposal',
    109: 'Dissolve your couple',
    110: 'Forget every couple proposal cooldown',
    111: 'Add the experience your couple needs for its next level: /ncl',
    113: 'Set a honor title counter: /hts <titleIdx or name> <count 0..100000000>',
    120: 'Rename yourself without a scroll: /spn <newName>',
    121: 'Rename your guild (master only): /sgn <newName>',
    138: 'Apply for the guild siege with your guild: /gcrquest <penya>',
    139: 'Cancel the guild siege application of your guild',
    217: 'Remove a campus member: /rcm <name>',
    218: 'Add campus points to yourself: /ucp <n> (negative removes)',
    'N04': 'Fill HP, MP and FP of yourself or a player: /heal [name]',
    'N05': 'Kill your focused target or a player: /kill [name]',
    'N06': 'Search items by name: /item <text>',
    'N07': 'Set the authority of a character (or its account): /setauth <character> <level> [account]',
    'N08': 'Ban the account of a character: /ban <character> [duration 30m/12h/7d/perm] [reason]',
    'N09': 'Lift the bans of the account of a character: /unban <character>',
    'N10': 'Kick every player below GM level: /kickall [message]',
    'N11': 'Save one character or everybody now: /save [name]',
    'N12': 'Shut the server down after a countdown: /shutdown <seconds> [message] or /shutdown cancel',
    'N13': 'List the live rates, or set one (administrator): /rates [exp|drop|gold|hit|shop|party|aggressive <value|default>]',
    'N15': 'Unmute a player: /unmute <name>',
    5: 'Show your position',
    8: 'Show the real time and the game time',
    13: 'Accept trade requests',
    14: 'Refuse trade requests',
    15: 'Accept whispers and messages',
    16: 'Refuse whispers and messages',
    17: 'Accept friend requests',
    18: 'Refuse friend requests',
    19: 'Accept party invitations',
    20: 'Refuse party invitations',
    21: 'Show friend login notices',
    22: 'Hide friend login notices',
    23: 'Hear shouts',
    24: 'Do not hear shouts',
    25: 'Add a player to your ignore list',
    26: 'Remove a player from your ignore list',
    27: 'Open the ignore list',
    68: 'Disband your guild',
    93: 'Open the pet food mill',
    105: 'Clear your pet name',
    114: 'Open a window by name',
    115: 'Close a window by name',
})

# ---- gm-events-quests (WP-GM-25)
HELP.update({
    75: 'List the Lua events as lines of the chat box',
    76: 'Show the lines of one Lua event: /eventinfo <eventId>',
    118: 'Set an event flag: /le <eventId> <state>',
    153: 'Set your attendance bits: /ctr <n> (the days 1 to n are attended)',
    163: 'Set your event coupon counter: /coupon <n>',
    87: 'Reload the NPC dialog scripts from the scripts folder',
    95: 'Reload a Lua script: /lua event | rr (ms and dps need a restart)',
    'N14': 'Reload a part of the server: /reload config | dialog | lua',
    57: 'Start a quest on yourself: /bq <questId>',
    58: 'End a quest of yourself: /eq <questId>',
    59: 'Remove a quest from yourself or an online player: /rq <questId> [name]',
    60: 'Remove all quests of yourself, active and completed',
    61: 'Remove the completed quests of yourself',
    86: 'Set the state (0-13) of a quest of yourself or an online player: /qs <questId> <state> [name]',
    135: 'Set the state (0-14) of a guild quest: /sgq <guildName> <questId> <state>',
})

# ---- gm-content-control (WP-GM-26)
HELP.update({
    96: 'Open the 1:1 guild combat (only while closed): /gc1to1open [1 = server was down]',
    97: 'Close the 1:1 guild combat: every running arena ends with its result',
    98: 'Skip the current timer of the 1:1 guild combat',
    166: 'Open the secret room now (only while closed)',
    167: 'Skip the current wait of the secret room',
    171: 'End the sides of a running secret room war without a winner (only in the war state)',
    178: 'Run or stop the lord election process: /ep <0|1>',
    179: 'Start the lord candidacy now',
    180: 'Start the lord vote now',
    181: 'End the lord vote now and decide the winner',
    184: 'Clear every lord event',
    188: 'Change the tax owners and rates now (not while the lord election votes)',
    193: 'Create a GM layer in a loaded world: /cl <worldId> <layer>',
    194: 'Release a GM layer nobody is in: /dl <worldId> <layer>',
    195: 'Move yourself into a layer: /lay <worldId> <layer> <x> <z>',
    197: 'Open the rainbow race (only while closed)',
    198: 'Skip the current timer of the rainbow race',
    199: 'Count your current rainbow race game as won',
    209: 'Remove every item of your own housing room',
    211: 'Open the quiz event when none runs',
    213: 'Run the next quiz step now (only while a quiz runs and you stand in the quiz world)',
    214: 'Close the quiz event',
    'N17': 'State of the timed M7 features, and verbs that start, stop and advance them: /content [feature [verb]]',
})

# commands of this repository that have no row in the plan (appended to the plan's new commands, 8 cells like the
# table of Plan/23 §4.9: key, name (aliases), side, min, args, behaviour, milestone, WP)
REPO_NEW = [
    ['N17', 'content (ctl)', 'S', 'P', '[lord|tax|sr|gc1to1|rr|quiz|gh|housing [<verb> [<name>]]]',
     'one state line per timed M7 feature, or the line of one; a verb starts, stops or advances it (22 §20)', 'M7', 'WP-GM-26'],
]

# rows 13-26 were browser-local until the server-side privacy settings of WP-GM-22 (M6): the plan's side C->B means
# the browser sends them as ChatReq from M6 on (23 §3.10), so nothing is client-only any more
CLIENT_UNTIL_M6 = set()

# rows that run without an in-world issuer character (the others need `self`)
NO_SELF = {
    32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 59, 75, 76, 82, 86, 87, 88, 95, 118, 134, 135, 'N01', 'N02', 'N04', 'N05', 'N06',
    'N07', 'N08', 'N09', 'N10', 'N11', 'N12', 'N13', 'N14', 'N15', 'N16',
    # ---- gm-content-control (WP-GM-26); 195 /lay, 199 /rrpass, 209 /hgmra and 213 /qsn keep needs_self
    96, 97, 98, 166, 167, 171, 178, 179, 180, 181, 184, 188, 193, 194, 197, 198, 211, 214, 'N17',
}

AUTH = {
    'F': 'GENERAL', 'G': 'LOG_CHATTING', 'H': 'JOURNALIST', 'J': 'HELPER', 'L': 'GAMEMASTER', 'M': 'GAMEMASTER2',
    'N': 'GAMEMASTER3', 'O': 'OPERATOR', 'P': 'ADMINISTRATOR',
}


def rust_str(s: str) -> str:
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'


def clean(s: str) -> str:
    s = s.replace('\\|', '|')
    s = re.sub(r'\*\*([^*]+)\*\*', r'\1', s)
    return s.strip()


def strip_md(s: str) -> str:
    s = clean(s)
    s = s.replace('`', '')
    return s


def parse_name(cell: str):
    """`name (alias1, alias2, **alias3**; note)` -> (name, [aliases])."""
    cell = clean(cell)
    m = re.match(r'^(\S+)\s*(?:\((.*)\))?$', cell)
    if not m:
        raise SystemExit(f'cannot parse command cell {cell!r}')
    name, inner = m.group(1), m.group(2)
    aliases = []
    if inner:
        head = inner.split(';')[0]
        for a in head.split(','):
            a = a.strip().strip('`')
            if a and a.lower() != name.lower() and a not in aliases:
                aliases.append(a)
    # `announce`, `spawn` and `nt` are new aliases written in bold; the plain text keeps them in the list already
    return name, aliases


def first_clause(text: str) -> str:
    t = strip_md(text)
    t = re.split(r'(?<=[.;])\s', t, maxsplit=1)[0]
    t = t.rstrip('.;')
    if len(t) > 110:
        t = t[:107].rstrip() + '...'
    return t


def main() -> None:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    plan = args.plan
    check = args.check
    lines = plan.read_text(encoding='utf-8').split('\n')
    legacy, new = [], []
    for l in lines:
        m = re.match(r'^\| (\d+|N\d+) \|', l)
        if not m:
            continue
        cells = re.split(r'(?<!\\) \| ', l.strip()[1:-1].strip())
        key = m.group(1)
        if key.startswith('N') and len(cells) == 8:
            new.append(cells)
        elif len(cells) == 10:
            legacy.append(cells)
    assert len(legacy) == 219, len(legacy)
    assert len(new) == 16, len(new)
    new.extend(REPO_NEW)  # the repository's own command N17 (WP-GM-26) follows the plan's rows
    assert len(new) == 17, len(new)

    entries = []
    for c in legacy:
        row = int(c[0])
        name, aliases = parse_name(c[1])
        orig, side, minc, args, beh, dec, mile, wp = c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9]
        entries.append(dict(key=row, row=row, name=name, aliases=aliases, orig=orig.strip(), side=clean(side), minc=clean(minc),
                            args=clean(args), beh=beh, dec=clean(dec), mile=clean(mile), wp=clean(wp)))
    for c in new:
        key = c[0]
        name, aliases = parse_name(c[1])
        side, minc, args, beh, mile, wp = c[2], c[3], c[4], c[5], c[6], c[7]
        entries.append(dict(key=key, row=0, name=name, aliases=aliases, orig='NEW', side=clean(side), minc=clean(minc),
                            args=clean(args), beh=beh, dec='NEW', mile=clean(mile), wp=clean(wp)))

    out = []
    out.append('//! The command table of Plan/23 §4 (GENERATED by `tools/gen_gm_table.py`; do not edit by hand).')
    out.append('//!')
    out.append('//! One [`CommandSpec`] per legacy row 1..=219 (in table order, DROP/INACT rows as metadata) and the new commands')
    out.append('//! N01..N16 of the plan and the repository\'s N17. A `PORT` row without a handler in this repository is')
    out.append('//! `Deferred("<WP>")` and behaves like an unknown command (23 §3.4). Rerun the script when a work package adds a')
    out.append('//! handler.')
    out.append('')
    out.append('use super::cmd;')
    out.append('use super::{CmdSide, CmdStatus, CommandSpec, Dec};')
    out.append('use flyff_common::gm::Authority;')
    out.append('')
    out.append('/// Every command of 23 §4.2-§4.9 in table order.')
    out.append('pub static COMMANDS: &[CommandSpec] = &[')
    for e in entries:
        key = e['key']
        dec = e['dec']
        side_c = e['side']
        # decision
        if e['orig'] == 'NEW':
            decision = 'New'
        elif dec.startswith('CLIENT'):
            decision = 'Client'
        elif dec.startswith('DEFER'):
            decision = 'Defer'
        elif dec.startswith('DROP'):
            decision = 'Drop'
        elif dec.startswith('INACT'):
            decision = 'Inact'
        elif dec.startswith('SIMPL'):
            decision = 'Simpl'
        elif dec.startswith('PORT*'):
            decision = 'PortFix'
        elif dec.startswith('PORT'):
            decision = 'Port'
        else:
            raise SystemExit(f'row {key}: unknown decision {dec!r}')
        side = {'S': 'Server', 'C': 'Client', 'B': 'Both', 'C→B': 'Both', '—': 'Server', '-': 'Server'}.get(side_c)
        if side is None:
            raise SystemExit(f'row {key}: unknown side {side_c!r}')
        minc = e['minc']
        auth = AUTH.get(minc, 'ADMINISTRATOR')
        mile = re.sub(r'[^0-9]', '', e['mile']) or '0'
        wp = e['wp']
        wp_short = re.sub(r'\s*\(.*\)$', '', wp)
        handler = HANDLERS.get(key)
        reason = re.sub(r'^[A-Z*]+\s*\(?(.*?)\)?$', r'\1', dec).strip() or dec
        if key in CLIENT_UNTIL_M6:
            status = 'CmdStatus::Client'
            side = 'Client'
        elif decision in ('Port', 'PortFix', 'Simpl', 'New'):
            if handler:
                status = 'CmdStatus::Port'
            else:
                wp_name = wp_short if wp_short not in ('–', '-', '') else 'unassigned'
                status = f'CmdStatus::Deferred({rust_str(wp_name)})'
        elif decision == 'Client':
            status = 'CmdStatus::Client'
        elif decision == 'Defer':
            status = 'CmdStatus::Deferred("PL-147")'
        elif decision == 'Drop':
            status = f'CmdStatus::Drop({rust_str(reason)})'
        else:
            status = f'CmdStatus::Inactive({rust_str(reason)})'
        needs_self = (key not in NO_SELF) and decision in ('Port', 'PortFix', 'Simpl', 'New')
        item_ok = 'Item-privileged' in e['beh']
        usage = strip_md(e['args']) if e['args'] not in ('—', '-', '') else ''
        helptext = HELP.get(key) or first_clause(e['beh'])
        aliases = ', '.join(rust_str(a) for a in e['aliases'])
        if handler:
            hexpr = f'Some({handler})'
        else:
            hexpr = 'None'
        out.append('    CommandSpec {')
        out.append(f'        row: {e["row"]},')
        out.append(f'        name: {rust_str(e["name"])},')
        out.append(f'        aliases: &[{aliases}],')
        out.append(f'        min_auth: Authority::{auth},')
        out.append(f'        side: CmdSide::{side},')
        out.append(f'        status: {status},')
        out.append(f'        dec: Dec::{decision},')
        out.append(f'        needs_self: {str(needs_self).lower()},')
        out.append(f'        item_ok: {str(item_ok).lower()},')
        out.append(f'        usage: {rust_str(usage)},')
        out.append(f'        help: {rust_str(helptext)},')
        out.append(f'        milestone: {mile},')
        out.append(f'        handler: {hexpr},')
        out.append('    },')
    out.append('];')
    out.append('')
    text = '\n'.join(out)
    if check:
        cur = OUT.read_text(encoding='utf-8') if OUT.exists() else ''
        if cur != text:
            print(f'{OUT} is stale: rerun python3 -I tools/gen_gm_table.py', file=sys.stderr)
            sys.exit(1)
        return
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding='utf-8')
    print(f'wrote {OUT} ({len(entries)} entries)')


if __name__ == '__main__':
    main()
