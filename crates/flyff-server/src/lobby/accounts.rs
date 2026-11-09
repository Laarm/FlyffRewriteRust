//! Accounts: the name rule and the `AccountAdmin` service (Plan/12 §3.1, §5; WP-AUTH-02, WP-AUTH-05).
//!
//! [`AccountAdmin`] is the single path by which accounts are created, changed, listed and banned. It is used by
//! the first-start administrator ([`super::bootstrap_admin`]), `POST /api/register`, the offline CLI, the
//! admin-API account routes (WP-AUTH-19) and doc 23's console words. Every write goes through the persistence
//! actor of the process that holds the instance lock (D5 single writer).

use super::auth::{AuthError, PasswordHasher, PwHash, legacy_pwhash};
use super::online::LobbyClock;
use crate::admin::account_events::{AccountEventKind, AccountEvents, EventSource};
use crate::audit::{Actor, AuditFile};
use flyff_common::gm::{Authority, AuthorityInputs, GmConfig, grant_overrides};
use flyff_common::{AccountId, CharacterId, UnixTime};
pub use flyff_db::CreatedVia;
use flyff_db::model::{AccountListFilter, AccountRecord};
use flyff_db::{
    AccountBanRecord, AtomicBatch, AtomicKind, AtomicOutcome, DbError, DbErrorKind, DbOp,
    NewAccountRecord, PersistHandle, queries,
};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

/// Shortest account name.
pub const ACCOUNT_NAME_MIN: usize = 3;
/// Longest account name (legacy `IsAcValid`, `src:_Network/CmnHdr.h:308-352`).
pub const ACCOUNT_NAME_MAX: usize = 16;
/// Shortest password (client side only, `IsPwdValid`).
pub const PASSWORD_MIN: usize = 3;
/// Longest password in UTF-8 bytes (client side only).
pub const PASSWORD_MAX: usize = 15;

/// A normalised account name: trimmed, ASCII-lower-cased, `^[a-z][a-z0-9]{2,15}$` (12 §3.1). The only account
/// name rule; it applies to every creation path and to every lookup by name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AccountName(String);

/// The text is not a valid account name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("account names are 3 to 16 characters: a letter, then letters and digits")]
pub struct InvalidAccountName;

impl AccountName {
    /// Trims ASCII whitespace, lower-cases ASCII and checks `^[a-z][a-z0-9]{2,15}$`.
    pub fn parse(raw: &str) -> Result<AccountName, InvalidAccountName> {
        let name = raw
            .trim_matches(|c: char| c.is_ascii_whitespace())
            .to_ascii_lowercase();
        let bytes = name.as_bytes();
        if bytes.len() < ACCOUNT_NAME_MIN || bytes.len() > ACCOUNT_NAME_MAX {
            return Err(InvalidAccountName);
        }
        if !bytes.first().is_some_and(u8::is_ascii_lowercase) {
            return Err(InvalidAccountName);
        }
        if !bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return Err(InvalidAccountName);
        }
        Ok(AccountName(name))
    }

    /// The normalised text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AccountName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for AccountName {
    type Err = InvalidAccountName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AccountName::parse(s)
    }
}

/// What every operation of [`AccountAdmin`] can fail with (12 §5.6).
#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    /// No such account, or the named character does not belong to it (CLI exit 2 / HTTP 404).
    #[error("not found")]
    NotFound,
    /// The account name is taken (exit 2 / 409).
    #[error("the account already exists")]
    Exists,
    /// `unban` without an active ban (exit 2 / 404).
    #[error("the account has no active ban")]
    NoActiveBan,
    /// The name breaks the account-name rule (exit 1 / 400).
    #[error("invalid account name")]
    InvalidName,
    /// The authority level is not one the tools write (exit 1 / 400): `Z` and the undefined letters.
    #[error("invalid authority level")]
    InvalidLevel,
    /// The password hash is not 32 hex characters, or is the hash of the empty password (exit 1 / 400).
    #[error("invalid password")]
    InvalidPwHash,
    /// The ban end is not in the future (exit 1 / 400).
    #[error("the ban end must be in the future")]
    InvalidUntil,
    /// The e-mail address is malformed (exit 1 / 400).
    #[error("invalid e-mail address")]
    InvalidEmail,
    /// Persistence failed (exit 3 / 500).
    #[error("database error: {0}")]
    Db(#[from] DbError),
    /// Hashing failed (exit 3 / 500; `PermitTimeout` is 503).
    #[error("password hashing failed: {0}")]
    Hash(AuthError),
}

