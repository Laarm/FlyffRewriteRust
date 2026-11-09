//! Session tokens (Plan/12 §7.4; WP-AUTH-03): 256 random bits from the OS CSPRNG, shown once in the login
//! response (base64url without padding, 43 characters), stored only as `blake3(token)`.
//!
//! A token is never put in a URL, a cookie or a log line (01 §11.3): [`SessionToken`]'s `Debug` and
//! [`TokenHash`]'s `Debug` print no value. The browser keeps the token in `sessionStorage`.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use flyff_common::AccountId;
use std::collections::HashMap;

/// Token length in bytes.
pub const TOKEN_BYTES: usize = 32;
/// Length of the base64url text of a token.
pub const TOKEN_TEXT_LEN: usize = 43;

/// A session token (the secret the browser sends in `Hello.token`).
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken([u8; TOKEN_BYTES]);

/// `blake3(token bytes)`: the registry indexes tokens by hash only, so a lookup has no timing side channel on
/// the secret.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TokenHash([u8; 32]);

impl SessionToken {
    /// 32 fresh bytes from the OS CSPRNG.
    pub fn generate() -> Result<SessionToken, getrandom::Error> {
        let mut bytes = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes)?;
        Ok(SessionToken(bytes))
    }

    /// The token of a base64url text (`Hello.token`); `None` for anything that is not exactly 43 characters of
    /// the alphabet.
    pub fn parse(text: &str) -> Option<SessionToken> {
        if text.len() != TOKEN_TEXT_LEN {
            return None;
        }
        let bytes = URL_SAFE_NO_PAD.decode(text.as_bytes()).ok()?;
        let arr: [u8; TOKEN_BYTES] = bytes.try_into().ok()?;
        // canonical form only: the last character carries 4 unused bits that must be zero
        let token = SessionToken(arr);
        (token.to_text() == text).then_some(token)
    }

    /// The 43-character base64url text.
    pub fn to_text(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// `blake3` of the bytes.
    pub fn hash(&self) -> TokenHash {
        TokenHash(*blake3::hash(&self.0).as_bytes())
    }
}

impl std::fmt::Display for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_text())
    }
}

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken(\u{2026})")
    }
}

impl std::fmt::Debug for TokenHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenHash(\u{2026})")
    }
}

/// `TokenHash -> AccountId` of the live tokens (at most one per account; the registry enforces it).
#[derive(Debug, Default)]
pub struct SessionTokens {
    by_hash: HashMap<TokenHash, AccountId>,
}

impl SessionTokens {
    /// An empty table.
    pub fn new() -> SessionTokens {
        SessionTokens::default()
    }

    /// Creates a token for `account`, stores its hash and returns the secret (shown once).
    pub fn issue(
        &mut self,
        account: AccountId,
    ) -> Result<(SessionToken, TokenHash), getrandom::Error> {
        loop {
            let token = SessionToken::generate()?;
            let hash = token.hash();
            // 2^-256: a collision is impossible in practice, but a duplicate must never alias two accounts
            if let std::collections::hash_map::Entry::Vacant(slot) = self.by_hash.entry(hash) {
                slot.insert(account);
                return Ok((token, hash));
            }
        }
    }

    /// The account of a live token.
    pub fn lookup(&self, hash: &TokenHash) -> Option<AccountId> {
        self.by_hash.get(hash).copied()
    }

    /// Removes a token; `true` when it was live.
    pub fn revoke(&mut self, hash: &TokenHash) -> bool {
        self.by_hash.remove(hash).is_some()
    }

    /// Number of live tokens.
    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    /// `true` when no token is live.
    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }

    /// `true` when `hash` is live for exactly `account` (invariant checks).
    pub fn is_live_for(&self, hash: &TokenHash, account: AccountId) -> bool {
        self.lookup(hash) == Some(account)
    }
}
