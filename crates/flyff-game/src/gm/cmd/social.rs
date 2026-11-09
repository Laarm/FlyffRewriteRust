//! The guild, couple, campus, honor and rename commands (Plan/23 §4.5-§4.7; WP-GM-23): `/GuildStat` (66),
//! `/createguild` (67), `/Propose` .. `/NextCoupleLevel` (106-111), `/HonorTitleSet` (113), `/SetPlayerName` (120),
//! `/SetGuildName` (121), `/RemoveCampusMember` (217) and `/UpdateCampusPoint` (218).
//!
//! Every handler is a thin wrapper over the entry point that the wire handler of the same feature calls (doc 21): no
//! second copy of a rule. The GM only skips payment, scrolls and the NPC. Names are one token (`args.token()`, quote a
//! name with spaces), numbers are `args.number()` (`atoi`; a missing argument reads as 0), lines go through
//! `cx.out`. The permission tier and the `gm.command` audit are the dispatcher's.

use crate::ctx::PlayerRef;
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::social::campus::points::{PointReason, add_points};
use crate::social::guild::contribution::{contribute_direct, on_guild_logo, on_guild_notice};
use crate::social::guild::create::{
    CreateMembers, GuildCreateError, GuildCreateReq, create_paid, validate_guild_name,
};
use crate::social::guild::ops::{Applied, GuildActor, GuildCmd, GuildRefusal, apply};
use crate::social::guild::wire as guild_wire;
use crate::social::player_data::{RenameVia, submit_rename};
use crate::social::relation;
use crate::social::{campus, couple, honor};
use flyff_common::CharacterId;
use flyff_common::defs::text::{TID_DIAG_0034, TID_DIAG_0060, TID_GAME_COMHAVECOM};
use flyff_db::DbOp;
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::names::{NameKind, name_error_tid, validate_character_name};
use flyff_rules::social::{COUPLE_MAX_LEVEL, HONOR_TITLE_COUNT, couple_exp_to_next};
use serde_json::json;

/// The most a honor counter takes (`/hts`: `dwNum > 100000000` is cut to it).
pub const HONOR_COUNT_MAX: i32 = 100_000_000;

fn me(cx: &CmdCtx<'_, '_>) -> Result<PlayerRef, CmdError> {
    cx.self_player.ok_or(CmdError::NeedsSelf)
}

/// The character behind the issuer.
fn character(cx: &CmdCtx<'_, '_>, who: PlayerRef) -> Result<CharacterId, CmdError> {
    relation::character_of(cx.ctx, who).ok_or(CmdError::NeedsSelf)
}

// ---- guilds --------------------------------------------------------------------------------------------------------

/// Row 66 `/GuildStat (gstat) logo <n> | pxp <n> | penya <n> | notice <text>` (`TextCmd_GuildStat`): the first token is
/// lower-cased. `logo` is the `GuildLogoReq` service as the master (guild level 2 or more, not at war, set once,
/// `0..=27`; above 20 is allowed because the issuer is a GM), `notice` the notice service (master only, VR-045 binds
/// the GM too, the rest of the line is the text; the legacy kept one token). `pxp` and `penya` are
/// [`contribute_direct`]: the issuer's contribution in his own guild with NO debit (levels up at most once per call;
/// `GuildContributionNtf` except for an unnamed guild, KQ-39 c). A negative number changes nothing (the legacy
/// unsigned compare turned it into a huge one). Anything else prints `<lower-cased token>syntax error guild stat
/// command.` with NO space. Without a guild the four stats are silent.
pub fn guild_stat(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let stat = args.token().to_ascii_lowercase();
    match stat.as_str() {
        "logo" => {
            let logo = u32::try_from(args.number()).unwrap_or(u32::MAX);
            on_guild_logo(cx.svc, cx.ctx, who, pb::GuildLogoReq { logo });
        }
        "pxp" => {
            if let Ok(n) = u32::try_from(args.number()) {
                contribute_direct(cx.svc, cx.ctx, who, n, 0);
            }
        }
        "penya" => {
            if let Ok(n) = u32::try_from(args.number()) {
                contribute_direct(cx.svc, cx.ctx, who, 0, n);
            }
        }
        "notice" => {
            let notice = args.last_full();
            on_guild_notice(cx.svc, cx.ctx, who, pb::GuildNoticeReq { notice });
        }
        other => cx
            .out
            .text(format!("{other}syntax error guild stat command.")),
    }
    Ok(CmdDone::default())
}

