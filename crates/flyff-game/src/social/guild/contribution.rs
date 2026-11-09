//! Contribution, levels, logo, notice and the kill contribution (Plan/21 §5.3, §5.7; WP-SOCIAL-12).
//!
//! **Contribution** (`GuildContributionReq{gold, item_flag}`, requires `events.guild_bank`): penya goes into the
//! guild pool, gem stacks into its contribution points (PXP); [`add_contribution`] is `CGuild::AddContribution`
//! (`_Common/guild.cpp:557-610`) verbatim: it fails for `pxp > 0` at level 50 and on any overflow and raises the guild
//! by at most ONE level per call ([`flyff_rules::social::guild_level_up`]). A request is ONE `GuildContribution`
//! batch (the contributor's snapshot after the removals, the guild row, the member row, the log rows); each successful
//! call tells the members with `GuildContributionNtf` - except when the guild is still unnamed (KEEP-QUIRK KQ-39 c:
//! state and persistence change, nothing is sent).
//!
//! **Kill contribution**: [`kill_hook`] (`DeathHooks.guild_kill`) adds `gameplay.guild_kill_contri` PXP for EVERY
//! kill by a guild member, any victim, not gated by `events.guild_bank` (as the original), persisted as the two single
//! ops `UpsertGuild` + `UpsertGuildMember`: no snapshot, no log row, no audit.

use super::commit::{self, Origin};
use super::model::Guild;
use super::ops::{GuildChange, GuildRefusal};
use crate::chat::ingress::notice;
use crate::combat::hooks::KillCtx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch};
use crate::item::container::StorageId;
use crate::item::ops::{RemoveReason, remove_item};
use flyff_common::config::EventId;
use flyff_common::defs::itemkind::IK3_GEM;
use flyff_common::defs::text::TID_GAME_GUILDNOTENGGOLD;
use flyff_common::defs::text::TID_GAME_GUILDWARNOLOGO;
use flyff_common::{CharacterId, GuildId};
use flyff_data::social::guild::GuildLevelRow;
use flyff_db::audit::social as audit;
use flyff_db::{AtomicBatch, DbOp, GuildBankLogKind, GuildBankLogRecord};
use flyff_proto::pb;
use flyff_rules::social::{
    GUILD_LOGO_GM_FROM, GUILD_LOGO_MAX, GUILD_LOGO_MIN_LEVEL, GUILD_NOTICE_MAX, MAX_GUILD_LEVEL,
    guild_level_up,
};

// ---- the arithmetic ----------------------------------------------------------------------------------------------

/// The numbers one contribution changes: the guild's level, PXP and penya pool and the contributor's totals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContribState {
    /// The guild level.
    pub level: i32,
    /// `m_dwContributionPxp`.
    pub pxp: u32,
    /// `m_nGoldGuild`.
    pub gold: u32,
    /// The member's `m_dwGivePxpCount`.
    pub member_pxp: u32,
    /// The member's `m_nGiveGold`.
    pub member_gold: i32,
}

impl ContribState {
    /// The state of `member` in `guild`; `None` when it is no member.
    pub fn of(guild: &Guild, member: CharacterId) -> Option<ContribState> {
        let m = guild.members.get(&member)?;
        Some(ContribState {
            level: guild.level,
            pxp: guild.pxp,
            gold: guild.gold,
            member_pxp: m.give_pxp,
            member_gold: m.give_gold,
        })
    }
}

/// `AddContribution(pxp, penya)` on a [`ContribState`]: `None` (and nothing changes) for `pxp > 0` at the top level
/// and for any overflow (the guild's PXP or penya or the member's PXP in `u32`, the member's penya in `i32`);
/// otherwise the new state and whether the guild rose one level. `table` is `GameData.guild.levels`.
pub fn add_to(
    state: &ContribState,
    pxp: u32,
    penya: u32,
    table: &[GuildLevelRow],
) -> Option<(ContribState, bool)> {
    if pxp > 0 && state.level >= MAX_GUILD_LEVEL {
        return None;
    }
    let mut s = *state;
    s.pxp = s.pxp.checked_add(pxp)?;
    s.gold = s.gold.checked_add(penya)?;
    s.member_pxp = s.member_pxp.checked_add(pxp)?;
    s.member_gold = s.member_gold.checked_add(i32::try_from(penya).ok()?)?;
    let mut level_up = false;
    if let Some((p, g)) = guild_level_up(s.level, s.pxp, s.gold, table) {
        s.pxp = p;
        s.gold = g;
        s.level += 1;
        level_up = true;
    }
    Some((s, level_up))
}

