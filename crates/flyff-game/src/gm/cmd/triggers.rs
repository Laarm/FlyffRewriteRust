//! The execution side of the item-triggered GM commands (Plan/23 §5.2, §5.3; WP-GM-19): what using an `IK2_GMTEXT`
//! item adds to the buff of its row ([`use_plan`]) and the executor that runs a command text with the item
//! privilege ([`run`]). The data side - the parser, the whitelist validation and the table built at game
//! start - is [`crate::gm::item_text`], which re-exports this module's public items under the plan's names.
//!
//! The call sites are the buff engine's: `item::buffitems::ik2::gmtext` asks [`use_plan`] when the item is used
//! and queues `Followup::ItemCommand`; `buff::core::release` queues the release half; the follow-up lands in
//! [`crate::buff::api::run_item_command`], which is [`run`].

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::gm::item_text::{spec_of, split};
use crate::gm::{CmdArgs, CmdCtx, CmdOutput, Issuer, dispatch_line};
use flyff_common::defs::item::II_SYS_SYS_GM_NODISGUISE;
use flyff_data::props::item::ItemProp;
use flyff_rules::names::{NameError, NameKind, name_error_tid};
use std::sync::Arc;

/// The row of `/createguild`, the command of the guild-creation scroll (23 §13 Q5).
const ROW_CREATE_GUILD: u16 = 67;

/// What using an item adds to the buff of its row (23 §5.2 step 3, §13 Q5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsePlan {
    /// Nothing to run (no command, or the remover item which only ends a disguise).
    Nothing,
    /// Run this command with the item privilege (queued as `Followup::ItemCommand`).
    Run(String),
    /// Refuse with this text id and do not consume the item.
    Refuse(u32),
}

/// The use plan of an `IK2_GMTEXT` item: its use half, except for `II_SYS_SYS_GM_NODISGUISE` (which "executes no
/// command of its own", 23 §5.2 step 2). A command that needs an argument the scroll cannot supply - the
/// guild-creation scroll's `/createguild` has no name - is refused with the guild-name text and the scroll is
/// not consumed (23 §13 Q5, the default decision under PD-11).
pub fn use_plan(prop: &ItemProp) -> UsePlan {
    if prop.id == II_SYS_SYS_GM_NODISGUISE {
        return UsePlan::Nothing;
    }
    let Some(text) = split(&prop.text_file).on_use else {
        return UsePlan::Nothing;
    };
    let mut args = CmdArgs::new(&text);
    let _ = args.token();
    let command = args.token().to_owned();
    if spec_of(&command).is_some_and(|s| s.row == ROW_CREATE_GUILD) && args.token().is_empty() {
        return UsePlan::Refuse(name_error_tid(NameKind::Guild, NameError::Empty) as u32);
    }
    UsePlan::Run(text)
}

/// Runs the command `text` of the item `item` for the player `who` with the item privilege
/// (`ParsingCommand( text, mover, TRUE )`, 23 §5.2/§5.3). A half the table did not validate does not run; a
/// player without a session cannot receive the output and is skipped. The registry applies the whitelist and
/// `fidelity.item_gm_commands`: whitelisted rows run regardless of authority with the switch on; with it off the
/// player's own authority decides (no effect for a player, KEEP). The command is audited as `gm.command` with
/// `issuer = "item"`.
pub fn run(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, item: u32, text: &str) {
    if !svc.gm.item_text.permits(item, text) {
        tracing::debug!(item, text, "GM item command not validated: skipped");
        return;
    }
    let Some(actor) = crate::chat::actor(ctx, who) else {
        return;
    };
    let Some(session) = actor.session else {
        return;
    };
    let reg = Arc::clone(&svc.gm.registry);
    let mut cx = CmdCtx {
        svc,
        ctx,
        issuer: Issuer::Item {
            session,
            player: who,
            item,
        },
        authority: actor.authority,
        self_player: Some(who),
        out: CmdOutput::default(),
        shortcut: false,
    };
    let _ = dispatch_line(&mut cx, &reg, text);
    let out = std::mem::take(&mut cx.out);
    crate::chat::deliver_lines(ctx, who, out);
}
