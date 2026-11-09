//! Typed constructors for the atomic batches of the social systems (Plan/13 §18, Plan/21 §2.4) and the op lists
//! that several batches share. Style of [`crate::batches`]: every constructor is one row of the §18 table, so the
//! guild, siege and war code cannot forget a snapshot, a log row or a counter raise. Attach the answer with
//! [`AtomicBatch::with_ticket`].
//!
//! | constructor | [`AtomicKind`] | snapshots | ops (in order) |
//! |---|---|---|---|
//! | [`AtomicBatch::guild_create`] | `GuildCreate` | master (charge paid) | [`DbOp::guild_create_ops`], `RaiseCounter(next_guild_id)` |
//! | [`AtomicBatch::guild_disband`] | `GuildDisband` | - | [`DbOp::guild_disband_ops`] |
//! | [`AtomicBatch::guild_rename`] | `GuildRename` | master (scroll consumed) | `UpsertGuild` |
//! | [`AtomicBatch::guild_bank_put_item`] | `GuildBankPutItem` | member | `SetGuildBank`, log `A` |
//! | [`AtomicBatch::guild_bank_take_item`] | `GuildBankTakeItem` | member | `SetGuildBank`, log `D` |
//! | [`AtomicBatch::guild_bank_take_gold`] | `GuildBankTakeGold` | member | `UpsertGuild`, `UpsertGuildMember`, log `O` |
//! | [`AtomicBatch::guild_contribution`] | `GuildContribution` | member | `UpsertGuild`, `UpsertGuildMember`, log `I`/`L` x n |
//! | [`AtomicBatch::guild_cloak`] | `GuildCloak` | - | `UpsertGuild`, `SetGuildBank`, log `C` |
//! | [`AtomicBatch::guild_salary`] | `GuildSalary` | online paid members | `UpsertGuild`, `InsertPendingGrant` x n, log `P`, `SetServerKv(guild.salary_last_paid_on)` |
//! | [`AtomicBatch::siege_bid`] | `SiegeBid` | master (difference paid) | `UpsertGuildSiegeBid` |
//! | [`AtomicBatch::siege_cancel`] | `SiegeCancel` | - | `UpsertGuildSiegeBid` (11), `InsertMail` + `RaiseCounter(next_mail_id)` |
//! | [`AtomicBatch::siege_result`] | `SiegeResult` | online chip recipients | `UpsertGuildSiege`, bids, players, `InsertMail` x n, `UpsertGuildSiege` (next), `RaiseCounter` x 2 |
//! | [`AtomicBatch::siege_claim_guild`] / [`AtomicBatch::siege_claim_player`] | `SiegeClaim` | claimer | `UpsertGuildSiegeBid` / `UpsertGuildSiegePlayer` (31) |
//! | [`AtomicBatch::gc1to1_tender`] / [`gc1to1_cancel`](AtomicBatch::gc1to1_cancel) / [`gc1to1_refund`](AtomicBatch::gc1to1_refund) | `Gc1to1Tender` / `Gc1to1Cancel` / `Gc1to1Refund` | master | `UpsertGc1to1Tender`, `RaiseCounter(gc1to1_round)` |
//! | [`AtomicBatch::siege_open`] | `Other("SiegeOpen")` | - | `UpsertGuildSiege` (20), bids (20), `ExpireSiegeRewards` |
//! | [`AtomicBatch::guild_war_start`] | `Other("GuildWarStart")` | - | [`DbOp::guild_war_start_ops`], `SetGuildWarOfMembers(.., war)` |
//! | [`AtomicBatch::guild_war_end`] | `Other("GuildWarEnd")` | - | `UpsertGuildWar`, `UpsertGuild` x 2, `SetGuildWarOfMembers(.., 0)` |
//!
//! The three `Other` kinds are additive: [`AtomicKind::NAMED`] still lists the 50 kinds of the plan.
//!
//! Rules (Plan/13 §18): the in-memory change, the snapshot (a **new** `SaveGen`) and `persist.atomic(..)` belong into
//! the same panic-free commit block. An *Expected* failure (`Conflict`, `NotFound`, `GuildNameTaken`) rolls the whole
//! batch back, snapshots included; for a social batch that is a game bug (21 §2.4). The single-kind constructors set
//! the `kind` and the guild of the log row they carry, so a row cannot disagree with the batch.

