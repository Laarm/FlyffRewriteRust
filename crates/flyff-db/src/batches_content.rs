//! Typed constructors for the atomic batches of the content systems (Plan/13 §18, Plan/22 §2.8): the lord election,
//! tax, secret room, guild house, housing, rainbow race and quiz, and the op lists that several batches share. Style of
//! [`crate::batches_social`]: every constructor is one row of the §18 table (or an additive `Other` kind), so the
//! event code cannot forget a snapshot, a mail counter or a round counter. Attach the answer with
//! [`AtomicBatch::with_ticket`].
//!
//! | constructor | [`AtomicKind`] | snapshots | ops (in order) |
//! |---|---|---|---|
//! | [`AtomicBatch::lord_deposit`] | `LordDeposit` | candidate (delta paid) | `UpsertLordCandidate` |
//! | [`AtomicBatch::lord_pledge`] | `LordPledge` | candidate (`None` for the free first pledge) | `UpsertLordCandidate` |
//! | [`AtomicBatch::lord_vote`] | `LordVote` | voter | `InsertLordVote`, `UpsertLordCandidate` (votes + 1) |
//! | [`AtomicBatch::lord_event_create`] | `LordEventCreate` | creator (cost paid) | `UpsertLordEvent` |
//! | [`AtomicBatch::lord_begin_vote`] | `Other("lord_begin_vote")` | - | `UpsertLordElection` (2), `UpsertLordCandidate` x n (left out), `InsertMail` x n, `RaiseCounter(next_mail_id)` |
//! | [`AtomicBatch::lord_election_end`] | `LordElectionEnd` | - | `UpsertLordElection` (3), `UpsertLordElection` (next, 0), `InsertMail` x n, `SetLordSkillCooldown` x n, `RaiseCounter(next_mail_id)`, `RaiseCounter(next_lord_election_id)` |
//! | [`AtomicBatch::tax_payout`] | `TaxPayout` | - | [`DbOp::tax_round_ops`], `InsertMail` x n, `RaiseCounter(next_mail_id)`, `SetServerKv(tax.last_paid_on)`, `SetServerKv(tax.last_changed_at)` |
//! | [`AtomicBatch::tax_rate_set`] | `Other("TaxRate")` | - | `UpsertTaxDetail` x n, `UpsertTaxOwner` |
//! | [`AtomicBatch::secret_room_tender`] / [`secret_room_cancel`](AtomicBatch::secret_room_cancel) | `SecretRoomTender` / `SecretRoomCancel` | master | `UpsertSecretRoomTender` |
//! | [`AtomicBatch::secret_room_result`] | `SecretRoomResult` | - | `UpsertSecretRoomTender` x n, `InsertMail` x n, [`DbOp::tax_round_ops`], `RaiseCounter(next_mail_id)`, `RaiseCounter(secret_room_round)` |
//! | [`AtomicBatch::guild_house_buy`] | `GuildHouseBuy` | master (100 000 000 paid) | `UpsertGuildHouse`, `UpsertGuildHouseFurniture`, `RaiseCounter(next_guild_furniture_seq)` |
//! | [`AtomicBatch::guild_house_upkeep`] | `GuildHouseUpkeep` | payer | `UpsertGuildHouse` |
//! | [`AtomicBatch::guild_house_furniture_add`] | `GuildHouseFurnitureAdd` | user (item consumed) | `UpsertGuildHouseFurniture`, `RaiseCounter(next_guild_furniture_seq)` |
//! | [`AtomicBatch::guild_house_expire`] | `Other("GuildHouseExpire")` | - | `UpsertGuildHouse` (`upkeep_until` 0), `UpsertGuildHouseFurniture` x n (`installed` 0) |
//! | [`AtomicBatch::housing_furniture_add`] | `HousingFurnitureAdd` | user (item consumed) | `InsertHousingFurniture` |
//! | [`AtomicBatch::rainbow_apply`] | `RainbowApply` | applicant (fee paid) | `UpsertRainbowRaceEntry` (`N`) |
//! | [`AtomicBatch::rainbow_result`] | `RainbowResult` | online prize recipients | `UpsertRainbowRaceEntry` x n, `InsertMail` x n, `RaiseCounter(next_mail_id)`, `RaiseCounter(rainbow_race_round)` |
//! | [`AtomicBatch::quiz_prize`] | `QuizPrize` | - | `InsertMail` x n, `RaiseCounter(next_mail_id)`, `MarkQuizQuestionsUsed` |
//! | [`AtomicBatch::quiz_import`] | `Other("QuizImport")` | - | `PruneQuizQuestions` (when replacing), `UpsertQuizQuestion` x n |
//!
//! The `Other` kinds are additive: [`AtomicKind::NAMED`] still lists the 50 kinds of the plan (the sixteen kinds
//! `LordDeposit` ... `QuizPrize` were there since the foundation).
//!
//! Rules (Plan/13 §18): the in-memory change, the snapshot (a **new** `SaveGen`) and `persist.atomic(..)` belong into
//! the same panic-free commit block. An *Expected* failure (`Conflict`, `NotFound`, `Invalid`) rolls the whole batch
//! back, snapshots and mails included; for a content batch that is a game bug or a lost race (a second vote, a second
//! application, a second list-up). The constructors force the ids of the rows they carry (the election of a vote, the
//! character of a snapshot, the guild of a house) and the state their kind means, so a row cannot disagree with its
//! batch. Mails: ONE `RaiseCounter(next_mail_id)` above the highest mail id of a batch, none without mails.

