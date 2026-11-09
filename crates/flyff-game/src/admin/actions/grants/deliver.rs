//! The body of `GrantBatch` (Plan/25 §5.3, §11.5; 19 §4.1, §11.1, §20): every recipient is decided and served inside
//! one commit block of the tick.
//!
//! * **Direct.** The whole basket is tried on a copy of the bag first, so a grant is *all or nothing* per recipient
//!   (`NoSpace` changes nothing). Then the items go through `item::ops::create_item` (`ItemOrigin::Other("admin")`:
//!   fresh serials, the `CreateItem` message, the `item.create` audit line), the penya through `add_gold`, and one
//!   `AdminGrant` batch carries a fresh snapshot of the receiver (13 §18).
//! * **Mail.** One system mail per item line (a mail holds one item), the penya on the first (a penya-only grant is
//!   one mail), all inserted by one `MailSend` batch without snapshots. Works for offline recipients. The mailbox
//!   limit (50) is respected: `MailboxFull` changes nothing.
//! * **Auto.** Online with room: Direct. Online without room: Mail. Offline: pending when the delivery of WP-ECON-09
//!   is installed ([`GrantGuard::deliver`](crate::persist::grants::GrantGuard)), else Mail. A full mailbox falls
//!   back to pending when that exists.
//! * **Pending.** Answered `NeedsPending`: the route inserts the rows (it needs their database ids).
//!
//! A request is remembered by its `request_id` for ten minutes: a retry returns the stored report (`replayed`).

use super::validate::{FieldError, Normalised, ValidateEnv, validate};
use super::{
    GrantBatch, GrantMode, GrantNow, GrantOutcome, GrantedReport, ItemSpec, MAX_LINES,
    MAX_RECIPIENTS, RecipientResult,
};
use crate::admin::actions::common::{check_rank, fail, failed};
use crate::admin::types::{AdminErrorCode, AdminFailure};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch, submit};
use crate::econ::mail::{MAX_MAIL, MAX_TEXT_BYTES, MAX_TITLE_BYTES};
use crate::input::{AdminActor, AdminOutput};
use crate::item::container::{AddError, SeqSerials, SerialSource};
use crate::item::gold::GOLD_MAX;
use crate::item::model::Item;
use crate::item::ops::{CtxSerials, ItemOrigin};
use crate::tickets::TicketResult;
use flyff_common::{CharacterId, ItemSerial, UnixTime};
use flyff_db::{AtomicBatch, AtomicKind, BatchReply, CharacterState, DbOp};
use flyff_rules::item::refine::UpgradeTables;

/// `GrantBatch`: see the module documentation.
pub(in crate::admin::actions) fn grant_batch(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: GrantBatch,
) -> AdminOutput {
    if cmd.grants.is_empty() {
        return failed(
            AdminErrorCode::Invalid,
            "a grant needs at least one recipient",
        );
    }
    if cmd.grants.len() > MAX_RECIPIENTS {
        return failed(
            AdminErrorCode::TooManyTargets,
            format!("at most {MAX_RECIPIENTS} recipients per batch"),
        );
    }
    let now_ms = ctx.clock.game_ms.0;
    if let Some(stored) = svc.grants.admin.recall(cmd.request_id, now_ms) {
        let mut again = stored.clone();
        again.replayed = true;
        return AdminOutput::Granted(again);
    }
    let core = svc.items.handle();
    let tables = UpgradeTables::from_data(
        core.data.lua.item_upgrade.as_ref(),
        &core.data.accessory,
        &core.data.collecting,
    );
    let now = ctx.clock.unix_ms.to_unix_time();
    let venv = ValidateEnv {
        data: &core.data,
        cfg: &core.cfg,
        tables: &tables,
        now,
    };
    let mut per_recipient = Vec::with_capacity(cmd.grants.len());
    for g in &cmd.grants {
        let outcome = match prepare(&venv, g) {
            Ok(items) => serve(svc, ctx, &actor, &cmd.mode, g, items, now),
            Err(f) => GrantOutcome::Failed(f),
        };
        per_recipient.push(RecipientResult {
            character: g.character,
            outcome,
        });
    }
    let report = GrantedReport {
        request_id: cmd.request_id,
        per_recipient,
        replayed: false,
    };
    svc.grants.admin.remember(now_ms, report.clone());
    AdminOutput::Granted(report)
}

