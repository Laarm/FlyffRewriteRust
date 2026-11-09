//! Guild queries (Plan/25 §6.1, §11.5; **WP-ADMIN-23**): `GuildList`, `GuildDetail`, `GuildNameCheck` and
//! `PartyMembers`, plus the guild-bank snapshot behind `Containers{GuildBank(g)}`.
//!
//! The guild model lives on the game thread, so the list, the detail, the roster, the bank and the pools are always
//! answered from `Services.social.guild` (never from the database); only the bank log and the war history come from
//! the DB (the route reads them). Like every query these take `&Game`: nothing here changes, wakes or schedules
//! anything (ADM-D4).

use super::AdminQueryResult;
use crate::admin::watch::{
    ContainerKind, ContainerRef, ContainerSnapshot, Region, capacity_of, region_items,
};
use crate::social::guild::model::max_members;
use crate::social::guild::{Guild, GuildMemberState, GuildRank};
use crate::tick::Game;
use flyff_common::{AccountId, CharacterId, GuildId, GuildWarId, PartyId};
use flyff_rules::names::{NameError, NameKind, NameLimits, validate_name};
use flyff_rules::social::guild_rank_cap;

/// Which guilds `GuildList` returns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuildFilter {
    /// A name fragment (case-insensitive contains), or the exact guild id when it parses as a number; empty = all.
    pub name: String,
    /// At least this guild level.
    pub level_min: Option<u8>,
    /// At most this guild level.
    pub level_max: Option<u8>,
    /// `Some(true)`: only guilds at war; `Some(false)`: only guilds at peace.
    pub at_war: Option<bool>,
    /// Only the guild this character belongs to.
    pub member: Option<CharacterId>,
}

/// One row of the guild list (Plan/25 §6.1 columns).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildRow {
    /// The guild.
    pub id: GuildId,
    /// The name; empty until the master named the guild.
    pub name: String,
    /// `true` once the guild has its name.
    pub named: bool,
    /// Level `1..=50`.
    pub level: u8,
    /// The logo (0 none, 1..=27, 999 a custom cloak).
    pub logo: u32,
    /// The master and his name; `None` for a guild whose master was purged.
    pub master: Option<(CharacterId, String)>,
    /// Members.
    pub members: u32,
    /// Members in the world.
    pub online: u32,
    /// The member limit of the level.
    pub max_members: u32,
    /// The penya pool.
    pub gold: u32,
    /// The contribution points.
    pub pxp: u32,
    /// The running war and the enemy guild.
    pub war: Option<(GuildWarId, GuildId)>,
    /// Creation time (unix seconds).
    pub created_at: i64,
}

/// The war record of a guild.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WarRecord {
    /// Wins.
    pub wins: i32,
    /// Losses.
    pub losses: i32,
    /// Surrenders.
    pub surrenders: i32,
    /// War points.
    pub points: i32,
}

/// One member in the detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildMemberRow {
    /// The character.
    pub character: CharacterId,
    /// Its name (`#id` when the directory does not know it).
    pub name: String,
    /// Its account.
    pub account: AccountId,
    /// The rank.
    pub rank: GuildRank,
    /// The class within the rank `0..=2`.
    pub class: u8,
    /// The nickname.
    pub alias: String,
    /// The character level.
    pub level: u8,
    /// The job id.
    pub job: u8,
    /// 0 male, 1 female.
    pub sex: u8,
    /// In the world.
    pub online: bool,
    /// Penya contributed.
    pub give_gold: i32,
    /// Contribution points given.
    pub give_pxp: u32,
    /// Surrenders of this member.
    pub surrenders: i32,
    /// When the member joined (unix seconds).
    pub joined_at: i64,
}

/// How many log rows the in-memory tail holds per listable kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogTailCounts {
    /// Kind `A`.
    pub put: u32,
    /// Kind `D`.
    pub taken: u32,
    /// Kind `I`.
    pub penya_in: u32,
    /// Kind `O`.
    pub penya_out: u32,
}

/// `GuildDetail`: the row plus what the drawer shows.
#[derive(Clone, Debug, PartialEq)]
pub struct GuildDetail {
    /// The list row.
    pub row: GuildRow,
    /// The notice.
    pub notice: String,
    /// The permission bits per rank (Master .. Rookie).
    pub powers: [u32; 5],
    /// The daily salary per rank.
    pub salaries: [u32; 5],
    /// Wins, losses, surrenders and points.
    pub war_record: WarRecord,
    /// The name of the enemy guild of the running war.
    pub enemy_name: Option<String>,
    /// How many members each rank may have at this level (Master .. Rookie).
    pub rank_caps: [u32; 5],
    /// The roster: rank order, then name.
    pub members: Vec<GuildMemberRow>,
    /// The bank (42 cells and the pool).
    pub bank: ContainerSnapshot,
    /// The log tail counts per kind.
    pub log_tail: LogTailCounts,
}