use crate::model::{
    CampusMemberRecord, CharacterSnapshot, CoupleRecord, Gc1to1TenderRecord, GuildBankDoc,
    GuildBankLogKind, GuildBankLogRecord, GuildMemberRecord, GuildRecord, GuildSiegeBidRecord,
    GuildSiegePlayerRecord, GuildSiegeRecord, GuildWarRecord, MailRecord, PendingGrantRecord,
    RANK_ROOKIE,
};
use crate::ops::{AtomicBatch, AtomicKind, BatchReply, DbOp};
use crate::repo::meta::{counter, kv};
use flyff_common::{CharacterId, GuildId, UnixTime};

impl DbOp {
    /// The ops that create a guild, in the order of Plan/13 §18: `UpsertGuild`, `UpsertGuildMember` for every
    /// founder (the master first), `SetGuildBank` with an empty bank. No counter raise: [`AtomicBatch::guild_create`]
    /// and `AtomicBatch::admin_guild_create` add `RaiseCounter(next_guild_id)` themselves.
    pub fn guild_create_ops(guild: GuildRecord, members: Vec<GuildMemberRecord>) -> Vec<DbOp> {
        let id = guild.id;
        let mut ops = Vec::with_capacity(members.len() + 2);
        ops.push(DbOp::UpsertGuild(guild));
        ops.extend(members.into_iter().map(DbOp::UpsertGuildMember));
        ops.push(DbOp::SetGuildBank {
            guild_id: id,
            doc: GuildBankDoc::empty(),
        });
        ops
    }

    /// The ops that dissolve a guild: `SetGuildRejoinCooldown` for every `(member, until)` (only when cooldowns
    /// are enabled, `features.remove_guild_cooldown = false`), then `DeleteGuild`, which cascades members, bank
    /// (**its contents are lost**, KEEP), log, quests and the house. War rows and siege rows survive.
    pub fn guild_disband_ops(guild: GuildId, cooldowns: Vec<(CharacterId, UnixTime)>) -> Vec<DbOp> {
        let mut ops: Vec<DbOp> = cooldowns
            .into_iter()
            .map(|(character_id, until)| DbOp::SetGuildRejoinCooldown {
                character_id,
                until: Some(until),
            })
            .collect();
        ops.push(DbOp::DeleteGuild { guild_id: guild });
        ops
    }

    /// The two member upserts of a master change in the only order the schema allows: the old master is demoted to
    /// Rookie (class 0) **first**, then the new master is promoted to rank 0 (class 0) - the unique index
    /// `guild_members_master` refuses a second rank-0 member (21 §5.5). Pass the current rows; the ranks are set here.
    pub fn guild_master_change_ops(
        mut old_master: GuildMemberRecord,
        mut new_master: GuildMemberRecord,
    ) -> [DbOp; 2] {
        old_master.rank = RANK_ROOKIE;
        old_master.class = 0;
        new_master.rank = 0;
        new_master.class = 0;
        [
            DbOp::UpsertGuildMember(old_master),
            DbOp::UpsertGuildMember(new_master),
        ]
    }

    /// `UpsertGuildWar` of a new war plus `RaiseCounter(next_guild_war_id)` above it (the war id comes from the
    /// service's memory).
    pub fn guild_war_start_ops(war: GuildWarRecord) -> [DbOp; 2] {
        let raise = DbOp::RaiseCounter {
            key: counter::NEXT_GUILD_WAR_ID,
            at_least: i64::from(war.id.0) + 1,
        };
        [DbOp::UpsertGuildWar(war), raise]
    }

    /// A GM removal of bank **items** (`removal_apply` of a guild-bank row, `admin_guild_edit` `BankRemove`):
    /// `SetGuildBank` with the bank after the removal and the log row `R` (kind and guild are set here). Use it as
    /// the `extra_ops` of [`AtomicBatch::removal_apply`]`(None, ..)`.
    pub fn guild_bank_removal_ops(
        guild: GuildId,
        bank: GuildBankDoc,
        mut log: GuildBankLogRecord,
    ) -> [DbOp; 2] {
        log.guild_id = guild;
        log.kind = GuildBankLogKind::GmRemoval;
        [
            DbOp::SetGuildBank {
                guild_id: guild,
                doc: bank,
            },
            DbOp::InsertGuildBankLog(log),
        ]
    }

