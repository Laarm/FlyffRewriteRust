//! Account and session moderation commands (Plan/23 §4.4, §4.9; WP-GM-09): `/out` (row 33), `/setauth` (N07),
//! `/ban` (N08), `/unban` (N09), `/kickall` (N10), `/save` (N11) and `/shutdown` (N12).
//!
//! Persistent effects go to the writer as `DbOp`s (`InsertAccountBan`, `LiftAccountBansOf`,
//! `SetCharacterAuthority`, `SetAccountAuthority`); the online effects are the game-side kick path of Plan/12 §12.2
//! (`GameCtx::request_leave` with `LeaveTrigger::Kicked(GmKick | Banned)`, the lobby revokes the token from
//! `LostCause::Kicked`) and, for the authority, the stored value of the live avatar.

use crate::ctx::PlayerRef;
use crate::gm::admin::apply_authority;
use crate::gm::audit::{event, issuer_character, issuer_label};
use crate::gm::requests::GmRequest;
use crate::gm::target::{self, Target};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult, Issuer};
use crate::input::{AccountChange, AccountChangeKind};
use crate::session::LeaveTrigger;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::gm::Authority;
use flyff_common::rt::KickReason;
use flyff_common::{AccountId, ObjId, UnixTime};
use flyff_db::{AccountBanRecord, AtomicBatch, AtomicKind, AtomicOutcome, BatchReply, DbOp};
use flyff_proto::pb;
use serde_json::json;

/// Longest `Kick.detail` (`session.proto`: at most 255 bytes).
const MAX_KICK_DETAIL: usize = 255;
/// Longest ban reason (`account_bans.reason`, 12 §5.5).
const MAX_BAN_REASON_CHARS: usize = 255;
/// Longest `/shutdown` countdown in seconds.
pub const MAX_SHUTDOWN_S: u32 = 3600;

/// Row 33 `/out <name>`: kicks an online character through the game-side `KickAccount` path with
/// `KickReason::GmKick` (12 §12.2: `Kick{GM_KICK}` on the sink, close, then the leave mode the rule assigns).
/// Self -> `ReturnSay{2}`, unknown -> `ReturnSay{3}`; a known but offline character: nothing.
pub fn out(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    if name.is_empty() {
        return Ok(CmdDone::default());
    }
    let (player, name) = match target::online(cx, &name, true) {
        Ok(t) => t,
        Err(CmdError::Offline(_)) => return Ok(CmdDone::default()),
        Err(e) => return Err(e),
    };
    cx.ctx
        .request_leave(player.0, LeaveTrigger::Kicked(KickReason::GmKick));
    Ok(CmdDone::on(name))
}

/// `s` cut to at most `max` bytes on a character boundary.
fn cut_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("")
}

/// N10 `/kickall [message...]`: every session with authority below `GAMEMASTER` (lingering avatars included)
/// takes the kick path of row 33 (`Kick.detail` = the message). GMs stay.
pub fn kickall(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let message = args.last_full();
    let detail = cut_bytes(message.trim(), MAX_KICK_DETAIL).to_owned();
    let targets: Vec<ObjId> = cx
        .ctx
        .sessions
        .iter()
        .filter(|e| !e.authority.is_gm())
        .map(|e| e.player)
        .collect();
    for &p in &targets {
        if !detail.is_empty()
            && let Some(e) = cx.ctx.sessions.get_mut(p)
        {
            e.kick_detail = Some(detail.clone());
        }
        cx.ctx
            .request_leave(p, LeaveTrigger::Kicked(KickReason::GmKick));
    }
    cx.out.text(format!("{} players kicked", targets.len()));
    Ok(CmdDone::default())
}

/// N11 `/save [name]`: an urgent snapshot of one online character, or of every avatar (lingering ones included);
/// the writer takes it at the end of the tick (13 §17.1).
pub fn save(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    if name.is_empty() {
        let all: Vec<ObjId> = cx.ctx.sessions.iter().map(|e| e.player).collect();
        for &p in &all {
            cx.svc.saves.mark_urgent(p);
        }
        cx.out.text(format!("saving {} characters", all.len()));
        return Ok(CmdDone::default());
    }
    match target::find(cx.svc, &name) {
        Target::Unknown => Err(CmdError::NotFound(name)),
        Target::Offline { name, .. } => {
            cx.out.text(format!("{name} is not connected."));
            Ok(CmdDone::on(name))
        }
        Target::Online { player, name, .. } => {
            cx.svc.saves.mark_urgent(player.0);
            cx.out.text("saving 1 characters");
            Ok(CmdDone::on(name))
        }
    }
}

