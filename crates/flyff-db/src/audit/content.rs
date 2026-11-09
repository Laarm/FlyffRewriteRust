//! Constructors of the content audit events (Plan/13 §25.2, Plan/22 §2.8): `lord.*`, `tax.*`, `secret_room.*`,
//! `guild_house.*`, `housing.*`, `rainbow.*` and `quiz.*`.
//!
//! The events are emitted from the **same commit block** as the batch they describe (`AuditLog::emit`, non-blocking).
//! These functions fix the `data` shape of each event so that the lord, tax, secret-room, guild-house, housing,
//! rainbow-race and quiz code of different owners write the same records. Ids are plain numbers, a missing value is
//! `null`, penya amounts are `i64`, a continent is its code (15 East, 240 West, 255 All). `lord.*` uses the fields of
//! the catalog (`election, candidate, amount, skill, target`).
//!
//! | event | `data` |
//! |---|---|
//! | `lord.deposit` | `election`, `candidate`, `amount` (penya paid now), `total` (the deposit afterwards) |
//! | `lord.pledge` | `election`, `candidate`, `amount` (penya paid: 0 for the free first pledge) |
//! | `lord.vote` | `election`, `candidate` (voted for), `voter`, `votes` (the candidate's votes afterwards) |
//! | `lord.elected` | `election`, `candidate` (the winner or `null`), `votes` (his votes), `candidates` (the ranked field) |
//! | `lord.event` | `candidate` (the lord who created it), `amount` (cost in penya), `exp_factor`, `item_factor` |
//! | `lord.skill` | `candidate` (the lord), `skill`, `target` (character or `null`) |
//! | `tax.payout` | `round`, `paid_on` (`YYYY-MM-DD` or `null`), `mails`, `gold`, `perin` (totals paid) |
//! | `tax.rate` | `round`, `continent`, `guild`, `sales`, `purchase` (the next rates) |
//! | `secret_room.tender` | `round`, `continent`, `guild`, `member` (the master), `amount` (the tender afterwards), `paid` (penya paid now) |
//! | `secret_room.cancel` | `round`, `continent`, `guild`, `member`, `amount` (the tender), `refund` (80 %) |
//! | `secret_room.result` | `round`, `stage` (`open` or `close`), `tenders` (rows written), `failed`, `refunds` (total penya mailed), `winners` (`[{continent, guild}]`) |
//! | `guild_house.buy` | `guild`, `member`, `amount`, `upkeep_until` |
//! | `guild_house.upkeep` | `guild`, `member`, `amount`, `days`, `upkeep_until` |
//! | `guild_house.furniture_add` | `guild`, `member`, `item_id`, `seq` |
//! | `housing.furniture_add` | `member`, `item_id`, `keep_until` |
//! | `rainbow.apply` | `round`, `member`, `fee` |
//! | `rainbow.finish` | `round`, `member`, `ranking` |
//! | `rainbow.result` | `round`, `racers`, `winners`, `losers`, `failed`, `prizes` (mails) |
//! | `quiz.open` | `event`, `quiz_type`, `questions`, `by` (the GM or `null`) |
//! | `quiz.winner` | `event`, `member`, `prize_item`, `prize_count` |
//! | `quiz.import` | `file`, `questions`, `inserted`, `updated`, `unchanged`, `pruned`, `replace` (the type code or `null`) |

use super::{AuditEvent, ev};
use crate::model::{QuizType, TaxContinent};
use crate::quiz_import::QuizImportReport;
use flyff_common::{CharacterId, GuildId};
use serde_json::{Value, json};

// ---- lord -------------------------------------------------------------------------------------------------------

/// `lord.deposit`: `candidate` paid `amount` penya more; his deposit is `total` now.
pub fn lord_deposit(election: u32, candidate: CharacterId, amount: i64, total: i64) -> AuditEvent {
    AuditEvent::new(
        ev::LORD_DEPOSIT,
        json!({"election": election, "candidate": candidate.0, "amount": amount, "total": total}),
    )
}

/// `lord.pledge`: `candidate` set his pledge and paid `amount` penya (0 for the first one).
pub fn lord_pledge(election: u32, candidate: CharacterId, amount: i64) -> AuditEvent {
    AuditEvent::new(
        ev::LORD_PLEDGE,
        json!({"election": election, "candidate": candidate.0, "amount": amount}),
    )
}

/// `lord.vote`: `voter` voted for `candidate`, who has `votes` votes now.
pub fn lord_vote(
    election: u32,
    candidate: CharacterId,
    voter: CharacterId,
    votes: i32,
) -> AuditEvent {
    AuditEvent::new(
        ev::LORD_VOTE,
        json!({"election": election, "candidate": candidate.0, "voter": voter.0, "votes": votes}),
    )
}

