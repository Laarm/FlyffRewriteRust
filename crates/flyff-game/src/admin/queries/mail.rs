//! The mailbox query (Plan/25 §4.4, §11.5): `Mailbox` (WP-ADMIN-16).
//!
//! The mail service keeps every undeleted mail of the server in memory (`MailService`, 19 §11.1), online or not, so
//! the answer is the same for an offline character. The query is read-only: it does not mark a mail as read and
//! does not touch the envelope.

use super::AdminQueryResult;
use crate::tick::Game;
use flyff_common::CharacterId;
use flyff_proto::pb;

/// One mail of a mailbox as the console shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct MailView {
    /// `mails.id`.
    pub id: u32,
    /// The sender's character id; `0` = the system.
    pub sender_id: u32,
    /// The sender's name (`System` for the system, `#<id>` for a purged sender).
    pub sender_name: String,
    /// The title.
    pub title: String,
    /// The text.
    pub body: String,
    /// The attached penya (0 once taken).
    pub gold: i64,
    /// The attached item (`obj_id` 0), `None` once taken.
    pub item: Option<pb::ItemElem>,
    /// Unix seconds the mail was sent.
    pub created_at: i64,
    /// Unix seconds the mail is deleted (15 days after sending, `MAX_KEEP_MAX_DAY`).
    pub expires_at: i64,
    /// The receiver read it.
    pub read: bool,
}

/// `Mailbox`: the box of `character` in id order, at most `MAX_MAIL` (50) entries as the game enforces it.
pub(super) fn mailbox(g: &Game, character: CharacterId) -> AdminQueryResult {
    if g.services.presence.get(character).is_none()
        && super::super::watch::online_avatar(g, character, None).is_none()
    {
        return AdminQueryResult::not_found(format!("character {character} does not exist"));
    }
    let keep = crate::econ::mail::MAX_KEEP_MAX_DAY * 86_400;
    let rows = g
        .services
        .econ
        .mail
        .box_of(character)
        .map(|e| {
            let sender_name = if e.is_system() {
                "System".to_owned()
            } else {
                g.services
                    .presence
                    .get(e.sender)
                    .map_or_else(|| format!("#{}", e.sender.0), |d| d.name.clone())
            };
            MailView {
                id: e.id.0,
                sender_id: e.sender.0,
                sender_name,
                title: e.title.clone(),
                body: e.body.clone(),
                gold: e.gold,
                item: e.item.as_ref().map(|i| i.to_pb(0)),
                created_at: e.created_at.0,
                expires_at: e.created_at.0.saturating_add(keep),
                read: e.read,
            }
        })
        .collect();
    AdminQueryResult::Mailbox(rows)
}