/// `CGuild::AddContribution` (21 §5.3): adds `pxp` and `penya` of `member` to `guild` and levels it up once when the
/// next row is paid. `false` (nothing changes) when [`add_to`] refuses or `member` is no member.
pub fn add_contribution(
    guild: &mut Guild,
    member: CharacterId,
    pxp: u32,
    penya: u32,
    table: &[GuildLevelRow],
) -> bool {
    let Some(state) = ContribState::of(guild, member) else {
        return false;
    };
    let Some((s, _)) = add_to(&state, pxp, penya, table) else {
        return false;
    };
    apply_state(guild, member, &s);
    true
}

fn apply_state(guild: &mut Guild, member: CharacterId, s: &ContribState) {
    guild.level = s.level;
    guild.pxp = s.pxp;
    guild.gold = s.gold;
    if let Some(m) = guild.members.get_mut(&member) {
        m.give_pxp = s.member_pxp;
        m.give_gold = s.member_gold;
    }
}

// ---- the request -------------------------------------------------------------------------------------------------

/// What one successful `AddContribution` call did (the numbers of its notice and its log row).
#[derive(Clone, Copy, Debug)]
struct Call {
    pxp: u32,
    penya: u32,
    level_up: bool,
    after: ContribState,
}

/// The gem stacks of an inventory in slot order that give contribution points: `(slot, count, value)` with
/// `value = ((item level + 1) / 2) * count` (usable `IK3_GEM` stacks only).
fn gem_stacks(ctx: &GameCtx<'_>, who: PlayerRef) -> Vec<(StorageId, u16, u32)> {
    let Some(p) = ctx.player(who).and_then(|m| m.player()) else {
        return Vec::new();
    };
    p.items
        .inventory
        .iter()
        .filter(|(_, it)| it.is_usable() && it.count > 0)
        .filter_map(|(id, it)| {
            let prop = ctx.data.item(it.item_id)?;
            if prop.item_kind3 != IK3_GEM {
                return None;
            }
            let value = (prop.item_lv.saturating_add(1) / 2).checked_mul(u32::from(it.count))?;
            (value > 0).then_some((id, it.count, value))
        })
        .collect()
}

/// `GuildContributionReq{gold, item_flag}` (`CDPSrvr::OnGuildContribution`): with `events.guild_bank` on and a
/// guild: `gold > 0` pays penya (`TID_GAME_GUILDNOTENGGOLD` when the purse is short), else `item_flag` walks the
/// inventory in slot order and turns every usable gem stack into PXP while the guild can take it (each stack is its
/// own call, so several level-ups per request are possible; at level 50 the gems stay); anything else is ignored.
pub fn on_guild_contribution(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildContributionReq,
) {
    if ctx.live.event_state(EventId::GuildInventory) == 0 {
        return;
    }
    let Some(c) = super::character_of(ctx, who) else {
        return;
    };
    let Some(gid) = svc.social.guild.guild_of(c) else {
        return;
    };
    let Some(state) = svc
        .social
        .guild
        .get(gid)
        .and_then(|g| ContribState::of(g, c))
    else {
        return;
    };
    let table = &ctx.data.guild.levels;
    let mut calls: Vec<Call> = Vec::new();
    let mut paid_gold = 0_u32;
    let mut stacks: Vec<(StorageId, u16)> = Vec::new();
    if m.gold > 0 {
        let have = ctx
            .player(who)
            .and_then(|mv| mv.player())
            .map_or(0, |p| i64::from(p.items.gold));
        if have < i64::from(m.gold) {
            notice::defined(ctx, who, TID_GAME_GUILDNOTENGGOLD, Vec::new());
            return;
        }
        let penya = u32::try_from(m.gold).unwrap_or(0);
        if let Some((after, level_up)) = add_to(&state, 0, penya, table) {
            paid_gold = penya;
            calls.push(Call {
                pxp: 0,
                penya,
                level_up,
                after,
            });
        }
    } else if m.item_flag {
        let mut st = state;
        for (slot, count, value) in gem_stacks(ctx, who) {
            if let Some((after, level_up)) = add_to(&st, value, 0, table) {
                st = after;
                stacks.push((slot, count));
                calls.push(Call {
                    pxp: value,
                    penya: 0,
                    level_up,
                    after,
                });
            }
        }
    }
    if calls.is_empty() || !can_snapshot(ctx, who) {
        return;
    }
    // ---- the commit block ----
    if paid_gold > 0 {
        crate::item::gold::add_gold(svc, ctx, who, -i64::from(paid_gold), true);
    }
    for (slot, count) in &stacks {
        remove_item(
            svc,
            ctx,
            who,
            *slot,
            *count,
            RemoveReason::Other("guild_contribution"),
        );
    }
    let at = commit::now(ctx);
    let mut logs: Vec<GuildBankLogRecord> = Vec::new();
    if let Some(last) = calls.last()
        && let Some(g) = svc.social.guild.get_mut(gid)
    {
        apply_state(g, c, &last.after);
    }
    for call in &calls {
        let kind = if call.level_up {
            Some(GuildBankLogKind::ContributionLevelUp)
        } else if call.penya > 0 {
            Some(GuildBankLogKind::PenyaIn)
        } else {
            None
        };
        if let Some(kind) = kind {
            let mut log =
                GuildBankLogRecord::new(gid, c, kind, at).with_penya(i64::from(call.penya));
            if call.level_up {
                log.upgrade_level = call.after.level;
            }
            if let Some(g) = svc.social.guild.get_mut(gid) {
                g.log_tail.push(log.clone());
            }
            logs.push(log);
        }
    }
    if let (Some((snap, generation)), Some(g)) =
        (snapshot_for_batch(svc, ctx, who), svc.social.guild.get(gid))
    {
        if let Some(row) = g.members.get(&c).map(|m| m.to_record(gid)) {
            let batch = AtomicBatch::guild_contribution(snap, g.to_record(), row, logs);
            commit::submit(
                svc,
                ctx,
                "GuildContribution",
                batch,
                &[(who, generation)],
                None,
            );
        }
    } else {
        tracing::error!(
            guild = gid.0,
            "a guild contribution could not be saved: no snapshot"
        );
    }
    for call in &calls {
        announce(svc, ctx, gid, c, call);
        commit::emit(
            ctx,
            Origin::Player(who),
            audit::guild_contribution(gid, c, call.pxp, call.penya, call.level_up, call.after.gold),
        );
    }
}

