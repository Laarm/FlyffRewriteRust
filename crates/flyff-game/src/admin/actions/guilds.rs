//! Guild administration of the console (Plan/25 §6, §11.5, §11.10; **WP-ADMIN-23**): `GuildCreate`, `GuildEdit`,
//! `GuildDisband`.
//!
//! The three bodies are thin: they build the [`AdminGuildCtx`] (the actor and the accounts the route holds) and call
//! ONLY the trusted-caller API of B8's guild service (`social/guild/ops.rs`, `bank/removal.rs`), which already
//! validates against the live model, changes memory, sends the normal guild messages and writes ONE atomic batch
//! (`AdminGuildCreate` / `AdminGuildEdit`). All three are **deferred** ([`super::defers`]): the answer is sent when
//! the database committed (a [`CommitCb`]), so an offline master can never log in between the model change and the
//! commit (the route keeps its account holds until the answer, Plan/25 §4.6).
//!
//! * `GuildCreate` answers [`AdminOutput::Guild`] with the new id.
//! * `GuildEdit` and `GuildDisband` answer [`AdminOutput::Report`] with one [`TargetResult`] per
//!   [`GuildChange`], whose `info` is [`GuildChange::audit_data`] (the `admin.guild` record data, Plan/25 §10.3). A
//!   repeat that finds the target state is [`Applied::Unchanged`] and answers an empty report at once.
//! * A refusal answers at once with the mapping of `refusal` and changes nothing.
//!
//! **Admin locks** (a ruling of the task spec, B8's trusted path does not check them): a roster edit or the
//! dissolution of a guild that is a bidder of the guild siege (or, with the seams installed, a tenderer of the 1:1
//! guild combat or of the secret room) is refused with `StaleTarget` while the event runs.
//!
//! **The bank.** The guild bank is edited ONLY through `GuildEdit::BankRemove` and `GuildEdit::Gold` (Plan/25 §6.3);
//! `RemoveItem` / `AdjustGold` with a guild-bank location answer `Invalid` naming the guild route.

use super::{TargetId, TargetResult};
use crate::admin::actions::offline::db_failure;
use crate::admin::types::{AdminErrorCode, AdminFailure};
use crate::ctx::{GameCtx, Services};
use crate::input::{AdminActor, AdminOutput};
use crate::social::guild::bank::removal::remove_stack;
use crate::social::guild::model::max_members;
use crate::social::guild::ops::{
    self, AdminGuildCtx, AdminGuildSpec, Applied, CommitCb, GuildActor, GuildChange, GuildCmd,
    GuildRefusal,
};
use crate::tickets::TicketResult;
use flyff_common::rt::Reply;
use flyff_common::{AccountId, CharacterId, GuildId, UnixTime};
use flyff_rules::social::{GUILD_LOGO_MAX, MAX_GUILD_LEVEL, guild_rank_cap};
use std::collections::BTreeSet;

pub use crate::social::guild::GuildRank;

/// What `GuildCreate` builds (25 §11.10 `GuildSpec`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildSpec {
    /// The guild name.
    pub name: String,
    /// The master.
    pub master: CharacterId,
    /// The other members.
    pub members: Vec<(CharacterId, GuildRank)>,
    /// Guild level.
    pub level: u8,
    /// Guild penya.
    pub gold: u64,
    /// Guild contribution points.
    pub pxp: u32,
    /// Logo `0..=27`.
    pub logo: u8,
    /// The notice.
    pub notice: String,
    /// Also give the master the guild cloak: acted on by the route (a `GrantBatch` after the create), not here.
    pub give_cloak: bool,
    /// Ignore the re-creation cool-down.
    pub override_cooldown: bool,
}

/// One edit of a guild (25 §11.10 `GuildEdit`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuildEdit {
    /// Rename.
    Rename(String),
    /// Change the logo.
    Logo(u8),
    /// Change the master.
    Master(CharacterId),
    /// Set the level.
    Level(u8),
    /// Set the contribution points.
    Pxp(u32),
    /// Add (or remove) penya.
    Gold(i64),
    /// Set the notice.
    Notice(String),
    /// Add a member.
    AddMember(CharacterId, GuildRank),
    /// Remove a member.
    RemoveMember(CharacterId),
    /// Change a member's rank.
    Rank(CharacterId, GuildRank),
    /// Take an item out of the guild bank.
    BankRemove {
        /// The slot.
        slot: u8,
        /// The serial the console saw.
        serial: u64,
        /// How many of the stack.
        count: u16,
    },
}

