//! The first-start administrator (Plan/12 §5.4, D5; WP-AUTH-05): on the first start of a fresh database an
//! account `admin` with authority `P` and a random 12-character one-time password is created, the flag
//! `server_kv["admin.bootstrap_done"]` is set in the same batch, and the notice is written to
//! `logs/first-start.txt` (mode 0600) for the console. The password never goes through `tracing`. The account is
//! created with `must_change_password` (Plan/25 §1.3): the admin console asks for a new password at the first staff
//! sign-in, and the banner names the `/admin` URL.

use super::accounts::{AccountAdmin, AdminError};
use super::auth::legacy_pwhash;
use flyff_common::UnixTime;
use flyff_common::gm::Authority;
use flyff_db::{
    AccountListFilter, AtomicBatch, AtomicKind, AtomicOutcome, CreatedVia, DbError, DbErrorKind,
    DbOp, NewAccountRecord, PersistHandle, queries, repo::meta,
};
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The name of the first-start account.
pub const ADMIN_NAME: &str = "admin";
/// The `server_kv` key that records that the bootstrap ran.
pub const BOOTSTRAP_KEY: &str = "admin.bootstrap_done";
/// The password alphabet: no `0 O 1 l I` look-alikes.
pub const PASSWORD_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";
/// The generated password length (12 <= 15, so the login form accepts it).
pub const PASSWORD_LEN: usize = 12;
/// The notice file below the log directory.
pub const NOTICE_FILE: &str = "first-start.txt";

/// What the operator sees once.
#[derive(Clone, PartialEq, Eq)]
pub struct BootstrapNotice {
    /// `admin`.
    pub account: String,
    /// The one-time password.
    pub password: String,
    /// The banner (also the content of the file).
    pub text: String,
    /// The file the banner was written to.
    pub file: PathBuf,
}

impl std::fmt::Debug for BootstrapNotice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootstrapNotice")
            .field("account", &self.account)
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}

/// `len` characters of [`PASSWORD_ALPHABET`] from the OS CSPRNG (rejection sampling: no modulo bias).
pub fn random_password(len: usize) -> Result<String, getrandom::Error> {
    let n = PASSWORD_ALPHABET.len();
    let limit = 256 - (256 % n);
    let mut out = String::with_capacity(len);
    let mut buf = [0u8; 32];
    while out.len() < len {
        getrandom::fill(&mut buf)?;
        for &b in &buf {
            if usize::from(b) < limit
                && let Some(&c) = PASSWORD_ALPHABET.get(usize::from(b) % n)
            {
                out.push(char::from(c));
                if out.len() == len {
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// The banner of 12 §5.4. `login_url` is where the operator logs in (for example `http://192.168.0.5:8080/`).
pub fn banner(password: &str, login_url: &str) -> String {
    format!(
        "================================ FLYFF FIRST START ================================\n\
         An administrator account was created.\n    \
         account : {ADMIN_NAME}\n    \
         password: {password}\n\
         Log in at {login_url} and change the password with:\n    \
         ./flyff account passwd {ADMIN_NAME}\n\
         The admin console is at {login_url}admin (its first staff sign-in asks for a new password).\n\
         This password is shown only once (copy kept in logs/first-start.txt, readable by the owner only).\n\
         ===================================================================================\n"
    )
}

/// Writes `bytes` to `path` readable by the owner only (mode 0600), replacing an older copy.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()
}

/// The bootstrap of 12 §5.4 for threads outside the async runtime: the start-up sequence calls it after the
/// database is open and before the HTTP bind (S8, `wiring::first_start`).
///
/// 1. `server_kv["admin.bootstrap_done"] == true` -> `Ok(None)`.
/// 2. Otherwise, when `accounts` is empty: `admin` (authority `P`, `created_via = 'bootstrap'`, a random
///    password hashed over `legacy_pwhash(md5_salt, password)`) is created in the same batch that sets the flag;
///    the notice is written to `<logs_dir>/first-start.txt` and returned.
/// 3. When accounts already exist (for example created earlier by the offline CLI) only the flag is set.
pub fn run_blocking(
    persist: &PersistHandle,
    md5_salt: &str,
    now: UnixTime,
    logs_dir: &Path,
    login_url: &str,
) -> Result<Option<BootstrapNotice>, AdminError> {
    let done = persist
        .read_blocking(|c| meta::get_kv(c, BOOTSTRAP_KEY))?
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if done {
        return Ok(None);
    }
    let any_account = !persist
        .read_blocking(|c| {
            queries::list_accounts(
                c,
                &AccountListFilter {
                    name_substr: None,
                    gm_only: false,
                    limit: 1,
                },
                UnixTime(0),
            )
        })?
        .is_empty();
    let flag = DbOp::SetServerKv {
        key: BOOTSTRAP_KEY.to_owned(),
        value: serde_json::Value::Bool(true),
    };
    if any_account {
        persist.call_blocking(AtomicBatch::of_ops(
            AtomicKind::Other("bootstrap.flag"),
            vec![flag],
        ))?;
        return Ok(None);
    }

    let password = random_password(PASSWORD_LEN).map_err(|e| {
        AdminError::Db(DbError::new(
            DbErrorKind::Bug,
            format!("OS random generator: {e}"),
        ))
    })?;
    let pwhash = legacy_pwhash(md5_salt, &password);
    let phc = flyff_db::password::hash_pwhash(pwhash.as_str())?;
    // 25 §1.3: the admin console demands a new password at the first staff sign-in (the game login is unaffected)
    let mut rec = NewAccountRecord::new(ADMIN_NAME, phc, now, CreatedVia::Bootstrap)
        .with_must_change_password();
    rec.authority = Authority::ADMINISTRATOR.as_u8();
    match persist.call_blocking(AtomicBatch::of_ops(
        AtomicKind::Other("account.create"),
        vec![DbOp::CreateAccount(rec), flag],
    ))? {
        AtomicOutcome::AccountCreated(_) => {}
        other => {
            return Err(AdminError::Db(DbError::new(
                DbErrorKind::Bug,
                format!("bootstrap batch answered {other:?}"),
            )));
        }
    }
    let text = banner(&password, login_url);
    let file = logs_dir.join(NOTICE_FILE);
    if let Err(e) = write_private(&file, text.as_bytes()) {
        // the account exists; the password is still on the console
        tracing::error!(file = %file.display(), "cannot write the first-start notice: {e}");
    }
    tracing::info!(
        file = %file.display(),
        "first start: administrator account 'admin' created (one-time password in the file and on the console)"
    );
    Ok(Some(BootstrapNotice {
        account: ADMIN_NAME.to_owned(),
        password,
        text,
        file,
    }))
}

/// [`run_blocking`] for async callers (tests, the admin tooling): the hash and the database waits run on the
/// blocking pool.
pub async fn run(
    admin: &AccountAdmin,
    logs_dir: &Path,
    login_url: &str,
) -> Result<Option<BootstrapNotice>, AdminError> {
    let persist = admin.persist().clone();
    let salt = admin.md5_salt().to_owned();
    let now = admin.clock().unix();
    let logs_dir = logs_dir.to_owned();
    let login_url = login_url.to_owned();
    tokio::task::spawn_blocking(move || run_blocking(&persist, &salt, now, &logs_dir, &login_url))
        .await
        .map_err(|e| {
            AdminError::Db(DbError::new(
                DbErrorKind::Bug,
                format!("bootstrap task: {e}"),
            ))
        })?
}
