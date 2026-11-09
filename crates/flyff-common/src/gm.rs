//! GM authority, mode flags and the `[gm]` grant list (Plan/23 §1-§2, WP-GM-01; doc 12 §5.9 calls it).
//!
//! * [`Authority`]: one `AUTH_*` letter, compared **numerically** like the legacy `IsAuthHigher`
//!   (`src:_Common/Mover.h:1116`). `D < F < G < H < J < L < M < N < O < P < Z`.
//! * [`ModeFlags`]: `CMover::m_dwMode` (`src:_Common/authorization.h:20-60`), every bit and combo of 23 §2.1.
//! * [`GmConfig`] / [`GmGrant`]: the `[gm]` section that replaces `Program/Authorization.ini` (D5, PL-036).
//! * [`resolve_authority`]: the one resolution rule (23 §1.3), called by the lobby at `/api/login` (no
//!   character) and at every enter world (the chosen character); [`validate_gm_config`] runs in the config
//!   loader.
//!
//! The module is wasm-safe (no I/O, no clocks): the generated TypeScript and the browser's command layer
//! share the constants.

use core::net::IpAddr;
use serde::{Deserialize, Serialize};

/// One `AUTH_*` letter. The ordering is the numeric byte ordering (legacy `IsAuthHigher`).
///
/// The byte is public so that code written against the walking-skeleton placeholder (`Authority(76)`) keeps
/// compiling; use the named constants and [`Authority::at_least`] in new code.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Authority(pub u8);

impl Authority {
    /// `AUTH_OBSERVER` `'D'`: monitoring account; ranks **below** a player, no commands.
    pub const OBSERVER: Self = Self(b'D');
    /// `AUTH_GENERAL` `'F'`: the default.
    pub const GENERAL: Self = Self(b'F');
    /// `AUTH_LOGCHATTING` `'G'`: a player whose chat is audited (23 §6.12).
    pub const LOG_CHATTING: Self = Self(b'G');
    /// `AUTH_JOURNALIST` `'H'`: "\[Staff\]" label, sees players' HP bars (client).
    pub const JOURNALIST: Self = Self(b'H');
    /// `AUTH_HELPER` `'J'`: exempt from the chaotic item drop on death.
    pub const HELPER: Self = Self(b'J');
    /// `AUTH_GAMEMASTER` `'L'`: command tier 1.
    pub const GAMEMASTER: Self = Self(b'L');
    /// `AUTH_GAMEMASTER2` `'M'`: command tier 2.
    pub const GAMEMASTER2: Self = Self(b'M');
    /// `AUTH_GAMEMASTER3` `'N'`: command tier 3.
    pub const GAMEMASTER3: Self = Self(b'N');
    /// `AUTH_OPERATOR` `'O'`: **KEEP-QUIRK (KQ-24)**: ranks below `P`, so it cannot run administrator commands.
    pub const OPERATOR: Self = Self(b'O');
    /// `AUTH_ADMINISTRATOR` `'P'`: everything.
    pub const ADMINISTRATOR: Self = Self(b'P');
    /// The legacy test value `'Z'` (`- Database/2. Account.sql`): numerically above `P`, so it passes every
    /// check. Accepted from the database and from `[gm]`; the account tools never write it.
    pub const LEGACY_TEST: Self = Self(b'Z');

    /// The authority with this raw byte (wire value, `HelloAck.authority`, `PlayerPublic.authorization`).
    pub const fn from_u8(byte: u8) -> Self {
        Self(byte)
    }

    /// The authority of a database byte (`accounts.authority` / `characters.authority` hold one ASCII
    /// letter): a letter is upper-cased and kept raw (so `'Z'` stays `'Z'`), anything else is `None`.
    pub const fn try_from_db_byte(byte: u8) -> Option<Self> {
        if byte.is_ascii_alphabetic() {
            Some(Self(byte.to_ascii_uppercase()))
        } else {
            None
        }
    }

    /// [`Authority::try_from_db_byte`] with the fallback of [`Authority::from_db`] (`GENERAL`).
    pub const fn from_db_byte(byte: u8) -> Self {
        match Self::try_from_db_byte(byte) {
            Some(a) => a,
            None => Self::GENERAL,
        }
    }

    /// `None` unless `s` (surrounding ASCII whitespace ignored) is exactly one ASCII letter: callers that
    /// want the "warning in the server log" of 23 §1.2 use this and log on `None`.
    pub fn try_from_db(s: &str) -> Option<Self> {
        match s.trim().as_bytes() {
            [b] => Self::try_from_db_byte(*b),
            _ => None,
        }
    }