impl AdminError {
    /// The exit code of `flyff account ...` (12 §5.5): 1 invalid value, 2 not found / exists, 3 database error.
    pub fn exit_code(&self) -> u8 {
        match self {
            AdminError::InvalidName
            | AdminError::InvalidLevel
            | AdminError::InvalidPwHash
            | AdminError::InvalidUntil
            | AdminError::InvalidEmail => 1,
            AdminError::NotFound | AdminError::Exists | AdminError::NoActiveBan => 2,
            AdminError::Db(_) | AdminError::Hash(_) => 3,
        }
    }

    /// The HTTP status of the admin-API routes (12 §5.5): 400, 404, 409, 500.
    pub fn http_status(&self) -> u16 {
        match self {
            AdminError::InvalidName
            | AdminError::InvalidLevel
            | AdminError::InvalidPwHash
            | AdminError::InvalidUntil
            | AdminError::InvalidEmail => 400,
            AdminError::NotFound | AdminError::NoActiveBan => 404,
            AdminError::Exists => 409,
            AdminError::Hash(AuthError::PermitTimeout(_)) => 503,
            AdminError::Db(_) | AdminError::Hash(_) => 500,
        }
    }

    /// The `error` token of the admin-API body (12 §5.5).
    pub fn token(&self) -> &'static str {
        match self {
            AdminError::NotFound => "not_found",
            AdminError::Exists => "exists",
            AdminError::NoActiveBan => "no_active_ban",
            AdminError::InvalidName => "invalid_name",
            AdminError::InvalidLevel => "invalid_level",
            AdminError::InvalidPwHash => "invalid_pwhash",
            AdminError::InvalidUntil => "invalid_until",
            AdminError::InvalidEmail => "invalid_email",
            AdminError::Db(_) | AdminError::Hash(_) => "db",
        }
    }
}

impl From<AuthError> for AdminError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::Hash(d) => AdminError::Db(d),
            other => AdminError::Hash(other),
        }
    }
}

/// Where an account stands in the registry (`flyff account list --online`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnlineState {
    /// Not logged in (always, in offline mode).
    Offline,
    /// Logged in at character select.
    CharSelect,
    /// Entering or in the world.
    InWorld,
}

/// An account ban as the list shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BanUntil {
    /// Until this time.
    Until(UnixTime),
    /// Permanent.
    Permanent,
}

/// One row of `flyff account list` / `GET /admin/api/accounts` (12 §5.6).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AccountSummary {
    /// Account id.
    pub id: AccountId,
    /// Name.
    pub name: String,
    /// Authority letter.
    pub authority: char,
    /// Creation time.
    pub created_at: UnixTime,
    /// `cli`, `web` or `bootstrap`.
    pub created_via: String,
    /// Last successful login.
    pub last_login_at: Option<UnixTime>,
    /// Address of the last successful login.
    pub last_login_ip: Option<String>,
    /// The active ban: an end time (Unix seconds), or `"permanent"`.
    #[serde(serialize_with = "ser_ban")]
    pub banned_until: Option<BanUntil>,
    /// Active characters.
    pub characters: u8,
    /// Online state.
    pub online: OnlineState,
}

fn ser_ban<S: serde::Serializer>(v: &Option<BanUntil>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        None => s.serialize_none(),
        Some(BanUntil::Permanent) => s.serialize_str("permanent"),
        Some(BanUntil::Until(t)) => s.serialize_i64(t.0),
    }
}

/// The filter of [`AccountAdmin::list`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountFilter {
    /// Case-insensitive substring of the name.
    pub name_substr: Option<String>,
    /// Only accounts whose stored authority is `L` or higher.
    pub gm_only: bool,
    /// Only accounts with a registry entry in `CharSelect` or `InWorld`.
    pub online_only: bool,
    /// Maximum rows (default 1000), ordered by id.
    pub limit: u32,
}

impl Default for AccountFilter {
    fn default() -> Self {
        Self {
            name_substr: None,
            gm_only: false,
            online_only: false,
            limit: 1000,
        }
    }
}

