//! The hooks other subsystems call into the social systems (Plan/21 §2.9, WP-SOCIAL-01).
//!
//! Docs 16, 17, 19 and 22 call these from M3 on; the callers own WHEN they fire, doc 21 owns WHAT they do. All
//! hooks are synchronous (same tick, same phase) so the message order matches the original call stacks. The kill,
//! siege and war hooks are still stubs of their owners; real are the bookkeeping of the
//! [`PlayerDirectory`](crate::presence::PlayerDirectory) that the join, leave, level and job changes imply (the
//! `UpdatePlayerData` broadcasts of 21 §3.2 are WP-SOCIAL-08) and the honor feeds (WP-SOCIAL-23: item use, egg,
//! lord, recheck).
//!
//! **Deviation from the plan's signatures:** feature code never takes `&mut Game` (10 §4.1.1, R15), so every hook
//! takes `(&mut Services, &mut GameCtx)`, and an online player is addressed by its [`PlayerRef`] (`ObjId`), not by
//! the `CharacterId` that 21 §2.9 writes (a `CharacterId` is for offline players only, `ctx.rs`).

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::presence::OnlineRef;
use flyff_common::{CharacterId, ObjId};

/// How a player died in PvP (doc 17 defines the real enum; WP-MOVER/combat replaces this one without renaming the
/// variants it keeps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PvpDeathMode {
    /// A duel, a free PK or a plain player kill.
    Normal,
    /// Guild war (21 §6.4).
    GuildWar,
    /// Guild siege (21 §7.5).
    Siege,
    /// Guild combat 1:1 (21 §8.5).
    Gc1to1,
}

/// The character id and the session of an online player.
fn identity(ctx: &GameCtx<'_>, p: PlayerRef) -> Option<(CharacterId, flyff_common::SessionId)> {
    let e = ctx.entry(p)?;
    Some((e.character, e.session.or(e.last_session)?))
}

/// After the join batch was queued (21 §3.4 join step 1): the directory marks the character online. The fan-out
/// messages, party re-attach, guild/messenger notices and honor recheck are WP-SOCIAL-08/03/14/19.
pub fn on_player_joined(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    let Some((character, session)) = identity(ctx, p) else {
        return;
    };
    let Some((world, layer)) = ctx.player(p).map(|m| (m.core.world, m.core.layer)) else {
        return;
    };
    // 21 §3.4 join 3: a re-attach keeps the entry online, so the guild-mates get no second login notice
    let was_online = svc
        .presence
        .get(character)
        .is_some_and(|e| e.online.is_some());
    svc.presence.set_online(
        character,
        Some(OnlineRef {
            session,
            obj: p.0,
            world,
            layer,
        }),
    );
    super::player_data::announce_login(svc, ctx, character);
    // step 3: the guild-mates learn of the login, the joiner gets the list of the online members
    super::guild::on_login(svc, ctx, p, was_online);
    // 21 §5.11 FIX: a guild buff carried over from the last session is stripped from a player without a guild
    super::guild::buffs::strip_stale(svc, ctx, p);
    // step 4: the online friends learn of the login (only when the character was offline before)
    super::messenger::presence::on_login(svc, ctx, character, was_online);
    // step 7: the notes the join batch showed are marked delivered (once per join, never by a re-attach)
    super::messenger::notes::after_join(svc, ctx, character);
    // 19 §20.5: the mail envelope (MODE_MAILBOX) shows exactly when unread mail exists
    crate::econ::mail::on_player_joined(svc, ctx, p);
}

/// At the final removal of an avatar, after the linger (21 §3.4 leave step 6): the directory marks it offline.
pub fn on_player_leaving(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    let Some(character) = ctx.entry(p).map(|e| e.character) else {
        return;
    };
    // 21 §3.4 leave step 1: the invitations of either side disappear
    svc.social.invites.drop_player(character);
    // step 2: the party member goes offline (the party worker purges it after 600 s)
    crate::social::party::on_logout(svc, ctx, p);
    // step 3: the guild-mates see the member go offline
    super::guild::on_logout(svc, ctx, p);
    // step 4: `AddFriendLogout` to every online friend that lists the leaver
    super::messenger::presence::on_logout(svc, ctx, character);
    // steps 5 and 6: `PresenceLogout` to the listeners, the directory goes offline, `UpdatePlayerData`
    super::player_data::announce_logout(svc, ctx, character);
    // the list of the leaver and its pending summons go (it was needed for the audiences above)
    super::messenger::on_leave(svc, character);
    super::messenger::notes::on_leave(svc, character);
}

/// A level change (doc 16): the directory entry follows. Campus reward and honor recheck are later WPs.
pub fn on_level_changed(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    p: PlayerRef,
    _old: u32,
    new: u32,
) {
    let Some(e) = ctx.entry(p) else { return };
    let Some(entry) = svc.presence.get(e.character) else {
        return;
    };
    let job = entry.job;
    let character = e.character;
    svc.presence
        .set_level_job(character, u8::try_from(new).unwrap_or(u8::MAX), job);
    super::player_data::announce_change(svc, ctx, character);
}

/// A job change (doc 16): the directory entry follows.
pub fn on_job_changed(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef, job: u8) {
    let Some(e) = ctx.entry(p) else { return };
    let Some(entry) = svc.presence.get(e.character) else {
        return;
    };
    let level = entry.level;
    let character = e.character;
    svc.presence.set_level_job(character, level, job);
    super::player_data::announce_change(svc, ctx, character);
}

/// A player killed a monster (doc 17): honor `HI_HUNT_MONSTER`, guild kill PXP (WP-SOCIAL-14/19). Stub.
pub fn on_monster_killed(
    _svc: &mut Services,
    _ctx: &mut GameCtx<'_>,
    _killer: PlayerRef,
    _monster: ObjId,
    _monster_level: i32,
) {
}