    /// DB `TEXT(1)` to value (23 §1.2): an ASCII letter is upper-cased and kept raw, anything else becomes
    /// [`Authority::GENERAL`] (use [`Authority::try_from_db`] to learn about the fallback and log it).
    pub fn from_db(s: &str) -> Self {
        Self::try_from_db(s).unwrap_or(Self::GENERAL)
    }

    /// The database text: one upper-case letter.
    pub fn to_db(self) -> String {
        self.letter().to_string()
    }

    /// The letter (`'F'`, ...). Bytes above 127 (not a letter) show as the Latin-1 character of that value.
    pub const fn letter(self) -> char {
        self.0 as char
    }

    /// The wire value.
    pub const fn as_u8(self) -> u8 {
        self.0
    }

    /// `self >= min` (legacy `IsAuthHigher`).
    pub const fn at_least(self, min: Authority) -> bool {
        self.0 >= min.0
    }

    /// `self >= GAMEMASTER` (`'L'`).
    pub const fn is_gm(self) -> bool {
        self.0 >= Self::GAMEMASTER.0
    }

    /// The short token of a defined level (`player`, `gm1`, `admin`, ...), `None` for undefined letters.
    pub const fn short_name(self) -> Option<&'static str> {
        Some(match self.0 {
            b'D' => "observer",
            b'F' => "player",
            b'G' => "logchat",
            b'H' => "journalist",
            b'J' => "helper",
            b'L' => "gm1",
            b'M' => "gm2",
            b'N' => "gm3",
            b'O' => "operator",
            b'P' => "admin",
            b'Z' => "legacy-test",
            _ => return None,
        })
    }

    /// Accepts the letter (`"L"`, case-insensitive; only the defined letters `D F G H J L M N O P` and the
    /// raw `Z`), the legacy token (`"AUTH_GAMEMASTER"`) or the short token (`"gm1"`), case-insensitively.
    pub fn parse_token(s: &str) -> Result<Self, AuthorityParseError> {
        let t = s.trim();
        let lower = t.to_ascii_lowercase();
        let found = match lower.as_str() {
            "auth_observer" | "observer" => Some(Self::OBSERVER),
            "auth_general" | "player" => Some(Self::GENERAL),
            "auth_logchatting" | "logchat" => Some(Self::LOG_CHATTING),
            "auth_journalist" | "journalist" => Some(Self::JOURNALIST),
            "auth_helper" | "helper" => Some(Self::HELPER),
            "auth_gamemaster" | "gm1" => Some(Self::GAMEMASTER),
            "auth_gamemaster2" | "gm2" => Some(Self::GAMEMASTER2),
            "auth_gamemaster3" | "gm3" => Some(Self::GAMEMASTER3),
            "auth_operator" | "operator" => Some(Self::OPERATOR),
            "auth_administrator" | "admin" => Some(Self::ADMINISTRATOR),
            _ => match lower.as_bytes() {
                [
                    b
                    @ (b'd' | b'f' | b'g' | b'h' | b'j' | b'l' | b'm' | b'n' | b'o' | b'p' | b'z'),
                ] => Some(Self(b.to_ascii_uppercase())),
                _ => None,
            },
        };
        found.ok_or_else(|| AuthorityParseError(t.to_owned()))
    }
}

impl std::fmt::Display for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.letter())
    }
}

/// `Authority::parse_token` failed; carries the offending text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityParseError(pub String);

impl std::fmt::Display for AuthorityParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "'{}' is not an authority level (use a letter D F G H J L M N O P, an AUTH_* token, or \
             observer/player/logchat/journalist/helper/gm1/gm2/gm3/operator/admin)",
            self.0
        )
    }
}

impl std::error::Error for AuthorityParseError {}

/// `CMover::m_dwMode` (legacy `*_MODE`, `src:_Common/authorization.h:20-60`; 23 §2.1). All bits are persisted
/// in `characters.mode_flags` and broadcast with `ModifyMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModeFlags(pub u32);