/// N12 `/shutdown <seconds> [message...]` or `/shutdown cancel`: the in-game trigger of the
/// `ShutdownCoordinator` (10 §15.1; the countdown notices, the lobby refusal and the final logout are its).
/// `seconds` is `0..=3600`.
pub fn shutdown(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let first = args.token().to_owned();
    if first.is_empty() {
        return Err(CmdError::Usage);
    }
    let by = issuer_character(cx);
    if first.eq_ignore_ascii_case("cancel") {
        if cx.ctx.telemetry.health.state() != flyff_common::rt::HealthState::ShuttingDown {
            return Err(CmdError::Refused("no shutdown is running".to_owned()));
        }
        cx.svc.gm.request(GmRequest::CancelShutdown { by });
        return Ok(CmdDone::default());
    }
    if !args.token_is_number() {
        return Err(CmdError::Usage);
    }
    let seconds = flyff_script::cnum::c_atoi(first.as_bytes());
    let Ok(countdown_s) = u32::try_from(seconds) else {
        return Err(CmdError::Usage);
    };
    if countdown_s > MAX_SHUTDOWN_S {
        return Err(CmdError::Refused(format!(
            "the countdown is at most {MAX_SHUTDOWN_S} seconds"
        )));
    }
    let message = args.last_full();
    let message = Some(message.trim().to_owned()).filter(|m| !m.is_empty());
    cx.svc.gm.request(GmRequest::Shutdown {
        countdown_s,
        message,
        by,
    });
    Ok(CmdDone::default())
}

/// The `runtime_override.set_by` label of a live change (13 §5.1): `gm:<account id>` for a player, `admin:console`
/// and `admin:<audit actor>` (`admin:owner:local`, `admin:owner:token`, `admin:staff:<account>`, 25 §1.1) for the
/// operator surfaces.
pub fn issuer_set_by(cx: &CmdCtx<'_, '_>) -> String {
    match cx.issuer {
        Issuer::Console => "admin:console".to_owned(),
        Issuer::Admin { actor } => actor.set_by(),
        _ => issuer_label(cx),
    }
}

/// Sends `ops` to the writer as one batch; when the database refuses them the issuer (a player; the console has
/// the log) is told what did not happen, so that a GM never believes in a ban that was not stored.
fn persist_checked(
    cx: &mut CmdCtx<'_, '_>,
    kind: &'static str,
    ops: Vec<DbOp>,
    failure_text: String,
) {
    let reply_to = match cx.issuer {
        Issuer::Player { session, .. } | Issuer::Item { session, .. } => Some(session),
        _ => None,
    };
    let ticket = cx
        .ctx
        .tickets
        .issue(Pending::Service(ServiceTicket::new(move |_svc, ctx, r| {
            if let TicketResult::Failed(e) = r {
                tracing::error!(kind, "a GM command was not stored: {e}");
                if let Some(sid) = reply_to {
                    ctx.outbox.to_session(
                        sid,
                        0,
                        pb::Text {
                            kind: flyff_rules::chat::TEXT_GENERAL,
                            text: failure_text,
                        },
                    );
                }
            }
        })));
    cx.ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ..AtomicBatch::of_ops(AtomicKind::Other(kind), ops)
    });
}

/// The characters of `account` that are in the world (lingering ones included).
fn avatars_of(cx: &CmdCtx<'_, '_>, account: AccountId) -> Vec<ObjId> {
    cx.ctx
        .sessions
        .iter()
        .filter(|e| e.account == account)
        .map(|e| e.player)
        .collect()
}

/// The `[gm]` grant that pins a character's authority regardless of the database (character selectors only: the
/// game thread knows neither account names nor addresses, Plan/23 §1.3). The last matching entry wins.
pub fn character_grant(
    cfg: &flyff_common::gm::GmConfig,
    servers_authorize: bool,
    character: &str,
) -> Option<Authority> {
    if !servers_authorize {
        return None;
    }
    cfg.grants
        .iter()
        .filter(|g| {
            g.character
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(character))
        })
        .filter_map(|g| Authority::parse_token(&g.level).ok())
        .next_back()
}

