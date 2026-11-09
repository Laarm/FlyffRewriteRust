//! The one `unsafe` corner of the executable (everything else in this crate is safe Rust): two small libc calls
//! that have no safe wrapper in the dependencies we already have.
//!
//! * [`reset_sigpipe`]: Rust's runtime starts a program with `SIGPIPE` ignored, so `println!` into a closed pipe
//!   (`flyff check | head`) panics with `failed printing to stdout` and exits 101. A CLI command restores the
//!   default disposition, which ends it quietly like every Unix tool. `serve` never calls it (a server must see
//!   `EPIPE` as an error value, not die from a closed socket).
//! * [`install_startup_handlers`]: `SIGINT`/`SIGTERM` during the start-up (before tokio owns the signals) only
//!   record the signal number, so `serve` can stop the start-up gracefully (Plan/10 §15.2: a shutdown requested
//!   before S12 aborts the remaining steps, shuts the persistence down gracefully and exits 0). `SIGHUP` is
//!   ignored then and not recorded: the start reads `flyff.toml` anyway, and a reload request must neither kill
//!   the process nor abort the start.
//! * [`read_line_no_echo`]: the password prompt of `flyff account create|passwd` (12 §5.5) reads a line from the
//!   terminal with `ECHO` off and restores the terminal afterwards.

#![allow(unsafe_code)]

use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicI32, Ordering};

/// Restores the default `SIGPIPE` disposition for this process (call once, at the start of a CLI command).
pub fn reset_sigpipe() {
    // SAFETY: `signal(2)` with `SIG_DFL` only changes the disposition of one signal; no handler function is
    // installed and no memory is read or written. It runs before the command starts any thread.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// `true` when standard input is a terminal.
pub fn stdin_is_tty() -> bool {
    // SAFETY: `isatty(3)` only inspects a file descriptor number.
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

/// Prints `prompt` to stderr and reads one line from standard input with the terminal's echo switched off; the
/// trailing line break is removed. `Err(Unsupported)` when standard input is not a terminal.
pub fn read_line_no_echo(prompt: &str) -> io::Result<String> {
    if !stdin_is_tty() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "standard input is not a terminal",
        ));
    }
    // SAFETY: `termios` is a plain C struct of integers and arrays; the all-zero value is valid storage that
    // `tcgetattr` then fills in completely.
    let mut saved: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: `saved` is a valid, exclusively borrowed `termios`; fd 0 stays open for the whole call.
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut saved) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut quiet = saved;
    quiet.c_lflag &= !libc::ECHO;
    // SAFETY: `quiet` is a valid `termios` derived from the one `tcgetattr` returned.
    if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &quiet) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let _ = write!(io::stderr(), "{prompt}");
    let _ = io::stderr().flush();
    let mut line = String::new();
    let read = io::stdin().lock().read_line(&mut line);
    // SAFETY: `saved` is the unmodified `termios` of this terminal; restoring it undoes `quiet` even on an error.
    let _ = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &saved) };
    let _ = writeln!(io::stderr());
    read?;
    while line.ends_with(['\n', '\r']) {
        line.pop();
    }
    Ok(line)
}

/// The signal that arrived during the start-up (`0` = none).
static STARTUP_SIGNAL: AtomicI32 = AtomicI32::new(0);

/// The handler: stores the signal number and does nothing else (a lock-free atomic store is async-signal-safe).
extern "C" fn record_startup_signal(sig: libc::c_int) {
    STARTUP_SIGNAL.store(sig, Ordering::SeqCst);
}

/// Makes `SIGINT` and `SIGTERM` record themselves instead of killing the process (`SIGHUP` is ignored: it is not
/// recorded, so it never aborts the start); [`take_startup_signal`] reads the record. Call once, early in `serve`,
/// before any thread exists. Tokio's own handlers replace these when the runtime installs them.
pub fn install_startup_handlers() {
    let handler = record_startup_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    for sig in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: `signal(2)` installs a handler that only performs an atomic store, which is
        // async-signal-safe; no memory is shared with the interrupted code besides that atomic.
        unsafe {
            libc::signal(sig, handler);
        }
    }
    // SAFETY: `signal(2)` with `SIG_IGN` only changes the disposition of one signal; no handler function is
    // installed and no memory is read or written.
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
}

/// The signal recorded since [`install_startup_handlers`] (and clears it), if any.
pub fn take_startup_signal() -> Option<i32> {
    match STARTUP_SIGNAL.swap(0, Ordering::SeqCst) {
        0 => None,
        sig => Some(sig),
    }
}
