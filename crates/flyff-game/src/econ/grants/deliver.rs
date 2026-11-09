//! `econ::grants::deliver`: the delivery pass of the pending grants (Plan/19 §20.4, 13 §18; WP-ECON-09).
//!
//! One pass serves one receiver and plans on COPIES before anything is mutated:
//!
//! 1. an id the guard refuses (cancelling, cancelled a moment ago, in flight, recent) is skipped silently;
//! 2. an item whose `keep_until` has passed is skipped and CLOSED (`grant.skip_expired {reason: "expired"}`), and so
//!    is an unknown item, a count outside `1..=pack_max` or invalid cards / gems / vis (`reason: "invalid"`);
//! 3. penya is added up to [`GOLD_MAX`] (the excess is lost and logged, as the original clamp);
//! 4. an item is dry-run on a clone of the bag in id order; when it does not fit it STAYS OPEN (nothing is marked
//!    or sent; the next poll and the next login try again).
//!
//! The commit block is panic-free: [`deliver_once`](crate::persist::grants::GrantGuard::deliver_once) for every id
//! that goes into the batch, `item::ops::create_item` with a fresh serial (the `CreateItem` message and `item.create`),
//! `add_gold`, the audit lines (`grant.deliver {id, item|gold, source, note}`, `grant.skip_expired`), and ONE
//! `AtomicBatch::grant_delivery(snapshot, ids)` through `submit`: `[snap(receiver)] + MarkGrantsDelivered{ids}`.
//! A receiver that cannot be snapshotted (no live untainted session) is left entirely alone (VR-018).

use crate::admin::actions::grants::validate::{ValidateEnv, fields_of};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch, submit};
use crate::item::container::{AddError, SeqSerials};
use crate::item::env::ItemCore;
use crate::item::gold::{GOLD_MAX, add_gold};
use crate::item::model::{Item, MAX_PIERCING_ULTIMATE, MAX_VIS};
use crate::item::ops::{ItemOrigin, create_item};
use crate::social::relation::avatar_of;
use crate::tickets::TicketResult;
use flyff_common::CharacterId;
use flyff_common::UnixTime;
use flyff_common::defs::itemkind::IK3_VIRTUAL;
use flyff_db::audit::econ as audit;
use flyff_db::{AtomicBatch, AuditEvent, GrantKind, ItemRecord, PendingGrantRecord};
use flyff_rules::item::refine::UpgradeTables;
use serde_json::Value;
use std::cell::OnceCell;
use std::collections::BTreeMap;

