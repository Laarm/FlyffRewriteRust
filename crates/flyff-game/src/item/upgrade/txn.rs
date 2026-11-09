//! The shared toolkit of the upgrade handlers (Plan/19 §12.1): the call context, the ownership checks of the
//! client-named items, the legacy feedback (sound, effect, texts), the audit line and the two ways an
//! attempt takes materials.
//!
//! Every upgrade is **one commit block**: first all checks (nothing changes), then the consumption, the roll
//! and the result. The functions here never change anything on their own except [`take_one`], [`destroy`] and
//! [`pay`]; a handler calls them only after its last refusal. Messages and audit lines go to the caller's
//! [`Fx`], flushed once by the `GameCtx` wrapper (so a panic drops them with the staged outbox, 01 §10.2).
//!
//! Client-supplied object ids are never trusted (Plan/19 §12 "FIX"): [`usable`] resolves an id to a storage
//! entry that exists, is not reserved by a trade or a stall; a request that names the same entry twice is
//! refused by [`distinct`].

use crate::item::container::StorageId;
use crate::item::env::ItemEnv;
use crate::item::fx::{Fx, item_json};
use crate::item::model::Item;
use crate::item::ops::{RemoveReason, UiParam, core as ops};
use crate::mover::Mover;
use crate::world::fx::{Sound, create_sfx_msg, sound_msg};
use flyff_common::GameMs;
use flyff_common::defs::obj::{XI_INT_FAIL, XI_INT_SUCCESS};
use flyff_common::defs::sound::{SND_INF_UPGRADEFAIL, SND_INF_UPGRADESUCCESS};
use flyff_common::defs::text::{TID_GAME_LACKMONEY, TID_GAME_NOTUPGRADE};
use flyff_data::props::item::ItemProp;
use flyff_db::audit::ev;
use flyff_rules::consts::TRANSPARENT_MODE;
use flyff_rules::item::refine::UpgradeTables;
use serde_json::{Value, json};

/// What the upgrade functions read besides the mover.
pub struct UpgradeCx<'a> {
    /// The item environment (game data, switches, wall time).
    pub env: &'a ItemEnv<'a>,
    /// The game clock of the tick (the pacing).
    pub now: GameMs,
    /// The odds tables.
    pub tables: &'a UpgradeTables,
}

impl<'a> UpgradeCx<'a> {
    /// A context.
    pub fn new(env: &'a ItemEnv<'a>, now: GameMs, tables: &'a UpgradeTables) -> UpgradeCx<'a> {
        UpgradeCx { env, now, tables }
    }

    /// The prop of an item id.
    pub fn prop(&self, item_id: u32) -> Option<&'a ItemProp> {
        self.env.data().item(item_id)
    }
}

/// What an upgrade request did (for the callers and the tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Dropped without any message (a foreign, reserved or missing item, a busy state, the pacing).
    Ignored,
    /// Refused with a text or a result 0; nothing was consumed.
    Refused,
    /// The attempt ran and succeeded.
    Success,
    /// The attempt ran and failed; the target is kept.
    Failed,
    /// The attempt ran and failed; the target was destroyed.
    Destroyed,
}

impl Outcome {
    /// `true` when the attempt rolled (materials were consumed).
    pub fn rolled(self) -> bool {
        matches!(
            self,
            Outcome::Success | Outcome::Failed | Outcome::Destroyed
        )
    }
}

/// The inventory entry named by a client object id: it exists and is not reserved (`IsUsableItem`). A
/// storage index above `u16` cannot exist.
pub fn usable(m: &Mover, obj_id: u32) -> Option<StorageId> {
    let id = StorageId::try_from(obj_id).ok()?;
    let it = m.player()?.items.inventory.get_at_id(id)?;
    it.is_usable().then_some(id)
}

/// The item at a storage id.
pub fn item_at(m: &Mover, id: StorageId) -> Option<&Item> {
    m.player()?.items.inventory.get_at_id(id)
}

/// `m_Inventory.IsEquip( id )`.
pub fn equipped(m: &Mover, id: StorageId) -> bool {
    m.player().is_some_and(|p| p.items.inventory.is_equip(id))
}

