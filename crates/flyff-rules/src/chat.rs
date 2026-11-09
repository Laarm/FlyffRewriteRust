//! Pure chat-text helpers (Plan/23 §6.1, §6.8; WP-GM-05/06; ref/24 §5, §8, §9): the ingress normalisation of one
//! typed line, `ParsingEffect` (markup stripping), `RemoveCRLF`, `IsRight` and the chat-line classifiers the browser
//! shares with the server.
//!
//! Everything works on Unicode scalar values (the legacy code worked on the bytes of the single-byte ENG code
//! page, so one scalar = one legacy byte for every character the client could type; the DBCS and `#l` code-page
//! rules are DROP, S3). The functions allocate only their result and never panic.

/// `gameplay.chat_max_chars` default: the legacy `SetStrNull(string, 120)`.
pub const CHAT_MAX_CHARS: usize = 120;

/// `gameplay.chat_drop_bytes` default: whispers, party and guild lines of 260 bytes or more are dropped.
pub const CHAT_DROP_BYTES: usize = 260;

/// The numbered `Text` kind of a green chat line (11 §10.2).
pub const TEXT_GENERAL: u32 = 1;

/// The numbered `Text` kind of a message box (11 §10.2).
pub const TEXT_DIAG: u32 = 2;

/// The numbered `Text` kind of a green line written into the chat box (new: the list output of GM commands, BFIX1 #006;
/// `TEXT_GENERAL` lines of the legacy kind land in the centre notices, which keep only the last 6 lines).
pub const TEXT_CHAT: u32 = 3;

/// Replaces every `U+0000..=U+001F` and `U+007F` with a space (23 §6.1 step 2).
pub fn replace_controls(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c <= '\u{1f}' || c == '\u{7f}' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// `CString::TrimLeft()` / `TrimRight()`: ASCII whitespace.
fn trim_ws(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_ascii_whitespace())
}

/// The first `max` scalar values of `s` (`SetStrNull`, 23 §6.1 step 4).
pub fn cut_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => s.get(..i).unwrap_or(s),
        None => s,
    }
}

/// `CWndMgr::FormalizeChatString` (`WndManager.cpp:8013`, 23 §6.1 step 5): trim both ends; while the line starts
/// with `"`: delete it, and if the remainder is longer than one character and ends with `"` delete that too, then
/// trim both ends again.
pub fn formalize_chat_string(s: &str) -> String {
    let mut v: Vec<char> = trim_ws(s).chars().collect();
    while v.first() == Some(&'"') {
        v.remove(0);
        if v.len() > 1 {
            if v.last() == Some(&'"') {
                v.pop();
            }
            let s: String = v.iter().collect();
            v = trim_ws(&s).chars().collect();
        }
    }
    v.into_iter().collect()
}

/// `RemoveCRLF`: every two-character sequence `\` `n` becomes one space.
pub fn remove_crlf(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' && it.peek() == Some(&'n') {
            it.next();
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// The server half of the ingress normalisation of one typed player line (23 §6.1 steps 2-6, in the order
/// control characters, left trim, 120-character cut, `FormalizeChatString`, `\n` sequences): `None` for an empty
/// line (at step 3 or step 5), which is ignored without any reply.
pub fn normalize_line(raw: &str, max_chars: usize) -> Option<String> {
    let s = replace_controls(raw);
    let s = s.trim_start_matches(' ');
    if s.is_empty() {
        return None;
    }
    let s = formalize_chat_string(cut_chars(s, max_chars));
    if s.is_empty() {
        return None;
    }
    Some(remove_crlf(&s))
}

/// `ParsingEffect` (`FuncTextCmd.cpp:5733`, 23 §6.8): removes the `#` colour/style codes and turns the
/// two-character sequence `\` `n` into a real newline. Unknown `#x` pairs stay as they are.
///
/// | at `#` | action |
/// |---|---|
/// | end of string | dropped |
/// | `#c` + 8 chars | all 10 removed (fewer left: the rest is removed) |
/// | `#l` + 4 chars | all 6 removed (same truncation) |
/// | `#u`, `#b`, `#s` | removed |
/// | `#n` + any 1 char | all 3 removed (`#n` at the end: removed) |
/// | `#` + other | both kept |
pub fn parsing_effect(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(s.len());
    let mut i = 0_usize;
    while i < n {
        let c = chars.get(i).copied().unwrap_or('\0');
        if c == '#' {
            i += 1;
            let Some(&code) = chars.get(i) else { break };
            match code {
                'c' => {
                    // the code letter, then 8 colour characters
                    i += 1;
                    if i >= n {
                        break;
                    }
                    i += 7;
                }
                'l' => {
                    i += 1;
                    if i >= n {
                        break;
                    }
                    i += 3;
                }
                'n' => {
                    i += 1;
                    if i >= n {
                        break;
                    }
                }
                'u' | 'b' | 's' => {}
                other => {
                    out.push('#');
                    out.push(other);
                }
            }
            i += 1;
        } else if c == '\\' && chars.get(i + 1) == Some(&'n') {
            out.push('\n');
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// `IsRight` (`FuncTextCmd.cpp:1735`, 23 §6.4 step 1): `false` for a shout body that is only a `#`, exactly `#u`
/// or `#b`, or contains a `#c` with fewer than 8 characters from the `#c` to the end.
pub fn is_right(s: &str) -> bool {
    let len = s.chars().count();
    if len == 1 && s.starts_with('#') {
        return false;
    }
    if len == 2 && (s.starts_with("#u") || s.starts_with("#b")) {
        return false;
    }
    if let Some(byte_at) = s.find("#c") {
        let from = s.get(byte_at..).map_or(0, |t| t.chars().count());
        if from < 8 {
            return false;
        }
    }
    true
}

/// What the browser and the server both ask of a chat text (45 §8.3, 23 §6.2): `/` or `!` prefixed texts are
/// balloons/emoticons, never chat-log lines.
pub fn is_balloon_only(text: &str) -> bool {
    text.starts_with('/') || text.starts_with('!')
}
