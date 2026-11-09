//! Process exit codes (Plan/10 §3.4).

/// Exit code of the process.
pub type ExitStatusCode = i32;

/// Clean shutdown; successful CLI command.
pub const OK: ExitStatusCode = 0;
/// Any other startup or CLI error (bind failure, I/O, `check` findings).
pub const FAILURE: ExitStatusCode = 1;
/// Fatal data load error.
pub const DATA: ExitStatusCode = 2;
/// CLI usage error (clap).
pub const USAGE: ExitStatusCode = 64;
/// `Resource/` missing or without `Masquerade.prj`.
pub const NOINPUT: ExitStatusCode = 66;
/// Unscoped panic, or the persistence writer died.
pub const SOFTWARE: ExitStatusCode = 70;
/// A restart asked for in the admin console (Plan/25 §9.7): the shutdown was graceful and a supervisor
/// (`Restart=on-failure`) starts the server again. Only used when a supervisor was detected.
pub const RESTART: ExitStatusCode = 69;
/// Database corrupt or fatal I/O on the database.
pub const IOERR: ExitStatusCode = 74;
/// Instance lock held.
pub const TEMPFAIL: ExitStatusCode = 75;
/// Invalid configuration, init-only mismatch.
pub const CONFIG: ExitStatusCode = 78;