/// Create a guild (`GuildCreate`; answers `AdminOutput::Guild`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildCreate {
    /// The specification.
    pub spec: GuildSpec,
    /// The accounts the route holds (`OnlineRegistry::admin_hold`): the offline characters of these accounts may be
    /// put into the guild. A set, so the recorded command is deterministic.
    pub held: BTreeSet<AccountId>,
}

/// Edit a guild (`GuildEdit`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditGuild {
    /// The guild.
    pub guild: GuildId,
    /// The edit.
    pub edit: GuildEdit,
    /// The accounts the route holds (see [`GuildCreate::held`]).
    pub held: BTreeSet<AccountId>,
}

/// Disband a guild (`GuildDisband`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildDisband {
    /// The guild.
    pub guild: GuildId,
    /// Why (audited).
    pub reason: String,
    /// The accounts the route holds (see [`GuildCreate::held`]).
    pub held: BTreeSet<AccountId>,
}

// ---- the deferred answer ------------------------------------------------------------------------------------------

/// The reply of a deferred command on its way into the commit callback. When the callback is dropped without having
/// answered (the service refused, found the target state reached, or wrote no batch) the reply comes back through
/// `back` (a channel: the game state is owned by one thread, no lock is shared with the callback).
struct Pending {
    reply: Option<Reply<AdminOutput>>,
    back: flume::Sender<Reply<AdminOutput>>,
}

impl Pending {
    fn answer(mut self, out: AdminOutput) {
        if let Some(r) = self.reply.take() {
            let _ = r.send(out);
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(r) = self.reply.take() {
            let _ = self.back.send(r);
        }
    }
}

/// The reply of a deferred command: it goes into the commit callback ([`Answer::callback`]), which answers when the
/// batch committed, or it is answered at once (a refusal, an unchanged repeat) after it came back from the dropped
/// callback. A callback that the service drops unused although the guild changed (an empty op list) is noticed by
/// [`Answer::settle_changed`], so a reply never just vanishes.
struct Answer {
    pending: Option<Pending>,
    returned: flume::Receiver<Reply<AdminOutput>>,
}

impl Answer {
    fn new(reply: Reply<AdminOutput>) -> Answer {
        let (back, returned) = flume::bounded(1);
        Answer {
            pending: Some(Pending {
                reply: Some(reply),
                back,
            }),
            returned,
        }
    }

    /// The reply, wherever it is: not yet given to a callback, or returned by a dropped one.
    fn reclaim(&mut self) -> Option<Reply<AdminOutput>> {
        match self.pending.take() {
            Some(mut p) => p.reply.take(),
            None => self.returned.try_recv().ok(),
        }
    }

    /// Answers at once (a refusal, an unchanged repeat); nothing happens when the callback still owns the reply.
    fn send(&mut self, out: AdminOutput) {
        if let Some(r) = self.reclaim() {
            let _ = r.send(out);
        }
    }