/// What a pass decided for one grant.
#[derive(Debug)]
enum Fate {
    /// An item that fits: the template (serial 0) to create.
    Item(Box<Item>),
    /// Penya: how much the wallet takes and how much is lost to the clamp.
    Gold { applied: i64, lost: i64 },
    /// Closed without delivery, with the reason of the audit line.
    Skip { reason: &'static str, why: String },
}

/// One grant and its fate.
#[derive(Debug)]
struct Planned {
    grant: PendingGrantRecord,
    fate: Fate,
}

/// The `DeliverFn` of the poll and of the enter-world pass: groups `grants` by receiver (ids ascending) and runs one
/// pass per receiver. See the module documentation.
pub fn deliver(svc: &mut Services, ctx: &mut GameCtx<'_>, grants: Vec<PendingGrantRecord>) {
    let mut by_receiver: BTreeMap<CharacterId, Vec<PendingGrantRecord>> = BTreeMap::new();
    for g in grants.into_iter().filter(PendingGrantRecord::is_open) {
        by_receiver.entry(g.character_id).or_default().push(g);
    }
    for (receiver, mut list) in by_receiver {
        list.sort_by_key(|g| g.id);
        list.dedup_by_key(|g| g.id);
        if let Some(who) = avatar_of(svc, ctx, receiver)
            && can_snapshot(ctx, who)
        {
            pass(svc, ctx, who, list);
        }
    }
}

/// One receiver, one pass.
fn pass(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, list: Vec<PendingGrantRecord>) {
    let planned = plan(svc, ctx, who, list);
    if planned.is_empty() {
        return;
    }
    let now = ctx.clock.unix_ms.to_unix_time();
    let mut ids: Vec<i64> = Vec::with_capacity(planned.len());
    for Planned { grant, fate } in planned {
        if !svc.grants.deliver_once(grant.id) {
            continue; // planned from a guard that allowed it a moment ago: cannot happen
        }
        match fate {
            Fate::Skip { reason, why } => {
                tracing::warn!(grant = grant.id, reason, %why, "a pending grant is closed without delivery");
                emit(ctx, who, audit::grant_skip_expired(&grant, reason), &grant);
                ids.push(grant.id);
            }
            Fate::Gold { applied, lost } => {
                if applied > 0 {
                    add_gold(svc, ctx, who, applied, true);
                }
                if lost > 0 {
                    tracing::warn!(
                        grant = grant.id,
                        lost,
                        "a gold grant passes the wallet limit: the excess is lost"
                    );
                }
                emit(ctx, who, audit::grant_deliver(&grant), &grant);
                ids.push(grant.id);
            }
            Fate::Item(item) => match create_item(svc, ctx, who, *item, ItemOrigin::Grant) {
                Ok(_) => {
                    emit(ctx, who, audit::grant_deliver(&grant), &grant);
                    ids.push(grant.id);
                }
                Err(e) => {
                    // the dry run said it fits: a failure here is a bug; the grant stays open
                    tracing::error!(grant = grant.id, error = ?e, "a pending grant could not be created after a successful dry run");
                    svc.grants.forget(grant.id);
                }
            },
        }
    }
    if ids.is_empty() {
        return;
    }
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        // `can_snapshot` held a moment ago: the ids stay in flight (a retry would deliver twice)
        tracing::error!(
            ?ids,
            "pending grants were delivered in memory but no snapshot could be taken: not marked"
        );
        return;
    };
    let batch = AtomicBatch::grant_delivery(snap, ids.clone(), now);
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        batch,
        move |svc, ctx, result| match result {
            TicketResult::Committed(_) => svc.grants.resolve(&ids, ctx.clock.game_ms),
            TicketResult::Failed(e) => {
                tracing::error!(?ids, error = %e, "CRITICAL: the GrantDelivery batch failed; the ids stay in flight for the rest of the run (a retry would duplicate the items)");
            }
            other => {
                tracing::error!(
                    ?ids,
                    ?other,
                    "CRITICAL: the GrantDelivery batch completed with an unexpected result"
                );
            }
        },
    );
}

/// The decisions of a pass, on copies: nothing of the player changes here.
fn plan(
    svc: &Services,
    ctx: &GameCtx<'_>,
    who: PlayerRef,
    list: Vec<PendingGrantRecord>,
) -> Vec<Planned> {
    let core = svc.items.handle();
    let now = ctx.clock.unix_ms.to_unix_time();
    let Some(p) = ctx.player(who).and_then(|m| m.player()) else {
        return Vec::new();
    };
    let mut probe = p.items.inventory.clone();
    let mut wallet = i64::from(p.items.gold);
    let mut serials = SeqSerials(u64::MAX / 2);
    let tables: OnceCell<UpgradeTables> = OnceCell::new();
    let mut out = Vec::with_capacity(list.len());
    for g in list {
        if svc.grants.refuses(g.id) {
            continue;
        }
        let fate = match g.kind {
            GrantKind::Gold => {
                if g.gold <= 0 {
                    Fate::Skip {
                        reason: "invalid",
                        why: format!("a gold grant of {}", g.gold),
                    }
                } else {
                    let room = (GOLD_MAX - wallet).max(0);
                    let applied = g.gold.min(room);
                    wallet += applied;
                    Fate::Gold {
                        applied,
                        lost: g.gold - applied,
                    }
                }
            }
            GrantKind::Item => {
                let Some(stored) = g.item.as_ref() else {
                    out.push(Planned {
                        fate: Fate::Skip {
                            reason: "invalid",
                            why: "an item grant without an item".to_owned(),
                        },
                        grant: g,
                    });
                    continue;
                };
                let rec = &stored.item;
                if rec.keep_until != 0 && rec.keep_until <= now.0 {
                    Fate::Skip {
                        reason: "expired",
                        why: format!("keep_until {} has passed", rec.keep_until),
                    }
                } else if let Err(why) = check_record(&core, &tables, now, rec) {
                    Fate::Skip {
                        reason: "invalid",
                        why,
                    }
                } else {
                    let mut item = Item::from_record(rec);
                    item.serial = flyff_common::ItemSerial(0);
                    match probe.add(&item, &core.data, &mut serials) {
                        Ok(_) => Fate::Item(Box::new(item)),
                        Err(AddError::Full) => continue, // no room: the grant stays open
                        Err(e) => Fate::Skip {
                            reason: "invalid",
                            why: format!("{e}"),
                        },
                    }
                }
            }
        };
        out.push(Planned { grant: g, fate });
    }
    out
}