/// The effects of an account operation on a running server (the online registry and the game thread). The
/// offline CLI has none.
pub trait OnlineEffects: Send + Sync {
    /// A ban was committed: revoke the token, kick the sockets and the avatar (12 §5.7).
    fn account_banned(&self, account: AccountId);
    /// A ban was lifted: clear the cached flag.
    fn account_unbanned(&self, account: AccountId);
    /// An authority changed in the database: re-resolve the in-world character (23 §1.4).
    fn authority_changed(
        &self,
        account: AccountId,
        character: Option<CharacterId>,
        authority: Authority,
    );
    /// The registry state of an account.
    fn online_state(&self, account: AccountId) -> OnlineState;
}

/// The `[gm]` grant list and `features.servers_authorize`, to tell "overridden by \[gm\] config" (12 §5.5).
#[derive(Clone, Debug, Default)]
pub struct GmPolicy {
    /// `[gm]`.
    pub cfg: Arc<GmConfig>,
    /// `features.servers_authorize`.
    pub servers_authorize: bool,
}

/// A new account (12 §5.3).
#[derive(Clone, Debug)]
pub struct NewAccount<'a> {
    /// The name (any case; normalised by the rule of §3.1).
    pub name: &'a str,
    /// The client-side hash of the password.
    pub pw: &'a PwHash,
    /// The authority letter.
    pub authority: Authority,
    /// The creation path.
    pub via: CreatedVia,
    /// The registering address.
    pub ip: Option<IpAddr>,
    /// Optional e-mail (<= 100 bytes, contains `@`, no whitespace).
    pub email: Option<&'a str>,
}

/// The account service (12 §5.6).
#[derive(Clone)]
pub struct AccountAdmin {
    persist: PersistHandle,
    hasher: PasswordHasher,
    clock: LobbyClock,
    md5_salt: String,
    gm: GmPolicy,
    online: Option<Arc<dyn OnlineEffects>>,
    audit: Option<Arc<AuditFile>>,
    events: Option<AccountEvents>,
}

/// How a password change treats the rest of the world (25 §1.3, §1.8).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PasswordOptions {
    /// Sets `accounts.must_change_password` (an administrator's reset: the owner of the account must pick a
    /// password of their own at the next admin-console login); the default clears it (any password change does).
    pub must_change: bool,
    /// The public handle of the admin session that keeps running (the one that changed its own password); every
    /// other admin session of the account ends.
    pub keep_session: Option<String>,
}

/// The e-mail rule shared with the registration route: <= 100 bytes, contains `@`, no whitespace.
pub fn valid_email(email: &str) -> bool {
    email.len() <= 100
        && email.contains('@')
        && !email.chars().any(char::is_whitespace)
        && !email.starts_with('@')
        && !email.ends_with('@')
}

/// `true` for the letters the tools may write: the defined levels `D F G H J L M N O P` (the legacy test value
/// `Z` is accepted from the database and `[gm]` only).
pub fn writable_authority(a: Authority) -> bool {
    a.short_name().is_some() && a != Authority::LEGACY_TEST
}

impl AccountAdmin {
    /// A service over `persist`. `md5_salt` is `auth.legacy_md5_salt` (to refuse the hash of the empty
    /// password); `online` is `None` in offline mode.
    pub fn new(
        persist: PersistHandle,
        hasher: PasswordHasher,
        clock: LobbyClock,
        md5_salt: impl Into<String>,
        gm: GmPolicy,
        online: Option<Arc<dyn OnlineEffects>>,
    ) -> AccountAdmin {
        AccountAdmin {
            persist,
            hasher,
            clock,
            md5_salt: md5_salt.into(),
            gm,
            online,
            audit: None,
            events: None,
        }
    }

    /// Publishes bans, authority changes and password changes of an account to the admin console's
    /// [`AccountEvents`], which ends the admin sessions of the account (25 §1.8).
    pub fn with_events(mut self, events: AccountEvents) -> AccountAdmin {
        self.events = Some(events);
        self
    }

    fn publish(&self, account: AccountId, kind: AccountEventKind, keep_session: Option<String>) {
        if let Some(e) = &self.events {
            e.publish(crate::admin::account_events::AccountEvent {
                account,
                kind,
                source: EventSource::Admin,
                keep_session,
            });
        }
    }

    /// Emits the `account.*` audit events (12 §19.1) to `audit` (the server's appender, or the CLI's own in
    /// offline mode).
    pub fn with_audit(mut self, audit: Arc<AuditFile>) -> AccountAdmin {
        self.audit = Some(audit);
        self
    }