/// `lord.elected`: the election ended; `lord` is the winner (`None` = nobody was elected) with `votes` votes out of
/// `candidates` ranked candidates. No player actor.
pub fn lord_elected(
    election: u32,
    lord: Option<CharacterId>,
    votes: i32,
    candidates: u32,
) -> AuditEvent {
    AuditEvent::new(
        ev::LORD_ELECTED,
        json!({
            "election": election, "candidate": lord.map(|c| c.0), "votes": votes,
            "candidates": candidates,
        }),
    )
}

/// `lord.event`: the lord created a global boost for `amount` penya.
pub fn lord_event(lord: CharacterId, amount: i64, exp_factor: f32, item_factor: f32) -> AuditEvent {
    AuditEvent::new(
        ev::LORD_EVENT,
        json!({
            "candidate": lord.0, "amount": amount, "exp_factor": exp_factor,
            "item_factor": item_factor,
        }),
    )
}

/// `lord.skill`: the lord used `skill` on `target` (`None` for a skill without a target).
pub fn lord_skill(lord: CharacterId, skill: u32, target: Option<CharacterId>) -> AuditEvent {
    AuditEvent::new(
        ev::LORD_SKILL,
        json!({"candidate": lord.0, "skill": skill, "target": target.map(|c| c.0)}),
    )
}

// ---- tax --------------------------------------------------------------------------------------------------------

/// `tax.payout`: the payout of `round` (`paid_on` = the date it recorded, `None` for a change without a date) mailed
/// `mails` mails with `gold` penya and `perin` Perins in total.
pub fn tax_payout(
    round: u32,
    paid_on: Option<&str>,
    mails: u32,
    gold: i64,
    perin: i64,
) -> AuditEvent {
    AuditEvent::new(
        ev::TAX_PAYOUT,
        json!({
            "round": round, "paid_on": paid_on, "mails": mails, "gold": gold, "perin": perin,
        }),
    )
}

/// `tax.rate`: the master of `guild` set the next rates of `continent`.
pub fn tax_rate(
    round: u32,
    continent: TaxContinent,
    guild: GuildId,
    sales: i32,
    purchase: i32,
) -> AuditEvent {
    AuditEvent::new(
        ev::TAX_RATE,
        json!({
            "round": round, "continent": continent.code(), "guild": guild.0, "sales": sales,
            "purchase": purchase,
        }),
    )
}

// ---- secret room ------------------------------------------------------------------------------------------------

/// `secret_room.tender`: the master tendered (or raised to) `amount`, paying `paid` now.
pub fn secret_room_tender(
    round: u32,
    continent: TaxContinent,
    guild: GuildId,
    member: CharacterId,
    amount: i64,
    paid: i64,
) -> AuditEvent {
    AuditEvent::new(
        ev::SECRET_ROOM_TENDER,
        json!({
            "round": round, "continent": continent.code(), "guild": guild.0, "member": member.0,
            "amount": amount, "paid": paid,
        }),
    )
}

/// `secret_room.cancel`: the master cancelled the tender of `amount` and got `refund` back.
pub fn secret_room_cancel(
    round: u32,
    continent: TaxContinent,
    guild: GuildId,
    member: CharacterId,
    amount: i64,
    refund: i64,
) -> AuditEvent {
    AuditEvent::new(
        ev::SECRET_ROOM_CANCEL,
        json!({
            "round": round, "continent": continent.code(), "guild": guild.0, "member": member.0,
            "amount": amount, "refund": refund,
        }),
    )
}

/// `secret_room.result`: a result batch (`stage` `open` for the failed ranking, `close` for the war result) wrote
/// `tenders` tender rows, `failed` of them failed the ranking, `refunds` penya were mailed; `winners` are the
/// guilds that won a side.
pub fn secret_room_result(
    round: u32,
    stage: &str,
    tenders: u32,
    failed: u32,
    refunds: i64,
    winners: &[(TaxContinent, GuildId)],
) -> AuditEvent {
    let winners: Vec<Value> = winners
        .iter()
        .map(|(continent, guild)| json!({"continent": continent.code(), "guild": guild.0}))
        .collect();
    AuditEvent::new(
        ev::SECRET_ROOM_RESULT,
        json!({
            "round": round, "stage": stage, "tenders": tenders, "failed": failed,
            "refunds": refunds, "winners": winners,
        }),
    )
}

// ---- guild house and housing ------------------------------------------------------------------------------------