/// N07 `/setauth <character> <level> [account]`: writes `characters.authority` (default) or, with the word
/// `account`, the owning account's `accounts.authority`; an online avatar takes the value in the same tick (the
/// command gate follows at once). A `[gm]` grant on the character's name overrides the stored value: the reply
/// says so and the live value stays the grant's. Audit `account.authority`.
pub fn setauth(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    let level_tok = args.token().to_owned();
    if name.is_empty() || level_tok.is_empty() {
        return Err(CmdError::Usage);
    }
    let for_account = args.token().eq_ignore_ascii_case("account");
    let level = Authority::parse_token(&level_tok)
        .ok()
        .filter(|a| a.short_name().is_some() && *a != Authority::LEGACY_TEST)
        .ok_or_else(|| {
            CmdError::Refused(format!(
                "unknown level '{level_tok}': use player, logchat, journalist, helper, gm1, gm2, gm3, operator, admin or a letter"
            ))
        })?;
    let (id, account, name) = match cx.svc.presence.by_name(&name) {
        Some(e) => (e.id, e.account, e.name.clone()),
        None => return Err(CmdError::NotFound(name)),
    };
    let op = if for_account {
        DbOp::SetAccountAuthority {
            account_id: account,
            authority: level.as_u8(),
        }
    } else {
        DbOp::SetCharacterAuthority {
            character_id: id,
            authority: level.as_u8(),
        }
    };
    persist_checked(
        cx,
        "account.authority",
        vec![op],
        format!("{name}: the new authority could not be stored"),
    );
    if for_account {
        // 25 §1.8: the admin sessions of the account end (the staff letter is the account-level one)
        cx.svc.gm.account_events.notify(AccountChange {
            account,
            kind: AccountChangeKind::Authority,
        });
    }
    // the live avatars
    let targets: Vec<ObjId> = if for_account {
        avatars_of(cx, account)
    } else {
        avatars_of(cx, account)
            .into_iter()
            .filter(|&p| {
                cx.ctx
                    .entry(PlayerRef(p))
                    .is_some_and(|e| e.character == id)
            })
            .collect()
    };
    let grant = character_grant(
        &cx.ctx.cfg.gm,
        cx.ctx.cfg.rules.features.servers_authorize,
        &name,
    );
    apply_authority(cx.svc, cx.ctx, &targets, grant.unwrap_or(level));
    cx.out.text(format!(
        "{name}: {} authority set to {} ({})",
        if for_account { "account" } else { "character" },
        level.letter(),
        level.short_name().unwrap_or("?")
    ));
    if grant.is_some() {
        cx.out.text("warning: overridden by [gm] config");
    }
    let by = issuer_label(cx);
    event(
        cx,
        "account.authority",
        json!({
            "account": account.0,
            "character": name,
            "by": by,
            "authority": level.letter().to_string(),
            "scope": if for_account { "account" } else { "character" },
        }),
    );
    Ok(CmdDone::on(name))
}

/// `90m`, `12h`, `7d` (n >= 1) or `perm`/`permanent`: `Some(None)` is permanent, `Some(Some(seconds))` a
/// duration; `None` when `text` is not a duration (the syntax of `flyff account ban --for`, 12 §5.5).
pub fn parse_ban_duration(text: &str) -> Option<Option<i64>> {
    let t = text.trim().to_ascii_lowercase();
    if t == "perm" || t == "permanent" {
        return Some(None);
    }
    // the unit is the last character (a multi-byte one is simply not a unit)
    let unit = t.chars().last()?;
    let num = t.get(..t.len() - unit.len_utf8())?;
    let n: i64 = num.parse().ok().filter(|n| *n >= 1)?;
    let secs: i64 = match unit {
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => return None,
    };
    n.checked_mul(secs).map(Some)
}

/// The known character a ban command names: `(character, account, stored name)`.
fn account_of(
    cx: &CmdCtx<'_, '_>,
    name: &str,
) -> Result<(flyff_common::CharacterId, AccountId, String), CmdError> {
    match cx.svc.presence.by_name(name) {
        Some(e) => Ok((e.id, e.account, e.name.clone())),
        None => Err(CmdError::NotFound(name.to_owned())),
    }
}

