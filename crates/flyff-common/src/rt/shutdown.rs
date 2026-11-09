//! Game-thread entry to the `ShutdownCoordinator` (Plan/01 §4.4, 10 §15.1).

use crate::CharacterId;

/// In-game `/shutdown` and `/shutdown cancel` (Plan/23 §13 Q14). No tokio, no I/O wait; implemented
/// in `flyff-server` and handed to the game in `GameInit.shutdown`.
pub trait ShutdownHandle: Send + 'static {
    /// Requests a graceful shutdown; `None` countdown = `server.shutdown_countdown_s`.
    fn request(&self, countdown_s: Option<u32>, message: Option<String>, by: CharacterId);

    /// Cancels a running countdown (also resumes lobby logins); `false` when nothing was running.
    fn cancel(&self, by: CharacterId) -> bool;
}