/// `guild_house.buy`: the master bought the house for `amount` penya, upkeep paid until `upkeep_until`.
pub fn guild_house_buy(
    guild: GuildId,
    member: CharacterId,
    amount: i64,
    upkeep_until: i64,
) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_HOUSE_BUY,
        json!({
            "guild": guild.0, "member": member.0, "amount": amount, "upkeep_until": upkeep_until,
        }),
    )
}

/// `guild_house.upkeep`: `member` paid `amount` penya for `days` days, upkeep until `upkeep_until`.
pub fn guild_house_upkeep(
    guild: GuildId,
    member: CharacterId,
    amount: i64,
    days: u32,
    upkeep_until: i64,
) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_HOUSE_UPKEEP,
        json!({
            "guild": guild.0, "member": member.0, "amount": amount, "days": days,
            "upkeep_until": upkeep_until,
        }),
    )
}

/// `guild_house.furniture_add`: `member` listed the furniture item `item_id` as `seq`.
pub fn guild_house_furniture_add(
    guild: GuildId,
    member: CharacterId,
    item_id: u32,
    seq: u32,
) -> AuditEvent {
    AuditEvent::new(
        ev::GUILD_HOUSE_FURNITURE_ADD,
        json!({"guild": guild.0, "member": member.0, "item_id": item_id, "seq": seq}),
    )
}

/// `housing.furniture_add`: `member` listed the furniture or papering item `item_id`, kept until `keep_until`.
pub fn housing_furniture_add(member: CharacterId, item_id: u32, keep_until: i64) -> AuditEvent {
    AuditEvent::new(
        ev::HOUSING_FURNITURE_ADD,
        json!({"member": member.0, "item_id": item_id, "keep_until": keep_until}),
    )
}

// ---- rainbow race -----------------------------------------------------------------------------------------------

/// `rainbow.apply`: `member` applied for `round` and paid `fee` penya.
pub fn rainbow_apply(round: u32, member: CharacterId, fee: i64) -> AuditEvent {
    AuditEvent::new(
        ev::RAINBOW_APPLY,
        json!({"round": round, "member": member.0, "fee": fee}),
    )
}

/// `rainbow.finish`: `member` finished the race with rank `ranking`.
pub fn rainbow_finish(round: u32, member: CharacterId, ranking: i32) -> AuditEvent {
    AuditEvent::new(
        ev::RAINBOW_FINISH,
        json!({"round": round, "member": member.0, "ranking": ranking}),
    )
}

/// `rainbow.result`: the race of `round` ended with `racers` entries, `winners` finishers, `losers`, `failed`
/// no-shows and `prizes` prize mails.
pub fn rainbow_result(
    round: u32,
    racers: u32,
    winners: u32,
    losers: u32,
    failed: u32,
    prizes: u32,
) -> AuditEvent {
    AuditEvent::new(
        ev::RAINBOW_RESULT,
        json!({
            "round": round, "racers": racers, "winners": winners, "losers": losers,
            "failed": failed, "prizes": prizes,
        }),
    )
}

// ---- quiz -------------------------------------------------------------------------------------------------------

/// `quiz.open`: quiz `event` opened with `questions` questions of `quiz_type`, started by `by` (a GM) or by nobody.
pub fn quiz_open(
    event: u32,
    quiz_type: QuizType,
    questions: u32,
    by: Option<CharacterId>,
) -> AuditEvent {
    AuditEvent::new(
        ev::QUIZ_OPEN,
        json!({
            "event": event, "quiz_type": quiz_type.code(), "questions": questions,
            "by": by.map(|c| c.0),
        }),
    )
}

/// `quiz.winner`: `member` survived quiz `event` and got `prize_count` of `prize_item`.
pub fn quiz_winner(
    event: u32,
    member: CharacterId,
    prize_item: u32,
    prize_count: u32,
) -> AuditEvent {
    AuditEvent::new(
        ev::QUIZ_WINNER,
        json!({
            "event": event, "member": member.0, "prize_item": prize_item,
            "prize_count": prize_count,
        }),
    )
}

/// `quiz.import`: `flyff quiz import <file>` changed the questions as `report` says (`replace` = the type it replaced).
pub fn quiz_import(file: &str, report: &QuizImportReport, replace: Option<QuizType>) -> AuditEvent {
    AuditEvent::new(
        ev::QUIZ_IMPORT,
        json!({
            "file": file,
            "questions": report.inserted + report.updated + report.unchanged,
            "inserted": report.inserted, "updated": report.updated,
            "unchanged": report.unchanged, "pruned": report.pruned,
            "replace": replace.map(QuizType::code),
        }),
    )
}