    /// The continuation of the batch: `make` turns its result into the answer. It takes the reply with it.
    fn callback(
        &mut self,
        make: impl FnOnce(&mut Services, &mut GameCtx<'_>, TicketResult) -> AdminOutput + Send + 'static,
    ) -> CommitCb {
        let pending = self.pending.take();
        Box::new(move |svc, ctx, result| {
            let out = make(svc, ctx, result);
            if let Some(p) = pending {
                p.answer(out);
            }
        })
    }

    /// Called once the service reported a change: when the callback came back unused, nothing will ever answer, so the
    /// route gets an internal failure (a change that wrote nothing is a bug) instead of waiting.
    fn settle_changed(&mut self) {
        if let Ok(r) = self.returned.try_recv() {
            tracing::error!(
                "a guild change wrote no batch: the console reply would have been dropped"
            );
            let _ = r.send(AdminOutput::Failed(AdminFailure::new(
                AdminErrorCode::Internal,
                "the guild changed but nothing was written (see the server log)",
            )));
        }
    }
}

/// A value for the commit callback that exists only after the service call returned (the id of the new guild, the
/// changes of an edit): the body sends it into the channel, the callback reads it when the batch committed.
fn handoff<T>() -> (flume::Sender<T>, flume::Receiver<T>) {
    flume::bounded(1)
}

/// The answer of a failed batch: the database's failure plus the B8 convention (the memory stays, the batch is logged
/// at error).
fn batch_failed(result: &TicketResult) -> AdminOutput {
    let f = match result {
        TicketResult::Failed(e) => {
            let mut f = db_failure(e);
            f.message = format!(
                "{} (the guild change stays in memory and the failed batch is in the server log)",
                f.message
            );
            f
        }
        other => AdminFailure::new(
            AdminErrorCode::Internal,
            format!("the database answered unexpectedly: {other:?}"),
        ),
    };
    AdminOutput::Failed(f)
}

/// The report of a committed edit: one target per change.
fn report_of(changes: &[GuildChange]) -> AdminOutput {
    AdminOutput::Report {
        per_target: changes
            .iter()
            .map(|c| TargetResult::ok(TargetId::Guild(c.guild)).with_info(c.audit_data()))
            .collect(),
    }
}

// ---- the refusal mapping ------------------------------------------------------------------------------------------

/// What the texts of a refusal name: the characters of the request, the guild level the limits come from and the
/// rank that was asked for.
struct Why<'a> {
    svc: &'a Services,
    /// The characters the request names (a cooldown or a guild membership is looked up among them).
    chars: Vec<CharacterId>,
    /// The level whose member limit applies.
    level: i32,
    /// The ranks asked for (the first one over its cap is named).
    ranks: Vec<GuildRank>,
    /// `max_members(level)`.
    max: usize,
}

impl<'a> Why<'a> {
    fn new(svc: &'a Services, ctx: &GameCtx<'_>, chars: Vec<CharacterId>, level: i32) -> Why<'a> {
        Why {
            svc,
            chars,
            level,
            ranks: Vec::new(),
            max: max_members(ctx.data, level),
        }
    }

    fn with_ranks(mut self, ranks: Vec<GuildRank>) -> Why<'a> {
        self.ranks = ranks;
        self
    }

    fn name_of(&self, c: CharacterId) -> String {
        self.svc
            .presence
            .get(c)
            .map_or_else(|| format!("#{}", c.0), |e| e.name.clone())
    }
}

/// `YYYY-MM-DD HH:MM UTC` of a unix second (proleptic Gregorian, Howard Hinnant's algorithm).
fn utc_text(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        secs / 3_600,
        secs % 3_600 / 60
    )
}

fn invalid(message: impl Into<String>) -> AdminFailure {
    AdminFailure::new(AdminErrorCode::Invalid, message)
}

fn stale(message: impl Into<String>) -> AdminFailure {
    AdminFailure::new(AdminErrorCode::StaleTarget, message)
}

/// The console's text of a name rule refusal (the keys of `GuildNameCheck`).
pub(crate) fn name_error_text(e: flyff_rules::names::NameError) -> &'static str {
    use flyff_rules::names::NameError;
    match e {
        NameError::Empty => "the name is empty",
        NameError::Length => "a guild name has 3 to 16 characters",
        NameError::LeadingDigit => "a guild name must not start with a digit",
        NameError::InvalidChar => "a guild name may contain letters and digits only",
        NameError::Forbidden => "that name is not allowed",
        #[allow(unreachable_patterns)]
        _ => "the name is not valid",
    }
}