    /// A GM adjustment of the guild's **penya** (`admin_guild_edit` `AdjustGold`): `UpsertGuild` with the new pool
    /// and the log row `R` (`count` = the penya removed, kind and guild are set here).
    pub fn guild_gold_removal_ops(guild: GuildRecord, mut log: GuildBankLogRecord) -> [DbOp; 2] {
        log.guild_id = guild.id;
        log.kind = GuildBankLogKind::GmRemoval;
        [DbOp::UpsertGuild(guild), DbOp::InsertGuildBankLog(log)]
    }

    /// `UpsertCouple` plus `RaiseCounter(next_couple_id)` above the couple's id.
    pub fn couple_ops(couple: CoupleRecord) -> [DbOp; 2] {
        let raise = DbOp::RaiseCounter {
            key: counter::NEXT_COUPLE_ID,
            at_least: i64::from(couple.id) + 1,
        };
        [DbOp::UpsertCouple(couple), raise]
    }

    /// `UpsertCampus`, `UpsertCampusMember` for every member (give the master first) and
    /// `RaiseCounter(next_campus_id)` above the campus id. The members' `campus_id` must be `id`.
    pub fn campus_ops(
        id: u32,
        created_at: UnixTime,
        members: Vec<CampusMemberRecord>,
    ) -> Vec<DbOp> {
        let mut ops = Vec::with_capacity(members.len() + 2);
        ops.push(DbOp::UpsertCampus {
            campus_id: id,
            created_at,
        });
        ops.extend(members.into_iter().map(DbOp::UpsertCampusMember));
        ops.push(DbOp::RaiseCounter {
            key: counter::NEXT_CAMPUS_ID,
            at_least: i64::from(id) + 1,
        });
        ops
    }
}

impl PendingGrantRecord {
    /// The grant of an **offline** member's guild salary (F-020, PL-094): `gold` penya with the source
    /// `guild_salary:<guild id>`, delivered at the next login or by the 30 s poll (PL-055) instead of the legacy
    /// `ITEM_SEND_TBL` row.
    pub fn guild_salary(
        guild: GuildId,
        character: CharacterId,
        gold: i64,
        created_at: UnixTime,
    ) -> PendingGrantRecord {
        PendingGrantRecord::gold(
            character,
            gold,
            format!("guild_salary:{}", guild.0),
            "guild salary",
            created_at,
        )
    }
}

impl AtomicBatch {
    fn social(kind: AtomicKind, snapshots: Vec<CharacterSnapshot>, ops: Vec<DbOp>) -> AtomicBatch {
        AtomicBatch {
            kind,
            snapshots,
            ops,
            reply: BatchReply::None,
        }
    }

    fn log_of(guild: GuildId, kind: GuildBankLogKind, mut log: GuildBankLogRecord) -> DbOp {
        log.guild_id = guild;
        log.kind = kind;
        DbOp::InsertGuildBankLog(log)
    }

    // ---- guilds --------------------------------------------------------------------------------------------------

    /// `GuildCreate` (NPC Helena, `/createguild`): the master's snapshot after the charge, then
    /// [`DbOp::guild_create_ops`] and `RaiseCounter(next_guild_id)` above the new guild (Plan/21 §5.4).
    pub fn guild_create(
        master: CharacterSnapshot,
        guild: GuildRecord,
        members: Vec<GuildMemberRecord>,
    ) -> AtomicBatch {
        let raise = DbOp::RaiseCounter {
            key: counter::NEXT_GUILD_ID,
            at_least: i64::from(guild.id.0) + 1,
        };
        let mut ops = DbOp::guild_create_ops(guild, members);
        ops.push(raise);
        Self::social(AtomicKind::GuildCreate, vec![master], ops)
    }

    /// `GuildDisband`: [`DbOp::guild_disband_ops`] (`cooldowns` only when rejoin cooldowns are enabled).
    pub fn guild_disband(guild: GuildId, cooldowns: Vec<(CharacterId, UnixTime)>) -> AtomicBatch {
        Self::social(
            AtomicKind::GuildDisband,
            Vec::new(),
            DbOp::guild_disband_ops(guild, cooldowns),
        )
    }

    /// `GuildRename` (rename scroll): the master's snapshot (scroll consumed) and `UpsertGuild` with the new name.
    /// A clash (`GuildNameTaken`) rolls the snapshot back, i.e. the scroll is not consumed.
    pub fn guild_rename(master: CharacterSnapshot, guild: GuildRecord) -> AtomicBatch {
        Self::social(
            AtomicKind::GuildRename,
            vec![master],
            vec![DbOp::UpsertGuild(guild)],
        )
    }

