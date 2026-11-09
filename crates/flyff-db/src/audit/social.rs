//! Constructors of the social audit events (Plan/13 §25.2, Plan/21 §2.6): `guild.*`, `siege.*`, `couple.*` and
//! `campus.*`.
//!
//! The events are emitted from the **same commit block** as the batch they describe (`AuditLog::emit`, non-blocking).
//! These functions fix the `data` shape of each event so that the guild, war, siege and relation code of different
//! owners write the same records. Ids are plain numbers, a missing value is `null`, penya amounts are `i64`.
//!
//! | event | `data` |
//! |---|---|
//! | `guild.create`, `guild.disband` | `guild`, `name`, `member` (the master), `rank` 0 |
//! | `guild.rename` | `guild`, `name` (new), `old_name`, `member` (the master), `rank` 0 |
//! | `guild.member_join`, `guild.member_leave` | `guild`, `name`, `member`, `rank` |
//! | `guild.member_kick` | `guild`, `name`, `member`, `rank`, `by` |
//! | `guild.rank` | `guild`, `name`, `member`, `rank` (new), `old_rank`, `by` |
//! | `guild.master` | `guild`, `name`, `member` (new master), `rank` 0, `old_master` |
//! | `guild.bank` | `guild`, `member`, `kind` (the log letter), `item` (item JSON or `null`), `gold` |
//! | `guild.contribution` | `guild`, `member`, `pxp`, `gold`, `level_up`, `total` (the guild's penya pool afterwards) |
//! | `guild.salary` | `guild`, `gold` (paid in total), `members`, `online`, `offline`, `total` (pool afterwards), `paid_on` |
//! | `guild.war_start` | `war`, `declarer`, `acceptor`, `result` `null`, `points` `null` |
//! | `guild.war_end` | `war`, `declarer`, `acceptor`, `result` (`WR_*`), `points` (`{decl, acpt}`) |
//! | `siege.bid` | `siege`, `guild`, `amount`, `status` (10 or 12), `member` |
//! | `siege.cancel` | `siege`, `guild`, `amount` (the bid), `refund`, `status` 11, `member` |
//! | `siege.result` | `siege`, `winner` (guild or `null`), `streak`, `best_player`, `bidders`, `refunds` (total penya) |
//! | `siege.claim` | `siege`, `guild`, `member`, `amount`, `status` 31, `what` (`guild` or `player`) |
//! | `siege.chips` | `siege`, `guild`, `member`, `chips`, `rank`, `mailed` (the inventory was full) |
//! | `couple.create`, `couple.break` | `couple`, `member` (who acted), `partner`, `points` `null`, `reason` |
//! | `campus.join`, `campus.leave` | `campus`, `member`, `role`, `points` `null`, `reason` |
//! | `campus.points` | `campus` `null`, `member`, `points` (balance afterwards), `delta`, `reason` |

use super::{AuditEvent, ev};
use crate::model::{GuildBankLogKind, ItemRecord};
use flyff_common::{CharacterId, GuildId, GuildWarId};
use serde_json::{Value, json};

/// `couple.create`.
pub const COUPLE_CREATE: &str = "couple.create";
/// `couple.break`.
pub const COUPLE_BREAK: &str = "couple.break";
/// `campus.join`.
pub const CAMPUS_JOIN: &str = "campus.join";
/// `campus.leave`.
pub const CAMPUS_LEAVE: &str = "campus.leave";
/// `campus.points`.
pub const CAMPUS_POINTS: &str = "campus.points";

fn roster_event(
    name: &'static str,
    guild: GuildId,
    guild_name: &str,
    member: CharacterId,
    rank: u8,
) -> AuditEvent {
    AuditEvent::new(
        name,
        json!({"guild": guild.0, "name": guild_name, "member": member.0, "rank": rank}),
    )
}

// ---- guilds -----------------------------------------------------------------------------------------------------

/// `guild.create`: the guild exists, `master` is its master.
pub fn guild_create(guild: GuildId, name: &str, master: CharacterId) -> AuditEvent {
    roster_event(ev::GUILD_CREATE, guild, name, master, 0)
}

/// `guild.disband`: the guild was dissolved by its master (or an admin on his behalf).
pub fn guild_disband(guild: GuildId, name: &str, master: CharacterId) -> AuditEvent {
    roster_event(ev::GUILD_DISBAND, guild, name, master, 0)
}

/// `guild.rename`: the guild got `name` (first naming has `old_name` empty).
pub fn guild_rename(guild: GuildId, old_name: &str, name: &str, master: CharacterId) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_RENAME,
        json!({
            "guild": guild.0, "name": name, "old_name": old_name, "member": master.0, "rank": 0,
        }),
    )
}

