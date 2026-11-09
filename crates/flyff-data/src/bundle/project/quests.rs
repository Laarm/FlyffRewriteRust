//! `quests` (lazy): the projection `QuestProp -> QuestClientDef` of Plan/14 §10.5.1 (WP-DATA-15).
//!
//! Values are raw as loaded, except that texts are resolved and NPC keys are lower-cased. Nothing server-only is
//! exported: no `SetDialog` lines, drops, start effects, removals, params, guild or party quest props. With the
//! shipped data (no quests) the bundle is the empty stub with the destination tables of the client data.

use super::Sources;
use crate::bundle::BundleError;
use crate::bundle::types::{QuestClientDef, QuestsBundle};
use crate::quest::QuestProp;

/// `QuestProp -> QuestClientDef` (14 §10.5.1): [`QuestProp::client_def`].
pub fn project_quest(p: &QuestProp) -> QuestClientDef {
    p.client_def()
}

/// `quests` (lazy): every quest in ascending id order, the NPC links, and the two destination name tables of
/// the client data.
pub fn project_quests(src: &Sources<'_>) -> Result<QuestsBundle, BundleError> {
    let q = &src.game.quests;
    Ok(QuestsBundle {
        quests: q.quests.values().map(project_quest).collect(),
        quest_src: q.quest_src.clone(),
        quest_dst: q.quest_dst.clone(),
        quest_destinations: src.client.quest_destinations.clone(),
        patrol_destinations: src.client.patrol_destinations.clone(),
    })
}