/// The stored items of one recipient's basket, or why it is refused as a whole.
fn prepare(venv: &ValidateEnv<'_>, g: &GrantNow) -> Result<Vec<Item>, AdminFailure> {
    if g.items.len() > MAX_LINES {
        return Err(fail(
            AdminErrorCode::TooManyTargets,
            format!("at most {MAX_LINES} item lines per recipient"),
        ));
    }
    if g.items.is_empty() && g.gold == 0 {
        return Err(fail(AdminErrorCode::Invalid, "there is nothing to grant"));
    }
    if g.gold > u64::try_from(GOLD_MAX).unwrap_or(u64::MAX) {
        return Err(fail(
            AdminErrorCode::Invalid,
            format!("the penya is at most {GOLD_MAX}"),
        ));
    }
    let mut items = Vec::with_capacity(g.items.len());
    for (i, spec) in g.items.iter().enumerate() {
        match validate(venv, spec) {
            Ok(Normalised { record, .. }) => {
                let mut item = Item::from_record(&record);
                item.serial = ItemSerial(0);
                items.push(item);
            }
            Err(errors) => {
                return Err(fail(AdminErrorCode::Invalid, describe(i, spec, &errors)));
            }
        }
    }
    Ok(items)
}

fn describe(line: usize, spec: &ItemSpec, errors: &[FieldError]) -> String {
    let first = errors.first();
    format!(
        "item line {} (item {}): {}{}",
        line + 1,
        spec.item_id,
        first.map_or_else(String::new, |e| format!("{}: {}", e.field, e.message)),
        if errors.len() > 1 {
            format!(" (+{} more)", errors.len() - 1)
        } else {
            String::new()
        }
    )
}

fn pending_installed(svc: &Services) -> bool {
    svc.grants.deliver.is_some()
}

/// Decides and serves one recipient.
fn serve(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: &AdminActor,
    mode: &GrantMode,
    g: &GrantNow,
    items: Vec<Item>,
    now: UnixTime,
) -> GrantOutcome {
    let Some(entry) = svc.presence.get(g.character).cloned() else {
        return GrantOutcome::Failed(fail(
            AdminErrorCode::NotFound,
            format!("character {} does not exist", g.character.0),
        ));
    };
    if entry.state != CharacterState::Active {
        return GrantOutcome::Failed(fail(
            AdminErrorCode::Invalid,
            format!(
                "{} is not a playable character (deleted or sealed)",
                entry.name
            ),
        ));
    }
    let online = entry
        .online
        .map(|o| PlayerRef(o.obj))
        .filter(|p| ctx.worlds.player(p.0).is_some_and(|m| !m.core.deleted));
    if let Some(p) = online
        && let Err(f) = check_rank(actor, ctx, p.0)
    {
        return GrantOutcome::Failed(f);
    }
    match mode {
        GrantMode::Pending => GrantOutcome::NeedsPending,
        GrantMode::Direct => match online {
            Some(who) => match deliver_direct(svc, ctx, who, &items, g.gold, now) {
                Ok(done) => done,
                Err(f) => GrantOutcome::Failed(f),
            },
            None => GrantOutcome::Failed(fail(
                AdminErrorCode::CharacterOnline,
                format!(
                    "{} is offline: a direct grant needs an online character",
                    entry.name
                ),
            )),
        },
        GrantMode::Mail { subject, text } => {
            mail_or_fail(svc, ctx, g.character, items, g.gold, subject, text)
        }
        GrantMode::Auto => {
            let (subject, text) = ("Reward", "A game master sent you this.");
            match online {
                Some(who) => match deliver_direct(svc, ctx, who, &items, g.gold, now) {
                    Ok(done) => done,
                    Err(f) if matches!(f.code, AdminErrorCode::NoSpace) || wallet_full(&f) => {
                        auto_mail(svc, ctx, g.character, items, g.gold, subject, text)
                    }
                    Err(f) => GrantOutcome::Failed(f),
                },
                None if pending_installed(svc) => GrantOutcome::NeedsPending,
                None => auto_mail(svc, ctx, g.character, items, g.gold, subject, text),
            }
        }
    }
}

