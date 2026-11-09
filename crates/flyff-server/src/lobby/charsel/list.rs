//! The `CharList` (12 §8.4): `queries::character_list` mapped to `CharSlot`.

use super::{Lobby, db_error_msg, server_msg};
use crate::gateway::GatewaySession;
use flyff_common::{AccountId, UnixTime};
use flyff_db::{CharacterListEntry, DbError, queries};
use flyff_proto::pb;

/// One occupied slot (12 §8.4 table): `blocked` while `login_blocked_until` lies in the future, `mode` always 0
/// (the legacy list sent `EVENT_OLDBOY_MODE` only for `last_connect == 3`, which never happens, ref/02 §6.4),
/// `id_party` and `id_war` always 0, `equip_item_ids` the non-zero entries of `equipment_preview` in `PARTS_*`
/// index order (the client places each item by its `prop.dwParts`).
pub(crate) fn char_slot(e: &CharacterListEntry, now: UnixTime) -> pb::CharSlot {
    pb::CharSlot {
        slot: u32::from(e.slot),
        blocked: e.login_blocked_until.is_some_and(|t| t > now),
        world_id: e.world_id,
        mover_index: e.mover_index,
        name: e.name.clone(),
        pos: Some(pb::Vec3 {
            x: e.pos[0],
            y: e.pos[1],
            z: e.pos[2],
        }),
        id_player: e.id.0,
        id_party: 0,
        id_guild: e.guild_id.map_or(0, |g| g.0),
        id_war: 0,
        skin_set: u32::from(e.skin_set),
        hair_mesh: u32::from(e.hair_mesh),
        hair_color: e.hair_color,
        head_mesh: u32::from(e.head_mesh),
        sex: u32::from(e.sex),
        job: i32::from(e.job),
        level: i32::from(e.level),
        str: e.stats[0],
        sta: e.stats[1],
        dex: e.stats[2],
        int: e.stats[3],
        mode: 0,
        equip_item_ids: e
            .equipment_preview
            .iter()
            .copied()
            .filter(|id| *id != 0)
            .collect(),
    }
}

/// The list message of up to three entries, ordered by slot as the database returned them.
pub(crate) fn char_list(entries: &[CharacterListEntry], now: UnixTime) -> pb::CharList {
    pb::CharList {
        slots: entries.iter().map(|e| char_slot(e, now)).collect(),
    }
}

impl Lobby {
    /// A fresh read of the account's active characters (at most 3, by slot).
    pub(crate) async fn read_list(
        &self,
        account: AccountId,
    ) -> Result<Vec<CharacterListEntry>, DbError> {
        self.core
            .persist
            .read(move |c| queries::character_list(c, account))
            .await
    }

    /// Reads and pushes the list into the session's queue (after `HelloAck`, a create, a delete). A read error
    /// is answered with the `LobbyError` of 12 §16.3.
    pub(crate) async fn push_list(&self, s: &GatewaySession, account: AccountId) {
        match self.read_list(account).await {
            Ok(entries) => {
                let list = char_list(&entries, self.core.clock.unix());
                let _ = s.push_frame(server_msg(list));
            }
            Err(e) => {
                tracing::error!(account = account.0, "character list: {e}");
                let _ = s.push_frame(db_error_msg(&e));
            }
        }
    }
}