/// Row 67 `/createguild (cg) <name>` (`TextCmd_CreateGuild`, also the guild-creation scroll): the guild name rules
/// first (an error prints its `DefinedText` and stops; an empty name is `TID_DIAG_0011`), then
/// [`create_paid`] with the issuer as the only member and master, no party, no NPC, no quest and no charge. A
/// character that has a guild hears `TID_GAME_COMHAVECOM`, a name that is taken answers `GuildError{1}` (the
/// first-naming reply). The one-member guild, the `CreateGuild`/`Guild` messages and the `guild.create` audit are
/// guild-core's.
pub fn create_guild(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let raw = args.token().to_owned();
    let name = match validate_guild_name(cx.ctx, &raw) {
        Ok(n) => n,
        Err(e) => {
            let tid = u32::try_from(name_error_tid(NameKind::Guild, e)).unwrap_or_default();
            cx.out.defined(tid, Vec::new());
            return Ok(CmdDone::on(raw));
        }
    };
    let req = GuildCreateReq {
        leader: who,
        members: CreateMembers::SelfOnly,
        name: Some(name.clone()),
        charge: 0,
    };
    match create_paid(cx.svc, cx.ctx, req) {
        Ok(_) => {}
        Err(GuildCreateError::MemberInGuild) => cx.out.defined(TID_GAME_COMHAVECOM, Vec::new()),
        Err(GuildCreateError::NameTaken) => {
            guild_wire::to_one(cx.ctx, who.0, pb::GuildError { code: 1 });
        }
        Err(e) => {
            tracing::error!(error = ?e, %name, "/createguild was refused where it cannot be");
        }
    }
    Ok(CmdDone::on(name))
}

/// Row 121 `/SetGuildName (sgn) <newName>` (`TextCmd_QuerySetGuildName`): only the MASTER of a guild (anybody else is
/// silent, the legacy comment "is not kingpin"). The name rules (an error prints `name_error_tid(Guild, e)` and
/// stops), a name another guild has (ASCII case-insensitive, PL-058) answers `GuildError{1}`; the success is guild-core's
/// `apply(Rename)` as a game master: the rename commit WITHOUT a scroll (`GuildRename`, no snapshot),
/// `GuildSetNameNtf` to everybody and the `guild.rename` audit.
pub fn set_guild_name(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let c = character(cx, who)?;
    let raw = args.token().to_owned();
    let Some(guild) = cx.svc.social.guild.guild_record_of(c) else {
        return Ok(CmdDone::on(raw));
    };
    if guild.master() != Some(c) {
        return Ok(CmdDone::on(raw));
    }
    let id = guild.id;
    let name = match validate_guild_name(cx.ctx, &raw) {
        Ok(n) => n,
        Err(e) => {
            let tid = u32::try_from(name_error_tid(NameKind::Guild, e)).unwrap_or_default();
            cx.out.defined(tid, Vec::new());
            return Ok(CmdDone::on(raw));
        }
    };
    match apply(
        cx.svc,
        cx.ctx,
        id,
        GuildCmd::Rename(name.clone()),
        &GuildActor::Gm(who),
        None,
    ) {
        Ok(Applied::Changed(_) | Applied::Unchanged) => {}
        Err(GuildRefusal::NameTaken) => {
            guild_wire::to_one(cx.ctx, who.0, pb::GuildError { code: 1 });
        }
        Err(e) => tracing::debug!(error = ?e, %name, "/sgn was refused"),
    }
    Ok(CmdDone::on(name))
}

// ---- couples -------------------------------------------------------------------------------------------------------

/// Row 106 `/Propose <name>` (`CCoupleHelper::OnPropose`): [`couple::propose`] unchanged (every `TID_GAME_COUPLE_*`
/// text, the cooldown, the kneel, the honor titles and the audit stay there).
pub fn propose(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let name = args.token().to_owned();
    couple::propose(cx.svc, cx.ctx, who, &name);
    Ok(CmdDone::on(name))
}

