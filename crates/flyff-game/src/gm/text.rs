//! The output of a command (Plan/23 §3.2): [`OutLine`] values rendered as protocol messages for a player and
//! as text for the console and the admin API (PL-117: `DefinedText` arguments stay a `Vec<String>`; the console
//! substitutes them positionally into the text-table template).

use crate::chat::ingress::notice::defined_text;
use flyff_data::texts::TextTable;
use flyff_proto::pb;
use flyff_rules::chat::{TEXT_CHAT, TEXT_DIAG, TEXT_GENERAL};

/// One line of command output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutLine {
    /// `pb::Text{kind: TEXT_GENERAL}`: a green line.
    Text(String),
    /// `pb::Text{kind: TEXT_DIAG}`: a message box.
    TextDiag(String),
    /// `pb::Text{kind: TEXT_CHAT}`: a green line in the chat box (the list output of GM commands, BFIX1 #006).
    ChatText(String),
    /// `pb::DefinedText`.
    Defined {
        /// The `TID_*`.
        tid: u32,
        /// The positional arguments.
        args: Vec<String>,
    },
    /// `pb::ReturnSay`: 1 `"[name] str"`, 2 cannot PM yourself, 3 `"%s does not exist."`.
    ReturnSay {
        /// The flag.
        flag: i32,
        /// The text.
        text: String,
    },
}

impl OutLine {
    /// The protocol message of this line.
    pub fn into_message(self) -> pb::server_msg::Msg {
        match self {
            OutLine::Text(t) => pb::Text {
                kind: TEXT_GENERAL,
                text: t,
            }
            .into(),
            OutLine::TextDiag(t) => pb::Text {
                kind: TEXT_DIAG,
                text: t,
            }
            .into(),
            OutLine::ChatText(t) => pb::Text {
                kind: TEXT_CHAT,
                text: t,
            }
            .into(),
            OutLine::Defined { tid, args } => defined_text(tid, args).into(),
            OutLine::ReturnSay { flag, text } => pb::ReturnSay { flag, text }.into(),
        }
    }

    /// The text of this line for the console and the admin API: texts as they are, `DefinedText` through the
    /// template of `texts` with the arguments substituted by position, `ReturnSay` as the legacy client shows it.
    pub fn render(&self, texts: &TextTable) -> String {
        match self {
            OutLine::Text(t) | OutLine::TextDiag(t) | OutLine::ChatText(t) => t.clone(),
            OutLine::Defined { tid, args } => {
                let template = texts.get(*tid);
                if template.is_empty() || template == "error" {
                    return format!("[text {tid}] {}", args.join(" "));
                }
                format_positional(template, args)
            }
            OutLine::ReturnSay { flag, text } => match flag {
                1 => text.clone(),
                2 => "You cannot send a PM to yourself, silly.".to_owned(),
                _ => format!("{text} does not exist."),
            },
        }
    }
}

/// The lines a command produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CmdOutput {
    /// In order.
    pub lines: Vec<OutLine>,
}

impl CmdOutput {
    /// Appends a green text line.
    pub fn text(&mut self, t: impl Into<String>) {
        self.lines.push(OutLine::Text(t.into()));
    }

    /// Appends a green line for the chat box (a line of a list; the centre notices keep only 6 lines).
    pub fn chat_text(&mut self, t: impl Into<String>) {
        self.lines.push(OutLine::ChatText(t.into()));
    }

    /// Appends a message-box text.
    pub fn diag(&mut self, t: impl Into<String>) {
        self.lines.push(OutLine::TextDiag(t.into()));
    }

    /// Appends a `DefinedText`.
    pub fn defined(&mut self, tid: u32, args: Vec<String>) {
        self.lines.push(OutLine::Defined { tid, args });
    }

    /// Appends a `ReturnSay`.
    pub fn return_say(&mut self, flag: i32, text: &str) {
        self.lines.push(OutLine::ReturnSay {
            flag,
            text: text.to_owned(),
        });
    }

    /// Every line rendered for the console.
    pub fn render(&self, texts: &TextTable) -> Vec<String> {
        self.lines.iter().map(|l| l.render(texts)).collect()
    }
}

/// Replaces the i-th printf conversion (`%s`, `%d`, `%u`, `%ld`, `%5.2f`, ...) of `template` with `args[i]`;
/// a missing argument becomes `""`, extra arguments are ignored, `%%` is a percent sign (11 §10.1).
pub fn format_positional(template: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut it = template.chars().peekable();
    let mut next = 0_usize;
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if it.peek() == Some(&'%') {
            it.next();
            out.push('%');
            continue;
        }
        // flags, width, precision, length modifiers
        let mut spec = String::new();
        while let Some(&n) = it.peek() {
            if n.is_ascii_digit()
                || matches!(
                    n,
                    '-' | '+' | ' ' | '#' | '.' | 'l' | 'h' | 'z' | 'j' | 't' | 'L' | 'I'
                )
            {
                spec.push(n);
                it.next();
            } else {
                break;
            }
        }
        match it.peek() {
            Some('s' | 'd' | 'i' | 'u' | 'f' | 'x' | 'X' | 'c' | 'g' | 'e' | 'p' | 'o') => {
                it.next();
                out.push_str(args.get(next).map_or("", String::as_str));
                next += 1;
            }
            _ => {
                // not a conversion: keep the text
                out.push('%');
                out.push_str(&spec);
            }
        }
    }
    out
}