/// N08 `/ban <character> [duration] [reason...]`: bans the character's account (`<n>m`, `<n>h`, `<n>d` or `perm`,
/// default `perm`), kicks the live avatars with `KickReason::Banned` (the lobby revokes the token from
/// `LostCause::Kicked(Banned)`) and audits `account.ban`. An account that is only in character select is refused
/// at its next login (12 §5.7).
pub fn ban(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    if name.is_empty() {
        return Err(CmdError::Usage);
    }
    let (_id, account, name) = account_of(cx, &name)?;
    let own = cx
        .self_player
        .and_then(|p| cx.ctx.entry(p))
        .map(|e| e.account);
    if matches!(cx.issuer, Issuer::Player { .. } | Issuer::Item { .. }) && own == Some(account) {
        return Err(CmdError::NotSelf);
    }
    let rest = args.rest_raw().trim();
    let (first, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let (secs, reason) = match parse_ban_duration(first) {
        Some(d) => (d, tail.trim()),
        None => (None, rest),
    };
    let reason: String = reason.chars().take(MAX_BAN_REASON_CHARS).collect();
    let now = cx.ctx.clock.unix_ms.to_unix_time();
    let until = secs.map(|s| UnixTime(now.0.saturating_add(s)));
    let by = issuer_label(cx);
    persist_checked(
        cx,
        "account.ban",
        vec![DbOp::InsertAccountBan(AccountBanRecord {
            id: 0,
            account_id: account,
            created_at: now,
            until,
            reason: reason.clone(),
            created_by: by.clone(),
            lifted_at: None,
            lifted_by: None,
        })],
        format!("{name}: the ban could not be stored"),
    );
    // 25 §1.8: the admin sessions of a banned account end at once
    cx.svc.gm.account_events.notify(AccountChange {
        account,
        kind: AccountChangeKind::Banned,
    });
    let avatars = avatars_of(cx, account);
    for &p in &avatars {
        cx.ctx
            .request_leave(p, LeaveTrigger::Kicked(KickReason::Banned));
    }
    cx.out.text(match secs {
        None => format!("{name}: account banned permanently"),
        Some(s) => format!("{name}: account banned for {s} seconds"),
    });
    event(
        cx,
        "account.ban",
        json!({
            "account": account.0,
            "character": name,
            "by": by,
            "until": until.map(|u| u.0),
            "reason": reason,
        }),
    );
    Ok(CmdDone::on(name))
}

/// N09 `/unban <character>`: lifts every active ban of the character's account (`gm:<account>`); effective at
/// the next login. A player issuer gets the count when the writer answered. Audit `account.unban`.
pub fn unban(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    if name.is_empty() {
        return Err(CmdError::Usage);
    }
    let (_id, account, name) = account_of(cx, &name)?;
    let now = cx.ctx.clock.unix_ms.to_unix_time();
    let by = issuer_label(cx);
    let reply_to = match cx.issuer {
        Issuer::Player { session, .. } | Issuer::Item { session, .. } => Some(session),
        _ => None,
    };
    let shown = name.clone();
    let ticket = cx
        .ctx
        .tickets
        .issue(Pending::Service(ServiceTicket::new(move |_svc, ctx, r| {
            let text = match r {
                TicketResult::Committed(AtomicOutcome::Lifted(0)) => {
                    format!("{shown}: no active ban")
                }
                TicketResult::Committed(AtomicOutcome::Lifted(n)) => {
                    format!("{shown}: {n} ban(s) lifted")
                }
                TicketResult::Failed(e) => {
                    tracing::error!(account = account.0, "/unban failed: {e}");
                    format!("{shown}: the ban list could not be changed")
                }
                _ => return,
            };
            if let Some(sid) = reply_to {
                ctx.outbox.to_session(
                    sid,
                    0,
                    pb::Text {
                        kind: flyff_rules::chat::TEXT_GENERAL,
                        text,
                    },
                );
            }
        })));
    cx.ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ..AtomicBatch::of_ops(
            AtomicKind::Other("account.unban"),
            vec![DbOp::LiftAccountBansOf {
                account_id: account,
                at: now,
                by: by.clone(),
            }],
        )
    });
    if reply_to.is_none() {
        cx.out.text(format!("{name}: unban requested"));
    }
    event(
        cx,
        "account.unban",
        json!({"account": account.0, "character": name, "by": by}),
    );
    Ok(CmdDone::on(name))
}