/// Maps a refusal of the guild service to the failure the console shows (Plan/25 §11.10 `ErrorCode`). Every row is
/// unit-tested. The texts name the field (`name`, `master`, `members`, `logo`, `level`, `gold`, `notice`).
fn refusal(r: GuildRefusal, why: &Why<'_>) -> AdminFailure {
    match r {
        GuildRefusal::NotFound => AdminFailure::new(
            AdminErrorCode::NotFound,
            "the guild or a character of the request does not exist",
        ),
        GuildRefusal::NotMember => invalid("character: not a member of the guild"),
        GuildRefusal::Cooldown => {
            let hit = why
                .chars
                .iter()
                .find_map(|c| why.svc.social.guild.rejoin_until(*c).map(|t| (*c, t)));
            match hit {
                Some((c, UnixTime(t))) => invalid(format!(
                    "members: {} is under a rejoin cooldown until {}",
                    why.name_of(c),
                    utc_text(t)
                )),
                None => invalid("members: a character is under a rejoin cooldown"),
            }
        }
        GuildRefusal::Full => invalid(format!(
            "members: level {} allows {} members",
            why.level, why.max
        )),
        GuildRefusal::RankFull => {
            let rank = why
                .ranks
                .iter()
                .copied()
                .find(|r| {
                    why.ranks.iter().filter(|x| *x == r).count()
                        > guild_rank_cap(r.as_u8(), why.max)
                })
                .or_else(|| why.ranks.first().copied());
            match rank {
                Some(rank) => invalid(format!(
                    "members: rank {} is full: cap {}",
                    rank.audit_name(),
                    guild_rank_cap(rank.as_u8(), why.max)
                )),
                None => invalid("members: the rank is full"),
            }
        }
        GuildRefusal::Invalid(what) => invalid(match what {
            // the roster rules name their field (the console puts the error on the wizard's members step)
            "a second master" | "a character twice" => format!("members: {what}"),
            "demote the master with a master change" => format!("character: {what}"),
            _ => format!("{what} is not valid"),
        }),
        GuildRefusal::InvalidName(e) => invalid(format!("name: {}", name_error_text(e))),
        GuildRefusal::NameTaken => stale("name: another guild is called that already"),
        GuildRefusal::InGuild => {
            let hit = why.chars.iter().find_map(|c| {
                why.svc
                    .social
                    .guild
                    .guild_record_of(*c)
                    .map(|g| (*c, g.id, g.name.clone()))
            });
            match hit {
                Some((c, id, name)) => {
                    let guild = if name.is_empty() {
                        format!("#{}", id.0)
                    } else {
                        name
                    };
                    stale(format!("{} is in guild {guild}", why.name_of(c)))
                }
                None => stale("a character is in a guild"),
            }
        }
        GuildRefusal::AtWar => {
            stale("the guild is at war: roster edits and the dissolution are refused")
        }
        GuildRefusal::NeedsHold(c) => AdminFailure::new(
            AdminErrorCode::CharacterOnline,
            format!(
                "the account of character {} is in the world with another character, or logged in or out meanwhile: retry when it is offline",
                why.name_of(c)
            ),
        ),
        GuildRefusal::Offline => AdminFailure::new(
            AdminErrorCode::CharacterOnline,
            "the character is offline and its account is not held",
        ),
        GuildRefusal::Unavailable => AdminFailure::new(
            AdminErrorCode::Internal,
            "the session of a character cannot be saved right now",
        ),
    }
}

// ---- the admin locks ------------------------------------------------------------------------------------------------

/// Which event keeps a guild from losing or gaining members, or from being dissolved (the seams are function pointers
/// that the siege, the GC 1:1 and the secret-room work install; the default is "not locked").
fn invite_lock(svc: &Services, guild: GuildId) -> Option<&'static str> {
    let s = &svc.social.guild.seams;
    if (s.siege_invite_locked)(svc, guild) {
        Some("the guild is a bidder of the guild siege")
    } else if (s.gc1to1_invite_locked)(svc, guild) {
        Some("the guild is a tenderer of the 1:1 guild combat")
    } else if (s.secret_room_invite_locked)(svc, guild) {
        Some("the guild is a tenderer of the secret room")
    } else {
        None
    }
}

fn leave_lock(svc: &Services, guild: GuildId) -> Option<&'static str> {
    let s = &svc.social.guild.seams;
    if (s.siege_leave_locked)(svc, guild) {
        Some("the guild is a bidder of the guild siege")
    } else if (s.gc1to1_leave_locked)(svc, guild) {
        Some("the guild is a tenderer of the 1:1 guild combat")
    } else {
        None
    }
}

fn dissolve_lock(svc: &Services, guild: GuildId) -> Option<&'static str> {
    let s = &svc.social.guild.seams;
    if (s.siege_dissolve_locked)(svc, guild) {
        Some("the guild is a bidder of the guild siege")
    } else if (s.gc1to1_dissolve_locked)(svc, guild) {
        Some("the guild is a tenderer of the 1:1 guild combat")
    } else {
        None
    }
}

fn locked(what: &'static str, action: &str) -> AdminFailure {
    stale(format!("{what}: {action} is refused while the event runs"))
}