/// Is the stored template an item the game can create? An unknown item, a count outside `1..=pack_max`, cards that
/// are not `IK3_SOCKETCARD` / `IK3_SOCKETCARD2` of the item's kind, vis that are not `IK3_VIS`, gems that are not
/// ultimate ability items (the rules of the admin builder, `grants::validate`).
fn check_record(
    core: &ItemCore,
    tables: &OnceCell<UpgradeTables>,
    now: UnixTime,
    rec: &ItemRecord,
) -> Result<(), String> {
    let data = &core.data;
    let prop = data
        .item(rec.item_id)
        .ok_or_else(|| format!("unknown item {}", rec.item_id))?;
    if prop.item_kind3 == IK3_VIRTUAL {
        return Err(format!("item {} is virtual", rec.item_id));
    }
    let pack_max = prop.pack_max.max(1);
    if rec.count == 0 || u32::from(rec.count) > pack_max {
        return Err(format!(
            "count {} is outside 1..={pack_max} for item {}",
            rec.count, rec.item_id
        ));
    }
    if rec.piercing_cards.is_empty() && rec.ultimate_gems.is_empty() {
        return Ok(());
    }
    let tables = tables.get_or_init(|| {
        UpgradeTables::from_data(
            data.lua.item_upgrade.as_ref(),
            &data.accessory,
            &data.collecting,
        )
    });
    let env = ValidateEnv {
        data,
        cfg: &core.cfg,
        tables,
        now,
    };
    let fields = fields_of(&env, prop);
    if fields.vis {
        if rec.piercing_cards.len() > MAX_VIS {
            return Err(format!("{} vis slots", rec.piercing_cards.len()));
        }
        for &v in &rec.piercing_cards {
            if v != 0 && data.item(v).is_none_or(|p| !p.is_vis()) {
                return Err(format!("item {v} is not a vis"));
            }
        }
    } else if !rec.piercing_cards.is_empty() {
        let Some((max, kind)) = fields.piercing else {
            return Err(format!("item {} has no card sockets", rec.item_id));
        };
        if rec.piercing_cards.len() > usize::from(max) {
            return Err(format!(
                "{} sockets, at most {max}",
                rec.piercing_cards.len()
            ));
        }
        for &card in &rec.piercing_cards {
            if card != 0 && data.item(card).is_none_or(|c| c.item_kind3 != kind) {
                return Err(format!("item {card} is not a socket card of this item"));
            }
        }
    }
    if !rec.ultimate_gems.is_empty() {
        if !fields.ultimate {
            return Err(format!("item {} is not an ultimate weapon", rec.item_id));
        }
        if rec.ultimate_gems.len() > MAX_PIERCING_ULTIMATE {
            return Err(format!("{} gem sockets", rec.ultimate_gems.len()));
        }
        let ability = |id: u32| {
            data.ultimate
                .gem_ability
                .iter()
                .any(|d| d.rows.iter().any(|r| r.gem06 == id))
        };
        for &gem in &rec.ultimate_gems {
            if gem != 0 && !ability(gem) {
                return Err(format!("item {gem} is not an ultimate gem ability"));
            }
        }
    }
    Ok(())
}

/// Emits `event` with the actor context of `who` and the operator's note of the grant.
fn emit(ctx: &GameCtx<'_>, who: PlayerRef, mut event: AuditEvent, grant: &PendingGrantRecord) {
    if let Value::Object(map) = &mut event.data {
        map.insert("note".to_owned(), Value::from(grant.note.as_str()));
    }
    if let Some(actor) = crate::audit::actor_of(ctx, who) {
        event = event.by(actor);
    }
    ctx.audit.emit(event);
}