use crate::model::{
    CharacterSnapshot, ElectionState, GuildHouseFurnitureRecord, GuildHouseRecord,
    HousingFurnitureRecord, LordCandidateRecord, LordElectionRecord, LordEventRecord,
    LordVoteRecord, MailRecord, QuizQuestionRecord, QuizType, RainbowEntryState,
    RainbowRaceEntryRecord, SecretRoomTenderRecord, SecretRoomTenderState, TaxDetailRecord,
    TaxOwnerRecord,
};
use crate::ops::{AtomicBatch, AtomicKind, BatchReply, DbOp};
use crate::repo::meta::{counter, kv};
use flyff_common::UnixTime;

impl DbOp {
    /// The two ops that create the election row (`PrepareNext`, 22 §9.1): `UpsertLordElection` and
    /// `RaiseCounter(next_lord_election_id)` above the row's id. The lord service sends them at the front of its
    /// first batch of the election (or as their own batch at the first minute).
    pub fn lord_election_create_ops(election: LordElectionRecord) -> [DbOp; 2] {
        let raise = DbOp::RaiseCounter {
            key: counter::NEXT_LORD_ELECTION_ID,
            at_least: i64::from(election.id) + 1,
        };
        [DbOp::UpsertLordElection(election), raise]
    }

    /// The ops that write tax rows in the order of 13 §18: `UpsertTaxDetail` x n, `UpsertTaxOwner` x n and
    /// `RaiseCounter(tax_round)` up to the highest round of the rows (a new round raises the counter, so a restart
    /// loads it). Two empty lists give **no op** (a payout with nobody paid carries only its date).
    pub fn tax_round_ops(owners: Vec<TaxOwnerRecord>, details: Vec<TaxDetailRecord>) -> Vec<DbOp> {
        let top = owners
            .iter()
            .map(|o| o.round)
            .chain(details.iter().map(|d| d.round))
            .max();
        let Some(top) = top else {
            return Vec::new();
        };
        let mut ops = Vec::with_capacity(owners.len() + details.len() + 1);
        ops.extend(details.into_iter().map(DbOp::UpsertTaxDetail));
        ops.extend(owners.into_iter().map(DbOp::UpsertTaxOwner));
        ops.push(DbOp::RaiseCounter {
            key: counter::TAX_ROUND,
            at_least: i64::from(top),
        });
        ops
    }
}

/// Pushes `InsertMail` for every mail and returns the ONE `RaiseCounter(next_mail_id)` above the highest id (`None`
/// without mails); the caller places the raise where its batch wants it.
fn push_mails(ops: &mut Vec<DbOp>, mails: Vec<MailRecord>) -> Option<DbOp> {
    let top = mails.iter().map(|m| m.id).max();
    ops.extend(mails.into_iter().map(DbOp::InsertMail));
    top.map(DbOp::raise_mail_counter)
}

impl AtomicBatch {
    fn content(kind: AtomicKind, snapshots: Vec<CharacterSnapshot>, ops: Vec<DbOp>) -> AtomicBatch {
        AtomicBatch {
            kind,
            snapshots,
            ops,
            reply: BatchReply::None,
        }
    }

    // ---- lord ----------------------------------------------------------------------------------------------------

