//! The `NetEvent` router (Plan/10 §7.8): one task drains C3 (`NetEvent`s of the game thread) and hands every
//! event to the lobby, which updates the online registry and tells the session task through the
//! [`SessionDirectory`](super::SessionDirectory). The game thread never locks the registry or the directory
//! (01 §4.3).

use super::lobby_port::LobbyPort;
use flyff_game::NetEvent;
use std::sync::Arc;

/// Runs until the game drops its sender (shutdown).
pub async fn run_net_event_router(rx: flume::Receiver<NetEvent>, lobby: Arc<dyn LobbyPort>) {
    while let Ok(ev) = rx.recv_async().await {
        tracing::debug!(?ev, "net event");
        lobby.on_net_event(ev);
    }
}