fn wallet_full(f: &AdminFailure) -> bool {
    f.code == AdminErrorCode::Invalid && f.message.contains("penya limit")
}

/// Auto's mail: a full mailbox falls back to a pending grant when the delivery exists.
fn auto_mail(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    to: CharacterId,
    items: Vec<Item>,
    gold: u64,
    subject: &str,
    text: &str,
) -> GrantOutcome {
    match deliver_mail(svc, ctx, to, items, gold, subject, text) {
        Ok(ids) => GrantOutcome::Mailed { mail_ids: ids },
        Err(f) if f.code == AdminErrorCode::MailboxFull && pending_installed(svc) => {
            GrantOutcome::NeedsPending
        }
        Err(f) => GrantOutcome::Failed(f),
    }
}

fn mail_or_fail(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    to: CharacterId,
    items: Vec<Item>,
    gold: u64,
    subject: &str,
    text: &str,
) -> GrantOutcome {
    match deliver_mail(svc, ctx, to, items, gold, subject, text) {
        Ok(ids) => GrantOutcome::Mailed { mail_ids: ids },
        Err(f) => GrantOutcome::Failed(f),
    }
}

/// The serial of the stack that received an item line.
fn serial_of(ctx: &GameCtx<'_>, who: PlayerRef, plan: &crate::item::container::AddPlan) -> u64 {
    let inv = ctx
        .worlds
        .player(who.0)
        .and_then(|m| m.player())
        .map(|p| &p.items.inventory);
    let id = plan
        .created
        .first()
        .or_else(|| plan.touched.first().map(|t| &t.0));
    match (inv, id) {
        (Some(inv), Some(id)) => inv.get_at_id(*id).map_or(0, |i| i.serial.0),
        _ => plan.template_serial.0,
    }
}

/// Into the bag, all or nothing (see the module documentation).
fn deliver_direct(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    items: &[Item],
    gold: u64,
    now: UnixTime,
) -> Result<GrantOutcome, AdminFailure> {
    let core = svc.items.handle();
    {
        let Some(p) = ctx.worlds.player(who.0).and_then(|m| m.player()) else {
            return Err(fail(
                AdminErrorCode::CharacterOnline,
                "the character is not in the world",
            ));
        };
        let mut probe = p.items.inventory.clone();
        let mut probe_serials = SeqSerials(u64::MAX / 2);
        for it in items {
            probe
                .add(it, &core.data, &mut probe_serials)
                .map_err(|e| match e {
                    AddError::Full => fail(
                        AdminErrorCode::NoSpace,
                        "the bag has no room for the basket",
                    ),
                    other => fail(AdminErrorCode::Invalid, format!("{other}")),
                })?;
        }
        let wallet = i64::from(p.items.gold);
        if wallet.saturating_add(i64::try_from(gold).unwrap_or(i64::MAX)) > GOLD_MAX {
            return Err(fail(
                AdminErrorCode::Invalid,
                "the wallet would exceed the penya limit (2,147,483,647)",
            ));
        }
        if !can_snapshot(ctx, who) {
            return Err(fail(
                AdminErrorCode::Internal,
                "the character cannot be saved right now (its session is tainted)",
            ));
        }
    }
    let mut serials = Vec::with_capacity(items.len());
    let mut broken: Option<String> = None;
    for it in items {
        match crate::item::ops::create_item(svc, ctx, who, it.clone(), ItemOrigin::Other("admin")) {
            Ok(plan) => serials.push(serial_of(ctx, who, &plan)),
            Err(e) => {
                // the dry run said it fits: a failure here is a bug (or no serial could be allocated); the batch
                // below still saves what was created
                tracing::error!(item = it.item_id, error = ?e, "admin grant: create_item failed after a successful dry run");
                broken = Some(format!("item {} could not be created: {e:?}", it.item_id));
                break;
            }
        }
    }
    if gold > 0 && broken.is_none() {
        crate::item::gold::add_gold(svc, ctx, who, i64::try_from(gold).unwrap_or(0), true);
    }
    if let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) {
        submit(
            svc,
            ctx,
            who,
            Some(generation),
            AtomicBatch::admin_grant(snap, Vec::new(), now),
            |_svc, _ctx, result| {
                if let TicketResult::Failed(e) = result {
                    tracing::error!(error = %e, "admin grant: the AdminGrant batch failed");
                }
            },
        );
    }
    if let Some(why) = broken {
        return Err(fail(
            AdminErrorCode::Internal,
            format!("the grant was only partly delivered: {why}"),
        ));
    }
    Ok(GrantOutcome::Delivered { serials, gold })
}

