//! Identities of the admin console and their capabilities (Plan/25 §1.1, §1.4, §1.6; WP-ADMIN-01).
//!
//! | identity | how it gets in | acts as | audit actor |
//! |---|---|---|---|
//! | staff | `POST /admin/api/session {account, pwhash}` and the `flyff_admin` cookie | its account-level authority letter `>= L` | `staff:<account>` |
//! | owner (token) | `POST /admin/api/session {token}` and a cookie, or `Authorization: Bearer` | `ADMINISTRATOR` | `owner:token` |
//! | owner (local) | a loopback request without a valid cookie (`admin.owner_loopback`) | `ADMINISTRATOR` | `owner:local` |
//!
//! * [`Identity`] is what a request resolved to; [`Identity::actor`] is the [`AdminActor`] the game thread sees.
//! * [`StaffResolver`] computes the authority of a staff session with the rule of 23 §1.3 and `character_db = None`;
//!   `[gm]` grants with an `ip` selector never apply (a freshly registered account from that address would
//!   otherwise get the letter).
//! * [`CapTable`] holds the minimum letter of every capability (`caps.toml` plus the `[admin.capabilities]`
//!   overrides); [`Identity::has`] decides. The owners hold every capability.

use super::caps::Cap;
use super::dto::IdentityKind;
use super::error::ApiError;
use flyff_common::AccountId;
use flyff_common::gm::{Authority, AuthorityInputs, GmConfig, resolve_authority};
use flyff_game::input::{ActorName, AdminActor, OwnerVia};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

/// The lowest authority a staff session may have (`L`, 25 §1.1).
pub const STAFF_MIN: Authority = Authority::GAMEMASTER;

/// The pseudo account id under which the asset cookies of owner sessions are issued (25 §11.7). Account ids start
/// at 1, so it never names an account.
pub const OWNER_ASSET_KEY: AccountId = AccountId(0);

/// The staff account behind a staff identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaffAccount {
    /// Account id.
    pub id: AccountId,
    /// Normalised account name.
    pub name: String,
}

/// The cookie session an identity came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRef {
    /// The public handle of the session (not the cookie): what `DELETE /admin/api/admin-sessions/{id}` and the
    /// WebSocket's `SessionEnded` use.
    pub id: String,
}

/// Who a request is (25 §1.1). Built by [`super::auth::AdminAuth`] for every authenticated request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// The kind.
    pub kind: IdentityKind,
    /// The staff account (`None` for the owners).
    pub account: Option<StaffAccount>,
    /// The authority letter the identity acts with (`P` for the owners).
    pub authority: Authority,
    /// The client address of this request.
    pub ip: IpAddr,
    /// The request comes from the host itself (loopback, no forwarding headers, local `Host`).
    pub local: bool,
    /// The request came over TLS.
    pub tls: bool,
    /// The cookie session (`None` for the local owner and for bearer calls).
    pub session: Option<SessionRef>,
    /// The password must be changed before anything else (25 §1.3).
    pub must_change_password: bool,
}

impl Identity {
    /// The implicit owner of a loopback request.
    pub fn owner_local(ip: IpAddr, tls: bool) -> Identity {
        Identity {
            kind: IdentityKind::OwnerLocal,
            account: None,
            authority: Authority::ADMINISTRATOR,
            ip,
            local: true,
            tls,
            session: None,
            must_change_password: false,
        }
    }

    /// An owner that presented the token (bearer header or exchanged for a cookie session).
    pub fn owner_token(
        ip: IpAddr,
        local: bool,
        tls: bool,
        session: Option<SessionRef>,
    ) -> Identity {
        Identity {
            kind: IdentityKind::OwnerToken,
            account: None,
            authority: Authority::ADMINISTRATOR,
            ip,
            local,
            tls,
            session,
            must_change_password: false,
        }
    }

    /// A staff member of `account` acting with `authority`.
    pub fn staff(
        account: StaffAccount,
        authority: Authority,
        ip: IpAddr,
        local: bool,
        tls: bool,
        session: Option<SessionRef>,
        must_change_password: bool,
    ) -> Identity {
        Identity {
            kind: IdentityKind::Staff,
            account: Some(account),
            authority,
            ip,
            local,
            tls,
            session,
            must_change_password,
        }
    }

    /// An identity for tests and tools: a staff member with `authority`.
    pub fn test_staff(name: &str, id: u32, authority: Authority) -> Identity {
        Identity::staff(
            StaffAccount {
                id: AccountId(id),
                name: name.to_owned(),
            },
            authority,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            true,
            false,
            None,
            false,
        )
    }

    /// `true` for the two owner identities (exempt from rank protection and from the capability table).
    pub fn is_owner(&self) -> bool {
        !matches!(self.kind, IdentityKind::Staff)
    }

    /// The audit actor of 25 §1.1: `staff:<account>`, `owner:token` or `owner:local`.
    pub fn label(&self) -> String {
        match (&self.kind, &self.account) {
            (IdentityKind::Staff, Some(a)) => format!("staff:{}", a.name),
            (IdentityKind::OwnerToken, _) => "owner:token".to_owned(),
            (IdentityKind::OwnerLocal, _) => "owner:local".to_owned(),
            (IdentityKind::Staff, None) => "staff:?".to_owned(),
        }
    }

    /// The key of the per-identity limits (mutation bucket, streaming permit).
    pub fn limit_key(&self) -> String {
        match (&self.kind, &self.account) {
            (IdentityKind::Staff, Some(a)) => format!("staff:{}", a.id.0),
            _ => self.label(),
        }
    }