/// The request names `ids` pairwise different entries (a modified client could name one entry as target and
/// material).
pub fn distinct(ids: &[StorageId]) -> bool {
    ids.iter()
        .enumerate()
        .all(|(i, a)| ids.iter().skip(i + 1).all(|b| a != b))
}

/// `IsMode( TRANSPARENT_MODE )`: the effect is not shown for an invisible GM.
pub fn transparent(m: &Mover) -> bool {
    m.mv.mode & TRANSPARENT_MODE != 0
}

/// The sound and the effect of one attempt: `PlaySound{SND_INF_UPGRADESUCCESS | FAIL}` to the player and
/// `CreateSfxObj{XI_INT_SUCCESS | FAIL}` at the player for everybody who sees it (not for an invisible GM).
pub fn feedback(m: &Mover, fx: &mut Fx, success: bool) {
    sound(fx, success);
    if !transparent(m) {
        let sfx = if success { XI_INT_SUCCESS } else { XI_INT_FAIL };
        fx.out.near(create_sfx_msg(sfx, m.core.pos, false));
    }
}

/// Only the sound (`AddPlaySound`).
pub fn sound(fx: &mut Fx, success: bool) {
    let id = if success {
        SND_INF_UPGRADESUCCESS
    } else {
        SND_INF_UPGRADEFAIL
    };
    fx.out.to_self(sound_msg(&Sound::Id(id)));
}

/// Takes one piece of the stack `id` (`UpdateItem( id, UI_NUM, count - 1 )`; the last piece deletes the
/// entry). The audit line of the attempt lists it, so no `item.destroy` line is written.
pub fn take_one(m: &mut Mover, cx: &UpgradeCx<'_>, id: StorageId, fx: &mut Fx) {
    let Some(count) = item_at(m, id).map(|it| i64::from(it.count)) else {
        return;
    };
    ops::update_item(
        m,
        cx.env,
        id,
        UiParam::Num,
        u32::try_from(count - 1).unwrap_or(0),
        0,
        fx,
    );
}

/// The target of a failed attempt is destroyed (`RemoveItem( id, 1 )`): `item.destroy{reason: "upgrade_fail"}`.
pub fn destroy(m: &mut Mover, cx: &UpgradeCx<'_>, id: StorageId, fx: &mut Fx) {
    ops::remove_item(m, cx.env, id, 1, RemoveReason::UpgradeFail, fx);
}

/// `AddGold( -cost )` after the check `gold >= cost`; `false` (with `TID_GAME_LACKMONEY`) when the player is
/// short and nothing changed.
pub fn pay(m: &mut Mover, cx: &UpgradeCx<'_>, cost: i64, fx: &mut Fx) -> bool {
    if crate::item::gold::core::gold(m) < cost {
        fx.text(TID_GAME_LACKMONEY);
        return false;
    }
    crate::item::gold::core::add_gold(m, cx.env, -cost, true, fx);
    true
}

/// `TID_GAME_NOTUPGRADE` for a target with an applied commercial element stone (the legacy path only).
pub fn refuse_stone(fx: &mut Fx) {
    fx.text(TID_GAME_NOTUPGRADE);
}

/// One material in the audit line: the item and how many pieces were taken.
pub fn material(item_id: u32, count: u32) -> Value {
    json!({ "item": item_id, "count": count })
}

/// One consumed buff (a legacy protection or smelting scroll) in the audit line.
pub fn buff_material(item_id: u32) -> Value {
    json!({ "buff": item_id })
}

/// `item.upgrade{kind, path, result, before, after, materials}` (Plan/19 §21). `after` is `None` when the
/// target no longer exists.
pub fn audit(
    fx: &mut Fx,
    kind: &str,
    path: &str,
    result: &str,
    before: &Item,
    after: Option<&Item>,
    materials: Vec<Value>,
) {
    fx.audit(
        ev::ITEM_UPGRADE,
        json!({
            "kind": kind,
            "path": path,
            "result": result,
            "before": item_json(before),
            "after": after.map(item_json),
            "materials": materials,
        }),
    );
}