    fn emit(
        &self,
        ev: &'static str,
        account: Option<AccountId>,
        ip: Option<IpAddr>,
        data: serde_json::Value,
    ) {
        if let Some(a) = &self.audit {
            a.emit(
                ev,
                &Actor {
                    account,
                    ip: ip.map(|i| i.to_string()),
                    ..Actor::default()
                },
                data,
            );
        }
    }

    /// The persistence handle.
    pub fn persist(&self) -> &PersistHandle {
        &self.persist
    }

    /// The hasher (shared permits).
    pub fn hasher(&self) -> &PasswordHasher {
        &self.hasher
    }

    /// The lobby clock.
    pub fn clock(&self) -> &LobbyClock {
        &self.clock
    }

    /// `auth.legacy_md5_salt`.
    pub fn md5_salt(&self) -> &str {
        &self.md5_salt
    }

    async fn find(&self, name: &str) -> Result<AccountRecord, AdminError> {
        let name = AccountName::parse(name).map_err(|_| AdminError::InvalidName)?;
        let wanted = name.as_str().to_owned();
        self.persist
            .read(move |c| queries::find_account(c, &wanted))
            .await?
            .ok_or(AdminError::NotFound)
    }

    fn check_pw(&self, pw: &PwHash) -> Result<(), AdminError> {
        if *pw == legacy_pwhash(&self.md5_salt, "") {
            Err(AdminError::InvalidPwHash)
        } else {
            Ok(())
        }
    }

    /// Creates an account (12 §5.3): one atomic batch `CreateAccount`; a taken name (any letter case) is
    /// [`AdminError::Exists`].
    pub async fn create(
        &self,
        name: &str,
        pw: &PwHash,
        auth: Authority,
        via: CreatedVia,
        ip: Option<IpAddr>,
    ) -> Result<AccountId, AdminError> {
        self.create_full(NewAccount {
            name,
            pw,
            authority: auth,
            via,
            ip,
            email: None,
        })
        .await
    }

    /// [`AccountAdmin::create`] with the optional e-mail address.
    pub async fn create_full(&self, new: NewAccount<'_>) -> Result<AccountId, AdminError> {
        let by = new.via.as_str();
        self.create_as(new, by).await
    }

    /// [`AccountAdmin::create_full`] with an explicit `by` for the audit event (`cli`, `admin_api`, `web`,
    /// `bootstrap`; the admin API passes what the caller sent).
    pub async fn create_as(&self, new: NewAccount<'_>, by: &str) -> Result<AccountId, AdminError> {
        let name = AccountName::parse(new.name).map_err(|_| AdminError::InvalidName)?;
        if !writable_authority(new.authority) {
            return Err(AdminError::InvalidLevel);
        }
        self.check_pw(new.pw)?;
        if let Some(mail) = new.email
            && !valid_email(mail)
        {
            return Err(AdminError::InvalidEmail);
        }
        let phc = self.hasher.hash(new.pw).await?;
        let mut rec = NewAccountRecord::new(name.as_str(), phc, self.clock.unix(), new.via);
        rec.authority = new.authority.as_u8();
        rec.registration_ip = new.ip.map(|ip| ip.to_string());
        rec.email = new.email.map(str::to_owned);
        let outcome = self
            .persist
            .call(AtomicBatch::of_ops(
                AtomicKind::Other("account.create"),
                vec![DbOp::CreateAccount(rec)],
            ))
            .await;
        match outcome {
            Ok(AtomicOutcome::AccountCreated(id)) => {
                tracing::info!(account = %name, via = new.via.as_str(), "account created");
                self.emit(
                    "account.create",
                    Some(id),
                    new.ip,
                    serde_json::json!({"account": name.as_str(), "by": by, "ip": new.ip.map(|i| i.to_string())}),
                );
                Ok(id)
            }
            Ok(other) => Err(AdminError::Db(DbError::new(
                DbErrorKind::Bug,
                format!("CreateAccount answered {other:?}"),
            ))),
            Err(e) if e.kind == DbErrorKind::AccountExists => Err(AdminError::Exists),
            Err(e) => Err(AdminError::Db(e)),
        }
    }