    /// The actor the game thread sees (`GameInput::Admin.actor`, replay-visible).
    pub fn actor(&self) -> AdminActor {
        match (&self.kind, &self.account) {
            (IdentityKind::Staff, Some(a)) => AdminActor::Staff {
                account_id: a.id,
                account: ActorName::new(&a.name),
                authority: self.authority,
                remote: self.ip,
            },
            (IdentityKind::OwnerLocal, _) => AdminActor::Owner {
                via: OwnerVia::Local,
                remote: self.ip,
            },
            _ => AdminActor::Owner {
                via: OwnerVia::Token,
                remote: self.ip,
            },
        }
    }

    /// May the identity use `cap` under `table`? The owners may use everything.
    pub fn has(&self, table: &CapTable, cap: Cap) -> bool {
        self.is_owner() || table.allows(self.authority, cap)
    }

    /// `403 forbidden` unless the identity holds `cap` (the finer check inside a handler whose route the catalogue
    /// opens with a coarser capability).
    pub fn require(&self, table: &CapTable, cap: Cap) -> Result<(), ApiError> {
        if self.has(table, cap) {
            Ok(())
        } else {
            Err(ApiError::forbidden(cap))
        }
    }

    /// The capabilities of the identity (wire names, table order).
    pub fn caps(&self, table: &CapTable) -> Vec<&'static str> {
        Cap::ALL
            .iter()
            .filter(|c| self.has(table, **c))
            .map(|c| c.name())
            .collect()
    }
}

// ---- the capability table ------------------------------------------------------------------------------------------

/// The minimum authority letter of every capability: the defaults of `caps.toml` with the `[admin.capabilities]`
/// overrides applied (25 §1.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapTable {
    min: [Authority; Cap::ALL.len()],
}

impl Default for CapTable {
    fn default() -> Self {
        CapTable {
            min: Cap::ALL.map(Cap::default_min),
        }
    }
}

impl CapTable {
    /// Applies the overrides `name -> letter` of `[admin.capabilities]`. Every problem is reported (unknown
    /// capability, unparsable letter, a letter below `L`).
    pub fn with_overrides(overrides: &BTreeMap<String, String>) -> Result<CapTable, Vec<String>> {
        let mut table = CapTable::default();
        let mut problems = Vec::new();
        for (name, level) in overrides {
            let Some(cap) = Cap::parse(name) else {
                problems.push(format!("'{name}' is not a capability of the admin console"));
                continue;
            };
            match Authority::parse_token(level) {
                Ok(a) if a >= STAFF_MIN => {
                    if let Some(slot) = table.min.get_mut(cap as usize) {
                        *slot = a;
                    }
                }
                Ok(_) => problems.push(format!(
                    "'{name}': the letter must be L or higher, not '{level}'"
                )),
                Err(e) => problems.push(format!("'{name}': {e}")),
            }
        }
        if problems.is_empty() {
            Ok(table)
        } else {
            Err(problems)
        }
    }

    /// The minimum letter of `cap`.
    pub fn min(&self, cap: Cap) -> Authority {
        self.min
            .get(cap as usize)
            .copied()
            .unwrap_or(Authority::LEGACY_TEST)
    }

    /// Does `authority` hold `cap`?
    pub fn allows(&self, authority: Authority, cap: Cap) -> bool {
        authority.at_least(self.min(cap))
    }

    /// The capabilities an authority letter holds (wire names).
    pub fn caps_for(&self, authority: Authority) -> Vec<&'static str> {
        Cap::ALL
            .iter()
            .filter(|c| self.allows(authority, **c))
            .map(|c| c.name())
            .collect()
    }
}

// ---- authority resolution ------------------------------------------------------------------------------------------

/// Resolves the authority of staff sessions and of rank-protection targets (25 §1.4).
#[derive(Clone, Debug)]
pub struct StaffResolver {
    /// `[gm]` without the grants that carry an `ip` selector.
    gm: Arc<GmConfig>,
    servers_authorize: bool,
}

impl StaffResolver {
    /// Builds the resolver from the `[gm]` list: `ip` grants are dropped.
    pub fn new(gm: &GmConfig, servers_authorize: bool) -> StaffResolver {
        StaffResolver {
            gm: Arc::new(GmConfig {
                grants: gm
                    .grants
                    .iter()
                    .filter(|g| g.ip.is_none())
                    .cloned()
                    .collect(),
            }),
            servers_authorize,
        }
    }

    /// A resolver without any grant (tests, hand-built servers).
    pub fn plain() -> StaffResolver {
        StaffResolver {
            gm: Arc::new(GmConfig::default()),
            servers_authorize: false,
        }
    }

    fn resolve(
        &self,
        account_db: Authority,
        character_db: Option<Authority>,
        account_name: &str,
        character_name: Option<&str>,
    ) -> Authority {
        resolve_authority(
            &self.gm,
            self.servers_authorize,
            &AuthorityInputs {
                account_db,
                character_db,
                account_name,
                character_name,
                peer_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            },
        )
    }

    /// The authority of a staff session: 23 §1.3 with `character_db = None` and `character_name = None`, without the
    /// `ip` grants.
    pub fn staff_authority(&self, account_db: Authority, account_name: &str) -> Authority {
        self.resolve(account_db, None, account_name, None)
    }

    /// The authority of a rank-protection target (an account, or one of its characters): the same rule with the
    /// character's own letter and name when the target is a character.
    pub fn target_authority(
        &self,
        account_db: Authority,
        character_db: Option<Authority>,
        account_name: &str,
        character_name: Option<&str>,
    ) -> Authority {
        self.resolve(account_db, character_db, account_name, character_name)
    }
}