    /// `LordDeposit` (ELECTION_ADD_DEPOSIT): the candidate's snapshot (the delta paid) and `UpsertLordCandidate` with
    /// the new total (a deposit never decreases). The row's character is the snapshot's.
    pub fn lord_deposit(candidate: CharacterSnapshot, mut row: LordCandidateRecord) -> AtomicBatch {
        row.character_id = candidate.character_id;
        Self::content(
            AtomicKind::LordDeposit,
            vec![candidate],
            vec![DbOp::UpsertLordCandidate(row)],
        )
    }

    /// `LordPledge` (ELECTION_SET_PLEDGE): the candidate's snapshot (1 000 000 paid for a re-pledge; `None` for the
    /// free first pledge, which changes no wallet) and `UpsertLordCandidate` with the pledge text.
    pub fn lord_pledge(
        candidate: Option<CharacterSnapshot>,
        mut row: LordCandidateRecord,
    ) -> AtomicBatch {
        if let Some(snapshot) = &candidate {
            row.character_id = snapshot.character_id;
        }
        Self::content(
            AtomicKind::LordPledge,
            candidate.into_iter().collect(),
            vec![DbOp::UpsertLordCandidate(row)],
        )
    }

    /// `LordVote` (ELECTION_INC_VOTE): the voter's snapshot (the vote-thanks buff), `InsertLordVote` and
    /// `UpsertLordCandidate` with the votes + 1. The vote's elector is the snapshot's character and its election the
    /// candidate's. A second vote of the elector is `Conflict` and rolls the snapshot and the vote count back.
    pub fn lord_vote(
        voter: CharacterSnapshot,
        mut vote: LordVoteRecord,
        candidate: LordCandidateRecord,
    ) -> AtomicBatch {
        vote.elector_id = voter.character_id;
        vote.election_id = candidate.election_id;
        Self::content(
            AtomicKind::LordVote,
            vec![voter],
            vec![
                DbOp::InsertLordVote(vote),
                DbOp::UpsertLordCandidate(candidate),
            ],
        )
    }

    /// `LordEventCreate` (L_EVENT_CREATE): the creator's snapshot (the cost paid) and `UpsertLordEvent`. A second
    /// component of the same creator is `Conflict` and rolls the charge back (PL-103, persistence half). The event's
    /// creator is the snapshot's character.
    pub fn lord_event_create(
        creator: CharacterSnapshot,
        mut event: LordEventRecord,
    ) -> AtomicBatch {
        event.character_id = creator.character_id;
        Self::content(
            AtomicKind::LordEventCreate,
            vec![creator],
            vec![DbOp::UpsertLordEvent(event)],
        )
    }

    /// The vote starts (additive kind `Other("lord_begin_vote")`, 22 §2.8/§9.3): the election in state 2 with its
    /// requirement, the candidates cut at the top 10 (`left_out`, final) and their 98 % deposit refund mails with ONE
    /// `RaiseCounter(next_mail_id)`. A candidate is left out once: a second run is `Conflict`.
    pub fn lord_begin_vote(
        mut election: LordElectionRecord,
        left_out: Vec<LordCandidateRecord>,
        mails: Vec<MailRecord>,
    ) -> AtomicBatch {
        election.state = ElectionState::Vote;
        election.lord_id = None;
        election.ended_at = None;
        let mut ops = Vec::with_capacity(left_out.len() + mails.len() + 2);
        let id = election.id;
        ops.push(DbOp::UpsertLordElection(election));
        ops.extend(left_out.into_iter().map(|mut row| {
            row.election_id = id;
            row.left_out = true;
            DbOp::UpsertLordCandidate(row)
        }));
        let raise = push_mails(&mut ops, mails);
        ops.extend(raise);
        Self::content(AtomicKind::Other("lord_begin_vote"), Vec::new(), ops)
    }

