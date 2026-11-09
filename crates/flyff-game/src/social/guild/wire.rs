//! The guild messages (Plan/21 §5.1, 11 §7.4): `GuildBrief` / `GuildFull` and the little senders every guild module
//! shares. Every sender resolves its audience in `CharacterId` order and encodes once.

use super::GuildService;
use super::model::Guild;
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use flyff_common::{GuildId, ObjId};
use flyff_proto::pb;

/// `CGuild::Serialize(bDesc = TRUE)`: the one-line record of `AllGuilds` and the head of `GuildFull`. `win`, `lose`
/// and `surrender` are the war record.
pub fn brief(g: &Guild) -> pb::GuildBrief {
    pb::GuildBrief {
        id_guild: g.id.0,
        id_master: g.master().map_or(0, |m| m.0),
        level: g.level,
        name: g.name.clone(),
        logo: g.logo,
        guild_gold: g.gold,
        win: g.war_wins,
        lose: g.war_losses,
        surrender: g.war_surrenders,
    }
}

/// `CGuild::Serialize(bDesc = FALSE)`: the whole guild for a member. Members in `CharacterId` order with `pay`,
/// `win`, `lose` and `id_selected_vote` 0 and `member_level` = the rank; the power and salary arrays always have 5
/// entries; no votes (21 §5.14); `quests` are the live guild quests (`quest::guild`, 21 line 587).
pub fn full(g: &Guild) -> pb::GuildFull {
    pb::GuildFull {
        brief: Some(brief(g)),
        power: g.powers.to_vec(),
        penya_per_level: g.salaries.to_vec(),
        notice: g.notice.clone(),
        contribution_pxp: g.pxp,
        id_enemy_guild: g.enemy.map_or(0, |e| e.0),
        members: g
            .members
            .values()
            .map(|m| pb::GuildMember {
                id_player: m.id.0,
                pay: 0,
                give_gold: m.give_gold,
                give_pxp_count: m.give_pxp,
                win: 0,
                lose: 0,
                member_level: u32::from(m.rank.as_u8()),
                id_selected_vote: 0,
                surrender: m.surrenders,
                class: i32::from(m.class),
                alias: m.alias.clone(),
            })
            .collect(),
        votes: Vec::new(),
        quests: g
            .quests
            .iter()
            .map(|q| pb::GuildQuest {
                quest_id: i32::from(q.quest_id),
                state: q.state,
                id_guild: g.id.0,
            })
            .collect(),
    }
}

/// `AllGuilds{next_id, every guild}` of the join batch: every guild in id order, named or not (KEEP).
pub fn all_guilds(svc: &GuildService) -> pb::AllGuilds {
    pb::AllGuilds {
        next_id: svc.next_id(),
        guilds: svc.guilds().map(brief).collect(),
    }
}

/// Sends `msg` to one avatar (nothing when it has no session).
pub fn to_one(ctx: &mut GameCtx<'_>, obj: ObjId, msg: impl Into<pb::server_msg::Msg>) {
    send_to(ctx, PlayerRef(obj), msg);
}

/// Sends `msg` to every online member of `guild` in `CharacterId` order, wire `obj_id` 0 (`Audience::Guild`).
pub fn to_members(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    msg: impl Into<pb::server_msg::Msg>,
) {
    ctx.broadcast(Some(svc), ObjId::NULL, Audience::Guild(guild), msg);
}

/// Sends `msg` to every player in a world (`Audience::AllOnline`), wire `obj_id` 0.
pub fn to_all(svc: &Services, ctx: &mut GameCtx<'_>, msg: impl Into<pb::server_msg::Msg>) {
    ctx.broadcast(Some(svc), ObjId::NULL, Audience::AllOnline, msg);
}

/// Sends `msg` to the listed avatars in order, wire `obj_id` 0.
pub fn to_list(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    list: &[ObjId],
    msg: impl Into<pb::server_msg::Msg>,
) {
    ctx.broadcast(
        Some(svc),
        ObjId::NULL,
        Audience::Players(list.iter().copied().collect()),
        msg,
    );
}

/// `SetGuild{id_guild}` for the avatar `obj` (0 = none) to its viewers and itself (`obj_id` = the member, 21 §5.12).
pub fn set_guild_near(svc: &Services, ctx: &mut GameCtx<'_>, obj: ObjId, guild: Option<GuildId>) {
    ctx.broadcast(
        Some(svc),
        obj,
        Audience::NearInclSelf,
        pb::SetGuild {
            id_guild: guild.map_or(0, |g| g.0),
        },
    );
}
