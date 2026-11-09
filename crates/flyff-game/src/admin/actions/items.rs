//! Item and penya removal of the console (Plan/25 §4.5, §11.5): `RemoveItem`, `AdjustGold`, `PetAction`
//! (**WP-ADMIN-16**).
//!
//! All three act on an **online** character (an offline one is the pending-removal / pending-grant rows that the
//! HTTP layer inserts, 25 §4.5). Banks are account-scoped (ADM-D8): the target of a bank operation is *any online
//! character of the account* (the console asks `OnlineSiblings` first) and `Bank{slot}` names the compartment of
//! the character in that slot of the account. Every body validates first and changes nothing when a check fails:
//!
//! * the character is in the world and does not outrank the actor ([`check_rank`]);
//! * the cell holds exactly the serial the console saw (`StaleTarget`), the stack is large enough, the item is not
//!   reserved by a trade or a vendor window and not in use (a summoned pet, an active ticket or feed pocket);
//! * a penya change never leaves a trade window with less than it reserved.
//!
//! The change is applied through the item services (`item.destroy` audit line `admin_remove`, the owner's
//! `UpdateItem` / `PocketRemoveItem` / `UpdateBankItem` / `SetPointParam` / `PutGoldBank` messages), then one
//! `AdminItemRemoval` / `AdminGold` atomic batch carries a fresh snapshot of the target (25 §11.6). The answer is an
//! [`AdminOutput::Report`] with one entry whose `info` says what was removed or changed (the HTTP layer audits it).

use super::common::{check_rank, fail, failed, report};
use super::{TargetId, TargetResult};
use crate::admin::types::{AdminErrorCode, AdminFailure, ContainerLoc};
use crate::buff::core::BuffCx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::TradePartner;
use crate::econ::mail::commit::{snapshot_for_batch, submit};
use crate::input::{AdminActor, AdminOutput};
use crate::item::container::StorageId;
use crate::item::fx::{Fx, item_json};
use crate::item::gold::GOLD_MAX;
use crate::item::model::Item;
use crate::item::ops::{RemoveReason, core as ops};
use crate::item::pets::system::process::send_state;
use crate::mover::Mover;
use crate::tickets::TicketResult;
use flyff_common::CharacterId;
use flyff_db::audit::ev;
use flyff_db::{AtomicBatch, AtomicKind};
use flyff_proto::pb;
use flyff_rules::item::buffitem::pocket_available;
use serde_json::{Value, json};

/// The answer to an item or penya edit of a guild bank: ONE path, `GuildEdit` (Plan/25 §6.3). These two bodies run
/// at once against an online avatar while the guild bodies are deferred to their batch, so they must not delegate.
pub(crate) const GUILD_BANK_ROUTE: &str =
    "the guild bank is edited with GuildEdit (PATCH /admin/api/guilds/{id}: bank_remove, gold)";

/// Take an item (or some of a stack) out of a container (`RemoveItem`). The serial must match: a mismatch is
/// `StaleTarget`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoveItem {
    /// The online character whose container it is (for a bank: any online character of the account).
    pub character: CharacterId,
    /// Which container.
    pub container: ContainerLoc,
    /// The cell: the **display slot** of the snapshot the console looked at (`ItemElem.obj_id` of an admin
    /// `ContainerSnapshot`; for the equipment the part).
    pub slot: u32,
    /// The serial the console saw there.
    pub serial: u64,
    /// How many of the stack.
    pub count: u32,
}

/// Add or remove penya in a container (`AdjustGold`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdjustGold {
    /// The online character (for a bank: any online character of the account).
    pub character: CharacterId,
    /// Which container (inventory wallet or a bank compartment; the guild bank is edited with `GuildEdit`).
    pub container: ContainerLoc,
    /// The change; negative removes. The result is clamped to `0..=GOLD_MAX` (2,147,483,647, 19 E12).
    pub delta: i64,
}

/// What a pet action does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PetAdminAction {
    /// Take the summoned pet back into its egg (`PetRelease`).
    Release,
    /// Revive a dead pet (the inventory item is flagged expired): the flag is cleared, its life is 0 and its energy
    /// half of the maximum, as the resurrection fragment does (19 §13.4).
    Revive {
        /// The inventory cell of the pet item (display slot).
        slot: u32,
        /// The serial the console saw there.
        serial: u64,
    },
}

