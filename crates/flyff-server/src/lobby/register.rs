//! `POST /api/register` (Plan/12 §6.5; WP-AUTH-10; F-048, PD-05, PL-039): web self-registration with a
//! per-IP sliding-window limit. No CAPTCHA and no e-mail verification (LAN-first default; internet-facing
//! operators set `auth.allow_registration = false` and use the CLI).

use super::accounts::{AccountAdmin, AccountName, AdminError, CreatedVia, NewAccount, valid_email};
use super::auth::{PwHash, ip_key, legacy_pwhash};
use super::errors::LobbyErr;
use super::{Availability, LobbyCore};
use flyff_common::gm::Authority;
use parking_lot::Mutex;
use std::collections::{BTreeMap, VecDeque};
use std::net::IpAddr;

/// The window of `auth.register_per_ip_per_hour`.
pub const WINDOW_S: i64 = 3600;
/// Hard cap on tracked addresses.
const MAX_ENTRIES: usize = 100_000;

/// The sliding window of registration times per client address (12 §6.5 step 4). A slot is **reserved** before
/// the (slow) hash and account creation so that concurrent requests cannot overshoot the limit, and given
/// back when the registration did not create an account.
#[derive(Debug, Default)]
pub struct RegisterLimiter {
    map: Mutex<BTreeMap<IpAddr, VecDeque<i64>>>,
}

/// A reserved slot ([`RegisterLimiter::reserve`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reservation {
    ip: IpAddr,
    at: i64,
    counted: bool,
}

impl RegisterLimiter {
    /// An empty limiter.
    pub fn new() -> RegisterLimiter {
        RegisterLimiter::default()
    }

    /// Reserves a slot for `ip` at `now` (Unix seconds). `limit = 0` is unlimited. `Err(retry_after_ms)` when
    /// the window already holds `limit` registrations.
    pub fn reserve(&self, ip: IpAddr, now: i64, limit: u32) -> Result<Reservation, u32> {
        let key = ip_key(ip);
        if limit == 0 {
            return Ok(Reservation {
                ip: key,
                at: now,
                counted: false,
            });
        }
        let mut map = self.map.lock();
        if map.len() >= MAX_ENTRIES && !map.contains_key(&key) {
            map.retain(|_, q| q.back().is_some_and(|t| now - *t < WINDOW_S));
        }
        let q = map.entry(key).or_default();
        while q.front().is_some_and(|t| now - *t >= WINDOW_S) {
            q.pop_front();
        }
        if q.len() >= usize::try_from(limit).unwrap_or(usize::MAX) {
            let oldest = q.front().copied().unwrap_or(now);
            let wait_s = (oldest + WINDOW_S - now).max(1);
            return Err(u32::try_from(wait_s.saturating_mul(1000)).unwrap_or(u32::MAX));
        }
        q.push_back(now);
        Ok(Reservation {
            ip: key,
            at: now,
            counted: true,
        })
    }

    /// Gives a reserved slot back (the registration created no account).
    pub fn refund(&self, r: Reservation) {
        if !r.counted {
            return;
        }
        let mut map = self.map.lock();
        if let Some(q) = map.get_mut(&r.ip) {
            if let Some(pos) = q.iter().rposition(|t| *t == r.at) {
                q.remove(pos);
            }
            if q.is_empty() {
                map.remove(&r.ip);
            }
        }
    }

    /// Drops the windows without a registration in the last hour (the sweeper).
    pub fn prune(&self, now: i64) {
        self.map
            .lock()
            .retain(|_, q| q.back().is_some_and(|t| now - *t < WINDOW_S));
    }

    /// Tracked addresses (tests).
    pub fn len(&self) -> usize {
        self.map.lock().len()
    }

    /// `true` when no address is tracked.
    pub fn is_empty(&self) -> bool {
        self.map.lock().is_empty()
    }
}

/// A parsed `POST /api/register` body plus the client address.
#[derive(Clone, Debug)]
pub struct RegisterRequest {
    /// `account` as typed.
    pub account: String,
    /// `pwhash`: 32 lowercase hex characters (checked by the caller: `400 / 2000` otherwise).
    pub pwhash: PwHash,
    /// `email`, optional.
    pub email: Option<String>,
    /// The client address (`http::client_ip`).
    pub ip: IpAddr,
}

impl LobbyCore {
    /// The checks of 12 §6.5 in order; returns the normalised account name. `admin` creates the account
    /// (`created_via = 'web'`, authority `F`, `registration_ip`).
    pub async fn register(
        &self,
        admin: &AccountAdmin,
        req: RegisterRequest,
    ) -> Result<String, LobbyErr> {
        let r = self.register_inner(admin, &req).await;
        let label = match &r {
            Ok(_) => "ok".to_owned(),
            Err(e) => e.code.to_string(),
        };
        self.stats.register(&label);
        r
    }

    async fn register_inner(
        &self,
        admin: &AccountAdmin,
        req: &RegisterRequest,
    ) -> Result<String, LobbyErr> {
        let s = self.settings();
        // 2
        if !s.auth.allow_registration {
            return Err(LobbyErr::registration_disabled());
        }
        // 3
        if self.availability() != Availability::Open {
            return Err(LobbyErr::unavailable());
        }
        if s.auth.maintenance && !s.maintenance_allow.contains(req.ip) {
            return Err(LobbyErr::unavailable());
        }
        // 4
        let now = self.clock.unix().0;
        let slot = self
            .register_limiter
            .reserve(req.ip, now, s.auth.register_per_ip_per_hour)
            .map_err(LobbyErr::registration_rate_limited)?;
        let result = self
            .register_checked(admin, req, &s.auth.legacy_md5_salt)
            .await;
        if result.is_err() {
            self.register_limiter.refund(slot);
        }
        result
    }

    async fn register_checked(
        &self,
        admin: &AccountAdmin,
        req: &RegisterRequest,
        salt: &str,
    ) -> Result<String, LobbyErr> {
        // 5
        let name =
            AccountName::parse(&req.account).map_err(|_| LobbyErr::invalid_account_name())?;
        // 6
        if req.pwhash == legacy_pwhash(salt, "") {
            return Err(LobbyErr::invalid_password());
        }
        if let Some(mail) = &req.email
            && !mail.is_empty()
            && !valid_email(mail)
        {
            return Err(LobbyErr::bad_request("invalid e-mail address"));
        }
        // 7
        let email = req.email.as_deref().filter(|m| !m.is_empty());
        admin
            .create_full(NewAccount {
                name: name.as_str(),
                pw: &req.pwhash,
                authority: Authority::GENERAL,
                via: CreatedVia::Web,
                ip: Some(req.ip),
                email,
            })
            .await
            .map_err(admin_to_lobby)?;
        Ok(name.as_str().to_owned())
    }
}

fn admin_to_lobby(e: AdminError) -> LobbyErr {
    match e {
        AdminError::Exists => LobbyErr::account_exists(),
        AdminError::InvalidName => LobbyErr::invalid_account_name(),
        AdminError::InvalidPwHash => LobbyErr::invalid_password(),
        AdminError::InvalidEmail => LobbyErr::bad_request("invalid e-mail address"),
        AdminError::Hash(super::auth::AuthError::PermitTimeout(_)) => LobbyErr::overflow(),
        other => {
            tracing::error!("registration failed: {other}");
            LobbyErr::internal(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