    /// Replaces the password; online sessions are not kicked (the next login uses the new password). Any password
    /// change clears `accounts.must_change_password` and ends the account's admin sessions (25 §1.3, §1.8).
    pub async fn set_password(&self, name: &str, pw: &PwHash, by: &str) -> Result<(), AdminError> {
        self.set_password_with(name, pw, by, PasswordOptions::default())
            .await
    }

    /// [`AccountAdmin::set_password`] with the options of [`PasswordOptions`]: an administrator's reset sets
    /// `must_change_password`, a change of one's own password keeps the session it was made from.
    pub async fn set_password_with(
        &self,
        name: &str,
        pw: &PwHash,
        by: &str,
        opts: PasswordOptions,
    ) -> Result<(), AdminError> {
        let account = self.find(name).await?;
        self.check_pw(pw)?;
        let phc = self.hasher.hash(pw).await?;
        self.persist
            .call(AtomicBatch::of_ops(
                AtomicKind::Other("account.password"),
                vec![
                    DbOp::SetAccountPassword {
                        account_id: account.id,
                        password_hash: phc,
                    },
                    DbOp::SetMustChangePassword {
                        account_id: account.id,
                        value: opts.must_change,
                    },
                ],
            ))
            .await
            .map_err(not_found_or_db)?;
        tracing::info!(account = %account.name, by, "account password changed");
        self.emit(
            "account.password",
            Some(account.id),
            None,
            serde_json::json!({"account": &account.name, "by": by, "must_change": opts.must_change}),
        );
        self.publish(account.id, AccountEventKind::Password, opts.keep_session);
        Ok(())
    }

    /// Sets the authority of the account, or of one of its characters. Returns whether a `[gm]` grant
    /// overrides the new value (12 §5.5 prints the warning "overridden by \[gm\] config"). A running server
    /// re-resolves the in-world character at once ([`OnlineEffects::authority_changed`]).
    pub async fn set_authority(
        &self,
        name: &str,
        auth: Authority,
        character: Option<&str>,
        by: &str,
    ) -> Result<bool, AdminError> {
        if !writable_authority(auth) {
            return Err(AdminError::InvalidLevel);
        }
        let account = self.find(name).await?;
        let mut character_id = None;
        let mut character_name = None;
        let op = match character {
            None => DbOp::SetAccountAuthority {
                account_id: account.id,
                authority: auth.as_u8(),
            },
            Some(wanted) => {
                let id = account.id;
                let list = self
                    .persist
                    .read(move |c| queries::character_list(c, id))
                    .await?;
                let found = list
                    .into_iter()
                    .find(|c| c.name.eq_ignore_ascii_case(wanted.trim()))
                    .ok_or(AdminError::NotFound)?;
                character_id = Some(found.id);
                character_name = Some(found.name);
                DbOp::SetCharacterAuthority {
                    character_id: found.id,
                    authority: auth.as_u8(),
                }
            }
        };
        self.persist
            .call(AtomicBatch::of_ops(
                AtomicKind::Other("account.authority"),
                vec![op],
            ))
            .await
            .map_err(not_found_or_db)?;
        tracing::info!(account = %account.name, by, authority = %auth, "authority changed");
        self.emit(
            "account.authority",
            Some(account.id),
            None,
            serde_json::json!({
                "account": &account.name,
                "by": by,
                "authority": auth.letter().to_string(),
                "character": &character_name,
            }),
        );
        if let Some(online) = &self.online {
            online.authority_changed(account.id, character_id, auth);
        }
        if character_id.is_none() {
            // the staff letter is the account-level one (25 §1.4): the admin sessions end
            self.publish(account.id, AccountEventKind::Authority, None);
        }
        let (account_db, character_db) = match character_id {
            None => (auth, None),
            Some(_) => (Authority::from_db_byte(account.authority), Some(auth)),
        };
        let overridden = grant_overrides(
            &self.gm.cfg,
            self.gm.servers_authorize,
            &AuthorityInputs {
                account_db,
                character_db,
                account_name: &account.name,
                character_name: character_name.as_deref(),
                // an address grant cannot be evaluated without a client
                peer_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            },
        );
        Ok(overridden)
    }