impl ModeFlags {
    /// No bit.
    pub const NONE: Self = Self(0);
    /// `MATCHLESS_MODE`: takes no damage.
    pub const MATCHLESS: Self = Self(0x0000_0001);
    /// `TRANSPARENT_MODE`: invisible to non-GM clients.
    pub const TRANSPARENT: Self = Self(0x0000_0002);
    /// `ONEKILL_MODE`: every hit deals the target's current HP.
    pub const ONEKILL: Self = Self(0x0000_0004);
    /// `DONMOVE_MODE`: the client blocks movement (the server does not enforce, KEEP).
    pub const DONMOVE: Self = Self(0x0000_0008);
    /// `SAYTALK_MODE`: `/w` and `/say` are silently dropped.
    pub const SAYTALK: Self = Self(0x0000_0010);
    /// `MATCHLESS2_MODE`: takes damage but HP never drops below 1.
    pub const MATCHLESS2: Self = Self(0x0000_0020);
    /// `NO_ATTACK_MODE`: attacks and skills are refused.
    pub const NO_ATTACK: Self = Self(0x0000_0040);
    /// `ITEM_MODE`: cannot pick up or drop items.
    pub const ITEM: Self = Self(0x0000_0080);
    /// `COMMUNITY_MODE`: no server effect (KEEP-QUIRK).
    pub const COMMUNITY: Self = Self(0x0000_0100);
    /// `TALK_MODE`: normal chat is silently dropped.
    pub const TALK: Self = Self(0x0000_0200);
    /// `SHOUTTALK_MODE`: `/s` is silently dropped.
    pub const SHOUTTALK: Self = Self(0x0000_0400);
    /// `RECOVERCHAO_MODE` (PK system).
    pub const RECOVERCHAO: Self = Self(0x0000_0800);
    /// `FREEPK_MODE` (PK options).
    pub const FREEPK: Self = Self(0x0000_1000);
    /// `PVPCONFIRM_MODE` (options).
    pub const PVPCONFIRM: Self = Self(0x0000_2000);
    /// `QUERYSETPLAYERNAME_MODE` (rename scroll).
    pub const QUERYSETPLAYERNAME: Self = Self(0x0000_4000);
    /// `MODE_MAILBOX` (mail).
    pub const MAILBOX: Self = Self(0x0000_8000);
    /// `EVENT_OLDBOY_MODE`.
    pub const EVENT_OLDBOY: Self = Self(0x0001_0000);
    /// `EQUIP_DENIAL_MODE` (options).
    pub const EQUIP_DENIAL: Self = Self(0x0002_0000);
    /// `MODE_EXPUP_STOP`: monster-kill EXP is 0.
    pub const EXPUP_STOP: Self = Self(0x0004_0000);
    /// `GCWAR_NOT_CLICK_MODE` (siege).
    pub const GCWAR_NOT_CLICK: Self = Self(0x0008_0000);
    /// `GCWAR_RENDER_SKIP_MODE` (siege; also hides chat from such speakers).
    pub const GCWAR_RENDER_SKIP: Self = Self(0x0010_0000);
    /// `QUIZ_RENDER_SKIP_MODE` (quiz).
    pub const QUIZ_RENDER_SKIP: Self = Self(0x0020_0000);
    /// `MODE_OPTION_DONT_RENDER_MASK` (options).
    pub const OPTION_DONT_RENDER_MASK: Self = Self(0x0100_0000);
    /// `FRESH_MODE` (creation).
    pub const FRESH: Self = Self(0x1000_0000);
    /// `NOTFRESH_MODE` (face change).
    pub const NOTFRESH: Self = Self(0x2000_0000);
    /// `NOTFRESH_MODE2` (face change).
    pub const NOTFRESH2: Self = Self(0x4000_0000);
    /// `MODE_OUTOF_PARTYQUESTRGN` (party quest).
    pub const OUTOF_PARTYQUESTRGN: Self = Self(0x8000_0000);
    /// `DONTALK_MODE` = `TALK | SAYTALK | SHOUTTALK`.
    pub const DONTALK: Self = Self(0x0000_0610);
    /// `OBSERVE_MODE` = `ITEM | NO_ATTACK` (the legacy comment claims more; the value says `0xC0`).
    pub const OBSERVE: Self = Self(0x0000_00C0);
    /// `ALL_MODE` (unused).
    pub const ALL: Self = Self(0x0000_05D0);
    /// `MATCHLESS | TRANSPARENT | ONEKILL | MATCHLESS2`: stripped from non-GMs at login (VR-085, 23 §2.5).
    pub const GM_ONLY: Self = Self(0x0000_0027);

    /// The raw bits.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// The flags with these raw bits (no bit is reserved).
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// `true` when **every** bit of `other` is set (the plain `IsMode` test of 23 §2.2 (c)).
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// `true` when at least one bit of `other` is set.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// `self | other`.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `self & !other`.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// The login normalisation of VR-085 (23 §2.5): below `GAMEMASTER` the [`ModeFlags::GM_ONLY`] bits are
    /// removed.
    #[must_use]
    pub const fn strip_gm_only_for(self, authority: Authority) -> Self {
        if authority.is_gm() {
            self
        } else {
            self.without(Self::GM_ONLY)
        }
    }
}

impl std::ops::BitOr for ModeFlags {
    type Output = ModeFlags;
    fn bitor(self, rhs: ModeFlags) -> ModeFlags {
        self.with(rhs)
    }
}

