//! `WorldDialog.txt`: the NPC dialog line table (`LoadDialogText`, Plan/14 §6.16, ref/16 §4.1;
//! src:WORLDDIALOG/NpcScript.cpp:147-232).
//!
//! A line table read with `fgets` in text mode: line `n` (0-based) is dialog string `n` (1,250 lines, windows-1252,
//! forced by the hint table of `flyff-vfs`). Each line keeps everything before its `\n` with one trailing `\r`
//! removed. **KEEP-QUIRK:** the original removes the last character of every `fgets` buffer
//! (`szBuffer[strlen - 1] = 0`), which is the `\n`, except for a final line without a newline, whose last
//! character is lost (`test 72` reads `test 7`). Lines longer than 1,023 bytes are not split as `fgets`
//! would (none exist).

use serde::{Deserialize, Serialize};

/// The dialog lines.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogText {
    /// Line `n` is `lines[n]`.
    pub lines: Vec<String>,
}

impl DialogText {
    /// `GetDialogText(n)`: the line, or `""` when out of range (scripts reference up to 1,266).
    pub fn get(&self, n: u32) -> &str {
        usize::try_from(n)
            .ok()
            .and_then(|i| self.lines.get(i))
            .map_or("", String::as_str)
    }

    /// The number of lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// `true` when there is no line.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Splits decoded `text` into dialog lines (see the module documentation).
pub fn parse_dialog_text(text: &str) -> DialogText {
    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(i) => {
                let line = rest.get(..i).unwrap_or("");
                lines.push(line.strip_suffix('\r').unwrap_or(line).to_owned());
                rest = rest.get(i + 1..).unwrap_or("");
            }
            None => {
                // the last line has no newline: `fgets` drops its last character
                let mut line = rest.to_owned();
                line.pop();
                lines.push(line);
                break;
            }
        }
    }
    DialogText { lines }
}
