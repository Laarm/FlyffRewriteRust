//! The login flow behind `POST /api/login` (Plan/12 §6.3, §14; WP-AUTH-04): the processing order of the
//! table of §6.3 and the claim loop with the duplicate-login takeover. The HTTP handler is in
//! [`super::http`]; everything here is testable without sockets.

use super::accounts::AccountName;
use super::auth::{AuthError, PwHash, Verify};
use super::errors::LobbyErr;
use super::online::{ClaimRequest, ClaimStep, OnlineRegistry};
use super::tokens::SessionToken;
use super::{Availability, LobbyCore};
use crate::audit::Actor;
use axum::http::StatusCode;
use flyff_common::gm::{Authority, AuthorityInputs, resolve_authority};
use flyff_common::{AccountId, UnixTime};
use flyff_db::{AccountBanRecord, DbError, DbErrorKind, DbOp, queries};
use std::net::IpAddr;
use std::time::Duration;
use tokio::time::Instant;

/// `retry_after_ms` of a login whose takeover or enter wait ran out (12 §14 step 3).
pub const TAKEOVER_RETRY_MS: u32 = 2000;
/// `retry_after_ms` of a login that meets an admin hold (the offline edit commits within a moment).
pub const HELD_RETRY_MS: u32 = 1000;
/// How long a forced login waits for a cancelled enter task (12 §7.3 `Entering` + `force`).
pub const ENTER_WAIT: Duration = Duration::from_secs(5);

/// A validated login request.
#[derive(Clone, Debug)]
pub struct LoginRequest {
    /// `account` as typed.
    pub account: String,
    /// `pwhash`: 32 lowercase hex characters.
    pub pwhash: PwHash,
    /// `force`.
    pub force: bool,
    /// The client address (`http::client_ip`).
    pub ip: IpAddr,
}

/// What a successful login returns (12 §6.3 "Response 200").
#[derive(Debug)]
pub struct LoginOk {
    /// The session token, shown once.
    pub token: SessionToken,
    /// The normalised account name.
    pub account: AccountName,
    /// The account id.
    pub account_id: AccountId,
    /// The account-level authority (UI gating only).
    pub authority: Authority,
    /// `auth.token_grace_s`.
    pub token_grace_s: u32,
    /// Server wall clock, milliseconds.
    pub server_time_ms: i64,
}

/// Clears the takeover marker when the login stops waiting for any reason, including the HTTP request being
/// dropped (12 §14 step 3 "drop guard").
struct TakeoverGuard<'a> {
    registry: &'a OnlineRegistry,
    account: AccountId,
    id: u64,
}

impl Drop for TakeoverGuard<'_> {
    fn drop(&mut self) {
        // after a successful claim the marker is already gone: this is then a no-op
        self.registry.cancel_takeover(self.account, self.id);
    }
}

/// `s` cut to at most `max` bytes on a character boundary.
fn truncate_bytes(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("")
}