    /// `LordElectionEnd` (end of the vote): `ended` (state 3 with `lord_id` and `ended_at`; a second end is
    /// `Conflict`), `next` (state 0 with its `begin_at`), the rank-refund and lord-item mails, the reset of the lord
    /// skill cooldowns (`SetLordSkillCooldown{skill, 0}`), ONE `RaiseCounter(next_mail_id)` and
    /// `RaiseCounter(next_lord_election_id)` above `next`.
    pub fn lord_election_end(
        mut ended: LordElectionRecord,
        mails: Vec<MailRecord>,
        skills_reset: Vec<u32>,
        mut next: LordElectionRecord,
    ) -> AtomicBatch {
        ended.state = ElectionState::Expired;
        next.state = ElectionState::Ready;
        next.lord_id = None;
        next.ended_at = None;
        let raise_election = DbOp::RaiseCounter {
            key: counter::NEXT_LORD_ELECTION_ID,
            at_least: i64::from(next.id) + 1,
        };
        let mut ops = Vec::with_capacity(mails.len() + skills_reset.len() + 4);
        ops.push(DbOp::UpsertLordElection(ended));
        ops.push(DbOp::UpsertLordElection(next));
        let raise_mail = push_mails(&mut ops, mails);
        ops.extend(
            skills_reset
                .into_iter()
                .map(|skill_id| DbOp::SetLordSkillCooldown {
                    skill_id,
                    minutes: 0,
                }),
        );
        ops.extend(raise_mail);
        ops.push(raise_election);
        Self::content(AtomicKind::LordElectionEnd, Vec::new(), ops)
    }

    // ---- tax -----------------------------------------------------------------------------------------------------

    /// `TaxPayout` (daily payout and weekly change): [`DbOp::tax_round_ops`] (the detail and owner rows, reset or of
    /// the new round, and the round counter), the payout mails with ONE `RaiseCounter(next_mail_id)`, and, when
    /// given, `SetServerKv(tax.last_paid_on, "<YYYY-MM-DD>")` and `SetServerKv(tax.last_changed_at, <unix s>)` in the
    /// SAME transaction, so a crash never pays a day twice (PL-100, persistence half) and never records a day whose
    /// mails were lost. With no rows, no mails and no keys the batch has no op.
    pub fn tax_payout(
        owners: Vec<TaxOwnerRecord>,
        details: Vec<TaxDetailRecord>,
        mails: Vec<MailRecord>,
        paid_on: Option<String>,
        changed_at: Option<UnixTime>,
    ) -> AtomicBatch {
        let mut ops = DbOp::tax_round_ops(owners, details);
        let raise = push_mails(&mut ops, mails);
        ops.extend(raise);
        if let Some(date) = paid_on {
            ops.push(DbOp::SetServerKv {
                key: kv::TAX_LAST_PAID_ON.to_owned(),
                value: serde_json::Value::from(date),
            });
        }
        if let Some(at) = changed_at {
            ops.push(DbOp::SetServerKv {
                key: kv::TAX_LAST_CHANGED_AT.to_owned(),
                value: serde_json::Value::from(at.0),
            });
        }
        Self::content(AtomicKind::TaxPayout, Vec::new(), ops)
    }

    /// The master of the next owner set the rates (additive kind `Other("TaxRate")`, 22 §10.5): the detail rows with
    /// the next rates and the owner row with `rate_set`, together.
    pub fn tax_rate_set(owner: TaxOwnerRecord, details: Vec<TaxDetailRecord>) -> AtomicBatch {
        let mut ops: Vec<DbOp> = details.into_iter().map(DbOp::UpsertTaxDetail).collect();
        ops.push(DbOp::UpsertTaxOwner(owner));
        Self::content(AtomicKind::Other("TaxRate"), Vec::new(), ops)
    }

    // ---- secret room ---------------------------------------------------------------------------------------------

    /// `SecretRoomTender`: the master's snapshot (the full tender, or the difference of a raise, paid) and
    /// `UpsertSecretRoomTender` in state `T` (a raise must be higher; `C -> T` and `F -> T` tender again).
    pub fn secret_room_tender(
        master: CharacterSnapshot,
        mut tender: SecretRoomTenderRecord,
    ) -> AtomicBatch {
        tender.state = SecretRoomTenderState::Tendered;
        Self::content(
            AtomicKind::SecretRoomTender,
            vec![master],
            vec![DbOp::UpsertSecretRoomTender(tender)],
        )
    }

    /// `SecretRoomCancel`: the master's snapshot (the 80 % refund is in its wallet) and the tender in state `C`. A
    /// second cancel is `Conflict`: it can never refund twice.
    pub fn secret_room_cancel(
        master: CharacterSnapshot,
        mut tender: SecretRoomTenderRecord,
    ) -> AtomicBatch {
        tender.state = SecretRoomTenderState::Cancelled;
        Self::content(
            AtomicKind::SecretRoomCancel,
            vec![master],
            vec![DbOp::UpsertSecretRoomTender(tender)],
        )
    }

