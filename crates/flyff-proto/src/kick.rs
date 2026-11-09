//! `KickReason` (flyff-common's dependency-free mirror) to the wire enum (Plan/11 §3.7, 10 §7.7).

use crate::pb;
use flyff_common::kick::KickReason;

impl From<KickReason> for pb::kick::Reason {
    fn from(r: KickReason) -> Self {
        match r {
            KickReason::Unspecified => pb::kick::Reason::Unspecified,
            KickReason::ProtocolError => pb::kick::Reason::ProtocolError,
            KickReason::RateLimit => pb::kick::Reason::RateLimit,
            KickReason::MessageTooBig => pb::kick::Reason::MessageTooBig,
            KickReason::InvalidState => pb::kick::Reason::InvalidState,
            KickReason::DuplicateLogin => pb::kick::Reason::DuplicateLogin,
            KickReason::Timeout => pb::kick::Reason::Timeout,
            KickReason::GmKick => pb::kick::Reason::GmKick,
            KickReason::Banned => pb::kick::Reason::Banned,
            KickReason::Shutdown => pb::kick::Reason::Shutdown,
            KickReason::ServerError => pb::kick::Reason::ServerError,
            KickReason::SlowConsumer => pb::kick::Reason::SlowConsumer,
            KickReason::ReloadRequired => pb::kick::Reason::ReloadRequired,
            KickReason::SessionExpired => pb::kick::Reason::SessionExpired,
        }
    }
}
