//! The five S2C layouts of the secret room (`WORLDSERVER/User.cpp:8124-8231`) and the helpers that send them.
//!
//! * `SecretRoomMngState{state, remain_ms}`: the manager states 1..=6 and the side states 101..=104 share the
//!   field (the client shows a timer for 3 (masters only), 5, 103 and 104),
//! * `SecretRoomInfo{continent, sub}` with `GuildList` (the guilds of the side in rank order, the line-up only for
//!   the receiver's own guild, and the kills needed per war state), `KillCount{index, kills}` and
//!   `WarState{index, war_state}`; `index` is the rank index of the guild in the list,
//! * the three window messages (`SecretRoomTenderOpenWndNtf`, `SecretRoomLineUpOpenWndNtf`,
//!   `SecretRoomTenderViewNtf`) are built where they are sent ([`super::tender`], [`super::lineup`]).
//!
//! A legacy `GetUserByID` + `IsValidObj` is "an online character whose avatar is in a world": [`online_obj`].

use super::{SrTender, SrWarState};
use crate::ctx::GameCtx;
use crate::presence::PlayerDirectory;
use flyff_common::{CharacterId, GuildId, ObjId};
use flyff_proto::pb;
use flyff_proto::pb::secret_room_info::{
    GuildList, KillCount, SecretRoomGuild, SecretRoomMonsterTotal, Sub, WarState,
};

/// The avatar of `c` when it is online and in a world (`prj.GetUserByID` + `IsValidObj`).
pub fn online_obj(pres: &PlayerDirectory, ctx: &GameCtx<'_>, c: CharacterId) -> Option<ObjId> {
    let obj = pres.get(c)?.online?.obj;
    ctx.worlds.player(obj).is_some().then_some(obj)
}

/// `SecretRoomMngState{state, remain_ms}`.
pub fn mng_state(state: i32, remain_ms: u32) -> pb::SecretRoomMngState {
    pb::SecretRoomMngState { state, remain_ms }
}

/// `SECRETROOM_GUILDLIST` of `side`: every tender in rank order with its war state and kills, the line-up only of
/// the receiver's own guild, then `(war state, total)` of the three monster classes (`m_mapMonsterNum`, key =
/// the war state: ref/15 §10.3).
pub fn guild_list(
    side: u8,
    tenders: &[SrTender],
    receiver: Option<GuildId>,
    totals: [i32; 3],
) -> pb::SecretRoomInfo {
    pb::SecretRoomInfo {
        continent: u32::from(side),
        sub: Some(Sub::GuildList(GuildList {
            guilds: tenders
                .iter()
                .map(|t| SecretRoomGuild {
                    id_guild: t.guild.0,
                    war_state: t.war.code(),
                    kill_count: t.kills,
                    lineup: if receiver == Some(t.guild) {
                        t.lineup.iter().map(|c| c.0).collect()
                    } else {
                        Vec::new()
                    },
                })
                .collect(),
            monster_totals: totals
                .iter()
                .enumerate()
                .map(|(i, total)| SecretRoomMonsterTotal {
                    war_state: i32::try_from(i + 1).unwrap_or(i32::MAX),
                    total: *total,
                })
                .collect(),
        })),
    }
}

/// `SECRETROOM_KILLCOUNT{index, kills}`.
pub fn kill_count(side: u8, index: usize, kills: i32) -> pb::SecretRoomInfo {
    pb::SecretRoomInfo {
        continent: u32::from(side),
        sub: Some(Sub::KillCount(KillCount {
            index: i32::try_from(index).unwrap_or(i32::MAX),
            kills,
        })),
    }
}

/// `SECRETROOM_WARSTATE{index, war_state}`.
pub fn war_state(side: u8, index: usize, war: SrWarState) -> pb::SecretRoomInfo {
    pb::SecretRoomInfo {
        continent: u32::from(side),
        sub: Some(Sub::WarState(WarState {
            index: i32::try_from(index).unwrap_or(i32::MAX),
            war_state: war.code(),
        })),
    }
}

/// Sends `SecretRoomMngState{state, remain_ms}` to the avatar of `c` when it is online (`SendNowState`).
pub fn send_state(
    pres: &PlayerDirectory,
    ctx: &mut GameCtx<'_>,
    c: CharacterId,
    state: i32,
    remain_ms: u32,
) {
    if let Some(obj) = online_obj(pres, ctx, c) {
        crate::social::guild::wire::to_one(ctx, obj, mng_state(state, remain_ms));
    }
}

/// Sends a `SecretRoomInfo` to the avatar of `c` when it is online (`UpdateInfoToMember`).
pub fn send_info(
    pres: &PlayerDirectory,
    ctx: &mut GameCtx<'_>,
    c: CharacterId,
    info: pb::SecretRoomInfo,
) {
    if let Some(obj) = online_obj(pres, ctx, c) {
        crate::social::guild::wire::to_one(ctx, obj, info);
    }
}