/// `guild.member_join`: `member` joined with `rank` (a new member is a Rookie, rank 4).
pub fn guild_member_join(guild: GuildId, name: &str, member: CharacterId, rank: u8) -> AuditEvent {
    roster_event(ev::GUILD_MEMBER_JOIN, guild, name, member, rank)
}

/// `guild.member_leave`: `member` left the guild.
pub fn guild_member_leave(guild: GuildId, name: &str, member: CharacterId, rank: u8) -> AuditEvent {
    roster_event(ev::GUILD_MEMBER_LEAVE, guild, name, member, rank)
}

/// `guild.member_kick`: `by` (the master) removed `member`.
pub fn guild_member_kick(
    guild: GuildId,
    name: &str,
    member: CharacterId,
    rank: u8,
    by: CharacterId,
) -> AuditEvent {
    let mut event = roster_event(ev::GUILD_MEMBER_KICK, guild, name, member, rank);
    if let Value::Object(map) = &mut event.data {
        map.insert("by".to_owned(), Value::from(by.0));
    }
    event
}

/// `guild.rank`: `by` changed the rank of `member` from `old_rank` to `rank`.
pub fn guild_rank(
    guild: GuildId,
    name: &str,
    member: CharacterId,
    old_rank: u8,
    rank: u8,
    by: CharacterId,
) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_RANK,
        json!({
            "guild": guild.0, "name": name, "member": member.0, "rank": rank,
            "old_rank": old_rank, "by": by.0,
        }),
    )
}

/// `guild.master`: the mastership passed from `old_master` to `new_master`.
pub fn guild_master(
    guild: GuildId,
    name: &str,
    old_master: CharacterId,
    new_master: CharacterId,
) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_MASTER,
        json!({
            "guild": guild.0, "name": name, "member": new_master.0, "rank": 0,
            "old_master": old_master.0,
        }),
    )
}

/// `guild.bank` (also a `guild_bank_log` row): `kind` is the log letter (`A`, `D`, `O`, `C`, `R`, ...), `item` the
/// item JSON without `obj_id` ([`AuditEvent::item`]) or `null`, `gold` the penya moved (0 for a pure item move).
pub fn guild_bank(
    guild: GuildId,
    member: CharacterId,
    kind: GuildBankLogKind,
    item: Option<&ItemRecord>,
    gold: i64,
) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_BANK,
        json!({
            "guild": guild.0,
            "member": member.0,
            "kind": kind.code().to_string(),
            "item": item.map_or(Value::Null, AuditEvent::item),
            "gold": gold,
        }),
    )
}

/// `guild.contribution`: `member` gave `pxp` and `gold`; `level_up` when the guild level rose; `total` is the guild's
/// penya pool afterwards.
pub fn guild_contribution(
    guild: GuildId,
    member: CharacterId,
    pxp: u32,
    gold: u32,
    level_up: bool,
    total: u32,
) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_CONTRIBUTION,
        json!({
            "guild": guild.0, "member": member.0, "pxp": pxp, "gold": gold,
            "level_up": level_up, "total": total,
        }),
    )
}

/// `guild.salary`: one paying guild paid `gold` penya in total to `members` members (`online` credited at once,
/// `offline` through pending grants) on `paid_on`; `total` is the pool afterwards.
pub fn guild_salary(
    guild: GuildId,
    gold: i64,
    online: u32,
    offline: u32,
    total: u32,
    paid_on: &str,
) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_SALARY,
        json!({
            "guild": guild.0, "gold": gold, "members": online + offline, "online": online,
            "offline": offline, "total": total, "paid_on": paid_on,
        }),
    )
}

/// `guild.war_start`.
pub fn guild_war_start(war: GuildWarId, declarer: GuildId, acceptor: GuildId) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_WAR_START,
        json!({
            "war": war.0, "declarer": declarer.0, "acceptor": acceptor.0,
            "result": Value::Null, "points": Value::Null,
        }),
    )
}

/// `guild.war_end`: `result` is the exact `WR_*` code, `points` the win points of both guilds after the war.
pub fn guild_war_end(
    war: GuildWarId,
    declarer: GuildId,
    acceptor: GuildId,
    result: u8,
    decl_points: i32,
    acpt_points: i32,
) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_WAR_END,
        json!({
            "war": war.0, "declarer": declarer.0, "acceptor": acceptor.0, "result": result,
            "points": {"decl": decl_points, "acpt": acpt_points},
        }),
    )
}

// ---- siege ------------------------------------------------------------------------------------------------------