/// Act on a character's pet (`PetAction`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PetAction {
    /// The character.
    pub character: CharacterId,
    /// What to do.
    pub action: PetAdminAction,
}

// ---- shared ----------------------------------------------------------------------------------------------------------

/// The avatar of an online character.
fn avatar(ctx: &GameCtx<'_>, c: CharacterId) -> Result<PlayerRef, AdminFailure> {
    ctx.sessions
        .iter()
        .find(|e| e.character == c && ctx.worlds.player(e.player).is_some())
        .map(|e| PlayerRef(e.player))
        .ok_or_else(|| {
            fail(
                AdminErrorCode::CharacterOnline,
                format!("character {} is not in the world", c.0),
            )
        })
}

/// The cell a `RemoveItem` / `Revive` addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// The bag (inventory region).
    Bag,
    /// A worn item.
    Worn,
    /// Pocket `0..3`.
    Pocket(usize),
    /// Bank compartment `0..3`.
    Bank(usize),
}

struct Located {
    place: Place,
    id: StorageId,
    item: Item,
}

fn slot_u16(slot: u32) -> Result<u16, AdminFailure> {
    u16::try_from(slot).map_err(|_| {
        fail(
            AdminErrorCode::Invalid,
            format!("slot {slot} is out of range"),
        )
    })
}

fn locate(
    m: &Mover,
    cfg_perm_extrabags: bool,
    loc: ContainerLoc,
    slot: u32,
) -> Result<Located, AdminFailure> {
    let p = m
        .player()
        .ok_or_else(|| fail(AdminErrorCode::NotFound, "not a player"))?;
    let slot = slot_u16(slot)?;
    let stale = || {
        fail(
            AdminErrorCode::StaleTarget,
            format!("slot {slot} of {loc:?} holds no item"),
        )
    };
    let (place, container, display) = match loc {
        ContainerLoc::Inventory => {
            let c = &p.items.inventory;
            if slot >= c.index_num() {
                return Err(fail(
                    AdminErrorCode::Invalid,
                    format!("slot {slot} is outside the bag ({} slots)", c.index_num()),
                ));
            }
            (Place::Bag, c, slot)
        }
        ContainerLoc::Equipment => {
            let c = &p.items.inventory;
            let parts = c.item_max().saturating_sub(c.index_num());
            if slot >= parts {
                return Err(fail(
                    AdminErrorCode::Invalid,
                    format!("part {slot} does not exist"),
                ));
            }
            (Place::Worn, c, c.index_num() + slot)
        }
        ContainerLoc::Pocket(i) => {
            let pk = p.items.pockets.get(usize::from(i)).ok_or_else(|| {
                fail(
                    AdminErrorCode::Invalid,
                    format!("pocket {i} does not exist"),
                )
            })?;
            if !pocket_available(cfg_perm_extrabags, usize::from(i), pk.available, pk.expired) {
                return Err(fail(
                    AdminErrorCode::Invalid,
                    format!("pocket {i} is not available"),
                ));
            }
            (Place::Pocket(usize::from(i)), &pk.items, slot)
        }
        ContainerLoc::Bank { slot: b } => {
            let bank = p.items.bank(usize::from(b)).ok_or_else(|| {
                fail(
                    AdminErrorCode::NotFound,
                    format!("the account has no bank {b}"),
                )
            })?;
            (Place::Bank(usize::from(b)), &bank.items, slot)
        }
        ContainerLoc::GuildBank(_) => {
            return Err(AdminFailure::new(AdminErrorCode::Invalid, GUILD_BANK_ROUTE));
        }
        ContainerLoc::Mailbox => {
            return Err(fail(
                AdminErrorCode::Invalid,
                "mail attachments are not removed here",
            ));
        }
    };
    let id = container.obj_at_slot(display).ok_or_else(stale)?;
    let item = container.get_at_id(id).ok_or_else(stale)?.clone();
    Ok(Located { place, id, item })
}

/// Queues the `AdminItemRemoval` / `AdminGold` / pet batch with a fresh snapshot of the target; the result is only
/// logged (the game never compensates a failed batch, 19 §20.3).
fn commit(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    what: &'static str,
    make: impl FnOnce(flyff_db::CharacterSnapshot) -> AtomicBatch,
) {
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        tracing::warn!(
            what,
            "admin: no snapshot of the target (tainted or gone): the change is saved by the next save"
        );
        return;
    };
    let character = ctx
        .worlds
        .player(who.0)
        .and_then(Mover::player)
        .map(|p| p.ident.character_id.0)
        .unwrap_or(0);
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        make(snap),
        move |_svc, _ctx, result| {
            if let TicketResult::Failed(e) = result {
                tracing::error!(what, character, error = %e, "admin: the batch failed");
            }
        },
    );
}

