//! Constructors of the admin atomic kinds (Plan/13 §18 addendum, Plan/25 §11.6).
//!
//! | constructor | [`AtomicKind`] | snapshots | ops |
//! |---|---|---|---|
//! | [`AtomicBatch::admin_grant`] | `AdminGrant` | receiver | `MarkGrantsDelivered` when it consumes pending rows |
//! | [`AtomicBatch::admin_item_removal`] | `AdminItemRemoval` | target | - |
//! | [`AtomicBatch::admin_gold`] | `AdminGold` | target | - |
//! | [`AtomicBatch::admin_guild_create`] | `AdminGuildCreate` | master, if online | the guild ops, `RaiseCounter(next_guild_id)` |
//! | [`AtomicBatch::admin_guild_edit`] | `AdminGuildEdit` | members whose state changes | the guild ops |
//! | [`AtomicBatch::admin_offline_edit`] | `AdminOfflineEdit` | - | `SetCharacterPosition` / `RenameCharacter` / `UndeleteCharacter` |
//!
//! The in-memory change and the `persist.atomic(..)` call belong into the same panic-free commit block as for
//! every other batch (Plan/13 §18); the admin command answers from the `DbReply` of the batch's ticket
//! (`AtomicBatch::with_ticket`). Banks are account-scoped: the snapshot of any online character of the account
//! carries all its banks (`CharacterSnapshot.banks`), so a bank removal or bank penya change is
//! `admin_item_removal` / `admin_gold` with the snapshot of the online sibling.

use super::model::AdminOfflineEdit;
use crate::model::CharacterSnapshot;
use crate::ops::{AtomicBatch, AtomicKind, BatchReply, DbOp};
use crate::repo::meta::counter;
use flyff_common::{GuildId, UnixTime};

impl AtomicBatch {
    fn admin(kind: AtomicKind, snapshots: Vec<CharacterSnapshot>, ops: Vec<DbOp>) -> AtomicBatch {
        AtomicBatch {
            kind,
            snapshots,
            ops,
            reply: BatchReply::None,
        }
    }

    /// `AdminGrant`: the receiver's snapshot after a direct grant (fresh serials, items and penya added). When
    /// the grant consumes pending rows (an admin delivers a `pending_grants` row now) their ids are closed in
    /// the same transaction with `MarkGrantsDelivered{ids}`; a row that was cancelled meanwhile is not closed
    /// (logged as CRITICAL, `DbStatsSnapshot::anomalies`).
    pub fn admin_grant(
        receiver: CharacterSnapshot,
        consumed_grants: Vec<i64>,
        at: UnixTime,
    ) -> AtomicBatch {
        let ops = if consumed_grants.is_empty() {
            Vec::new()
        } else {
            vec![DbOp::MarkGrantsDelivered {
                ids: consumed_grants,
                at,
            }]
        };
        Self::admin(AtomicKind::AdminGrant, vec![receiver], ops)
    }

    /// `AdminItemRemoval`: the target's snapshot after an item was removed from its inventory, a pocket or (via
    /// the snapshot of an online sibling) a bank of the account.
    pub fn admin_item_removal(target: CharacterSnapshot) -> AtomicBatch {
        Self::admin(AtomicKind::AdminItemRemoval, vec![target], Vec::new())
    }

    /// `AdminGold`: the target's snapshot after a penya adjustment (inventory wallet or a bank).
    pub fn admin_gold(target: CharacterSnapshot) -> AtomicBatch {
        Self::admin(AtomicKind::AdminGold, vec![target], Vec::new())
    }

    /// `AdminGuildCreate`: the master's snapshot (only when the master is online), the guild's ops in order
    /// (`UpsertGuild`, `UpsertGuildMember` x n, `SetGuildBank`; built by the guild model, WP-DB-11) and
    /// `RaiseCounter(next_guild_id)` above `guild`, so the counter can never fall behind a persisted guild.
    pub fn admin_guild_create(
        master: Option<CharacterSnapshot>,
        guild: GuildId,
        guild_ops: Vec<DbOp>,
    ) -> AtomicBatch {
        let mut ops = guild_ops;
        ops.push(DbOp::RaiseCounter {
            key: counter::NEXT_GUILD_ID,
            at_least: i64::from(guild.0) + 1,
        });
        Self::admin(
            AtomicKind::AdminGuildCreate,
            master.into_iter().collect(),
            ops,
        )
    }

    /// `AdminGuildEdit`: the guild's ops (`UpsertGuild`, `UpsertGuildMember`, `SetGuildBank`, and
    /// `InsertGuildBankLog('R')` for a bank removal) plus the snapshots of online members whose own state
    /// changed with it.
    pub fn admin_guild_edit(
        snapshots: Vec<CharacterSnapshot>,
        guild_ops: Vec<DbOp>,
    ) -> AtomicBatch {
        Self::admin(AtomicKind::AdminGuildEdit, snapshots, guild_ops)
    }

    /// `AdminOfflineEdit`: one change to an offline character (taken under an online-registry hold).
    pub fn admin_offline_edit(edit: AdminOfflineEdit) -> AtomicBatch {
        let op = match edit {
            AdminOfflineEdit::SetPosition {
                character_id,
                world_id,
                pos,
            } => DbOp::SetCharacterPosition {
                character_id,
                world_id,
                pos,
            },
            AdminOfflineEdit::Rename { character_id, name } => {
                DbOp::RenameCharacter { character_id, name }
            }
            AdminOfflineEdit::Undelete { character_id, slot } => {
                DbOp::UndeleteCharacter { character_id, slot }
            }
        };
        Self::admin(AtomicKind::AdminOfflineEdit, Vec::new(), vec![op])
    }
}
