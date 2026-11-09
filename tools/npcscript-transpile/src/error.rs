//! Error classes of the transpiler (Plan/20 §5.7: everything marked *error* ends in exit code 2).

use serde::Serialize;
use std::fmt;

/// The class of a finding that makes the run fail.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    /// A call of a name that is not in the helper table.
    UnknownHelper,
    /// A helper call with an argument count no C++ overload has.
    BadArity,
    /// An `UPPER_CASE` constant that no define header defines.
    UndefinedConstant,
    /// A `case` that neither ends in `break` nor is a grouped label, in the middle of a switch.
    SwitchFallThrough,
    /// A token, keyword or construct outside the grammar of §5.3.
    UnsupportedToken,
    /// Two definitions of the same C++ function.
    DuplicateFunction,
    /// Anything else that does not parse (unbalanced braces, a missing `;`, ...).
    Syntax,
    /// A void helper used as a value, an int where a statement is required, and similar.
    Type,
    /// A `g_tables` problem: malformed entry, unknown member, duplicate registration name.
    Registration,
    /// A function key outside `DIALOG_KEY_LINES`, or a `g_nKeys` that differs from the table.
    KeyTable,
    /// The `WorldDialog.txt` key lines are not usable (an empty key line).
    DialogKey,
    /// An input file is missing or unreadable.
    Input,
    /// The helper table disagrees with `NpcScript.h`.
    HeaderMismatch,
}

impl ErrorClass {
    /// The stable name used in the report and in test expectations.
    pub fn name(self) -> &'static str {
        match self {
            ErrorClass::UnknownHelper => "unknown_helper",
            ErrorClass::BadArity => "bad_arity",
            ErrorClass::UndefinedConstant => "undefined_constant",
            ErrorClass::SwitchFallThrough => "switch_fallthrough",
            ErrorClass::UnsupportedToken => "unsupported_token",
            ErrorClass::DuplicateFunction => "duplicate_function",
            ErrorClass::Syntax => "syntax",
            ErrorClass::Type => "type",
            ErrorClass::Registration => "registration",
            ErrorClass::KeyTable => "key_table",
            ErrorClass::DialogKey => "dialog_key",
            ErrorClass::Input => "input",
            ErrorClass::HeaderMismatch => "header_mismatch",
        }
    }

    /// Every class, in report order.
    pub const ALL: [ErrorClass; 13] = [
        ErrorClass::UnknownHelper,
        ErrorClass::BadArity,
        ErrorClass::UndefinedConstant,
        ErrorClass::SwitchFallThrough,
        ErrorClass::UnsupportedToken,
        ErrorClass::DuplicateFunction,
        ErrorClass::Syntax,
        ErrorClass::Type,
        ErrorClass::Registration,
        ErrorClass::KeyTable,
        ErrorClass::DialogKey,
        ErrorClass::Input,
        ErrorClass::HeaderMismatch,
    ];
}

impl fmt::Display for ErrorClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One error with its place in the input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// The class.
    pub class: ErrorClass,
    /// The C++ function (or file) the finding is about, empty when global.
    pub subject: String,
    /// 1-based line in the original file, 0 when unknown.
    pub line: u32,
    /// What is wrong.
    pub message: String,
}

impl Finding {
    /// A finding about `subject` at `line`.
    pub fn new(
        class: ErrorClass,
        subject: impl Into<String>,
        line: u32,
        message: impl Into<String>,
    ) -> Finding {
        Finding {
            class,
            subject: subject.into(),
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]", self.class)?;
        if !self.subject.is_empty() {
            write!(f, " {}", self.subject)?;
        }
        if self.line > 0 {
            write!(f, " (line {})", self.line)?;
        }
        write!(f, ": {}", self.message)
    }
}
