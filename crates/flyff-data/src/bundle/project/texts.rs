//! `texts`, `help`, `names`, `defines` (`quests` is `quests.rs`).

use super::Sources;
use crate::bundle::BundleError;
use crate::bundle::rowtable::RowTable;
use crate::bundle::types::{
    DefinesBundle, EmotionRow, HelpBundle, HelpTopic, NamesBundle, TextRow, TextsBundle,
};
use crate::client::panels::NOTICE_KEY;
use crate::names::NameRules;

/// `texts`: the `textClient.inc` table, the emoticon commands and the dialog lines.
pub fn project_texts(src: &Sources<'_>) -> Result<TextsBundle, BundleError> {
    let g = src.game;
    let rows: Vec<TextRow> = g
        .texts
        .iter()
        .map(|(id, e)| TextRow {
            id,
            color: e.color,
            text: e.text.clone(),
        })
        .collect();
    Ok(TextsBundle {
        tids: RowTable::new(rows),
        size: g.texts.size(),
        emotions: src
            .client
            .text_emotions
            .iter()
            .map(|(id, text)| EmotionRow {
                id: *id,
                text: text.clone(),
            })
            .collect(),
        dialog: g.dialog_text.lines.clone(),
    })
}

/// `help` (lazy): topics, tree, FAQ, tips, instant help, tutorial, guide, the panels and the notice.
pub fn project_help(src: &Sources<'_>) -> Result<HelpBundle, BundleError> {
    let c = src.client;
    Ok(HelpBundle {
        topics: c
            .help
            .topics
            .iter()
            .map(|(k, v)| HelpTopic {
                key: k.clone(),
                text: v.clone(),
            })
            .collect(),
        tree: c.help.tree.clone(),
        faq: c.help.faq.clone(),
        tips: c.help.tips.clone(),
        instant: c.help.instant.clone(),
        tutorial: c.help.tutorial.clone(),
        guide: c.help.guide.clone(),
        panels: c
            .panels
            .iter()
            .filter(|(k, _)| k.as_str() != NOTICE_KEY)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        notice: c.notice().to_owned(),
    })
}

/// `names`: the chat word filter and the name rules.
pub fn project_names(src: &Sources<'_>) -> Result<NamesBundle, BundleError> {
    let r = &src.game.name_rules;
    Ok(NamesBundle {
        chat_filter: src
            .client
            .chat_filter
            .iter()
            .map(|(w, r)| [w.clone(), r.clone()])
            .collect(),
        invalid_substrings: r.invalid_substrings.clone(),
        allowed_letters: NameRules::allowed_string(&r.allowed_letters),
        vendor_allowed_letters: NameRules::allowed_string(&r.vendor_allowed_letters),
    })
}

/// `defines` (lazy): every define in file order. Values are the unsigned 32-bit patterns, like the generated
/// `flyff_common::defs` constants (`WBS_NOFOCUS` is 2,147,483,648).
pub fn project_defines(src: &Sources<'_>) -> Result<DefinesBundle, BundleError> {
    Ok(DefinesBundle {
        defines: src
            .game
            .defines
            .iter()
            .map(|(n, v)| (n.to_owned(), i64::from(v as u32)))
            .collect(),
    })
}
