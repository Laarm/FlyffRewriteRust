//! Constructors of the economy audit events (Plan/13 §25.2, Plan/19 §21): `trade.complete`, `vendor.sale`,
//! `mail.*`, `grant.*` and `removal.apply`.
//!
//! The events are emitted from the **same commit block** as the mutation they describe (`AuditLog::emit`,
//! non-blocking). These functions fix the `data` shape of each event so that the trade, vendor, mail and grant
//! code of different owners write the same records; items are the `ItemRecord` JSON without `obj_id`
//! ([`AuditEvent::item`]).

use super::{AuditActor, AuditEvent, ev};
use crate::model::{ItemRecord, MailRecord, PendingGrantRecord, PendingRemovalRecord};
use flyff_common::{CharacterId, MailId};
use serde_json::{Value, json};

fn items(list: &[ItemRecord]) -> Value {
    Value::Array(list.iter().map(AuditEvent::item).collect())
}

fn mail_item(mail: &MailRecord) -> Value {
    mail.item
        .as_ref()
        .map_or(Value::Null, |stored| AuditEvent::item(&stored.item))
}

/// `trade.complete` from the point of view of `actor` (one event per participant): the partner, what `actor` gave
/// and got, and the penya both ways.
pub fn trade_complete(
    actor: AuditActor,
    with_chr: CharacterId,
    with_name: &str,
    gave: &[ItemRecord],
    got: &[ItemRecord],
    gave_gold: i64,
    got_gold: i64,
) -> AuditEvent {
    AuditEvent::new(
        ev::TRADE_COMPLETE,
        json!({
            "with_chr": with_chr.0,
            "with_name": with_name,
            "gave": items(gave),
            "got": items(got),
            "gave_gold": gave_gold,
            "got_gold": got_gold,
        }),
    )
    .by(actor)
}

/// `vendor.sale`: `buyer` bought `count` pieces of `item` from `seller` for `price` penya in total.
pub fn vendor_sale(
    seller: CharacterId,
    buyer: CharacterId,
    item: &ItemRecord,
    count: u16,
    price: i64,
) -> AuditEvent {
    AuditEvent::new(
        ev::VENDOR_SALE,
        json!({
            "seller": seller.0,
            "buyer": buyer.0,
            "item": AuditEvent::item(item),
            "count": count,
            "price": price,
        }),
    )
}

/// `mail.send`: a player mail left the sender (`postage` is the fee the sender paid).
pub fn mail_send(mail: &MailRecord, postage: i64) -> AuditEvent {
    AuditEvent::new(
        ev::MAIL_SEND,
        json!({
            "mail_id": mail.id.0,
            "receiver": mail.receiver_id.0,
            "sender": mail.sender_id.0,
            "item": mail_item(mail),
            "gold": mail.gold,
            "postage": postage,
        }),
    )
}

/// `mail.take_item`: the receiver took the attachment and paid `fee` for the storage.
pub fn mail_take_item(mail: &MailRecord, fee: i64) -> AuditEvent {
    AuditEvent::new(
        ev::MAIL_TAKE_ITEM,
        json!({
            "mail_id": mail.id.0,
            "receiver": mail.receiver_id.0,
            "sender": mail.sender_id.0,
            "item": mail_item(mail),
            "fee": fee,
        }),
    )
}

/// `mail.take_gold`: the receiver took `gold` penya.
pub fn mail_take_gold(mail_id: MailId, receiver: CharacterId, gold: i64) -> AuditEvent {
    AuditEvent::new(
        ev::MAIL_TAKE_GOLD,
        json!({"mail_id": mail_id.0, "receiver": receiver.0, "gold": gold}),
    )
}

/// `mail.delete`: the receiver deleted the mail (an attachment still on it is lost, KEEP).
pub fn mail_delete(mail: &MailRecord) -> AuditEvent {
    AuditEvent::new(
        ev::MAIL_DELETE,
        json!({
            "mail_id": mail.id.0,
            "receiver": mail.receiver_id.0,
            "sender": mail.sender_id.0,
            "item": mail_item(mail),
            "gold": mail.gold,
        }),
    )
}

/// `mail.expire`: the mail was older than 15 days (attachment and gold lost, KEEP).
pub fn mail_expire(mail: &MailRecord) -> AuditEvent {
    AuditEvent::new(
        ev::MAIL_EXPIRE,
        json!({
            "mail_id": mail.id.0,
            "receiver": mail.receiver_id.0,
            "sender": mail.sender_id.0,
            "item": mail_item(mail),
            "gold": mail.gold,
        }),
    )
}

fn grant_data(grant: &PendingGrantRecord) -> Value {
    json!({
        "id": grant.id,
        "receiver": grant.character_id.0,
        "kind": grant.kind.as_str(),
        "item": grant.item.as_ref().map_or(Value::Null, |s| AuditEvent::item(&s.item)),
        "gold": grant.gold,
        "source": grant.source,
    })
}

/// `grant.deliver`: a pending grant reached its receiver.
pub fn grant_deliver(grant: &PendingGrantRecord) -> AuditEvent {
    AuditEvent::new(ev::GRANT_DELIVER, grant_data(grant))
}

/// `grant.skip_expired`: the grant was closed without delivery (its `keep_until` had passed, or its cards / gems
/// were invalid).
pub fn grant_skip_expired(grant: &PendingGrantRecord, reason: &str) -> AuditEvent {
    let mut data = grant_data(grant);
    if let Value::Object(map) = &mut data {
        map.insert("reason".to_owned(), Value::from(reason));
    }
    AuditEvent::new(ev::GRANT_SKIP_EXPIRED, data)
}

/// `removal.apply`: a pending removal took `removed_count` pieces (or penya) away.
pub fn removal_apply(removal: &PendingRemovalRecord, removed_count: u32) -> AuditEvent {
    AuditEvent::new(
        ev::REMOVAL_APPLY,
        json!({
            "id": removal.id,
            "character": removal.character_id.map(|c| c.0),
            "guild": removal.guild_id.map(|g| g.0),
            "location": removal.location.as_str(),
            "item_id": removal.item_id,
            "serial": removal.serial,
            "requested": removal.count,
            "removed_count": removed_count,
            "requested_by": removal.requested_by,
        }),
    )
}