    /// `GuildBankPutItem`: the depositor's snapshot (item removed from the inventory), the new bank document and the
    /// log row `A`. The item can never be in the bank **and** the inventory after a crash (PL-051).
    pub fn guild_bank_put_item(
        member: CharacterSnapshot,
        guild: GuildId,
        bank: GuildBankDoc,
        log: GuildBankLogRecord,
    ) -> AtomicBatch {
        Self::social(
            AtomicKind::GuildBankPutItem,
            vec![member],
            vec![
                DbOp::SetGuildBank {
                    guild_id: guild,
                    doc: bank,
                },
                Self::log_of(guild, GuildBankLogKind::ItemPut, log),
            ],
        )
    }

    /// `GuildBankTakeItem`: the withdrawer's snapshot (item added), the new bank document and the log row `D`.
    pub fn guild_bank_take_item(
        member: CharacterSnapshot,
        guild: GuildId,
        bank: GuildBankDoc,
        log: GuildBankLogRecord,
    ) -> AtomicBatch {
        Self::social(
            AtomicKind::GuildBankTakeItem,
            vec![member],
            vec![
                DbOp::SetGuildBank {
                    guild_id: guild,
                    doc: bank,
                },
                Self::log_of(guild, GuildBankLogKind::ItemTaken, log),
            ],
        )
    }

    /// `GuildBankTakeGold`: the withdrawer's snapshot (penya added), `UpsertGuild` (pool - n),
    /// `UpsertGuildMember` (`contributed_gold - n`, which may go negative) and the log row `O`.
    pub fn guild_bank_take_gold(
        member: CharacterSnapshot,
        guild: GuildRecord,
        member_row: GuildMemberRecord,
        log: GuildBankLogRecord,
    ) -> AtomicBatch {
        let id = guild.id;
        Self::social(
            AtomicKind::GuildBankTakeGold,
            vec![member],
            vec![
                DbOp::UpsertGuild(guild),
                DbOp::UpsertGuildMember(member_row),
                Self::log_of(id, GuildBankLogKind::PenyaOut, log),
            ],
        )
    }

    /// `GuildContribution`: the contributor's snapshot (penya or gems removed), `UpsertGuild` (pool, PXP, level),
    /// `UpsertGuildMember` and one log row per successful contribution call (`I`, or `L` when it raised the level).
    pub fn guild_contribution(
        member: CharacterSnapshot,
        guild: GuildRecord,
        member_row: GuildMemberRecord,
        logs: Vec<GuildBankLogRecord>,
    ) -> AtomicBatch {
        let id = guild.id;
        let mut ops = vec![
            DbOp::UpsertGuild(guild),
            DbOp::UpsertGuildMember(member_row),
        ];
        ops.extend(logs.into_iter().map(|log| {
            let kind = if log.kind == GuildBankLogKind::ContributionLevelUp {
                GuildBankLogKind::ContributionLevelUp
            } else {
                GuildBankLogKind::PenyaIn
            };
            Self::log_of(id, kind, log)
        }));
        Self::social(AtomicKind::GuildContribution, vec![member], ops)
    }

    /// `GuildCloak`: no snapshot (the master gets the cloak in the guild bank), `UpsertGuild` (pool - 10 000),
    /// `SetGuildBank` and the log row `C`.
    pub fn guild_cloak(
        guild: GuildRecord,
        bank: GuildBankDoc,
        log: GuildBankLogRecord,
    ) -> AtomicBatch {
        let id = guild.id;
        Self::social(
            AtomicKind::GuildCloak,
            Vec::new(),
            vec![
                DbOp::UpsertGuild(guild),
                DbOp::SetGuildBank {
                    guild_id: id,
                    doc: bank,
                },
                Self::log_of(id, GuildBankLogKind::Cloak, log),
            ],
        )
    }