/// Row 107 `/Refuse` (`OnRefuse`): [`couple::refuse`].
pub fn refuse(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    couple::refuse(cx.svc, cx.ctx, who);
    Ok(CmdDone::default())
}

/// Row 108 `/Couple` (`OnCouple`): [`couple::accept`].
pub fn couple(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    couple::accept(cx.svc, cx.ctx, who);
    Ok(CmdDone::default())
}

/// Row 109 `/Decouple` (`OnDecouple`): [`couple::decouple`].
pub fn decouple(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    couple::decouple(cx.svc, cx.ctx, who);
    Ok(CmdDone::default())
}

/// Row 110 `/ClearPropose` (`SendClearPropose`): forgets every proposal cooldown of the couple service and deletes
/// the stored `couple_proposals` rows with ONE batch (`SetCoupleProposal{p, None}` per proposer). The pending
/// `proposer` slots of online players stay (the legacy cleared only the database table).
pub fn clear_propose(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let _ = me(cx)?;
    let proposers = cx.svc.social.couple.clear_cooldowns();
    let ops: Vec<DbOp> = proposers
        .into_iter()
        .map(|p| DbOp::SetCoupleProposal {
            proposer_id: p,
            cooldown_until: None,
        })
        .collect();
    relation::commit(cx.ctx, "ClearPropose", ops);
    Ok(CmdDone::default())
}

/// Row 111 `/NextCoupleLevel (ncl)` (`TextCmd_NextCoupleLevel`): not coupled prints `COUPLE NOT FOUND`, couple level
/// 21 prints `MAX COUPLE LEVEL`; otherwise the EXP up to the next threshold is added through [`couple::add_experience`]
/// (the function of the cadence tick: level change, buff swap, `HS_COUPLE_LV`, rewards, `AddCoupleExperience`,
/// `UpsertCouple`).
pub fn next_couple_level(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let c = character(cx, who)?;
    let level_exp = &cx.ctx.data.couple.level_exp;
    let Some((level, exp)) = cx
        .svc
        .social
        .couple
        .get(c)
        .map(|k| (k.level(level_exp), k.experience))
    else {
        cx.out.text("COUPLE NOT FOUND");
        return Ok(CmdDone::default());
    };
    if level >= COUPLE_MAX_LEVEL {
        cx.out.text("MAX COUPLE LEVEL");
        return Ok(CmdDone::default());
    }
    match couple_exp_to_next(level_exp, exp) {
        Some(n) => couple::add_experience(cx.svc, cx.ctx, c, n),
        None => tracing::debug!(
            couple_exp = exp,
            level,
            "/ncl: the couple table has no next level"
        ),
    }
    Ok(CmdDone::default())
}

// ---- honor ---------------------------------------------------------------------------------------------------------

/// The `atoi` of a NUMBER token (`_ttoi`): the leading digits, 0 when there are none.
fn atoi(token: &str) -> i64 {
    let digits: String = token.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

/// Row 113 `/HonorTitleSet (hts) <titleIdx | "title text"> <n>` (`TextCmd_HonorTitleSet`): a NUMBER token is the index,
/// anything else an exact, case-sensitive match of a title text of `honorList.txt`; an index outside `0..150` or a
/// text without a match is silent. `n` is cut to `0..=100000000` (a negative `n` becomes 0; the legacy unsigned
/// compare made it 100000000, FIX). The counter is set DIRECTLY (no `need` equality rule, no `HonorChangeAck`, no
/// `honor_recheck`: a selected title whose counter fell below its `need` stays until the next recheck), the issuer gets
/// `HonorListAck`, an urgent save is asked for and `honor.gain {title, count, source: "gm"}` is audited.
pub fn honor_title_set(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let token = args.token().to_owned();
    let idx = if args.token_is_number() {
        atoi(&token)
    } else {
        match cx.ctx.data.honor.titles.iter().find(|t| t.title == token) {
            Some(t) => i64::from(t.id),
            None => return Ok(CmdDone::on(token)),
        }
    };
    let Some(slot) = u16::try_from(idx)
        .ok()
        .filter(|s| usize::from(*s) < HONOR_TITLE_COUNT)
    else {
        return Ok(CmdDone::on(token));
    };
    let n = args.number().clamp(0, HONOR_COUNT_MAX);
    match cx.ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        Some(p) => p.social.honor_doc.set(slot, n),
        None => return Ok(CmdDone::on(token)),
    }
    honor::list_ack(cx.ctx, who);
    cx.svc.saves.mark_urgent(who.0);
    crate::audit::emit_for(
        cx.ctx,
        who,
        ev::HONOR_GAIN,
        json!({ "title": slot, "count": n, "source": "gm" }),
    );
    Ok(CmdDone::on(token))
}

