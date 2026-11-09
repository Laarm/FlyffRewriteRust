//! Communication queries of the console (Plan/25 §8.2; **WP-ADMIN-27**): the history of the system mails.
//!
//! The mail service holds every undeleted mail in memory (19 §11.1), so the history is a walk over the newest ids:
//! the mails whose sender is the system (id 0), with the receiver, the subject, what is still attached and whether
//! the receiver read it. Mails the receiver deleted, mails past their 15 days and the attachments that were taken
//! are gone from memory (a taken attachment leaves the mail empty). Read-only, answered in the same tick.

use super::AdminQueryResult;
use crate::tick::Game;
use flyff_common::CharacterId;

/// The most rows one answer carries.
pub const SYSTEM_MAILS_MAX: usize = 500;

/// One system mail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemMailRow {
    /// `mails.id`.
    pub id: u32,
    /// The receiver.
    pub receiver: u32,
    /// The receiver's name (`#<id>` when the directory does not know it).
    pub receiver_name: String,
    /// The subject.
    pub title: String,
    /// The text.
    pub text: String,
    /// The penya still attached.
    pub gold: i64,
    /// The item still attached (`0` = none).
    pub item_id: u32,
    /// How many pieces of it.
    pub item_count: u32,
    /// When it was sent, Unix seconds.
    pub created_at: i64,
    /// The receiver read it.
    pub read: bool,
    /// The receiver is in the world now.
    pub online: bool,
}

/// `SystemMails`: the newest system mails, at most `limit` ([`SYSTEM_MAILS_MAX`]), newest first; with `receiver`
/// only those of that character.
pub(super) fn system_mails(
    g: &Game,
    limit: usize,
    receiver: Option<CharacterId>,
) -> AdminQueryResult {
    let mail = &g.services.econ.mail;
    let limit = limit.clamp(1, SYSTEM_MAILS_MAX);
    let row = |e: &crate::econ::mail::MailEntry| {
        let dir = g.services.presence.get(e.receiver);
        SystemMailRow {
            id: e.id.0,
            receiver: e.receiver.0,
            receiver_name: dir.map_or_else(|| format!("#{}", e.receiver.0), |d| d.name.clone()),
            title: e.title.clone(),
            text: e.body.clone(),
            gold: e.gold,
            item_id: e.item.as_ref().map_or(0, |i| i.item_id),
            item_count: e.item.as_ref().map_or(0, |i| u32::from(i.count)),
            created_at: e.created_at.0,
            read: e.read,
            online: dir.is_some_and(|d| d.online.is_some()),
        }
    };
    let rows: Vec<SystemMailRow> = match receiver {
        Some(r) => {
            let mut v: Vec<SystemMailRow> =
                mail.box_of(r).filter(|e| e.is_system()).map(row).collect();
            v.reverse();
            v.truncate(limit);
            v
        }
        None => mail
            .newest()
            .filter(|e| e.is_system())
            .take(limit)
            .map(row)
            .collect(),
    };
    AdminQueryResult::SystemMails(rows)
}
