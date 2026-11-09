//! Character queries (Plan/25 §4.3-§4.4, §11.5): `InspectCharacter` and `Containers` (WP-ADMIN-16).
//!
//! Both answer from the live game only: for a character that is not in the world `InspectCharacter` gives the
//! directory's facts with `online = false` (the route then reads the database) and `Containers` is `NotFound`
//! ("not online"). Banks are account-scoped (ADM-D8): the bank of any character of the account is served by an
//! **online sibling**; the console asks `OnlineSiblings(account)` and then `Containers{sibling, Bank{slot}}`.

use super::AdminQueryResult;
use crate::admin::types::{AdminErrorCode, AdminFailure, ContainerLoc};
use crate::admin::watch::{self, CharacterDetail};
use crate::tick::Game;
use flyff_common::CharacterId;

/// `InspectCharacter`: the live [`CharacterDetail`] of an online character; a character the directory knows but
/// that is not in the world answers with `online = false`; an unknown id is `NotFound`.
pub(super) fn inspect_character(g: &Game, character: CharacterId) -> AdminQueryResult {
    if let Some((_, m)) = watch::player_of(g, character)
        && let Some(d) = watch::detail_of(g, m)
    {
        return AdminQueryResult::Character(Box::new(d));
    }
    match watch::offline_detail(g, character) {
        Some(d) => AdminQueryResult::Character(Box::new(d)),
        None => AdminQueryResult::not_found(format!("character {character} does not exist")),
    }
}

/// `Containers`: one live container of an online character (a one-shot [`watch::ContainerSnapshot`], `rev` 0).
pub(super) fn containers(
    g: &Game,
    character: CharacterId,
    container: ContainerLoc,
) -> AdminQueryResult {
    match container {
        ContainerLoc::Mailbox => {
            return AdminQueryResult::Failed(AdminFailure::new(
                AdminErrorCode::Invalid,
                "the mailbox is not a container: ask for the mailbox",
            ));
        }
        // the bank of a guild lives in the guild service, not in a character (WP-ADMIN-23)
        ContainerLoc::GuildBank(guild) => return super::guilds::bank_container(g, guild),
        ContainerLoc::Pocket(i) if i >= 3 => {
            return AdminQueryResult::Failed(AdminFailure::new(
                AdminErrorCode::Invalid,
                format!("pocket {i} does not exist (0..=2)"),
            ));
        }
        ContainerLoc::Bank { slot } if slot >= 3 => {
            return AdminQueryResult::Failed(AdminFailure::new(
                AdminErrorCode::Invalid,
                format!("bank slot {slot} does not exist (0..=2)"),
            ));
        }
        _ => {}
    }
    if watch::player_of(g, character).is_none() {
        return AdminQueryResult::not_found(format!("character {character} is not online"));
    }
    match watch::live_container(g, character, container) {
        Some(s) => AdminQueryResult::Container(Box::new(s)),
        None => AdminQueryResult::not_found(format!(
            "character {character} has no such container ({container:?})"
        )),
    }
}

/// The `CharacterDetail` type, for the callers of [`AdminQueryResult::Character`].
pub type Detail = CharacterDetail;