    /// The account list (12 §5.5 `list`).
    pub async fn list(&self, filter: AccountFilter) -> Result<Vec<AccountSummary>, AdminError> {
        let now = self.clock.unix();
        let online_only = filter.online_only && self.online.is_some();
        if filter.online_only && self.online.is_none() {
            return Ok(Vec::new());
        }
        let db_filter = AccountListFilter {
            name_substr: filter.name_substr.clone().filter(|s| !s.is_empty()),
            gm_only: filter.gm_only,
            // the online filter is applied afterwards: fetch every match
            limit: if online_only { u32::MAX } else { filter.limit },
        };
        let rows = self
            .persist
            .read(move |c| queries::list_accounts(c, &db_filter, now))
            .await?;
        let mut out = Vec::new();
        for row in rows {
            let online = self
                .online
                .as_ref()
                .map_or(OnlineState::Offline, |o| o.online_state(row.id));
            if online_only && online == OnlineState::Offline {
                continue;
            }
            out.push(AccountSummary {
                id: row.id,
                name: row.name,
                authority: Authority::from_db_byte(row.authority).letter(),
                created_at: row.created_at,
                created_via: row.created_via.as_str().to_owned(),
                last_login_at: row.last_login_at,
                last_login_ip: row.last_login_ip,
                banned_until: row.banned_until.map(|u| match u {
                    Some(t) => BanUntil::Until(t),
                    None => BanUntil::Permanent,
                }),
                characters: u8::try_from(row.characters).unwrap_or(u8::MAX),
                online,
            });
            if out.len() >= usize::try_from(filter.limit).unwrap_or(usize::MAX) {
                break;
            }
        }
        Ok(out)
    }

    /// Bans the account until `until` (`None` = permanent). A running server revokes the token and kicks the
    /// sockets and the avatar (12 §5.7). `until` must lie in the future.
    pub async fn ban(
        &self,
        name: &str,
        until: Option<UnixTime>,
        reason: &str,
        by: &str,
    ) -> Result<(), AdminError> {
        let account = self.find(name).await?;
        let now = self.clock.unix();
        if until.is_some_and(|u| u <= now) {
            return Err(AdminError::InvalidUntil);
        }
        let ban = AccountBanRecord {
            id: 0,
            account_id: account.id,
            created_at: now,
            until,
            reason: reason.chars().take(255).collect(),
            created_by: by.to_owned(),
            lifted_at: None,
            lifted_by: None,
        };
        self.persist
            .call(AtomicBatch::of_ops(
                AtomicKind::Other("account.ban"),
                vec![DbOp::InsertAccountBan(ban)],
            ))
            .await
            .map_err(not_found_or_db)?;
        tracing::info!(account = %account.name, by, ?until, "account banned");
        self.emit(
            "account.ban",
            Some(account.id),
            None,
            serde_json::json!({
                "account": &account.name,
                "by": by,
                "until": until.map(|u| u.0),
                "reason": reason.chars().take(255).collect::<String>(),
            }),
        );
        if let Some(online) = &self.online {
            online.account_banned(account.id);
        }
        self.publish(account.id, AccountEventKind::Banned, None);
        Ok(())
    }

    /// Lifts every active ban of the account; [`AdminError::NoActiveBan`] when there is none.
    pub async fn unban(&self, name: &str, by: &str) -> Result<(), AdminError> {
        let account = self.find(name).await?;
        let mut lifted = 0usize;
        // `active_ban` answers the strongest active ban: lift it and ask again
        for _ in 0..64 {
            let id = account.id;
            let now = self.clock.unix();
            let Some(ban) = self
                .persist
                .read(move |c| queries::active_ban(c, id, now))
                .await?
            else {
                break;
            };
            self.persist
                .call(AtomicBatch::of_ops(
                    AtomicKind::Other("account.unban"),
                    vec![DbOp::LiftAccountBan {
                        ban_id: ban.id,
                        at: now,
                        by: by.to_owned(),
                    }],
                ))
                .await?;
            lifted += 1;
        }
        if lifted == 0 {
            return Err(AdminError::NoActiveBan);
        }
        tracing::info!(account = %account.name, by, lifted, "account unbanned");
        self.emit(
            "account.unban",
            Some(account.id),
            None,
            serde_json::json!({"account": &account.name, "by": by, "lifted": lifted}),
        );
        if let Some(online) = &self.online {
            online.account_unbanned(account.id);
        }
        Ok(())
    }
}

fn not_found_or_db(e: DbError) -> AdminError {
    if e.kind == DbErrorKind::NotFound {
        AdminError::NotFound
    } else {
        AdminError::Db(e)
    }
}