/// Why a guild name is not usable: a code of the console and its English text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameProblem {
    /// `length`, `chars`, `forbidden` or `taken`.
    pub code: &'static str,
    /// For the operator.
    pub message: String,
}

/// The answer of `GuildNameCheck`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildNameCheck {
    /// `true` when the name can be given to a guild now.
    pub ok: bool,
    /// The trimmed name.
    pub name: String,
    /// What is wrong.
    pub error: Option<NameProblem>,
}

fn clamp_u8(v: i32) -> u8 {
    u8::try_from(v.clamp(0, i32::from(u8::MAX))).unwrap_or(u8::MAX)
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The list row of `g`.
fn row_of(game: &Game, g: &Guild) -> GuildRow {
    let dir = &game.services.presence;
    let online = g
        .members
        .keys()
        .filter(|c| dir.get(**c).is_some_and(|e| e.online.is_some()))
        .count();
    GuildRow {
        id: g.id,
        name: g.name.clone(),
        named: g.is_named(),
        level: clamp_u8(g.level),
        logo: g.logo,
        master: g.master().map(|m| {
            (
                m,
                dir.get(m)
                    .map_or_else(|| format!("#{}", m.0), |e| e.name.clone()),
            )
        }),
        members: count(g.members.len()),
        online: count(online),
        max_members: count(max_members(&game.data, g.level)),
        gold: g.gold,
        pxp: g.pxp,
        war: g.war.zip(g.enemy),
        created_at: g.created_at.0,
    }
}

/// Does `g` pass the filter?
fn matches(game: &Game, g: &Guild, f: &GuildFilter, needle: &str, id: Option<u32>) -> bool {
    if !needle.is_empty() && id != Some(g.id.0) && !g.name.to_lowercase().contains(needle) {
        return false;
    }
    if f.level_min.is_some_and(|m| g.level < i32::from(m))
        || f.level_max.is_some_and(|m| g.level > i32::from(m))
    {
        return false;
    }
    if f.at_war.is_some_and(|w| w != g.at_war()) {
        return false;
    }
    if let Some(c) = f.member
        && game.services.social.guild.guild_of(c) != Some(g.id)
    {
        return false;
    }
    true
}

/// `GuildList`: every matching guild in id order (the guild count is small; the route pages).
pub(super) fn guild_list(game: &Game, filter: &GuildFilter) -> AdminQueryResult {
    let needle = filter.name.trim().to_lowercase();
    let id = needle.parse::<u32>().ok();
    AdminQueryResult::Guilds(
        game.services
            .social
            .guild
            .guilds()
            .filter(|g| matches(game, g, filter, &needle, id))
            .map(|g| row_of(game, g))
            .collect(),
    )
}

/// The snapshot of the bank of `g`: kind `GuildBank`, the guild id as owner, 42 cells with the display slot as
/// `obj_id` (the slot `GuildEdit::BankRemove` names), the pool as `gold`, always live.
pub(crate) fn bank_snapshot(g: &Guild) -> ContainerSnapshot {
    ContainerSnapshot {
        container: ContainerRef {
            kind: ContainerKind::GuildBank,
            index: 0,
            owner_character_id: g.id.0,
        },
        rev: 0,
        capacity: capacity_of(&g.bank.items, Region::Bag),
        items: region_items(&g.bank.items, Region::Bag),
        gold: u64::from(g.gold),
        live: true,
        saved_at: 0,
        live_via_character_id: 0,
    }
}

/// `Containers{GuildBank(guild)}`: the bank snapshot of an existing guild.
pub(super) fn bank_container(game: &Game, guild: GuildId) -> AdminQueryResult {
    match game.services.social.guild.get(guild) {
        Some(g) => AdminQueryResult::Container(Box::new(bank_snapshot(g))),
        None => AdminQueryResult::not_found(format!("guild {} does not exist", guild.0)),
    }
}

fn member_row(game: &Game, m: &GuildMemberState) -> GuildMemberRow {
    let e = game.services.presence.get(m.id);
    GuildMemberRow {
        character: m.id,
        name: e.map_or_else(|| format!("#{}", m.id.0), |e| e.name.clone()),
        account: e.map_or(AccountId(0), |e| e.account),
        rank: m.rank,
        class: m.class,
        alias: m.alias.clone(),
        level: e.map_or(0, |e| e.level),
        job: e.map_or(0, |e| e.job),
        sex: e.map_or(0, |e| e.sex),
        online: e.is_some_and(|e| e.online.is_some()),
        give_gold: m.give_gold,
        give_pxp: m.give_pxp,
        surrenders: m.surrenders,
        joined_at: m.joined_at.0,
    }
}

/// `GuildDetail`: the row, the settings, the roster (rank order, then name), the bank and the log tail counts; an
/// unknown id is `NotFound`.
pub(super) fn guild_detail(game: &Game, guild: GuildId) -> AdminQueryResult {
    let Some(g) = game.services.social.guild.get(guild) else {
        return AdminQueryResult::not_found(format!("guild {} does not exist", guild.0));
    };
    let mut members: Vec<GuildMemberRow> =
        g.members.values().map(|m| member_row(game, m)).collect();
    members.sort_by(|a, b| {
        a.rank
            .cmp(&b.rank)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then(a.character.cmp(&b.character))
    });
    let max = max_members(&game.data, g.level);
    let mut caps = [0u32; 5];
    for (cap, rank) in caps.iter_mut().zip(GuildRank::ALL) {
        *cap = count(guild_rank_cap(rank.as_u8(), max));
    }
    let name_of = |id: GuildId| game.services.social.guild.get(id).map(|e| e.name.clone());
    AdminQueryResult::Guild(Box::new(GuildDetail {
        row: row_of(game, g),
        notice: g.notice.clone(),
        powers: g.powers,
        salaries: g.salaries,
        war_record: WarRecord {
            wins: g.war_wins,
            losses: g.war_losses,
            surrenders: g.war_surrenders,
            points: g.war_points,
        },
        enemy_name: g.enemy.and_then(name_of),
        rank_caps: caps,
        members,
        bank: bank_snapshot(g),
        log_tail: LogTailCounts {
            put: count(g.log_tail.put.len()),
            taken: count(g.log_tail.taken.len()),
            penya_in: count(g.log_tail.penya_in.len()),
            penya_out: count(g.log_tail.penya_out.len()),
        },
    }))
}

/// The code and the console's text of a refused name.
fn problem_of(e: NameError) -> NameProblem {
    let (code, message) = match e {
        NameError::Empty => ("length", "the name is empty"),
        NameError::Length => ("length", "a guild name has 3 to 16 characters"),
        NameError::LeadingDigit => ("chars", "a guild name must not start with a digit"),
        NameError::InvalidChar => ("chars", "a guild name may contain letters and digits only"),
        NameError::Forbidden => ("forbidden", "that name is not allowed"),
        #[allow(unreachable_patterns)]
        _ => ("forbidden", "the name is not valid"),
    };
    NameProblem {
        code,
        message: message.to_owned(),
    }
}

/// `GuildNameCheck`: the name rules of `create.rs::validate_guild_name` (3..=16 characters, UD-21, the character
/// rules and the forbidden list) and the case-insensitive uniqueness against the live guilds (PL-058).
pub(super) fn guild_name_check(game: &Game, raw: &str) -> AdminQueryResult {
    let g = &game.cfg.rules.gameplay;
    let trimmed = raw.trim().to_owned();
    let checked = validate_name(
        NameKind::Guild,
        raw,
        &game.data.name_rules,
        NameLimits {
            min_name_letters: g.min_name_letters,
            max_name_letters: g.max_name_letters,
        },
        None,
    );
    let (name, error) = match checked {
        Err(e) => (trimmed, Some(problem_of(e))),
        Ok(name) => {
            let error = game
                .services
                .social
                .guild
                .by_name(&name)
                .map(|id| NameProblem {
                    code: "taken",
                    message: format!("a guild is called that already (guild #{})", id.0),
                });
            (name, error)
        }
    };
    AdminQueryResult::GuildName(GuildNameCheck {
        ok: error.is_none(),
        name,
        error,
    })
}

/// `PartyMembers`: the members of a party, in party order (the grant dialog's "party of …"); an unknown party is
/// `NotFound`.
pub(super) fn party_members(game: &Game, party: PartyId) -> AdminQueryResult {
    match game.services.social.party.get(party) {
        Some(p) => AdminQueryResult::Party(p.members.iter().map(|m| m.id).collect()),
        None => AdminQueryResult::not_found(format!("party {} does not exist", party.0)),
    }
}