    /// `SecretRoomResult` (the open's failed ranking, the close's result): the tender rows (`F`, `W`, `L`), the 98 %
    /// refund mails, the tax rows of a winner's `set_next_guild` ([`DbOp::tax_round_ops`]), ONE
    /// `RaiseCounter(next_mail_id)` and - at the close - `RaiseCounter(secret_room_round, next_round)`. The open's
    /// result passes `None` for the round. A failed tender is marked once: a second run is `Conflict`.
    pub fn secret_room_result(
        tenders: Vec<SecretRoomTenderRecord>,
        mails: Vec<MailRecord>,
        tax_owners: Vec<TaxOwnerRecord>,
        tax_details: Vec<TaxDetailRecord>,
        next_round: Option<u32>,
    ) -> AtomicBatch {
        let mut ops = Vec::with_capacity(tenders.len() + mails.len() + 4);
        ops.extend(tenders.into_iter().map(DbOp::UpsertSecretRoomTender));
        let raise = push_mails(&mut ops, mails);
        ops.extend(DbOp::tax_round_ops(tax_owners, tax_details));
        ops.extend(raise);
        if let Some(round) = next_round {
            ops.push(DbOp::RaiseCounter {
                key: counter::SECRET_ROOM_ROUND,
                at_least: i64::from(round),
            });
        }
        Self::content(AtomicKind::SecretRoomResult, Vec::new(), ops)
    }

    // ---- guild house ---------------------------------------------------------------------------------------------

    /// `GuildHouseBuy`: the master's snapshot (100 000 000 paid), `UpsertGuildHouse`, the listed and installed
    /// `II_GHOU_FUR_NPC_TELEPORTER` row (`teleporter`, whose guild is the house's) and
    /// `RaiseCounter(next_guild_furniture_seq)` above its seq. A buy over an existing house is `Conflict` and the
    /// charge rolls back (the game's `BUY_HAVE` check comes first).
    pub fn guild_house_buy(
        master: CharacterSnapshot,
        house: GuildHouseRecord,
        mut teleporter: GuildHouseFurnitureRecord,
    ) -> AtomicBatch {
        teleporter.guild_id = house.guild_id;
        let raise = DbOp::RaiseCounter {
            key: counter::NEXT_GUILD_FURNITURE_SEQ,
            at_least: i64::from(teleporter.seq) + 1,
        };
        Self::content(
            AtomicKind::GuildHouseBuy,
            vec![master],
            vec![
                DbOp::UpsertGuildHouse(house),
                DbOp::UpsertGuildHouseFurniture(teleporter),
                raise,
            ],
        )
    }

    /// `GuildHouseUpkeep`: the payer's snapshot (penya paid) and `UpsertGuildHouse` with the new `upkeep_until` and
    /// the stored `created_at`.
    pub fn guild_house_upkeep(payer: CharacterSnapshot, house: GuildHouseRecord) -> AtomicBatch {
        Self::content(
            AtomicKind::GuildHouseUpkeep,
            vec![payer],
            vec![DbOp::UpsertGuildHouse(house)],
        )
    }

    /// `GuildHouseFurnitureAdd` (furniture item used): the user's snapshot (item consumed),
    /// `UpsertGuildHouseFurniture` and `RaiseCounter(next_guild_furniture_seq)` above the new seq.
    pub fn guild_house_furniture_add(
        user: CharacterSnapshot,
        furniture: GuildHouseFurnitureRecord,
    ) -> AtomicBatch {
        let raise = DbOp::RaiseCounter {
            key: counter::NEXT_GUILD_FURNITURE_SEQ,
            at_least: i64::from(furniture.seq) + 1,
        };
        Self::content(
            AtomicKind::GuildHouseFurnitureAdd,
            vec![user],
            vec![DbOp::UpsertGuildHouseFurniture(furniture), raise],
        )
    }

    /// The upkeep ran out (additive kind `Other("GuildHouseExpire")`, KEEP ref/15 §15.4): `upkeep_until = 0` on the
    /// house and `installed = 0` on each piece in `uninstalled` (the pieces stay listed). The guild of every piece is
    /// the house's.
    pub fn guild_house_expire(
        mut house: GuildHouseRecord,
        uninstalled: Vec<GuildHouseFurnitureRecord>,
    ) -> AtomicBatch {
        house.upkeep_until = UnixTime(0);
        let guild = house.guild_id;
        let mut ops = Vec::with_capacity(uninstalled.len() + 1);
        ops.push(DbOp::UpsertGuildHouse(house));
        ops.extend(uninstalled.into_iter().map(|mut piece| {
            piece.guild_id = guild;
            piece.installed = false;
            DbOp::UpsertGuildHouseFurniture(piece)
        }));
        Self::content(AtomicKind::Other("GuildHouseExpire"), Vec::new(), ops)
    }