/// A player died to another player (doc 17, `SubWar` of `combat/pvp/kill.rs`). The ONLY caller passes
/// [`PvpDeathMode::GuildWar`], and only while `events.guild_war` is on and the guilds differ; the hook scores it
/// ([`super::guild::war::on_war_death`], WP-SOCIAL-15). The guild kill PXP rides `DeathHooks.guild_kill`, the siege's
/// deaths `DeathHooks::world_die`, so the other modes are no-ops here (WP-SOCIAL-16/25).
pub fn on_player_killed(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    killer: Option<PlayerRef>,
    victim: PlayerRef,
    mode: PvpDeathMode,
) {
    if let (PvpDeathMode::GuildWar, Some(killer)) = (mode, killer) {
        super::guild::war::on_war_death(svc, ctx, killer, victim);
    }
}

/// An item was used (doc 19, `OnAfterUseItem`): honor `HI_USE_ITEM` counts the item (WP-SOCIAL-23).
pub fn on_item_used(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef, item: u32) {
    super::honor::add(
        svc,
        ctx,
        p,
        item,
        flyff_common::defs::honor::HI_USE_ITEM,
        None,
    );
}

/// A pet egg hatched (doc 19): honor `HS_HATCHING_EGG` (WP-SOCIAL-23).
pub fn on_egg_hatched(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    super::honor::add(
        svc,
        ctx,
        p,
        flyff_common::defs::honor::HS_HATCHING_EGG,
        flyff_common::defs::honor::HI_COUNT_CHECK,
        None,
    );
}

/// A server-mode (SM) item mode changed (docs 16/19): the party-skill items re-evaluate the PARSKILL flag of the
/// owner's party (21 §4.8.4).
pub fn on_sm_mode_changed(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    p: PlayerRef,
    sm: u8,
    _active: bool,
) {
    crate::social::party::skills::on_sm_changed(svc, ctx, p, sm);
}

/// `CSLord::IsLord( idPlayer )` (doc 22 §9): `true` for the current lord of the election (`svc.event.lord`). The one body
/// that the honor title `HS_LORD` and the lord's party-skill time read.
pub fn is_lord(svc: &Services, c: CharacterId) -> bool {
    svc.event.lord.is_lord(c)
}

/// The lord changed (doc 22): the online players whose `HS_LORD` counter differs from [`is_lord`] are rechecked with
/// a notice (WP-SOCIAL-23).
pub fn on_lord_changed(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let data = ctx.data;
    let Some(title) = data
        .honor
        .title_by(
            flyff_common::defs::honor::HS_LORD,
            flyff_common::defs::honor::HI_EARN_TITLE,
        )
        .map(|t| t.id)
    else {
        return;
    };
    let stale: Vec<PlayerRef> = ctx
        .sessions
        .iter()
        .filter_map(|e| {
            let held = ctx
                .worlds
                .player(e.player)?
                .player()?
                .social
                .honor_doc
                .get(u16::try_from(title).ok()?);
            (held != i32::from(is_lord(svc, e.character))).then_some(PlayerRef(e.player))
        })
        .collect();
    for p in stale {
        super::honor::recheck(svc, ctx, p, true);
    }
}

/// Recomputes the honor titles of a player (21 §13.3; docs 16/17/20, `CheckHonorStat`). `notify` sends the list and
/// the selected title (WP-SOCIAL-23).
pub fn honor_recheck(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef, notify: bool) {
    super::honor::recheck(svc, ctx, p, notify);
}

/// The social part of a character deletion (21 §3.6; `CharacterRules::with_hooks`): the party side - an offline
/// member leaves like a purged one and a leader's stored advanced-party name is released (memory and database).
/// The guild part is here (WP-SOCIAL-10); the friend and campus parts join this type with their work packages.
#[derive(Debug, Default, Clone, Copy)]
pub struct SocialDeleteHooks;

impl crate::session::CharacterDeleteHooks for SocialDeleteHooks {
    fn check(
        &self,
        svc: &Services,
        _ctx: &GameCtx<'_>,
        character: CharacterId,
    ) -> Result<(), crate::input::CharOpError> {
        // 21 §3.6 / PL-040: no deletion while the guild is at war
        super::guild::check_character_delete(svc, character)
    }

    fn apply(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        character: CharacterId,
        ops: &mut Vec<flyff_db::DbOp>,
    ) {
        crate::social::party::on_character_deleted(svc, ctx, character);
        // the friend rows go with the batch, the online friends are told (21 §3.6)
        super::messenger::on_character_deleted(svc, ctx, character, ops);
        // a member leaves, the guild of a master is disbanded (its ops join the `CharacterDelete` batch)
        super::guild::on_character_deleted(svc, ctx, character, ops);
        // ---- campus, invitations and proposals (21 §3.6; couple-campus-honor): the character leaves its campus
        // like a deletion (no penalty; a deleted master takes the campus with him, the ops join the batch), the
        // pending invitations and the proposals that name it are dropped; the couple stays (13 OQ14, KEEP)
        super::campus::members::on_character_deleted(svc, ctx, character, ops);
        svc.social.invites.drop_player(character);
        super::couple::flow::forget_proposals_of(ctx, character);
        if svc.social.party.names.of(character).is_some() {
            svc.social.party.names.remove(character);
            ops.push(flyff_db::DbOp::DeletePartyName {
                character_id: character,
            });
        }
        // ---- housing (doc 22 §13, WP-EVENT-16): the character stops being a visitor of every room (the ops join the batch)
        crate::event::housing::on_character_deleted(svc, ctx, character, ops);
    }
}