impl std::ops::BitOrAssign for ModeFlags {
    fn bitor_assign(&mut self, rhs: ModeFlags) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for ModeFlags {
    type Output = ModeFlags;
    fn bitand(self, rhs: ModeFlags) -> ModeFlags {
        ModeFlags(self.0 & rhs.0)
    }
}

impl std::ops::Not for ModeFlags {
    type Output = ModeFlags;
    fn not(self) -> ModeFlags {
        ModeFlags(!self.0)
    }
}

/// `[gm]` (D5; replaces `Program/Authorization.ini`, PL-036): `grants` are evaluated in order, the **last**
/// matching entry wins and a match overrides the database value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GmConfig {
    /// The grant list, in file order.
    pub grants: Vec<GmGrant>,
}

/// One `[[gm.grants]]` entry: exactly one of `account` / `character` / `ip`, and a `level`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GmGrant {
    /// Account name (ASCII case-insensitive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Character name (ASCII case-insensitive), only matched once a character is chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
    /// Client address (an IPv4-mapped IPv6 peer is compared as IPv4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    /// [`Authority::parse_token`] syntax.
    pub level: String,
}

/// Everything [`resolve_authority`] looks at.
#[derive(Clone, Copy, Debug)]
pub struct AuthorityInputs<'a> {
    /// `accounts.authority`.
    pub account_db: Authority,
    /// `characters.authority`; `None` before a character is chosen (the login call).
    pub character_db: Option<Authority>,
    /// The normalised account name.
    pub account_name: &'a str,
    /// The chosen character's name, once known.
    pub character_name: Option<&'a str>,
    /// The client address (`http::client_ip`).
    pub peer_ip: IpAddr,
}

fn grant_matches(g: &GmGrant, inp: &AuthorityInputs<'_>) -> bool {
    if let Some(account) = &g.account {
        return account.trim().eq_ignore_ascii_case(inp.account_name);
    }
    if let Some(character) = &g.character {
        return inp
            .character_name
            .is_some_and(|n| character.trim().eq_ignore_ascii_case(n));
    }
    if let Some(ip) = &g.ip {
        return ip
            .trim()
            .parse::<IpAddr>()
            .is_ok_and(|a| a.to_canonical() == inp.peer_ip.to_canonical());
    }
    false
}

/// The resolution rule of 23 §1.3 (PL-036):
///
/// 1. the database value: `account_db` unless it is `F`, else `character_db` (legacy
///    `DbManagerJoin.cpp:237-240`);
/// 2. only when `servers_authorize` (`features.servers_authorize`): every grant is evaluated in file order and
///    the **last** matching one replaces the database value;
/// 3. no match keeps the database value (the legacy "no file / no match means everybody is `F`" is dropped).
pub fn resolve_authority(
    cfg: &GmConfig,
    servers_authorize: bool,
    inp: &AuthorityInputs<'_>,
) -> Authority {
    let db = if inp.account_db != Authority::GENERAL {
        inp.account_db
    } else {
        inp.character_db.unwrap_or(Authority::GENERAL)
    };
    if !servers_authorize {
        return db;
    }
    let mut effective = db;
    for grant in &cfg.grants {
        if grant_matches(grant, inp)
            && let Ok(level) = Authority::parse_token(&grant.level)
        {
            effective = level;
        }
    }
    effective
}

/// `true` when some grant matches (the `flyff account set-auth` warning "overridden by \[gm\] config", 12 §5.5).
pub fn grant_overrides(cfg: &GmConfig, servers_authorize: bool, inp: &AuthorityInputs<'_>) -> bool {
    servers_authorize
        && cfg
            .grants
            .iter()
            .any(|g| grant_matches(g, inp) && Authority::parse_token(&g.level).is_ok())
}

/// Startup validation (23 §1.3 step 5): an entry with zero or several selectors, an unparsable `level`, an
/// empty name or an unparsable `ip` is a configuration error. Returns one message per problem.
pub fn validate_gm_config(cfg: &GmConfig) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    for (i, g) in cfg.grants.iter().enumerate() {
        let n = i + 1;
        let selectors = [g.account.is_some(), g.character.is_some(), g.ip.is_some()]
            .iter()
            .filter(|s| **s)
            .count();
        if selectors != 1 {
            problems.push(format!(
                "grant {n}: exactly one of account / character / ip is required, found {selectors}"
            ));
        }
        for (what, value) in [("account", &g.account), ("character", &g.character)] {
            if let Some(v) = value
                && v.trim().is_empty()
            {
                problems.push(format!("grant {n}: {what} must not be empty"));
            }
        }
        if let Some(ip) = &g.ip
            && ip.trim().parse::<IpAddr>().is_err()
        {
            problems.push(format!("grant {n}: ip '{ip}' is not an IP address"));
        }
        if let Err(e) = Authority::parse_token(&g.level) {
            problems.push(format!("grant {n}: {e}"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}