/// The `GuildContributionNtf` of one call, to the members of a named guild (KQ-39 c).
fn announce(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    member: CharacterId,
    call: &Call,
) {
    let named = svc.social.guild.get(guild).is_some_and(Guild::is_named);
    if !named {
        return;
    }
    super::wire::to_members(
        svc,
        ctx,
        guild,
        pb::GuildContributionNtf {
            id_guild: guild.0,
            id_player: member.0,
            pxp_delta: call.pxp,
            penya_delta: call.penya,
            guild_pxp: call.after.pxp,
            guild_penya: call.after.gold,
            guild_level: u32::try_from(call.after.level).unwrap_or(1),
        },
    );
}

// ---- the direct and the kill contribution ------------------------------------------------------------------------

/// One contribution without a purse or an inventory: the numbers, the two single ops and the notice. `false` when
/// [`add_to`] refuses.
fn contribute(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    member: CharacterId,
    pxp: u32,
    penya: u32,
) -> bool {
    let Some(state) = svc
        .social
        .guild
        .get(guild)
        .and_then(|g| ContribState::of(g, member))
    else {
        return false;
    };
    let Some((after, level_up)) = add_to(&state, pxp, penya, &ctx.data.guild.levels) else {
        return false;
    };
    if let Some(g) = svc.social.guild.get_mut(guild) {
        apply_state(g, member, &after);
    }
    let rows = svc.social.guild.get(guild).map(|g| {
        (
            DbOp::UpsertGuild(g.to_record()),
            g.members
                .get(&member)
                .map(|m| DbOp::UpsertGuildMember(m.to_record(guild))),
        )
    });
    if let Some((guild_op, member_op)) = rows {
        ctx.persist.op(guild_op);
        if let Some(op) = member_op {
            ctx.persist.op(op);
        }
    }
    announce(
        svc,
        ctx,
        guild,
        member,
        &Call {
            pxp,
            penya,
            level_up,
            after,
        },
    );
    true
}

/// The `DeathHooks.guild_kill` consumer (death pipeline step 3): a player in a guild killed something (any victim,
/// PvE or PvP): `gameplay.guild_kill_contri` PXP for the guild, silently at level 50; the value 0 disables it.
pub fn kill_hook(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let amount = ctx.cfg.rules.gameplay.guild_kill_contri;
    if amount == 0 {
        return;
    }
    let Some(c) = ctx
        .worlds
        .player(k.attacker)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
    else {
        return;
    };
    let Some(gid) = svc.social.guild.guild_of(c) else {
        return;
    };
    contribute(svc, ctx, gid, c, amount, 0);
}