// ---- GuildCreate ------------------------------------------------------------------------------------------------------

/// `GuildCreate`: see the module documentation.
pub(crate) fn guild_create(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: GuildCreate,
    reply: Reply<AdminOutput>,
) {
    let mut answer = Answer::new(reply);
    let GuildCreate { spec, held } = cmd;
    let level = i32::from(spec.level);
    let mut chars = vec![spec.master];
    chars.extend(spec.members.iter().map(|(c, _)| *c));
    let mut ranks = vec![GuildRank::Master];
    ranks.extend(spec.members.iter().map(|(_, r)| *r));
    let svc_spec = AdminGuildSpec {
        name: spec.name,
        master: spec.master,
        members: spec.members,
        level: u32::from(spec.level),
        gold: spec.gold,
        pxp: spec.pxp,
        logo: u32::from(spec.logo),
        notice: spec.notice,
        bank: None,
        override_cooldown: spec.override_cooldown,
    };
    let admin = AdminGuildCtx { actor, held };
    let (created, seen) = handoff::<GuildId>();
    let cb = answer.callback(move |_, _, result| match result {
        TicketResult::Committed(_) => match seen.try_recv() {
            Ok(id) => AdminOutput::Guild { id },
            Err(_) => AdminOutput::Failed(AdminFailure::new(
                AdminErrorCode::Internal,
                "the guild was committed without an id",
            )),
        },
        other => batch_failed(&other),
    });
    match ops::create_admin(svc, ctx, svc_spec, &admin, Some(cb)) {
        Ok((id, _changes)) => {
            let _ = created.send(id);
            answer.settle_changed();
        }
        Err(r) => {
            let why = Why::new(svc, ctx, chars, level).with_ranks(ranks);
            answer.send(AdminOutput::Failed(refusal(r, &why)));
        }
    }
}

// ---- GuildEdit --------------------------------------------------------------------------------------------------------

/// The edit as a service command (`BankRemove` is not one).
fn command_of(edit: &GuildEdit) -> Option<GuildCmd> {
    Some(match edit {
        GuildEdit::Rename(n) => GuildCmd::Rename(n.clone()),
        GuildEdit::Logo(l) => GuildCmd::Logo(u32::from(*l)),
        GuildEdit::Master(c) => GuildCmd::Master(*c),
        GuildEdit::Level(l) => GuildCmd::Level(u32::from(*l)),
        GuildEdit::Pxp(p) => GuildCmd::Pxp(*p),
        GuildEdit::Gold(d) => GuildCmd::Gold(*d),
        GuildEdit::Notice(n) => GuildCmd::Notice(n.clone()),
        GuildEdit::AddMember(c, r) => GuildCmd::AddMember(*c, *r),
        GuildEdit::RemoveMember(c) => GuildCmd::RemoveMember(*c),
        GuildEdit::Rank(c, r) => GuildCmd::Rank(*c, *r),
        GuildEdit::BankRemove { .. } => return None,
    })
}

/// The checks of the console that come before the service: the structural limits of a field and the admin locks.
/// A repeat that the service would answer `Unchanged` is never locked.
fn precheck(svc: &Services, guild: GuildId, edit: &GuildEdit) -> Result<(), AdminFailure> {
    let Some(g) = svc.social.guild.get(guild) else {
        return Ok(()); // the service answers NotFound
    };
    match edit {
        GuildEdit::Level(l) if !(1..=MAX_GUILD_LEVEL).contains(&i32::from(*l)) => {
            Err(invalid("level is not valid (1 to 50)"))
        }
        GuildEdit::Logo(l) if u32::from(*l) > GUILD_LOGO_MAX => {
            Err(invalid("logo is not valid (0 to 27)"))
        }
        GuildEdit::AddMember(_, GuildRank::Master) => Err(invalid(
            "members: a second master is not valid; use a master change",
        )),
        GuildEdit::AddMember(c, _) if svc.social.guild.guild_of(*c) != Some(guild) => {
            match invite_lock(svc, guild) {
                Some(what) => Err(locked(what, "adding a member")),
                None => Ok(()),
            }
        }
        GuildEdit::RemoveMember(c) if g.members.contains_key(c) => match leave_lock(svc, guild) {
            Some(what) => Err(locked(what, "removing a member")),
            None => Ok(()),
        },
        _ => Ok(()),
    }
}