    // ---- housing -------------------------------------------------------------------------------------------------

    /// `HousingFurnitureAdd` (furniture or papering item used): the user's snapshot (item consumed) and the strict
    /// `InsertHousingFurniture`: an item the owner already lists is `Conflict` and the consumed item rolls back. The
    /// row's owner is the snapshot's character.
    pub fn housing_furniture_add(
        user: CharacterSnapshot,
        mut item: HousingFurnitureRecord,
    ) -> AtomicBatch {
        item.character_id = user.character_id;
        Self::content(
            AtomicKind::HousingFurnitureAdd,
            vec![user],
            vec![DbOp::InsertHousingFurniture(item)],
        )
    }

    // ---- rainbow race --------------------------------------------------------------------------------------------

    /// `RainbowApply`: the applicant's snapshot (the fee paid) and `UpsertRainbowRaceEntry` in state `N`. A second
    /// application of the round is `Conflict` and its fee rolls back. The entry's character is the snapshot's.
    pub fn rainbow_apply(
        applicant: CharacterSnapshot,
        mut entry: RainbowRaceEntryRecord,
    ) -> AtomicBatch {
        entry.character_id = applicant.character_id;
        entry.state = RainbowEntryState::Applied;
        entry.ranking = 0;
        entry.finished_at = None;
        Self::content(
            AtomicKind::RainbowApply,
            vec![applicant],
            vec![DbOp::UpsertRainbowRaceEntry(entry)],
        )
    }

    /// `RainbowResult` (race end): the snapshots of the online prize recipients, the final entry rows (`R -> W`,
    /// `N -> L`), the prize mails with ONE `RaiseCounter(next_mail_id)` and - when the round ends -
    /// `RaiseCounter(rainbow_race_round, next_round)`.
    pub fn rainbow_result(
        online: Vec<CharacterSnapshot>,
        entries: Vec<RainbowRaceEntryRecord>,
        mails: Vec<MailRecord>,
        next_round: Option<u32>,
    ) -> AtomicBatch {
        let mut ops = Vec::with_capacity(entries.len() + mails.len() + 2);
        ops.extend(entries.into_iter().map(DbOp::UpsertRainbowRaceEntry));
        let raise = push_mails(&mut ops, mails);
        ops.extend(raise);
        if let Some(round) = next_round {
            ops.push(DbOp::RaiseCounter {
                key: counter::RAINBOW_RACE_ROUND,
                at_least: i64::from(round),
            });
        }
        Self::content(AtomicKind::RainbowResult, online, ops)
    }

    // ---- quiz ----------------------------------------------------------------------------------------------------

    /// `QuizPrize` (quiz winners): the prize mails with ONE `RaiseCounter(next_mail_id)` and
    /// `MarkQuizQuestionsUsed{used_ids, at}`.
    pub fn quiz_prize(mails: Vec<MailRecord>, used_ids: Vec<i64>, at: UnixTime) -> AtomicBatch {
        let mut ops = Vec::with_capacity(mails.len() + 2);
        let raise = push_mails(&mut ops, mails);
        ops.extend(raise);
        ops.push(DbOp::MarkQuizQuestionsUsed { ids: used_ids, at });
        Self::content(AtomicKind::QuizPrize, Vec::new(), ops)
    }

    /// The same import as [`crate::quiz_import::import`], as a batch for an admin route of a later phase (additive
    /// kind `Other("QuizImport")`): `PruneQuizQuestions` first when replacing a type (it keeps the questions of that
    /// type in `questions`), then `UpsertQuizQuestion` x n.
    pub fn quiz_import(
        questions: Vec<QuizQuestionRecord>,
        replace: Option<QuizType>,
    ) -> AtomicBatch {
        let mut ops = Vec::with_capacity(questions.len() + 1);
        if let Some(quiz_type) = replace {
            ops.push(DbOp::PruneQuizQuestions {
                quiz_type,
                keep: questions
                    .iter()
                    .filter(|q| q.quiz_type == quiz_type)
                    .map(|q| q.question.clone())
                    .collect(),
            });
        }
        ops.extend(questions.into_iter().map(DbOp::UpsertQuizQuestion));
        Self::content(AtomicKind::Other("QuizImport"), Vec::new(), ops)
    }
}