fn db_err(e: &DbError) -> LobbyErr {
    tracing::error!("login: database error: {e}");
    match e.kind {
        DbErrorKind::ShuttingDown | DbErrorKind::Busy | DbErrorKind::SaveFailed => {
            LobbyErr::internal(StatusCode::SERVICE_UNAVAILABLE)
        }
        _ => LobbyErr::internal(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

fn auth_err(e: AuthError) -> LobbyErr {
    match e {
        AuthError::PermitTimeout(_) => LobbyErr::overflow(),
        other => {
            tracing::error!("login: {other}");
            LobbyErr::internal(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn ban_detail(ban: &AccountBanRecord) -> String {
    match ban.until {
        None => "permanent".to_owned(),
        Some(UnixTime(t)) => chrono::DateTime::from_timestamp(t, 0).map_or_else(
            || t.to_string(),
            |d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ),
    }
}

impl LobbyCore {
    /// `POST /api/login` without the HTTP layer: the first failing step of 12 §6.3 answers.
    pub async fn login(&self, req: LoginRequest) -> Result<LoginOk, LobbyErr> {
        let r = self.login_steps(&req).await;
        let label = match &r {
            Ok(_) => "ok".to_owned(),
            Err(e) => e.code.to_string(),
        };
        self.stats.login(&label);
        let ip = req.ip.to_string();
        match &r {
            Ok(ok) => self.audit.emit(
                "account.login",
                &Actor::account(ok.account_id, Some(ip.clone())),
                serde_json::json!({"account": ok.account.as_str(), "ip": ip, "forced": req.force}),
            ),
            Err(e) => self.audit.emit(
                "account.login_failed",
                &Actor::ip(&ip),
                serde_json::json!({
                    // as typed, at most 32 bytes (12 §19.1)
                    "account": truncate_bytes(&req.account, 32),
                    "ip": ip,
                    "code": e.code,
                }),
            ),
        }
        r
    }

    async fn login_steps(&self, req: &LoginRequest) -> Result<LoginOk, LobbyErr> {
        let s = self.settings();
        // 2: shutdown in progress or persistence degraded
        if self.availability() != Availability::Open {
            return Err(LobbyErr::unavailable());
        }
        // 3: the wrong-password lock of this address
        let now = self.clock.unix();
        self.throttle
            .check(req.ip, now.0)
            .map_err(|l| LobbyErr::throttled(l.long, l.retry_after_ms))?;
        // 4: the name rule (no database lookup)
        let name = AccountName::parse(&req.account).map_err(|_| LobbyErr::unknown_account())?;
        // 5: the account
        let wanted = name.as_str().to_owned();
        let record = self
            .persist
            .read(move |c| queries::find_account(c, &wanted))
            .await
            .map_err(|e| db_err(&e))?
            .ok_or_else(LobbyErr::unknown_account)?;
        // 6: the password (permit wait <= 10 s)
        let verdict = self
            .hasher
            .verify(&req.pwhash, &record.password_hash)
            .await
            .map_err(auth_err)?;
        let needs_rehash = match verdict {
            Verify::Mismatch => {
                // only wrong passwords count towards the throttle (KEEP)
                self.throttle.fail(req.ip, now.0);
                return Err(LobbyErr::wrong_password());
            }
            Verify::Ok { needs_rehash } => needs_rehash,
        };
        // 7
        self.throttle.success(req.ip);
        if needs_rehash {
            self.rehash(record.id, &req.pwhash).await;
        }
        // 8: a banned account (after the password: a wrong password on a banned account still answers 120)
        let id = record.id;
        let ban_now = self.clock.unix();
        if let Some(ban) = self
            .persist
            .read(move |c| queries::active_ban(c, id, ban_now))
            .await
            .map_err(|e| db_err(&e))?
        {
            return Err(LobbyErr::banned(&ban_detail(&ban)));
        }
        // the login authority (12 §5.9): no character yet
        let authority = resolve_authority(
            &s.gm,
            s.servers_authorize,
            &AuthorityInputs {
                account_db: Authority::from_db_byte(record.authority),
                character_db: None,
                account_name: name.as_str(),
                character_name: None,
                peer_ip: req.ip,
            },
        );
        // 9: maintenance (12 §5.8)
        if s.auth.maintenance
            && !authority.at_least(Authority::GAMEMASTER)
            && !s.maintenance_allow.contains(req.ip)
        {
            return Err(LobbyErr::unavailable());
        }
        // 10 + 11: capacity and the claim (may wait for a flush or a takeover)
        let token = self
            .claim_loop(&ClaimRequest {
                account: record.id,
                name: &name,
                authority,
                ip: req.ip,
                force: req.force,
                max_accounts: s.auth.max_online_accounts,
                takeover: None,
            })
            .await?;
        // 12
        self.persist.op(DbOp::RecordLogin {
            account_id: record.id,
            at: self.clock.unix(),
            ip: req.ip.to_string(),
        });
        Ok(LoginOk {
            token,
            account: name,
            account_id: record.id,
            authority,
            token_grace_s: s.auth.token_grace_s,
            server_time_ms: self.clock.unix_ms(),
        })
    }

    /// The transparent upgrade of a stored hash whose parameters differ from the constants (12 §5.2).
    pub(crate) async fn rehash(&self, account: AccountId, pw: &PwHash) {
        match self.hasher.hash(pw).await {
            Ok(phc) => {
                self.persist.op(DbOp::SetAccountPassword {
                    account_id: account,
                    password_hash: phc,
                });
            }
            Err(e) => tracing::warn!(account = account.0, "rehash skipped: {e}"),
        }
    }

    /// The claim of 12 §7.3 / §14, repeated after every wait until a token is issued or the login is refused.
    async fn claim_loop(&self, base: &ClaimRequest<'_>) -> Result<SessionToken, LobbyErr> {
        let s = self.settings();
        let takeover_wait = Duration::from_secs(u64::from(s.auth.takeover_wait_s));
        let mut req = base.clone();
        let mut guard: Option<TakeoverGuard<'_>> = None;
        let mut deadline: Option<Instant> = None;
        let account = base.account;
        // what the forced login displaced, for the `session.takeover` audit event (12 §19.1)
        let started = Instant::now();
        let mut old_state: Option<&'static str> = None;
        let audit_takeover = |old: &'static str, result: &str| {
            self.audit.emit(
                "session.takeover",
                &Actor::account(account, Some(base.ip.to_string())),
                serde_json::json!({
                    "account": base.name.as_str(),
                    "old_state": old,
                    "waited_ms": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    "result": result,
                }),
            );
        };
        loop {
            match self.registry.claim(&req, &self.persist) {
                ClaimStep::Issued { token, actions } => {
                    // a forced login that closed a character-select socket displaced nothing else
                    if old_state.is_none()
                        && req.force
                        && actions
                            .iter()
                            .any(|a| matches!(a, super::online::RegistryAction::CloseSocket { .. }))
                    {
                        old_state = Some("char_select");
                    }
                    self.run_actions(actions);
                    if deadline.is_some() {
                        self.stats.takeover("ok");
                    }
                    if let Some(old) = old_state {
                        audit_takeover(old, "ok");
                    }
                    drop(guard);
                    return Ok(token);
                }
                // an admin offline edit of one of the account's characters is committing (25 §4.6): retry
                ClaimStep::Held => {
                    return Err(LobbyErr::db_job_in_progress().with_retry(HELD_RETRY_MS));
                }
                ClaimStep::Duplicate => return Err(LobbyErr::duplicate()),
                ClaimStep::Overflow => return Err(LobbyErr::overflow()),
                ClaimStep::NoRandom => {
                    return Err(LobbyErr::internal(StatusCode::INTERNAL_SERVER_ERROR));
                }
                ClaimStep::WaitFlush { save_gen } => {
                    let mut wait = s.flush_wait;
                    if let Some(d) = deadline {
                        wait = wait.min(d.saturating_duration_since(Instant::now()));
                    }
                    let rx = self.persist.wait_flushed(account, save_gen);
                    match tokio::time::timeout(wait, rx.recv_async()).await {
                        Ok(Ok(Ok(()))) => {}
                        Ok(Ok(Err(e))) => {
                            tracing::error!(
                                account = account.0,
                                "login: waiting for the logout save: {e}"
                            );
                            return Err(LobbyErr::internal(StatusCode::SERVICE_UNAVAILABLE));
                        }
                        Ok(Err(_)) => {
                            return Err(LobbyErr::internal(StatusCode::SERVICE_UNAVAILABLE));
                        }
                        Err(_) => {
                            if deadline.is_some_and(|d| d <= Instant::now()) {
                                self.stats.takeover("timeout");
                                audit_takeover(old_state.unwrap_or("in_world"), "timeout");
                                return Err(LobbyErr::duplicate().with_retry(TAKEOVER_RETRY_MS));
                            }
                            return Err(LobbyErr::db_job_in_progress());
                        }
                    }
                }
                ClaimStep::WaitEnter { actions } => {
                    old_state = Some("entering");
                    self.run_actions(actions);
                    let left = self
                        .registry
                        .wait_for(account, ENTER_WAIT, |v| {
                            v.is_none_or(|v| v.kind != super::online::EntryKind::Entering)
                        })
                        .await;
                    if !left {
                        audit_takeover("entering", "timeout");
                        return Err(LobbyErr::duplicate().with_retry(TAKEOVER_RETRY_MS));
                    }
                }
                ClaimStep::WaitTakeover { id, actions } => {
                    old_state = Some("in_world");
                    if guard.is_none() {
                        guard = Some(TakeoverGuard {
                            registry: &self.registry,
                            account,
                            id,
                        });
                        deadline = Some(Instant::now() + takeover_wait);
                    }
                    req.takeover = Some(id);
                    self.run_actions(actions);
                    let remaining = deadline.map_or(takeover_wait, |d| {
                        d.saturating_duration_since(Instant::now())
                    });
                    let left = self
                        .registry
                        .wait_for(account, remaining, |v| {
                            v.is_none_or(|v| v.kind != super::online::EntryKind::InWorld)
                        })
                        .await;
                    if !left {
                        self.stats.takeover("timeout");
                        audit_takeover("in_world", "timeout");
                        // the guard clears the marker; the old avatar still finishes its leave normally
                        return Err(LobbyErr::duplicate().with_retry(TAKEOVER_RETRY_MS));
                    }
                }
            }
        }
    }
}