fn done(character: CharacterId, info: Value) -> AdminOutput {
    report(vec![
        TargetResult::ok(TargetId::Character(character)).with_info(info),
    ])
}

// ---- RemoveItem --------------------------------------------------------------------------------------------------------

/// `RemoveItem`: see the module documentation. Answers one [`TargetResult`] with
/// `info = {item: <the item as stored>, removed, left, container, slot}`.
pub(super) fn remove_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: RemoveItem,
) -> AdminOutput {
    let who = match avatar(ctx, cmd.character) {
        Ok(w) => w,
        Err(f) => return AdminOutput::Failed(f),
    };
    if let Err(f) = check_rank(&actor, ctx, who.0) {
        return AdminOutput::Failed(f);
    }
    if cmd.count == 0 {
        return failed(AdminErrorCode::Invalid, "count must be at least 1");
    }
    let perm = svc.items.handle().cfg.perm_extrabags;
    let Some(m) = ctx.worlds.player(who.0) else {
        return failed(AdminErrorCode::NotFound, "the avatar is gone");
    };
    let found = match locate(m, perm, cmd.container, cmd.slot) {
        Ok(f) => f,
        Err(f) => return AdminOutput::Failed(f),
    };
    if found.item.serial.0 != cmd.serial {
        return failed(
            AdminErrorCode::StaleTarget,
            format!(
                "slot {} of {:?} holds serial {}, not {}",
                cmd.slot, cmd.container, found.item.serial.0, cmd.serial
            ),
        );
    }
    let have = u32::from(found.item.count);
    if cmd.count > have {
        return failed(
            AdminErrorCode::StaleTarget,
            format!("the stack holds {have}, not {}", cmd.count),
        );
    }
    if !found.item.is_usable() {
        return failed(
            AdminErrorCode::Invalid,
            "the item is reserved by a trade or a personal shop",
        );
    }
    if matches!(found.place, Place::Bag | Place::Worn)
        && m.player().is_some_and(|p| p.items.is_using(found.id))
    {
        return failed(
            AdminErrorCode::Invalid,
            "the item is in use (a summoned pet, an active ticket or feed pocket): release it first",
        );
    }
    let item_json_before = item_json(&found.item);
    let removed = cmd.count;
    // apply
    match found.place {
        Place::Bag | Place::Worn => {
            crate::item::ops::remove_item(
                svc,
                ctx,
                who,
                found.id,
                u16::try_from(removed).unwrap_or(u16::MAX),
                RemoveReason::Other("admin_remove"),
            );
        }
        Place::Pocket(i) => {
            let mut fx = Fx::new();
            if let Some(pk) = ctx
                .worlds
                .player_mut(who.0)
                .and_then(Mover::player_mut)
                .and_then(|p| p.items.pockets.get_mut(i))
            {
                reduce(&mut pk.items, found.id, removed);
            }
            fx.out.to_self(pb::PocketRemoveItem {
                pocket: i as i32,
                obj_id: i32::from(found.id),
                num: i32::try_from(removed).unwrap_or(i32::MAX),
            });
            fx.audit(
                ev::ITEM_DESTROY,
                json!({"item": item_json_before, "count": removed, "reason": "admin_remove", "container": format!("pocket{i}")}),
            );
            fx.flush(svc, ctx, who);
        }
        Place::Bank(i) => {
            let mut fx = Fx::new();
            let rest = have - removed;
            let mut owner = 0;
            if let Some(b) = ctx
                .worlds
                .player_mut(who.0)
                .and_then(Mover::player_mut)
                .and_then(|p| p.items.banks.get_mut(i))
                .and_then(Option::as_mut)
            {
                owner = b.character_id.0;
                reduce(&mut b.items, found.id, removed);
            }
            fx.out.to_self(pb::UpdateBankItem {
                slot: i as u32,
                obj_id: u32::from(found.id),
                param: 0,
                value: rest,
            });
            fx.audit(
                ev::ITEM_DESTROY,
                json!({"item": item_json_before, "count": removed, "reason": "admin_remove", "container": format!("bank{i}"), "bank_owner": owner}),
            );
            fx.flush(svc, ctx, who);
        }
    }
    commit(svc, ctx, who, "RemoveItem", AtomicBatch::admin_item_removal);
    done(
        cmd.character,
        json!({
            "item": item_json_before,
            "removed": removed,
            "left": have - removed,
            "container": cmd.container_name(),
            "slot": cmd.slot,
        }),
    )
}

