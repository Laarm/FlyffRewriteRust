//! Errors, warnings and value sources of the configuration loader (Plan/10 §8.4).

use std::fmt;

/// Where a value came from (`flyff config show`, error messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueSource {
    /// Built-in default.
    Default,
    /// `flyff.toml` (line when known).
    File {
        /// 1-based line number of the key, best effort.
        line: Option<u32>,
    },
    /// Environment variable `FLYFF__SECTION__KEY`.
    Env {
        /// Variable name.
        var: String,
    },
    /// Command-line flag.
    Cli {
        /// Flag name.
        flag: &'static str,
    },
    /// Programmatic override (`ServerOptions::config_overrides`) or a `runtime_override` row
    /// (`set_by` = `"options"`, `"gm:<account>"`, `"admin:owner:local"`, `"admin:console"`).
    Override {
        /// Who set it.
        set_by: String,
    },
}

impl ValueSource {
    /// The `# …` annotation of `flyff config show` (`""` for defaults).
    pub fn annotation(&self) -> String {
        match self {
            ValueSource::Default => String::new(),
            ValueSource::File { line: Some(l) } => format!("# flyff.toml:{l}"),
            ValueSource::File { line: None } => "# flyff.toml".to_owned(),
            ValueSource::Env { var } => format!("# env {var}"),
            ValueSource::Cli { flag } => format!("# flag {flag}"),
            ValueSource::Override { set_by } => format!("# override ({set_by})"),
        }
    }
}

/// One validation problem; the loader collects all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// Dotted key (empty for file-level errors).
    pub key: String,
    /// Source of the offending value.
    pub source: ValueSource,
    /// Description.
    pub message: String,
}

impl ConfigError {
    /// An error about `key`.
    pub fn new(key: impl Into<String>, source: ValueSource, message: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            source,
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let origin = match &self.source {
            ValueSource::Default => String::new(),
            ValueSource::File { line: Some(l) } => format!(" (flyff.toml line {l})"),
            ValueSource::File { line: None } => " (flyff.toml)".to_owned(),
            ValueSource::Env { var } => format!(" (env {var})"),
            ValueSource::Cli { flag } => format!(" (flag {flag})"),
            ValueSource::Override { set_by } => format!(" (override: {set_by})"),
        };
        if self.key.is_empty() {
            write!(f, "{}{origin}", self.message)
        } else {
            write!(f, "{}: {}{origin}", self.key, self.message)
        }
    }
}

impl std::error::Error for ConfigError {}

/// A non-fatal finding (retired key, reserved key, odd environment variable, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWarning {
    /// The key concerned, when there is one.
    pub key: Option<String>,
    /// Description.
    pub message: String,
}

impl ConfigWarning {
    /// A warning about `key`.
    pub fn for_key(key: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            key: Some(key.into()),
            message: message.into(),
        }
    }

    /// A warning that belongs to no key.
    pub fn general(message: impl Into<String>) -> Self {
        Self {
            key: None,
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
