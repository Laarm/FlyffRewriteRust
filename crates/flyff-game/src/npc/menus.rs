//! NPC menus (Plan/20 §8.1-§8.2, ref/16 §4.2): which action a click on an NPC runs, and the registry through which
//! the owners of the menu services (shop, bank, mail, teleporter, ...) are reached.
//!
//! The *client* picks the menu: it counts the enabled `MMI_*` entries of the NPC (`NpcDef.menus`) and runs
//! [`plan_for`] — **0 entries open the dialog** (`MMI_DIALOG`, which sends `ScriptDlg{npc, ""}`), **exactly 1 is
//! executed directly**, **2 or more open a popup** that lists them in id order. `AddMenu(MMI_DIALOG)` is commented out
//! in every `character.inc` entry, so an NPC with a single shop menu never opens its dialog (KEEP-QUIRK KQ-26) and
//! 88 NPCs with a dialog key but no script open an empty window (the server answers nothing, [`super::engine`]).
//!
//! The *server* receives the consequence as the message of the chosen menu (`OpenShopWndReq`, `Teleporter`,
//! `NpcBuff`, ...), and each handler checks VR-012 itself (`Worlds::close_npc`, the call-site table of §8.1).
//! [`MenuOwners`] is the one place where owners register such an entry point by menu id, so server-driven
//! interactions (a bot primitive, a GM "talk to" command, a quest marker) reach a menu without knowing the owner:
//! [`MenuOwners::open`] runs the registered function, [`MenuAction::Dialog`] is left to the caller (it is the
//! `ScriptDlg` flow of the engine).

use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::ObjId;
use flyff_common::defs::neuz::MMI_DIALOG;
use flyff_data::types::MenuId;
use std::collections::BTreeMap;

/// What a click on an NPC does (the client rule of ref/16 §4.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Open the dialog window and send `ScriptDlg{npc, ""}`.
    Dialog,
    /// Run this menu directly.
    Menu(MenuId),
    /// Show a popup with these menus, in id order.
    Popup(Vec<MenuId>),
}

/// The action for an NPC with the enabled menus `menus` (any order, duplicates ignored).
pub fn plan_for(menus: &[MenuId]) -> MenuAction {
    let mut ids: Vec<MenuId> = menus.to_vec();
    ids.sort_unstable();
    ids.dedup();
    match ids.as_slice() {
        [] => MenuAction::Dialog,
        [only] if *only == MMI_DIALOG => MenuAction::Dialog,
        [only] => MenuAction::Menu(*only),
        _ => MenuAction::Popup(ids),
    }
}

/// The entry point of a menu service: `(services, ctx, player, npc)`. The function does its own VR-012 check.
pub type MenuHandler = fn(&mut Services, &mut GameCtx<'_>, PlayerRef, ObjId);

/// What [`MenuOwners::open`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuOutcome {
    /// `MMI_DIALOG`: the dialog opens through `ScriptDlg`, nothing was run.
    Dialog,
    /// The registered owner ran.
    Handled,
    /// Nobody registered this menu (its owner is not installed yet).
    NoOwner,
}

/// The owners of the menu services by `MMI_*` id.
#[derive(Clone, Debug, Default)]
pub struct MenuOwners {
    handlers: BTreeMap<MenuId, MenuHandler>,
}

impl MenuOwners {
    /// Registers (or replaces) the entry point of `menu`.
    pub fn register(&mut self, menu: MenuId, handler: MenuHandler) {
        self.handlers.insert(menu, handler);
    }

    /// `true` when `menu` has an owner.
    pub fn has_owner(&self, menu: MenuId) -> bool {
        self.handlers.contains_key(&menu)
    }

    /// Runs the owner of `menu` for `who` at `npc`.
    pub fn open(
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        who: PlayerRef,
        npc: ObjId,
        menu: MenuId,
    ) -> MenuOutcome {
        if menu == MMI_DIALOG {
            return MenuOutcome::Dialog;
        }
        let Some(handler) = svc.npc.menus.handlers.get(&menu).copied() else {
            return MenuOutcome::NoOwner;
        };
        handler(svc, ctx, who, npc);
        MenuOutcome::Handled
    }
}
