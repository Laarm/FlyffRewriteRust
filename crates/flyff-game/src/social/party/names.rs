//! Advanced parties and the share modes (Plan/21 §4.4, §4.7; WP-SOCIAL-04): `ChangeTroup`, `ChanPartyName`,
//! `PartyChangeItemModeReq`, `PartyChangeExpModeReq` and the party-name rule.
//!
//! **Names.** A party name is global and case-insensitive (PL-058); every leader keeps at most one stored name
//! (`party_names`, `DbOp::SetPartyName`). A leader may re-use his own stored name; any other taken name answers
//! `ErrorParty{204}`. `ChangeTroup` requires a valid name (FIX VR-052: the original read an uninitialised buffer
//! when the client sent none).

use super::wire::{ERROR_DIFFERNTUSERNAME, ERROR_NOPARTY, error, to_members};
use super::{identity, skills, sync_links};
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::CharacterId;
use flyff_data::names::NameRules;
use flyff_db::DbOp;
use flyff_proto::pb;
use flyff_rules::names::NameError;
use flyff_rules::social::{ADVANCED_MIN_LEVEL, PartyKind};

use super::model::{ExpShareMode, ItemShareMode};

/// The shortest party name (the "Party name" row of 23 §7: 4..=16 characters).
pub const MIN_PARTY_NAME: usize = 4;
/// The longest party name.
pub const MAX_PARTY_NAME: usize = 16;

/// The party-name rule of 23 §7 (kind `PartyName`): trimmed; 4 to 16 bytes; not starting with a digit; ASCII
/// alphanumeric only; no `InvalidName_ENG.inc` substring and only letters of `Letter_ENG.inc`. Returns the trimmed
/// name. Replaced by `validate_name(NameKind::PartyName, ..)` when WP-GM-02 lands.
pub fn validate_party_name<'a>(raw: &'a str, rules: &NameRules) -> Result<&'a str, NameError> {
    let name = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.len() < MIN_PARTY_NAME || name.len() > MAX_PARTY_NAME {
        return Err(NameError::Length);
    }
    if name.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return Err(NameError::LeadingDigit);
    }
    if !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(NameError::InvalidChar);
    }
    if rules.is_invalid_name(name) || !rules.is_allowed_letter(name, false) {
        return Err(NameError::Forbidden);
    }
    Ok(name)
}

/// What a leader's name request resolved to.
enum NameVerdict {
    /// Not allowed (invalid): ignore silently.
    Ignore,
    /// Taken by somebody else: `ErrorParty{204}`.
    Taken,
    /// Free for the leader; `true` when it is the leader's own stored name.
    Free { name: String, own: bool },
}

fn judge_name(svc: &Services, ctx: &GameCtx<'_>, leader: CharacterId, raw: &str) -> NameVerdict {
    let Ok(name) = validate_party_name(raw, &ctx.data.name_rules) else {
        return NameVerdict::Ignore;
    };
    if !svc.social.party.names.is_available_to(name, leader) {
        return NameVerdict::Taken;
    }
    let own = svc.social.party.names.owner_of(name) == Some(leader);
    // re-using the own stored name keeps its stored spelling
    let name = if own {
        svc.social.party.names.of(leader).unwrap_or(name)
    } else {
        name
    };
    NameVerdict::Free {
        name: name.to_owned(),
        own,
    }
}

/// Stores `name` for `leader` (in memory and `DbOp::SetPartyName`): replaces his previous name.
fn store_name(svc: &mut Services, ctx: &mut GameCtx<'_>, leader: CharacterId, name: &str) {
    svc.social.party.names.set(leader, name);
    ctx.persist.op(DbOp::SetPartyName {
        character_id: leader,
        name: name.to_owned(),
    });
}

