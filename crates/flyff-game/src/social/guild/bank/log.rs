//! The guild bank log view (Plan/21 §5.9; ref/07c §15.1; `CDPSrvr::OnGuildLogView`, `DPSrvr.cpp:10733`).
//!
//! `GuildLogViewReq{kind}` answers from the in-memory `GuildBankLogTail` of the guild: view 1 lists the rows `A`
//! (items put in), 2 `D` (items taken out), 3 `I` (penya contributed) and 4 `O` (penya taken out), newest first, at
//! most [`LOG_VIEW_MAX`]. Only the master and the kingpins may look (the rank, not a power bit); every other request
//! is ignored. The rows `C`, `P`, `R` and `L` are in the table only.

use super::member;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::guild::model::GuildRank;
use crate::social::guild::wire;
use crate::social::party::wire::name_of;
use flyff_db::GuildBankLogRecord;
use flyff_proto::pb;

/// The most lines a view shows (`GUILD_LOG_TAIL`).
pub const LOG_VIEW_MAX: usize = 100;

/// One line of the view: the member's name (empty when the directory does not know it), the time and the payload.
/// Views 1 and 2 carry the item, its upgrade level and its count; views 3 and 4 carry the penya in `item_id` and 0
/// in the other two fields (the legacy slot is shared).
fn entry(svc: &Services, view: u8, row: &GuildBankLogRecord) -> pb::guild_log_view_ntf::Entry {
    let player = name_of(svc, row.character_id);
    if view <= 2 {
        pb::guild_log_view_ntf::Entry {
            player,
            logged_at: row.at.0,
            item_id: row.item_id,
            ability_option: row.upgrade_level,
            count: i32::try_from(row.count).unwrap_or(i32::MAX),
        }
    } else {
        pb::guild_log_view_ntf::Entry {
            player,
            logged_at: row.at.0,
            item_id: u32::try_from(row.count.max(0)).unwrap_or(u32::MAX),
            ability_option: 0,
            count: 0,
        }
    }
}

/// `GuildLogViewReq{kind}`: `kind` in 1..=4 and rank Master or Kingpin, else ignored; the answer is
/// `GuildLogViewNtf{kind, entries}`.
pub fn on_guild_log_view(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildLogViewReq,
) {
    let Some(me) = member(svc, ctx, who) else {
        return;
    };
    let Some(view) = u8::try_from(m.kind).ok().filter(|v| (1..=4).contains(v)) else {
        return;
    };
    let Some(g) = svc.social.guild.get(me.guild) else {
        return;
    };
    let rank = g.members.get(&me.c).map(|mm| mm.rank);
    if !matches!(rank, Some(GuildRank::Master | GuildRank::Kingpin)) {
        return;
    }
    let entries = g
        .log_tail
        .view(view)
        .iter()
        .take(LOG_VIEW_MAX)
        .map(|row| entry(svc, view, row))
        .collect();
    wire::to_one(
        ctx,
        who.0,
        pb::GuildLogViewNtf {
            kind: u32::from(view),
            entries,
        },
    );
}
