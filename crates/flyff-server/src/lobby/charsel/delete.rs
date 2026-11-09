//! `DelPlayer` (Plan/12 §10; WP-AUTH-08): the lobby half of the deletion. The ownership check, the throttle
//! and the password check run here; the guild-war veto, the soft delete and the social cleanup are the game
//! thread's (`GameInput::DeleteCharacter`).

use super::create::char_op_error;
use super::{GAME_REPLY_TIMEOUT, Lobby, lobby_error_label, tid};
use crate::audit::Actor;
use crate::gateway::{GatewaySession, LobbyOutcome};
use crate::lobby::auth::{AuthError, PwHash, Verify};
use crate::lobby::errors::code;
use crate::lobby::lobby_error;
use flyff_common::defs::text::{
    TID_15MIN_PREVENT, TID_15SEC_PREVENT, TID_DIAG_0039, TID_DIAG_0053, TID_ERROR_CERT_GENERAL,
};
use flyff_common::rt::reply_pair;
use flyff_common::{AccountId, CharacterId};
use flyff_db::queries;
use flyff_game::GameInput;
use flyff_proto::pb;

impl Lobby {
    /// `DelPlayer{id_player, pwhash}`.
    pub(super) async fn on_delete(&self, s: &mut GatewaySession, m: pb::DelPlayer) -> LobbyOutcome {
        let account = match self.claimed_account(s) {
            Ok(a) => a,
            Err(outcome) => return outcome,
        };
        let refuse = |s: &GatewaySession, msg: pb::ServerMsg| {
            self.core.stats.char_delete(&lobby_error_label(&msg));
            let _ = s.push_frame(msg);
            LobbyOutcome::Stay
        };
        let wrong = || lobby_error(code::NO_SUCH_GROUP, tid(TID_DIAG_0039));
        // 2: an active character of the account (a fresh list)
        let list = match self.read_list(account).await {
            Ok(l) => l,
            Err(e) => return refuse(s, super::db_error_msg(&e)),
        };
        let Some(listed) = list.iter().find(|e| e.id.0 == m.id_player) else {
            return refuse(s, wrong());
        };
        let (listed_slot, listed_name) = (listed.slot, listed.name.clone());
        // 3: the address is not locked out (wrong delete passwords count towards the login lock)
        let now = self.core.clock.unix();
        if let Err(l) = self.core.throttle.check(s.peer(), now.0) {
            let (c, t) = if l.long {
                (code::PREVENT_LONG, TID_15MIN_PREVENT)
            } else {
                (code::PREVENT_SHORT, TID_15SEC_PREVENT)
            };
            return refuse(s, lobby_error(c, tid(t)));
        }
        // 4: the password (12 §10.1: any well-formed pwhash when `auth.delete_requires_password = false`)
        let Some(pw) = PwHash::parse(&m.pwhash) else {
            return refuse(s, wrong());
        };
        if self.core.settings().auth.delete_requires_password {
            match self.verify_account_password(account, &pw).await {
                Ok(true) => {}
                Ok(false) => {
                    self.core.throttle.fail(s.peer(), now.0);
                    return refuse(s, wrong());
                }
                Err(msg) => return refuse(s, *msg),
            }
            self.core.throttle.success(s.peer());
        }
        // 5-6 and the commit: the game thread
        let (reply, rx) = reply_pair();
        let input = GameInput::DeleteCharacter {
            account,
            character: CharacterId(m.id_player),
            reply,
        };
        if self.core.game_sender().send_async(input).await.is_err() {
            return refuse(s, lobby_error(code::UNAVAILABLE, tid(TID_DIAG_0053)));
        }
        match tokio::time::timeout(GAME_REPLY_TIMEOUT, rx.recv_async()).await {
            Ok(Ok(Ok(()))) => {
                self.core.stats.char_delete("ok");
                self.core.audit.emit(
                    "char.delete",
                    &Actor {
                        account: Some(account),
                        character: Some(CharacterId(m.id_player)),
                        name: Some(listed_name.clone()),
                        ip: Some(s.peer().to_string()),
                    },
                    serde_json::json!({
                        "character": m.id_player,
                        "account": account.0,
                        "slot": listed_slot,
                        "name": listed_name,
                        // the guild side of a deletion (doc 21) reports through its own hook
                        "guild_disbanded": false,
                    }),
                );
                self.push_list(s, account).await;
                LobbyOutcome::Stay
            }
            Ok(Ok(Err(e))) => refuse(s, char_op_error(e, true)),
            Ok(Err(_)) | Err(_) => {
                tracing::warn!(
                    account = account.0,
                    "the game thread did not answer a character deletion in time"
                );
                refuse(
                    s,
                    lobby_error(code::CERT_GENERAL, tid(TID_ERROR_CERT_GENERAL)),
                )
            }
        }
    }

    /// Verifies `pw` against the stored hash of `account`; `Ok(false)` for a mismatch. The transparent rehash
    /// of the login (12 §5.2) applies here too.
    async fn verify_account_password(
        &self,
        account: AccountId,
        pw: &PwHash,
    ) -> Result<bool, Box<pb::ServerMsg>> {
        let record = match self
            .core
            .persist
            .read(move |c| queries::account_by_id(c, account))
            .await
        {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Err(Box::new(lobby_error(
                    code::CERT_GENERAL,
                    tid(TID_ERROR_CERT_GENERAL),
                )));
            }
            Err(e) => return Err(Box::new(super::db_error_msg(&e))),
        };
        match self.core.hasher.verify(pw, &record.password_hash).await {
            Ok(Verify::Ok { needs_rehash }) => {
                if needs_rehash {
                    self.core.rehash(account, pw).await;
                }
                Ok(true)
            }
            Ok(Verify::Mismatch) => Ok(false),
            // no permit within 10 s: the server is busy (12 §5.2)
            Err(AuthError::PermitTimeout(_)) => Err(Box::new(lobby_error(
                code::OVERFLOW,
                tid(flyff_common::defs::text::TID_DIAG_0041),
            ))),
            Err(e) => {
                tracing::error!(account = account.0, "delete: password check failed: {e}");
                Err(Box::new(lobby_error(
                    code::CERT_GENERAL,
                    tid(TID_ERROR_CERT_GENERAL),
                )))
            }
        }
    }
}
