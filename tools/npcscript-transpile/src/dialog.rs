//! `WorldDialog.txt` as the transpiler sees it (Plan/20 §5.5): the key map and the text comments.

use crate::inputs::Inputs;
use crate::source::DIALOG_KEY_LINES;
use std::collections::BTreeMap;

/// The text of every key line mapped to its line number (`DialogKeyMap`, first occurrence wins like
/// `map::insert`).
#[derive(Clone, Debug, Default)]
pub struct KeyMap {
    by_text: BTreeMap<String, u32>,
    /// Key lines whose text is empty (the original refused to start).
    pub empty_lines: Vec<u32>,
}

impl KeyMap {
    /// Builds the map from the dialog lines.
    pub fn new(lines: &[String]) -> KeyMap {
        let mut map = KeyMap::default();
        for &line in &DIALOG_KEY_LINES {
            let text = usize::try_from(line)
                .ok()
                .and_then(|i| lines.get(i))
                .map_or("", String::as_str);
            if text.is_empty() {
                map.empty_lines.push(line);
            } else {
                map.by_text.entry(text.to_owned()).or_insert(line);
            }
        }
        map
    }

    /// The key line whose text is `text` (case-sensitive).
    pub fn line_of(&self, text: &str) -> Option<u32> {
        self.by_text.get(text).copied()
    }
}

/// The comment for one text id: the first 60 characters of the line, control characters removed;
/// ids past the end of the table read `<missing line N: empty text>` (KEEP: `GetDialogText` gives `""`).
pub fn text_comment(inputs: &Inputs, id: i64) -> String {
    let in_table = usize::try_from(id).is_ok_and(|i| i < inputs.dialog_lines.len());
    if !in_table {
        return format!("<missing line {id}: empty text>");
    }
    format!("\"{}\"", clip(inputs.text(id), 60))
}

/// Control characters removed, `]` followed by `=`/`]` broken up (the text may land inside a long
/// comment), at most `max` characters with `...` when cut.
pub fn clip(text: &str, max: usize) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .replace("]]", "] ]")
        .replace("]=", "] =");
    let count = cleaned.chars().count();
    if count <= max {
        return cleaned;
    }
    let mut cut: String = cleaned.chars().take(max).collect();
    cut.push_str("...");
    cut
}