/// System mails: one per item line, the penya on the first (a penya-only grant is one mail).
fn deliver_mail(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    to: CharacterId,
    items: Vec<Item>,
    gold: u64,
    subject: &str,
    text: &str,
) -> Result<Vec<u64>, AdminFailure> {
    let needed = items.len().max(1);
    if svc.econ.mail.box_len(to) + needed > MAX_MAIL {
        return Err(fail(
            AdminErrorCode::MailboxFull,
            format!("the mailbox would hold more than {MAX_MAIL} mails"),
        ));
    }
    let core = svc.items.handle();
    let title = clip(subject, MAX_TITLE_BYTES);
    let body = clip(text, MAX_TEXT_BYTES);
    let mut remaining_gold = i64::try_from(gold).unwrap_or(i64::MAX);
    let mut ids = Vec::with_capacity(needed);
    let mut ops: Vec<DbOp> = Vec::with_capacity(needed * 2);
    let mut records = Vec::with_capacity(needed);
    let mut lines: Vec<Option<Item>> = items.into_iter().map(Some).collect();
    if lines.is_empty() {
        lines.push(None);
    }
    for line in lines {
        let item = match line {
            Some(mut item) => {
                if item.endurance == -1
                    && let Some(prop) = core.data.item(item.item_id)
                {
                    item.endurance = i32::try_from(prop.endurance).unwrap_or(0);
                }
                let Services { serials, .. } = &mut *svc;
                let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
                match src.next_serial() {
                    Ok(s) => item.serial = ItemSerial(s),
                    Err(e) => {
                        // nothing was inserted yet for this line; earlier lines are rolled back below
                        forget(svc, &ids);
                        return Err(fail(AdminErrorCode::Internal, format!("no serial: {e}")));
                    }
                }
                Some(item)
            }
            None => None,
        };
        let line_gold = std::mem::take(&mut remaining_gold);
        let (id, mail_ops) = svc
            .econ
            .mail
            .send_system_with_id(to, &title, &body, item, line_gold);
        if let Some(entry) = svc.econ.mail.get(id) {
            records.push(entry.to_record());
        }
        ids.push(id);
        ops.extend(mail_ops);
    }
    let ticket = ctx.tickets.issue(crate::tickets::Pending::Service(
        crate::tickets::ServiceTicket::new({
            let ids = ids.clone();
            move |svc: &mut Services, _ctx: &mut GameCtx<'_>, result: TicketResult| {
                if let TicketResult::Failed(e) = result {
                    tracing::error!(error = %e, mails = ?ids, "admin grant: the MailSend batch failed; the mails are forgotten");
                    forget(svc, &ids);
                }
            }
        }),
    ));
    ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ops,
        ..AtomicBatch::of_ops(AtomicKind::MailSend, Vec::new())
    });
    for rec in &records {
        ctx.audit.emit(flyff_db::audit::econ::mail_send(rec, 0));
    }
    Ok(ids.iter().map(|m| u64::from(m.0)).collect())
}

fn forget(svc: &mut Services, ids: &[flyff_common::MailId]) {
    for id in ids {
        svc.econ.mail.remove(*id);
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("").to_owned()
}