/// `/gstat pxp|penya <n>` (23 row 66) for an already-authorised caller: adds `pxp` and `penya` to the contribution of
/// `who` in his own guild; no penya or item is removed. `false` without a guild or when the guild cannot take it.
pub fn contribute_direct(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    pxp: u32,
    penya: u32,
) -> bool {
    let Some(c) = super::character_of(ctx, who) else {
        return false;
    };
    let Some(gid) = svc.social.guild.guild_of(c) else {
        return false;
    };
    contribute(svc, ctx, gid, c, pxp, penya)
}

// ---- logo and notice ---------------------------------------------------------------------------------------------

/// `GuildLogoReq{logo}` (`OnGuildLogo` + the core's `OnGuildLogo`): `logo <= 27`, `logo >= 21` only for game masters,
/// the sender is the master of a guild of level 2 or more that is not at war (`TID_GAME_GUILDWARNOLOGO`) and has no
/// logo yet (set once); everything else is ignored silently.
pub fn on_guild_logo(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildLogoReq,
) {
    let Some(p) = ctx.player(who).and_then(|mv| mv.player()) else {
        return;
    };
    let (c, gm) = (p.ident.character_id, p.ident.authority.is_gm());
    if m.logo > GUILD_LOGO_MAX || (m.logo >= GUILD_LOGO_GM_FROM && !gm) {
        return;
    }
    let Some(g) = svc.social.guild.guild_record_of(c) else {
        return;
    };
    if g.master() != Some(c) || g.level < GUILD_LOGO_MIN_LEVEL {
        return;
    }
    if g.at_war() {
        notice::defined(ctx, who, TID_GAME_GUILDWARNOLOGO, Vec::new());
        return;
    }
    if g.logo != 0 {
        return;
    }
    let gid = g.id;
    if let Err(e) = commit::set_logo(svc, ctx, Origin::Player(who), gid, m.logo, None) {
        tracing::debug!(?e, "a logo change was refused after its checks");
    }
}

/// `GuildNoticeReq{notice}` (`OnGuildNotice`): non-empty, at most 127 bytes, MASTER only (FIX VR-045, PL-093: the
/// original let anybody set it); `GuildNoticeNtf` goes to the members.
pub fn on_guild_notice(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildNoticeReq,
) {
    if m.notice.is_empty() || m.notice.len() > GUILD_NOTICE_MAX {
        return;
    }
    let Some(c) = super::character_of(ctx, who) else {
        return;
    };
    let Some(g) = svc.social.guild.guild_record_of(c) else {
        return;
    };
    if g.master() != Some(c) {
        return;
    }
    let gid = g.id;
    if let Err(e) = commit::set_notice(svc, ctx, Origin::Player(who), gid, &m.notice, None) {
        tracing::debug!(?e, "a notice change was refused after its checks");
    }
}

/// `/gstat logo <n>` (23 row 66) for an already-authorised caller: the logo `0..=27` of `who`'s own guild, replacing
/// any logo, at any guild level and during a war. `Invalid` for a larger value, `NotMember` without a guild.
pub fn set_logo(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    logo: u32,
) -> Result<GuildChange, GuildRefusal> {
    if logo > GUILD_LOGO_MAX {
        return Err(GuildRefusal::Invalid("logo"));
    }
    let gid = own_guild(svc, ctx, who)?;
    commit::set_logo(svc, ctx, Origin::Gm(who), gid, logo, None)
}

/// `/gstat notice <text>` (23 row 66) for an already-authorised caller: the notice of `who`'s own guild (at most 127
/// bytes).
pub fn set_notice(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    notice: &str,
) -> Result<GuildChange, GuildRefusal> {
    if notice.len() > GUILD_NOTICE_MAX {
        return Err(GuildRefusal::Invalid("notice"));
    }
    let gid = own_guild(svc, ctx, who)?;
    commit::set_notice(svc, ctx, Origin::Gm(who), gid, notice, None)
}

fn own_guild(svc: &Services, ctx: &GameCtx<'_>, who: PlayerRef) -> Result<GuildId, GuildRefusal> {
    let c = super::character_of(ctx, who).ok_or(GuildRefusal::NotFound)?;
    svc.social.guild.guild_of(c).ok_or(GuildRefusal::NotMember)
}