impl RemoveItem {
    fn container_name(&self) -> String {
        container_name(self.container)
    }
}

/// `equipment`, `inventory`, `pocket2`, `bank1`, ... (the audit text of a container).
pub fn container_name(loc: ContainerLoc) -> String {
    match loc {
        ContainerLoc::Equipment => "equipment".to_owned(),
        ContainerLoc::Inventory => "inventory".to_owned(),
        ContainerLoc::Pocket(i) => format!("pocket{i}"),
        ContainerLoc::Bank { slot } => format!("bank{slot}"),
        ContainerLoc::GuildBank(g) => format!("guild_bank{}", g.0),
        ContainerLoc::Mailbox => "mailbox".to_owned(),
    }
}

/// Removes `n` pieces of the stack `id` of a pocket or a bank; the whole entry goes when nothing is left.
fn reduce(c: &mut crate::item::container::ItemContainer, id: StorageId, n: u32) {
    let Some(it) = c.get_at_id_mut(id) else {
        return;
    };
    let have = u32::from(it.count);
    if n >= have {
        c.remove_at_id(id);
    } else if let Some(it) = c.get_at_id_mut(id) {
        it.count = u16::try_from(have - n).unwrap_or(u16::MAX);
    }
}

// ---- AdjustGold --------------------------------------------------------------------------------------------------------

/// `AdjustGold`: the wallet (`Inventory`) or the balance of a bank compartment changes by `delta`, clamped to
/// `0..=GOLD_MAX`. Answers `info = {container, before, after, applied}` (`applied` is the real change after the
/// clamp). A negative change of the wallet is refused while the player has a trade window or a personal shop open
/// (the reserved penya must stay covered).
pub(super) fn adjust_gold(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: AdjustGold,
) -> AdminOutput {
    let who = match avatar(ctx, cmd.character) {
        Ok(w) => w,
        Err(f) => return AdminOutput::Failed(f),
    };
    if let Err(f) = check_rank(&actor, ctx, who.0) {
        return AdminOutput::Failed(f);
    }
    if cmd.delta == 0 {
        return failed(AdminErrorCode::Invalid, "the change is 0");
    }
    let Some(p) = ctx.worlds.player(who.0).and_then(Mover::player) else {
        return failed(AdminErrorCode::NotFound, "the avatar is gone");
    };
    let (before, bank_slot) = match cmd.container {
        ContainerLoc::Inventory => (i64::from(p.items.gold), None),
        ContainerLoc::Bank { slot } => match p.items.bank(usize::from(slot)) {
            Some(b) => (b.gold, Some(usize::from(slot))),
            None => {
                return failed(
                    AdminErrorCode::NotFound,
                    format!("the account has no bank {slot}"),
                );
            }
        },
        ContainerLoc::GuildBank(_) => {
            return AdminOutput::Failed(AdminFailure::new(
                AdminErrorCode::Invalid,
                GUILD_BANK_ROUTE,
            ));
        }
        ContainerLoc::Equipment | ContainerLoc::Pocket(_) | ContainerLoc::Mailbox => {
            return failed(
                AdminErrorCode::Invalid,
                "penya lives in the inventory and in the banks",
            );
        }
    };
    let after = before.saturating_add(cmd.delta).clamp(0, GOLD_MAX);
    let applied = after - before;
    if cmd.container == ContainerLoc::Inventory
        && cmd.delta < 0
        && (p.econ.vending || matches!(p.econ.partner, TradePartner::Player { .. }))
    {
        return failed(
            AdminErrorCode::Invalid,
            "the player has a trade window or a personal shop open: try again in a moment",
        );
    }
    if applied != 0 {
        match bank_slot {
            None => crate::item::gold::add_gold(svc, ctx, who, applied, true),
            Some(i) => {
                let mut fx = Fx::new();
                let mut wallet = 0;
                if let Some(pm) = ctx.worlds.player_mut(who.0).and_then(Mover::player_mut) {
                    wallet = pm.items.gold;
                    if let Some(b) = pm.items.banks.get_mut(i).and_then(Option::as_mut) {
                        b.gold = after;
                    }
                }
                fx.out.to_self(pb::PutGoldBank {
                    slot: i as u32,
                    gold: u32::try_from(wallet.max(0)).unwrap_or(0),
                    bank_gold: u32::try_from(after.max(0)).unwrap_or(u32::MAX),
                });
                fx.flush(svc, ctx, who);
            }
        }
        let name = container_name(cmd.container);
        let (event, data) = if applied < 0 {
            (
                ev::ITEM_DESTROY,
                json!({"item": null, "count": 0, "gold": -applied, "reason": "admin_gold", "container": name}),
            )
        } else {
            (
                ev::ITEM_CREATE,
                json!({"item": null, "count": 0, "gold": applied, "source": "admin_gold", "container": name}),
            )
        };
        crate::audit::emit_for(ctx, who, event, data);
        commit(svc, ctx, who, "AdjustGold", AtomicBatch::admin_gold);
    }
    done(
        cmd.character,
        json!({
            "container": container_name(cmd.container),
            "before": before,
            "after": after,
            "applied": applied,
            "requested": cmd.delta,
        }),
    )
}