/// `ChangeTroup{name}` (`CDPCacheSrvr::OnPartyChangeTroup`): the leader of a normal party of level 10 or more makes
/// it an advanced party. No party is `ErrorParty{202}`; a non-leader, an advanced party or a low level is ignored;
/// an invalid or missing name is ignored (VR-052); a name somebody else owns is `ErrorParty{204}`. On success the
/// members get `PartyChangeTroup{name}` and `PartyChangeName{name}`, and PARSKILL is evaluated.
pub fn on_change_troup(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::ChangeTroup,
) {
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let Some(pid) = svc.social.party.party_of(character) else {
        error(ctx, who.0, ERROR_NOPARTY, 0);
        return;
    };
    let Some(p) = svc.social.party.get(pid) else {
        return;
    };
    if p.leader_id() != Some(character)
        || p.kind == PartyKind::Advanced
        || p.level < ADVANCED_MIN_LEVEL
    {
        return;
    }
    let (name, own) = match judge_name(svc, ctx, character, &m.name) {
        NameVerdict::Ignore => return,
        NameVerdict::Taken => {
            error(ctx, who.0, ERROR_DIFFERNTUSERNAME, 0);
            return;
        }
        NameVerdict::Free { name, own } => (name, own),
    };
    if !own {
        store_name(svc, ctx, character, &name);
    }
    if let Some(p) = svc.social.party.get_mut(pid) {
        p.kind = PartyKind::Advanced;
        p.name = name.clone();
    }
    to_members(svc, ctx, pid, pb::PartyChangeTroup { name: name.clone() });
    to_members(svc, ctx, pid, pb::PartyChangeName { name });
    sync_links(svc, ctx, pid);
    skills::reevaluate_parskill(svc, ctx, pid);
}

/// `ChanPartyName{name}` (`OnPartyChangeName`): the leader of an advanced party renames it. No party is
/// `ErrorParty{202}`; a name somebody else owns is 204; the members get `PartyChangeName{name}`.
pub fn on_chan_party_name(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::ChanPartyName,
) {
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let Some(pid) = svc.social.party.party_of(character) else {
        error(ctx, who.0, ERROR_NOPARTY, 0);
        return;
    };
    let Some(p) = svc.social.party.get(pid) else {
        return;
    };
    if p.leader_id() != Some(character) || p.kind != PartyKind::Advanced {
        return;
    }
    let (name, own) = match judge_name(svc, ctx, character, &m.name) {
        NameVerdict::Ignore => return,
        NameVerdict::Taken => {
            error(ctx, who.0, ERROR_DIFFERNTUSERNAME, 0);
            return;
        }
        NameVerdict::Free { name, own } => (name, own),
    };
    if !own {
        store_name(svc, ctx, character, &name);
    }
    if let Some(p) = svc.social.party.get_mut(pid) {
        p.name = name.clone();
    }
    to_members(svc, ctx, pid, pb::PartyChangeName { name });
}

/// `PartyChangeItemModeReq{mode}` (`OnPartyChangeItemMode`): the leader of any party sets the item share mode,
/// `0..=3` only (VR-004). No party is `ErrorParty{202}`; a non-leader or another value is ignored.
pub fn on_item_mode(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::PartyChangeItemModeReq,
) {
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let Some(pid) = svc.social.party.party_of(character) else {
        error(ctx, who.0, ERROR_NOPARTY, 0);
        return;
    };
    let Some(mode) = ItemShareMode::from_wire(m.mode) else {
        return;
    };
    let Some(p) = svc.social.party.get_mut(pid) else {
        return;
    };
    if p.leader_id() != Some(character) {
        return;
    }
    p.item_share = mode;
    to_members(
        svc,
        ctx,
        pid,
        pb::PartyChangeItemModeNtf { mode: mode.wire() },
    );
    sync_links(svc, ctx, pid);
}

/// `PartyChangeExpModeReq{mode}` (`OnPartyChangeExpMode`): the leader of an **advanced** party sets the EXP share
/// mode, `0..=1` only (PL-086, VR-004: mode 2 and every other value are refused). No party is `ErrorParty{202}`.
pub fn on_exp_mode(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::PartyChangeExpModeReq,
) {
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let Some(pid) = svc.social.party.party_of(character) else {
        error(ctx, who.0, ERROR_NOPARTY, 0);
        return;
    };
    let Some(mode) = ExpShareMode::from_wire(m.mode) else {
        return;
    };
    let Some(p) = svc.social.party.get_mut(pid) else {
        return;
    };
    if p.leader_id() != Some(character) || p.kind != PartyKind::Advanced {
        return;
    }
    p.exp_share = mode;
    to_members(
        svc,
        ctx,
        pid,
        pb::PartyChangeExpModeNtf { mode: mode.wire() },
    );
}