// ---- rename ---------------------------------------------------------------------------------------------------------

/// Row 120 `/SetPlayerName (spn) <newName>` (`TextCmd_QuerySetPlayerName`): renames the issuer WITHOUT a scroll (the
/// legacy path had no scroll, no quiz-world check and no mode flag to clear). The name goes through the scroll's
/// validator (`validate_character_name`; an error prints its `DefinedText` and stops), the directory must not know it
/// (`TID_DIAG_0034`, like the scroll), then ONE `CharacterRename` batch (no snapshot); the shared tail
/// (`player_data::complete_rename`) takes the name in the directory and on the avatar, sends
/// `QuerySetPlayerNameNtf` to everybody (and `UpdatePlayerData` to the listeners) and audits `char.rename`. The
/// current name or a rename that is already running changes nothing.
pub fn set_player_name(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let c = character(cx, who)?;
    let raw = args.token().to_owned();
    let gameplay = &cx.ctx.cfg.rules.gameplay;
    let name = match validate_character_name(
        &raw,
        &cx.ctx.data.name_rules,
        gameplay.min_name_letters,
        gameplay.max_name_letters,
    ) {
        Ok(n) => n.to_owned(),
        Err(e) => {
            let tid = u32::try_from(e.tid()).unwrap_or_default();
            cx.out.defined(tid, Vec::new());
            return Ok(CmdDone::on(raw));
        }
    };
    let (current, pending) = cx
        .ctx
        .worlds
        .player(who.0)
        .and_then(|m| m.player())
        .map_or((String::new(), true), |p| {
            (p.ident.name.clone(), p.social.rename_pending)
        });
    if pending || current == name {
        return Ok(CmdDone::on(name));
    }
    let taken = match cx.svc.presence.by_name(&name) {
        Some(e) => e.id != c,
        None => cx.svc.presence.name_taken(&name),
    };
    if taken {
        cx.out.defined(TID_DIAG_0034, Vec::new());
        return Ok(CmdDone::on(name));
    }
    submit_rename(cx.ctx, who, c, name.clone(), RenameVia::GM);
    Ok(CmdDone::on(name))
}

// ---- campus --------------------------------------------------------------------------------------------------------

/// Row 217 `/RemoveCampusMember (rcm) <name>` (`TextCmd_RemoveCampusMember`): the name is looked up in the player
/// directory (online or offline, ASCII case-insensitive); an unknown one answers `TID_DIAG_0060` with the quoted name,
/// else [`campus::remove_member`] decides (its role rules and the 5-point penalty of the requester stay there).
pub fn remove_campus_member(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let name = args.token().to_owned();
    match cx.svc.presence.by_name(&name).map(|e| e.id) {
        Some(target) => campus::remove_member(cx.svc, cx.ctx, who, target),
        None => cx.out.defined(TID_DIAG_0060, vec![format!("\"{name}\"")]),
    }
    Ok(CmdDone::on(name))
}

/// Row 218 `/UpdateCampusPoint (ucp) <n>` (`TextCmd_UpdateCampusPoint`): adds `n` (negative removes, the balance
/// saturates) to the issuer's campus points through the campus service's single writer with the reason `'G'`
/// (`SetCampusPoints`, `CampusUpdatePoint` to the issuer, `campus.points`); no campus membership is needed.
pub fn update_campus_point(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let c = character(cx, who)?;
    let n = args.number();
    add_points(cx.svc, cx.ctx, c, n, PointReason::Gm);
    Ok(CmdDone::default())
}
