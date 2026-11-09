//! Debug check that game-thread-only state is not touched from another thread (Plan/10 §6.2, WP-OPS-06
//! `assert_game_thread()`).
//!
//! The owner is the first thread that touches the state (the game thread builds `Game` and runs it; `TestWorld`
//! uses the test thread), so no thread has to be named up front. In release builds the guard is empty and
//! [`GameThreadGuard::check`] compiles to nothing.

/// Remembers the first thread that called [`GameThreadGuard::check`].
#[derive(Debug, Default)]
pub struct GameThreadGuard {
    #[cfg(debug_assertions)]
    owner: std::sync::OnceLock<std::thread::ThreadId>,
}

impl GameThreadGuard {
    /// A guard without an owner yet.
    pub const fn new() -> Self {
        Self {
            #[cfg(debug_assertions)]
            owner: std::sync::OnceLock::new(),
        }
    }

    /// Debug assertion: the caller runs on the thread that owns this state.
    #[inline]
    pub fn check(&self) {
        #[cfg(debug_assertions)]
        {
            let me = std::thread::current().id();
            let owner = *self.owner.get_or_init(|| me);
            debug_assert_eq!(
                owner, me,
                "game-thread-only state touched from another thread"
            );
        }
    }
}
