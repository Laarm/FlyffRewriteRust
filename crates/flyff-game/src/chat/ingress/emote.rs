//! Emotes (Plan/23 §6.10, F-311; ref/24 §11): `/laugh`, `/sad`, ... are not commands, they are normal chat lines
//! that the receiving client renders as an icon above the speaker.

use std::collections::BTreeMap;

/// `textEmotion.txt` (29 ENG entries): index to command name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EmoteTable {
    /// `(index, lower-cased command)` in index order.
    entries: Vec<(u32, String)>,
}

impl EmoteTable {
    /// The table of `ClientData.text_emotions`.
    pub fn from_map(map: &BTreeMap<u32, String>) -> Self {
        Self {
            entries: map
                .iter()
                .map(|(i, c)| (*i, c.to_ascii_lowercase()))
                .collect(),
        }
    }

    /// Number of emotes.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` without any emote.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `true` when a `/`-line is an emote: the remainder after the `/` equals an emote command, ASCII
    /// case-insensitively, and contains no `#` (legacy `stricmp(line + 1, EmoticonCmd[j])` after the `#` test).
    pub fn is_emote_line(&self, line: &str) -> bool {
        let Some(rest) = line.strip_prefix('/') else {
            return false;
        };
        if rest.contains('#') {
            return false;
        }
        self.entries
            .iter()
            .any(|(_, cmd)| cmd.eq_ignore_ascii_case(rest))
    }

    /// The icon index of an emote command (case-insensitive), for tests and the browser export.
    pub fn index_of(&self, cmd: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|(_, c)| c.eq_ignore_ascii_case(cmd))
            .map(|(i, _)| *i)
    }
}