/// `GuildEdit`: see the module documentation.
pub(crate) fn guild_edit(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: EditGuild,
    reply: Reply<AdminOutput>,
) {
    let mut answer = Answer::new(reply);
    let EditGuild { guild, edit, held } = cmd;
    if let Err(f) = precheck(svc, guild, &edit) {
        return answer.send(AdminOutput::Failed(f));
    }
    let actor = GuildActor::Admin(AdminGuildCtx { actor, held });
    let level = svc.social.guild.get(guild).map_or(1, |g| g.level);
    let (chars, ranks) = match &edit {
        GuildEdit::AddMember(c, r) | GuildEdit::Rank(c, r) => (vec![*c], vec![*r]),
        GuildEdit::Master(c) | GuildEdit::RemoveMember(c) => (vec![*c], Vec::new()),
        _ => (Vec::new(), Vec::new()),
    };
    // ---- the bank is not a `GuildCmd` ----
    if let GuildEdit::BankRemove {
        slot,
        serial,
        count,
    } = edit
    {
        let (removed, seen) = handoff::<GuildChange>();
        let cb = answer.callback(move |_, _, result| match result {
            TicketResult::Committed(_) => report_of(seen.try_recv().ok().as_slice()),
            other => batch_failed(&other),
        });
        match remove_stack(svc, ctx, guild, slot, serial, count, &actor, Some(cb)) {
            Ok(change) => {
                let _ = removed.send(change);
                answer.settle_changed();
            }
            // a stack that changed or an empty slot: the console's view was stale (the serial convention)
            Err(GuildRefusal::Invalid(what)) => answer.send(AdminOutput::Failed(stale(format!(
                "bank slot {slot}: {what}"
            )))),
            Err(r) => {
                let why = Why::new(svc, ctx, chars, level);
                answer.send(AdminOutput::Failed(refusal(r, &why)));
            }
        }
        return;
    }
    let Some(command) = command_of(&edit) else {
        return answer.send(AdminOutput::Failed(AdminFailure::new(
            AdminErrorCode::Internal,
            "unknown guild edit",
        )));
    };
    let (applied, seen) = handoff::<Vec<GuildChange>>();
    let cb = answer.callback(move |_, _, result| match result {
        TicketResult::Committed(_) => report_of(&seen.try_recv().unwrap_or_default()),
        other => batch_failed(&other),
    });
    match ops::apply(svc, ctx, guild, command, &actor, Some(cb)) {
        Ok(Applied::Changed(changes)) => {
            let _ = applied.send(changes);
            answer.settle_changed();
        }
        Ok(Applied::Unchanged) => answer.send(AdminOutput::Report {
            per_target: Vec::new(),
        }),
        Err(r) => {
            let why = Why::new(svc, ctx, chars, level).with_ranks(ranks);
            answer.send(AdminOutput::Failed(refusal(r, &why)));
        }
    }
}

// ---- GuildDisband -----------------------------------------------------------------------------------------------------

/// `GuildDisband`: see the module documentation.
pub(crate) fn guild_disband(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: GuildDisband,
    reply: Reply<AdminOutput>,
) {
    let mut answer = Answer::new(reply);
    let GuildDisband {
        guild,
        reason: _,
        held,
    } = cmd;
    if svc.social.guild.get(guild).is_some()
        && let Some(what) = dissolve_lock(svc, guild)
    {
        return answer.send(AdminOutput::Failed(locked(what, "the dissolution")));
    }
    let actor = GuildActor::Admin(AdminGuildCtx { actor, held });
    let level = svc.social.guild.get(guild).map_or(1, |g| g.level);
    let (dissolved, seen) = handoff::<GuildChange>();
    let cb = answer.callback(move |_, _, result| match result {
        TicketResult::Committed(_) => report_of(seen.try_recv().ok().as_slice()),
        other => batch_failed(&other),
    });
    match ops::disband(svc, ctx, guild, &actor, Some(cb)) {
        Ok(change) => {
            let _ = dissolved.send(change);
            answer.settle_changed();
        }
        Err(r) => {
            let why = Why::new(svc, ctx, Vec::new(), level);
            answer.send(AdminOutput::Failed(refusal(r, &why)));
        }
    }
}
