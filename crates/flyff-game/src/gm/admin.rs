//! The GM variants of `AdminCmd` that doc 23 owns (Plan/23 §8.2; WP-GM-04): [`GmAdmin`] executes
//! `AdminCmd::RunLine` for the stdin console, the admin API and the CLI: the line runs as `Issuer::Console` /
//! `Issuer::Admin`, optionally acting as an online character (`as <name>`). The console, the CLI and the owner
//! identities run with `ADMINISTRATOR` authority; a staff member of the admin console runs with its own letter
//! (`min(letter, P)`, 25 §1.4, 23 §3.8), so the minimum letters of 23 §4 still apply to it.
//! Every other command goes to the handler it wraps (the runtime's built-ins are `crate::admin_ops`).

use super::{
    CmdCtx, CmdOutput, CommandRegistry, DispatchOutcome, Issuer, UNKNOWN_COMMAND, dispatch_line,
    target::{self, Target},
};
use crate::ctx::{GameCtx, Services};
use crate::handlers::{AdminHandler, NoAdmin};
use crate::input::{AdminActor, AdminCmd, AdminOutput};
use flyff_common::gm::Authority;
use std::sync::Arc;

/// Splits an optional `as <name> ` prefix off a console line (23 §3.8): `as Alice /lv Knight 60`,
/// `as "Mr Bob" /te Alice`.
pub fn split_as_prefix(line: &str) -> (Option<String>, &str) {
    let t = line.trim_start();
    let Some(rest) = t.strip_prefix("as ").or_else(|| t.strip_prefix("AS ")) else {
        return (None, t);
    };
    let rest = rest.trim_start();
    if let Some(quoted) = rest.strip_prefix('"') {
        if let Some(end) = quoted.find('"') {
            let name = quoted.get(..end).unwrap_or("");
            return (
                Some(name.to_owned()),
                quoted.get(end + 1..).unwrap_or("").trim_start(),
            );
        }
        return (None, t);
    }
    match rest.split_once(char::is_whitespace) {
        Some((name, tail)) => (Some(name.to_owned()), tail.trim_start()),
        None => (Some(rest.to_owned()), ""),
    }
}

/// Runs one console or admin line and returns the rendered output lines (`AdminOutput::Lines`) or the error text
/// (23 §3.8): a missing `as` character, an unknown command.
pub fn run_admin_line(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    line: &str,
    as_character: Option<&str>,
    actor: AdminActor,
) -> AdminOutput {
    let (prefix_as, body) = split_as_prefix(line);
    let acting = as_character.map(str::to_owned).or(prefix_as);
    let self_player = match acting {
        Some(name) => match target::find(svc, &name) {
            Target::Online { player, .. } => Some(player),
            Target::Offline { name, .. } => {
                return AdminOutput::Error(format!("{name} is not connected"));
            }
            Target::Unknown => return AdminOutput::Error(format!("{name} does not exist")),
        },
        None => None,
    };
    let body = body.trim();
    let line = if body.starts_with('/') {
        body.to_owned()
    } else {
        format!("/{body}")
    };
    let issuer = match actor {
        AdminActor::Owner { .. } | AdminActor::Staff { .. } => Issuer::Admin { actor },
        AdminActor::Console | AdminActor::Cli => Issuer::Console,
    };
    let reg: Arc<CommandRegistry> = Arc::clone(&svc.gm.registry);
    let mut cx = CmdCtx {
        svc,
        ctx,
        issuer,
        authority: actor.authority(),
        self_player,
        out: CmdOutput::default(),
        shortcut: false,
    };
    let outcome = dispatch_line(&mut cx, &reg, &line);
    let mut out = std::mem::take(&mut cx.out);
    if outcome == DispatchOutcome::NotACommand {
        out.text(UNKNOWN_COMMAND);
    }
    AdminOutput::Lines(out.render(&ctx.data.texts))
}