    /// `GuildSalary` of ONE paying guild: the snapshots of the online paid members (penya added in memory),
    /// `UpsertGuild` (pool - total), one `InsertPendingGrant` per **offline** paid member
    /// ([`PendingGrantRecord::guild_salary`]), the log row `P` (`count` = the total) and - when `paid_on` is given -
    /// `SetServerKv(guild.salary_last_paid_on, "<date>")`. The date rides in every paying guild's batch: a crash can
    /// under-pay one guild for a day, never pay twice (PL-094).
    pub fn guild_salary(
        online: Vec<CharacterSnapshot>,
        guild: GuildRecord,
        offline_grants: Vec<PendingGrantRecord>,
        log: GuildBankLogRecord,
        paid_on: Option<&str>,
    ) -> AtomicBatch {
        let id = guild.id;
        let mut ops = vec![DbOp::UpsertGuild(guild)];
        ops.extend(offline_grants.into_iter().map(DbOp::InsertPendingGrant));
        ops.push(Self::log_of(id, GuildBankLogKind::Salary, log));
        if let Some(date) = paid_on {
            ops.push(DbOp::SetServerKv {
                key: kv::GUILD_SALARY_LAST_PAID_ON.to_owned(),
                value: serde_json::Value::from(date),
            });
        }
        Self::social(AtomicKind::GuildSalary, online, ops)
    }

    // ---- wars ----------------------------------------------------------------------------------------------------

    /// A war starts (additive kind `Other("GuildWarStart")`): [`DbOp::guild_war_start_ops`] and
    /// `SetGuildWarOfMembers` so that every member of both guilds (offline ones too) carries the war id.
    pub fn guild_war_start(war: GuildWarRecord, guilds: [GuildId; 2]) -> AtomicBatch {
        let war_id = war.id.0;
        let mut ops: Vec<DbOp> = DbOp::guild_war_start_ops(war).into();
        ops.push(DbOp::SetGuildWarOfMembers { guilds, war_id });
        Self::social(AtomicKind::Other("GuildWarStart"), Vec::new(), ops)
    }

    /// A war ends (additive kind `Other("GuildWarEnd")`): `UpsertGuildWar` with the exact `WR_*` code and the points
    /// after, `UpsertGuild` for both guilds (points, wins, losses) and `SetGuildWarOfMembers(.., 0)`.
    pub fn guild_war_end(
        war: GuildWarRecord,
        decl_guild: GuildRecord,
        acpt_guild: GuildRecord,
    ) -> AtomicBatch {
        let guilds = [decl_guild.id, acpt_guild.id];
        Self::social(
            AtomicKind::Other("GuildWarEnd"),
            Vec::new(),
            vec![
                DbOp::UpsertGuildWar(war),
                DbOp::UpsertGuild(decl_guild),
                DbOp::UpsertGuild(acpt_guild),
                DbOp::SetGuildWarOfMembers { guilds, war_id: 0 },
            ],
        )
    }

    // ---- siege ---------------------------------------------------------------------------------------------------

    /// `SiegeBid`: the master's snapshot (the difference paid) and `UpsertGuildSiegeBid` (status 10, or 12 when the
    /// guild applies again after a cancel). A bid that is not higher is `Conflict` and rolls the payment back.
    pub fn siege_bid(master: CharacterSnapshot, bid: GuildSiegeBidRecord) -> AtomicBatch {
        Self::social(
            AtomicKind::SiegeBid,
            vec![master],
            vec![DbOp::UpsertGuildSiegeBid(bid)],
        )
    }

    /// `SiegeCancel`: `UpsertGuildSiegeBid` (status 11) and the refund mail to the master (`InsertMail` +
    /// `RaiseCounter(next_mail_id)`), atomically. A second cancel is `Conflict`: it can never mail a second refund.
    pub fn siege_cancel(bid: GuildSiegeBidRecord, refund_mail: MailRecord) -> AtomicBatch {
        let mut ops = vec![DbOp::UpsertGuildSiegeBid(bid)];
        ops.extend(DbOp::mail_ops(refund_mail));
        Self::social(AtomicKind::SiegeCancel, Vec::new(), ops)
    }