// ---- PetAction ----------------------------------------------------------------------------------------------------------

/// `PetAction`: `Release` takes the summoned pet back (19 `PetRelease`), `Revive` brings a dead pet item back.
pub(super) fn pet_action(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: PetAction,
) -> AdminOutput {
    let who = match avatar(ctx, cmd.character) {
        Ok(w) => w,
        Err(f) => return AdminOutput::Failed(f),
    };
    if let Err(f) = check_rank(&actor, ctx, who.0) {
        return AdminOutput::Failed(f);
    }
    let info = match cmd.action {
        PetAdminAction::Release => {
            let summoned = ctx
                .worlds
                .player(who.0)
                .and_then(Mover::player)
                .and_then(|p| p.items.pet_obj_id);
            let Some(summoned) = summoned else {
                return failed(AdminErrorCode::StaleTarget, "no pet is summoned");
            };
            crate::item::pets::system::hooks::with_player(svc, ctx, who, |m, cx, _, fx| {
                crate::item::pets::system::release(m, cx, fx);
            });
            json!({"action": "release", "item_obj": summoned})
        }
        PetAdminAction::Revive { slot, serial } => match revive(svc, ctx, who, slot, serial) {
            Ok(info) => info,
            Err(f) => return AdminOutput::Failed(f),
        },
    };
    commit(svc, ctx, who, "PetAction", |snap| AtomicBatch {
        snapshots: vec![snap],
        ..AtomicBatch::of_ops(AtomicKind::Other("admin_pet"), Vec::new())
    });
    done(cmd.character, info)
}

fn revive(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    slot: u32,
    serial: u64,
) -> Result<Value, AdminFailure> {
    let perm = svc.items.handle().cfg.perm_extrabags;
    let found = {
        let m = ctx
            .worlds
            .player(who.0)
            .ok_or_else(|| fail(AdminErrorCode::NotFound, "the avatar is gone"))?;
        locate(m, perm, ContainerLoc::Inventory, slot)?
    };
    if found.item.serial.0 != serial {
        return Err(fail(
            AdminErrorCode::StaleTarget,
            format!(
                "slot {slot} holds serial {}, not {serial}",
                found.item.serial.0
            ),
        ));
    }
    if found.item.pet.is_none() {
        return Err(fail(AdminErrorCode::Invalid, "the item is not a pet"));
    }
    if !found.item.is_expired() {
        return Err(fail(AdminErrorCode::StaleTarget, "the pet is not dead"));
    }
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let cx = BuffCx::new(env, ctx.clock.game_ms);
    let mut fx = Fx::new();
    let before = item_json(&found.item);
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        let tables = &cx.data().pet;
        let flags = found.item.flags.0;
        ops::set_item_flags(
            m,
            &cx.env,
            found.id,
            flags & !flyff_rules::item::ITEM_FLAG_EXPIRED,
            &mut fx,
        );
        if let Some(pet) = crate::item::pets::system::pet_mut(m, found.id) {
            let half = u32::from(pet.max_energy(tables)) / 2;
            pet.life = 0;
            pet.set_energy(tables, half);
        }
        send_state(m, &cx, found.id, &mut fx);
    }
    fx.mark_urgent();
    fx.flush(svc, ctx, who);
    Ok(json!({"action": "revive", "item": before, "slot": slot}))
}
