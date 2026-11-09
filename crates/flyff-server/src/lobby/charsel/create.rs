//! `CreatePlayer` (Plan/12 §9.1; WP-AUTH-07): the lobby half of character creation. The checks of steps 2-6 run
//! here with the shared rules of `flyff-rules`; the authoritative name and slot check, the id allocation and the
//! commit are the game thread's (`GameInput::CreateCharacter`, step 7).

use super::{GAME_REPLY_TIMEOUT, Lobby, illegal_access, lobby_error_label, tid};
use crate::audit::Actor;
use crate::gateway::{GatewaySession, LobbyOutcome};
use crate::lobby::errors::code;
use crate::lobby::lobby_error;
use flyff_common::AccountId;
use flyff_common::defs::text::{
    TID_DIAG_0018, TID_DIAG_0034, TID_DIAG_0039, TID_DIAG_0053, TID_ERROR_CERT_GENERAL,
};
use flyff_common::rt::{CloseReason, KickReason, reply_pair};
use flyff_game::GameInput;
use flyff_game::input::{CharOpError, CreateCharacterRequest};
use flyff_proto::pb;
use flyff_rules::appearance::validate_appearance;
use flyff_rules::names::validate_character_name;
use flyff_rules::pin::validate_bank_pin;

/// The `LobbyError` of a game-thread refusal (12 §16.3).
pub(super) fn char_op_error(e: CharOpError, delete: bool) -> pb::ServerMsg {
    match e {
        CharOpError::NameTaken => lobby_error(code::USER_EXISTS, tid(TID_DIAG_0034)),
        CharOpError::SlotTaken => lobby_error(code::DUPLICATE_SLOT, tid(TID_DIAG_0018)),
        CharOpError::NotFound if delete => lobby_error(code::NO_SUCH_GROUP, tid(TID_DIAG_0039)),
        CharOpError::NotFound => illegal_access(),
        CharOpError::Wartime => lobby_error(
            code::WARTIME,
            tid(flyff_common::defs::text::TID_GAME_GUILDWARDOING),
        ),
        CharOpError::ShuttingDown => lobby_error(code::UNAVAILABLE, tid(TID_DIAG_0053)),
        // `Online` cannot happen (the entry is `Claimed`, 12 §10.2 step 5)
        CharOpError::Online | CharOpError::Internal => {
            lobby_error(code::CERT_GENERAL, tid(TID_ERROR_CERT_GENERAL))
        }
    }
}

impl Lobby {
    /// Step 1 of 12 §9.1 and §10.2: this socket is the attached one of a `Claimed` entry. Returns the account.
    pub(super) fn claimed_account(&self, s: &GatewaySession) -> Result<AccountId, LobbyOutcome> {
        let invalid = || LobbyOutcome::Close(CloseReason::Kicked(KickReason::InvalidState));
        let account = s.account().ok_or_else(invalid)?;
        let view = self.core.registry.view(account).ok_or_else(invalid)?;
        if view.kind != crate::lobby::online::EntryKind::Claimed
            || view.socket != Some(s.id())
            || view.revoked
        {
            return Err(invalid());
        }
        Ok(account)
    }

    /// `CreatePlayer{slot, name, skin_set, hair_mesh, sex, head_mesh, bank_pin}`.
    pub(super) async fn on_create(
        &self,
        s: &mut GatewaySession,
        m: pb::CreatePlayer,
    ) -> LobbyOutcome {
        let account = match self.claimed_account(s) {
            Ok(a) => a,
            Err(outcome) => return outcome,
        };
        let refuse = |s: &GatewaySession, msg: pb::ServerMsg| {
            self.core.stats.char_create(&lobby_error_label(&msg));
            let _ = s.push_frame(msg);
            LobbyOutcome::Stay
        };
        // 2: the slot range
        let slot = match u8::try_from(m.slot) {
            Ok(v) if v <= 2 => v,
            _ => return refuse(s, lobby_error(code::SLOT_OUTOFRANGE, tid(TID_DIAG_0039))),
        };
        // 3: the slot is free and the account has fewer than three characters (a fresh list)
        let list = match self.read_list(account).await {
            Ok(l) => l,
            Err(e) => return refuse(s, super::db_error_msg(&e)),
        };
        if list.len() >= 3 || list.iter().any(|e| e.slot == slot) {
            return refuse(s, lobby_error(code::DUPLICATE_SLOT, tid(TID_DIAG_0018)));
        }
        // 4: sex, hair, head and skin ranges
        if validate_appearance(m.sex, m.hair_mesh, m.head_mesh, m.skin_set).is_err() {
            return refuse(s, illegal_access());
        }
        // 5: the bank PIN (VR-083)
        let pin = match validate_bank_pin(&m.bank_pin) {
            Ok(p) => p,
            Err(e) => return refuse(s, lobby_error(code::ILLEGAL_ACCESS, e.tid())),
        };
        // 6: the name rules (VR-070, KQ-23)
        let settings = self.core.settings();
        let name = match validate_character_name(
            &m.name,
            &self.data.name_rules,
            settings.min_name_letters,
            settings.max_name_letters,
        ) {
            Ok(n) => n.to_owned(),
            Err(e) => return refuse(s, lobby_error(code::INVALID_NAME_CHARACTER, e.tid())),
        };
        // 7: the game thread decides (names and serials are its own, R1)
        let (reply, rx) = reply_pair();
        let audit_name = name.clone();
        let req = CreateCharacterRequest {
            slot,
            name,
            sex: u8::try_from(m.sex).unwrap_or(u8::MAX),
            hair_mesh: u8::try_from(m.hair_mesh).unwrap_or(u8::MAX),
            head_mesh: u8::try_from(m.head_mesh).unwrap_or(u8::MAX),
            skin_set: u8::try_from(m.skin_set).unwrap_or(u8::MAX),
            bank_pin: pin,
        };
        let input = GameInput::CreateCharacter {
            account,
            req: Box::new(req),
            reply,
        };
        if self.core.game_sender().send_async(input).await.is_err() {
            return refuse(s, lobby_error(code::UNAVAILABLE, tid(TID_DIAG_0053)));
        }
        match tokio::time::timeout(GAME_REPLY_TIMEOUT, rx.recv_async()).await {
            Ok(Ok(Ok(id))) => {
                self.core.stats.char_create("ok");
                self.core.audit.emit(
                    "char.create",
                    &Actor {
                        account: Some(account),
                        character: Some(id),
                        name: Some(audit_name.clone()),
                        ip: None,
                    },
                    serde_json::json!({
                        "character": id.0,
                        "account": account.0,
                        "slot": slot,
                        "name": audit_name,
                    }),
                );
                self.push_list(s, account).await;
                LobbyOutcome::Stay
            }
            Ok(Ok(Err(e))) => refuse(s, char_op_error(e, false)),
            // the op may still commit and then shows up in the next list (12 §9.5 step 6)
            Ok(Err(_)) | Err(_) => {
                tracing::warn!(
                    account = account.0,
                    "the game thread did not answer a character creation in time"
                );
                refuse(
                    s,
                    lobby_error(code::CERT_GENERAL, tid(TID_ERROR_CERT_GENERAL)),
                )
            }
        }
    }
}