    /// `SiegeResult`: the snapshots of the online chip recipients and, in the order of Plan/13 §18,
    /// `UpsertGuildSiege` (the ended siege), `UpsertGuildSiegeBid` x n, `UpsertGuildSiegePlayer` x n, `InsertMail`
    /// x n (98 % refunds, chips for full inventories), `UpsertGuildSiege` (the next siege, status 10), then ONE
    /// `RaiseCounter(next_mail_id)` above the highest mail id (none without mails) and
    /// `RaiseCounter(next_siege_id)` above the next siege.
    pub fn siege_result(
        online: Vec<CharacterSnapshot>,
        ended: GuildSiegeRecord,
        bids: Vec<GuildSiegeBidRecord>,
        players: Vec<GuildSiegePlayerRecord>,
        mails: Vec<MailRecord>,
        next_siege: GuildSiegeRecord,
    ) -> AtomicBatch {
        let top_mail = mails.iter().map(|m| m.id).max();
        let next_id = next_siege.id;
        let mut ops = Vec::with_capacity(bids.len() + players.len() + mails.len() + 4);
        ops.push(DbOp::UpsertGuildSiege(ended));
        ops.extend(bids.into_iter().map(DbOp::UpsertGuildSiegeBid));
        ops.extend(players.into_iter().map(DbOp::UpsertGuildSiegePlayer));
        ops.extend(mails.into_iter().map(DbOp::InsertMail));
        ops.push(DbOp::UpsertGuildSiege(next_siege));
        if let Some(top) = top_mail {
            ops.push(DbOp::raise_mail_counter(top));
        }
        ops.push(DbOp::RaiseCounter {
            key: counter::NEXT_SIEGE_ID,
            at_least: i64::from(next_id) + 1,
        });
        Self::social(AtomicKind::SiegeResult, online, ops)
    }

    /// `SiegeClaim` of a guild: the master's snapshot (purse credited) and the bid row in status 31 with
    /// `rewarded_at`. A second claim is `Conflict`: it can never pay twice.
    pub fn siege_claim_guild(claimer: CharacterSnapshot, row: GuildSiegeBidRecord) -> AtomicBatch {
        Self::social(
            AtomicKind::SiegeClaim,
            vec![claimer],
            vec![DbOp::UpsertGuildSiegeBid(row)],
        )
    }

    /// `SiegeClaim` of the best player: the claimer's snapshot and the player row in status 31.
    pub fn siege_claim_player(
        claimer: CharacterSnapshot,
        row: GuildSiegePlayerRecord,
    ) -> AtomicBatch {
        Self::social(
            AtomicKind::SiegeClaim,
            vec![claimer],
            vec![DbOp::UpsertGuildSiegePlayer(row)],
        )
    }

    /// The siege opens (additive kind `Other("SiegeOpen")`): the siege row in status 20, the bids that fight
    /// (status 20) and the expiry of the unclaimed rewards of older sieges (`ExpireSiegeRewards`, which also decides
    /// the batch outcome `Changed(n)`).
    pub fn siege_open(
        siege: GuildSiegeRecord,
        fighting_bids: Vec<GuildSiegeBidRecord>,
        expire_before: u32,
        at: UnixTime,
    ) -> AtomicBatch {
        let mut ops = vec![DbOp::UpsertGuildSiege(siege)];
        ops.extend(fighting_bids.into_iter().map(DbOp::UpsertGuildSiegeBid));
        ops.push(DbOp::ExpireSiegeRewards {
            before_siege_id: expire_before,
            at,
        });
        Self::social(AtomicKind::Other("SiegeOpen"), Vec::new(), ops)
    }

    // ---- GC 1:1 --------------------------------------------------------------------------------------------------

    fn gc1to1(
        kind: AtomicKind,
        master: CharacterSnapshot,
        tender: Gc1to1TenderRecord,
    ) -> AtomicBatch {
        let raise = DbOp::RaiseCounter {
            key: counter::GC1TO1_ROUND,
            at_least: i64::from(tender.round),
        };
        Self::social(
            kind,
            vec![master],
            vec![DbOp::UpsertGc1to1Tender(tender), raise],
        )
    }

    /// `Gc1to1Tender`: the master's snapshot (the penya paid) and `UpsertGc1to1Tender` (state `T`; a re-tender must
    /// be higher), then `RaiseCounter(gc1to1_round)`.
    pub fn gc1to1_tender(master: CharacterSnapshot, tender: Gc1to1TenderRecord) -> AtomicBatch {
        Self::gc1to1(AtomicKind::Gc1to1Tender, master, tender)
    }

    /// `Gc1to1Cancel`: the master's snapshot (80 % refunded into the purse) and the tender in state `C`.
    pub fn gc1to1_cancel(master: CharacterSnapshot, tender: Gc1to1TenderRecord) -> AtomicBatch {
        Self::gc1to1(AtomicKind::Gc1to1Cancel, master, tender)
    }

    /// `Gc1to1Refund`: the master's snapshot (98 % of a failed tender refunded) and the tender in state `G`.
    pub fn gc1to1_refund(master: CharacterSnapshot, tender: Gc1to1TenderRecord) -> AtomicBatch {
        Self::gc1to1(AtomicKind::Gc1to1Refund, master, tender)
    }
}