/// `siege.bid`: `member` (the master) bid `amount` for `guild`; `status` is 10, or 12 when applying again.
pub fn siege_bid(
    siege: u32,
    guild: GuildId,
    member: CharacterId,
    amount: i64,
    status: u8,
) -> AuditEvent {
    AuditEvent::new(
        ev::SIEGE_BID,
        json!({
            "siege": siege, "guild": guild.0, "member": member.0, "amount": amount, "status": status,
        }),
    )
}

/// `siege.cancel`: the bid of `amount` was cancelled and `refund` mailed to the master.
pub fn siege_cancel(
    siege: u32,
    guild: GuildId,
    member: CharacterId,
    amount: i64,
    refund: i64,
) -> AuditEvent {
    AuditEvent::new(
        ev::SIEGE_CANCEL,
        json!({
            "siege": siege, "guild": guild.0, "member": member.0, "amount": amount,
            "refund": refund, "status": 11,
        }),
    )
}

/// `siege.result`: the siege ended; `refunds` is the penya mailed back to the `bidders` that were not admitted.
pub fn siege_result(
    siege: u32,
    winner: Option<GuildId>,
    streak: i32,
    best_player: Option<CharacterId>,
    bidders: u32,
    refunds: i64,
) -> AuditEvent {
    AuditEvent::new(
        ev::SIEGE_RESULT,
        json!({
            "siege": siege, "winner": winner.map(|g| g.0), "streak": streak,
            "best_player": best_player.map(|c| c.0), "bidders": bidders, "refunds": refunds,
        }),
    )
}

/// `siege.claim`: `member` collected `amount` penya (`what` is `"guild"` or `"player"`).
pub fn siege_claim(
    siege: u32,
    guild: GuildId,
    member: CharacterId,
    amount: i64,
    what: &str,
) -> AuditEvent {
    AuditEvent::new(
        ev::SIEGE_CLAIM,
        json!({
            "siege": siege, "guild": guild.0, "member": member.0, "amount": amount,
            "status": 31, "what": what,
        }),
    )
}

/// `siege.chips`: `member` of the guild ranked `rank` (1..=3) got `chips` red chips, `mailed` when the inventory was
/// full and the chips went by system mail.
pub fn siege_chips(
    siege: u32,
    guild: GuildId,
    member: CharacterId,
    chips: u32,
    rank: u8,
    mailed: bool,
) -> AuditEvent {
    AuditEvent::new(
        ev::SIEGE_CHIPS,
        json!({
            "siege": siege, "guild": guild.0, "member": member.0, "chips": chips,
            "rank": rank, "mailed": mailed,
        }),
    )
}

// ---- couples and campus -----------------------------------------------------------------------------------------

/// `couple.create`: `member` and `partner` became a couple.
pub fn couple_create(couple: u32, member: CharacterId, partner: CharacterId) -> AuditEvent {
    AuditEvent::new(
        COUPLE_CREATE,
        json!({
            "couple": couple, "member": member.0, "partner": partner.0,
            "points": Value::Null, "reason": "accept",
        }),
    )
}

/// `couple.break`: `member` ended the couple with `partner`.
pub fn couple_break(couple: u32, member: CharacterId, partner: CharacterId) -> AuditEvent {
    AuditEvent::new(
        COUPLE_BREAK,
        json!({
            "couple": couple, "member": member.0, "partner": partner.0,
            "points": Value::Null, "reason": "decouple",
        }),
    )
}

/// `campus.join`: `member` joined `campus` as `role` (`"master"` or `"pupil"`).
pub fn campus_join(campus: u32, member: CharacterId, role: &str) -> AuditEvent {
    AuditEvent::new(
        CAMPUS_JOIN,
        json!({
            "campus": campus, "member": member.0, "role": role, "points": Value::Null, "reason": "accept",
        }),
    )
}

/// `campus.leave`: `member` left `campus` (`reason`: `remove`, `leave`, `graduate`, `dissolve`).
pub fn campus_leave(campus: u32, member: CharacterId, role: &str, reason: &str) -> AuditEvent {
    AuditEvent::new(
        CAMPUS_LEAVE,
        json!({
            "campus": campus, "member": member.0, "role": role, "points": Value::Null, "reason": reason,
        }),
    )
}

/// `campus.points`: the balance of `member` changed by `delta` to `points` (`reason`: `level`, `delete`,
/// `recovery`); `campus` is `null` when the character is in none.
pub fn campus_points(
    campus: Option<u32>,
    member: CharacterId,
    points: i32,
    delta: i32,
    reason: &str,
) -> AuditEvent {
    AuditEvent::new(
        CAMPUS_POINTS,
        json!({
            "campus": campus, "member": member.0, "points": points, "delta": delta, "reason": reason,
        }),
    )
}