/// Stores `authority` on the live avatars `targets` (the stored value of the session and of the player state, the
/// two places the command gate and the chat read): the next command line of that player is gated by it (VR-073).
/// A demotion below `GAMEMASTER` also removes the GM-only mode bits (`MATCHLESS`, `TRANSPARENT`, `ONEKILL`,
/// `MATCHLESS2`, the rule of the join normalisation, 23 §2.5) at once. Returns how many avatars changed.
pub fn apply_authority(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    targets: &[flyff_common::ObjId],
    authority: Authority,
) -> u32 {
    let mut changed = 0;
    for &obj in targets {
        let player = crate::ctx::PlayerRef(obj);
        if let Some(e) = ctx.sessions.get_mut(obj) {
            e.authority = authority;
        }
        if let Some(p) = ctx.worlds.player_mut(obj).and_then(|m| m.player_mut()) {
            p.ident.authority = authority;
        }
        let mode = ctx.player(player).map_or(0, |m| m.mv.mode);
        let stripped = super::modes::strip_on_join(mode, authority);
        if stripped != mode {
            super::modes::change_mode(
                svc,
                ctx,
                player,
                flyff_common::gm::ModeFlags::NONE,
                flyff_common::gm::ModeFlags::from_bits(mode & !stripped),
                super::modes::ModeSource::System,
            );
        }
        changed += 1;
    }
    changed
}

/// `AdminCmd::AuthorityChanged` (23 §1.4, §8.2): sent by 12's `AccountAdmin` after its database commit; the
/// in-world character of `account` (all of them without `character`) takes `authority` in the same tick. A
/// `[gm]` grant on a character's name keeps pinning that character (the game thread cannot evaluate the account
/// and address selectors; the lobby does at the next login).
pub fn authority_changed(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    account: flyff_common::AccountId,
    character: Option<flyff_common::CharacterId>,
    authority: Authority,
) -> u32 {
    let entries: Vec<(flyff_common::ObjId, flyff_common::CharacterId)> = ctx
        .sessions
        .iter()
        .filter(|e| e.account == account && character.is_none_or(|c| c == e.character))
        .map(|e| (e.player, e.character))
        .collect();
    let mut changed = 0;
    for (obj, id) in entries {
        let name = svc.presence.get(id).map(|d| d.name.clone());
        let pinned = name.and_then(|n| {
            super::cmd::admin::character_grant(
                &ctx.cfg.gm,
                ctx.cfg.rules.features.servers_authorize,
                &n,
            )
        });
        changed += apply_authority(svc, ctx, &[obj], pinned.unwrap_or(authority));
    }
    changed
}

/// The admin handler of doc 23: `RunLine` here, everything else to the wrapped handler.
pub struct GmAdmin {
    inner: Box<dyn AdminHandler>,
}

impl GmAdmin {
    /// Wraps `inner` (the handler that executes the commands of other docs).
    pub fn new(inner: Box<dyn AdminHandler>) -> Self {
        Self { inner }
    }
}

impl Default for GmAdmin {
    fn default() -> Self {
        Self::new(Box::new(NoAdmin))
    }
}

impl AdminHandler for GmAdmin {
    fn run(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        cmd: AdminCmd,
        actor: AdminActor,
    ) -> AdminOutput {
        match cmd {
            AdminCmd::RunLine { line, as_character } => {
                run_admin_line(svc, ctx, &line, as_character.as_deref(), actor)
            }
            AdminCmd::AuthorityChanged {
                account,
                character,
                authority,
            } => AdminOutput::Done {
                affected: authority_changed(svc, ctx, account, character, authority),
            },
            other => self.inner.run(svc, ctx, other, actor),
        }
    }

    fn handles(&self, cmd: &AdminCmd) -> bool {
        matches!(
            cmd,
            AdminCmd::RunLine { .. } | AdminCmd::AuthorityChanged { .. }
        ) || self.inner.handles(cmd)
    }
}

impl std::fmt::Debug for GmAdmin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GmAdmin(..)")
    }
}
