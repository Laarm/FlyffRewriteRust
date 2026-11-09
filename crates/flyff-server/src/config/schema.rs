//! The sections owned by Plan/10 §8.5 (`[server]`, `[net]`, `[net.tls]`, `[runtime]`, `[admin]`, `[paths]`,
//! `[data]`, `[log]`, `[debug]`) and the keys it registers on behalf of 12 and 24 (`[auth]`, `[web]`,
//! `[assets]`) and 21/12/23 (`[guild]`, `[gm]`), with their documentation table [`OPS_KEYS`].
//!
//! One invocation of `ops_tables!` defines the structs (`#[serde(default, deny_unknown_fields)]`, defaults of
//! §8.5), their `Default`s and [`OPS_KEYS`]: key, type, default as TOML text, range, reload flag,
//! description and owner. The loader validates ranges from the table; `template.rs` renders the commented
//! default file from it; a test serialises `Config::default()` and compares every value with the table.

use serde::{Deserialize, Serialize};

/// Whether a changed key takes effect without a restart (Plan/10 §8.5 "Reload", §8.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reload {
    /// Restart required.
    No,
    /// Applied by `reload` (console `reload`, `POST /admin/api/reload`, SIGHUP).
    Yes,
}

/// The accepted values of a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    /// Anything the type allows.
    Any,
    /// Integer in `min..=max`.
    Int(i64, i64),
    /// `0` (feature off / unlimited) or an integer in `min..=max`.
    ZeroOr(i64, i64),
    /// One of these strings.
    Choice(&'static [&'static str]),
    /// String of `min..=max` characters.
    Len(usize, usize),
    /// Non-empty string.
    NonEmpty,
    /// Free-form rule validated elsewhere (cross-key rules); shown verbatim in the docs.
    Note(&'static str),
}

impl Range {
    /// Human-readable form for the template and the docs (`""` when unconstrained).
    pub fn describe(&self) -> String {
        match self {
            Any => String::new(),
            Int(a, b) => format!("{a}..={b}"),
            ZeroOr(a, b) => format!("0 or {a}..={b}"),
            Choice(c) => c
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(" | "),
            Len(a, b) => format!("{a}..={b} characters"),
            NonEmpty => "not empty".to_owned(),
            Note(n) => (*n).to_owned(),
        }
    }

    /// Checks a TOML value against the range; `Some(reason)` when it violates it.
    pub fn violation(&self, v: &toml::Value) -> Option<String> {
        match (self, v) {
            (Int(a, b), toml::Value::Integer(i)) if i < a || i > b => {
                Some(format!("must be in {a}..={b}"))
            }
            (ZeroOr(a, b), toml::Value::Integer(i)) if *i != 0 && (i < a || i > b) => {
                Some(format!("must be 0 or in {a}..={b}"))
            }
            (Choice(c), toml::Value::String(s)) if !c.contains(&s.as_str()) => Some(format!(
                "must be one of {}",
                c.iter()
                    .map(|x| format!("\"{x}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            (Len(a, b), toml::Value::String(s)) => {
                let n = s.chars().count();
                (n < *a || n > *b).then(|| format!("must be {a}..={b} characters long"))
            }
            (NonEmpty, toml::Value::String(s)) if s.trim().is_empty() => {
                Some("must not be empty".to_owned())
            }
            _ => None,
        }
    }
}

/// One documented key of the doc-10 sections (and the keys registered for 12/24, 21 and 12/23).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpsKey {
    /// `section.key` (`net.tls.cert` for the nested table).
    pub key: &'static str,
    /// The Rust type as written in the table (`u32`, `String`, `Vec<String>`, …).
    pub ty: &'static str,
    /// The default as TOML text.
    pub default: &'static str,
    /// Accepted values.
    pub range: Range,
    /// Reload class.
    pub reload: Reload,
    /// One-line description.
    pub description: &'static str,
    /// Owning plan document number (`"10"`, `"12"`, `"24"`, …).
    pub owner: &'static str,
}

macro_rules! ops_tables {
    (
        $(
            $path:literal $sname:ident { $($row:tt)* } $( extra { $($extra:tt)* } )?
        )*
    ) => {
        $( ops_section! { $path $sname { $($row)* } $( extra { $($extra)* } )? } )*
        ops_registry! { $( $path { $($row)* } )* }
    };
}

macro_rules! ops_section {
    (
        $path:literal $sname:ident {
            $(
                $name:ident : $ty:ty = $def:expr ;
                $text:literal ; $range:expr ; $reload:ident ; $doc:literal ; $owner:literal ;
            )*
        } $( extra { pub $en:ident : $et:ty , } )?
    ) => {
        #[doc = concat!("`[", $path, "]` (Plan/10 §8.5).")]
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct $sname {
            $( #[doc = $doc] pub $name: $ty, )*
            $( #[doc = "Nested table."] pub $en: $et, )?
        }

        impl Default for $sname {
            fn default() -> Self {
                Self {
                    $( $name: $def, )*
                    $( $en: <$et>::default(), )?
                }
            }
        }
    };
}

macro_rules! ops_registry {
    (
        $(
            $path:literal {
                $(
                    $name:ident : $ty:ty = $def:expr ;
                    $text:literal ; $range:expr ; $reload:ident ; $doc:literal ; $owner:literal ;
                )*
            }
        )*
    ) => {
        /// Documentation table of every key of the doc-10 sections and the keys registered for other
        /// owners, in template order (Plan/10 §8.7).
        pub static OPS_KEYS: &[OpsKey] = &[
            $( $(
                OpsKey {
                    key: concat!($path, ".", stringify!($name)),
                    ty: stringify!($ty),
                    default: $text,
                    range: $range,
                    reload: Reload::$reload,
                    description: $doc,
                    owner: $owner,
                },
            )* )*
        ];
    };
}

use Range::{Any, Choice, Int, Len, NonEmpty, Note, ZeroOr};

ops_tables! {
    "server" ServerConfig {
        name: String = "V15".to_owned(); "\"V15\""; Len(1, 32); No; "Server name in HelloAck.server_name and /api/bootstrap (replaces the SRVR_LIST entry)."; "10";
        shutdown_countdown_s: u32 = 60; "60"; Int(0, 3600); Yes; "Countdown in seconds for SIGTERM/SIGINT/console shutdown."; "10";
        timezone: String = "local".to_owned(); "\"local\""; Note("\"local\" or an IANA zone such as \"Europe/Berlin\""); No; "Time zone of the wall-clock schedules and Lua os.date (PD-08)."; "10";
    }
    "net" NetConfig {
        bind: String = "0.0.0.0:8080".to_owned(); "\"0.0.0.0:8080\""; Note("socket address"); No; "The one TCP port: HTTP, assets, /api and the WebSocket /ws (D3). --bind overrides."; "10";
        flush_interval_ms: u32 = 67; "67"; Int(67, 1000); No; "Snapshot flush cadence in ms; ticks = max(1, round(ms * 15 / 1000)); 200 = the original server (PL-020)."; "10";
        max_client_msg_bytes: u32 = 8192; "8192"; Int(1024, 65536); No; "Largest accepted client-to-server WebSocket message (GP-01, __SO1014)."; "10";
        strict_decode: bool = true; "true"; Any; No; "true: a malformed or empty envelope kicks with PROTOCOL_ERROR; false: dropped and counted (debugging only)."; "11";
        client_msgs_per_sec: u32 = 60; "60"; Int(1, 10000); No; "Per-session token bucket rate (GP-03)."; "11";
        client_msgs_burst: u32 = 120; "120"; Note(">= client_msgs_per_sec"); No; "Per-session token bucket capacity."; "11";
        rate_kick_factor: u32 = 10; "10"; Any; No; "Kick when drops within 10 s exceed factor x rate; 0 = never."; "11";
        max_batch_bytes: u32 = 1_048_576; "1048576"; Int(16384, 16_777_216); No; "A server batch larger than this is split at message boundaries."; "11";
        outbound_queue_frames: u32 = 256; "256"; Int(16, 4096); No; "Per-session queue of encoded frames (C2)."; "11";
        outbound_queue_bytes: u64 = 4_194_304; "4194304"; Note(">= max_batch_bytes"); No; "Queued bytes per session before SlowConsumer."; "11";
        ws_permessage_deflate: bool = false; "false"; Any; No; "Reserved: true logs a warning 'not supported' until a WP implements it."; "11";
        ws_ping_interval_s: u32 = 15; "15"; Int(5, 60); No; "WebSocket ping cadence in seconds."; "11";
        ws_idle_timeout_s: u32 = 60; "60"; Note("> ws_ping_interval_s"); No; "Liveness timeout in seconds."; "11";
        timeout_hello_s: u32 = 20; "20"; Int(5, 120); No; "Seconds from upgrade to a valid Hello."; "11";
        timeout_entering_s: u32 = 60; "60"; Int(10, 600); No; "Seconds from an accepted EnterWorld to Join."; "11";
        timeout_charselect_idle_s: u32 = 900; "900"; Any; No; "Character-select idle kick in seconds; 0 = off."; "11";
        allowed_origins: Vec<String> = Vec::new(); "[]"; Note("origins scheme://host[:port]"); Yes; "Extra WebSocket/API origins besides the serving host."; "11";
        upgrade_per_ip_per_sec: u32 = 10; "10"; Int(1, 1000); No; "Per-IP WebSocket upgrade bucket rate (GP-02)."; "11";
        upgrade_per_ip_burst: u32 = 50; "50"; Note(">= upgrade_per_ip_per_sec"); No; "Per-IP WebSocket upgrade bucket capacity."; "11";
        max_conns_per_ip: u32 = 16; "16"; Any; No; "Open WebSockets per IP (IPv6: per /64); 0 = unlimited."; "11";
        ip_block: Vec<String> = Vec::new(); "[]"; Note("CIDR networks"); Yes; "Blocked client networks (403 on every route; loopback is never blocked; replaces IPCut.ini, F-007)."; "10";
        npc_keyframe_hz: u32 = 1; "1"; Int(0, 15); No; "MoverKeyframe cadence for moving NPCs; 0 = off (PL-008)."; "10";
    } extra { pub tls: TlsConfig, }
    "net.tls" TlsConfig {
        cert: String = String::new(); "\"\""; Note("readable PEM"); No; "Certificate chain (PEM) for built-in TLS (rustls); with key, enables HTTP/2 via ALPN. Empty = plain HTTP."; "10";
        key: String = String::new(); "\"\""; Note("readable PEM"); No; "Private key (PEM) for built-in TLS."; "10";
        reload_check_s: u32 = 60; "60"; ZeroOr(5, 86_400); No; "Certificate reload polling interval in seconds; 0 = never."; "24";
    }
    "runtime" RuntimeConfig {
        io_threads: u32 = 0; "0"; Int(0, 256); No; "tokio worker threads; 0 = max(1, cores - 1)."; "10";
        asset_threads: u32 = 0; "0"; Int(0, 64); No; "Asset-preparation pool size; 0 = max(1, cores - 2)."; "10";
        asset_nice: u32 = 10; "10"; Int(0, 19); No; "Niceness of the asset threads."; "10";
        startup_threads: u32 = 0; "0"; Int(0, 256); No; "Temporary start-up pool; 0 = all cores."; "10";
    }
    "auth" AuthConfig {
        token_grace_s: u32 = 60; "60"; Note(">= gameplay.timewait_close_s + 10"); No; "Session token validity after the last socket closed."; "12";
        takeover_wait_s: u32 = 20; "20"; Note(">= timewait_close_s + ceil(database.flush_wait_timeout_ms / 1000)"); No; "Wait for the old claim's release on a forced login."; "12";
        legacy_md5_salt: String = "kikugalanet".to_owned(); "\"kikugalanet\""; Any; No; "NEUZ_PHASH salt of the browser md5 (D3)."; "12";
        allow_registration: bool = true; "true"; Any; Yes; "Allow /api/register (PD-05)."; "12";
        second_password: bool = false; "false"; Any; No; "Reserved (F-053): true warns and behaves as false."; "12";
        delete_requires_password: bool = true; "true"; Any; Yes; "Character deletion needs the account password (PL-035)."; "12";
        max_online_accounts: u32 = 50_000; "50000"; Int(1, 1_000_000); No; "Legacy AccountServer MAX_CONN (error 108)."; "12";
        max_in_world: u32 = 1000; "1000"; Int(1, 100_000); Yes; "Legacy channel lMax: Hello is refused with SERVER_FULL above this many in-world players."; "12";
        register_per_ip_per_hour: u32 = 5; "5"; Any; Yes; "Registrations per client IP per rolling hour (PD-05); 0 = unlimited."; "12";
        maintenance: bool = false; "false"; Any; Yes; "Only GMs and allow-listed IPs may log in (F-006)."; "12";
        maintenance_allow_ips: Vec<String> = Vec::new(); "[]"; Note("IP or CIDR"); Yes; "IPs allowed to log in during maintenance."; "12";
    }
    "admin" AdminConfig {
        allow_remote: bool = false; "false"; Any; No; "Non-loopback access to /metrics, /admin and /admin/api/* (still needs the bearer token in data/admin.token)."; "10";
        console: String = "auto".to_owned(); "\"auto\""; Choice(&["auto", "stdin", "off"]); No; "stdin admin console: auto = only on a TTY, stdin = always (systemd with a pipe), off."; "10";
        owner_loopback: bool = true; "true"; Any; No; "A loopback request without a valid session acts as the owner (ADMINISTRATOR); false: even the host needs a staff login or the token (Plan/25 §1.1)."; "25";
        staff_login: bool = true; "true"; Any; No; "Staff sessions: accounts with an authority of L or higher sign in at /admin (UD-19)."; "25";
        session_idle_min: u32 = 30; "30"; Int(1, 1440); Yes; "Idle timeout of an admin session in minutes."; "25";
        session_max_h: u32 = 12; "12"; Int(1, 720); Yes; "Absolute lifetime of an admin session in hours."; "25";
        totp_required: bool = false; "false"; Any; Yes; "Remote staff sign-ins to the admin console need a TOTP code; staff without one enrolled sign in only from the host (Plan/25 §1.3)."; "25";
        capabilities: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new(); "{}"; Note("capability name = authority letter L..P, never below L"); No; "Per-capability overrides of the minimum authority letter of the admin console (Plan/25 §1.6), e.g. \"act.kick\" = \"N\"."; "25";
        reason_presets: Vec<String> = ["Spam", "Abuse", "Botting", "RMT", "Exploit", "Impersonation", "Support", "Event", "Other"].map(str::to_owned).to_vec(); "[\"Spam\", \"Abuse\", \"Botting\", \"RMT\", \"Exploit\", \"Impersonation\", \"Support\", \"Event\", \"Other\"]"; Note("1..=32 entries of 1..=32 characters"); Yes; "One-click reason presets of the admin console (kick, mute, ban, grants, removals, ...)."; "25";
        max_objects_per_action: u32 = 500; "500"; Int(1, 10000); Yes; "Largest group action (targets per request); more answers 413 too_many_targets."; "25";
        mutations_per_s: u32 = 30; "30"; Int(1, 10000); Yes; "Mutating admin API requests per identity and second (token bucket); more answers 429 rate_limited."; "25";
        confirm_gold_threshold: u64 = 100_000_000; "100000000"; Any; Yes; "A penya change above this needs a typed confirmation."; "25";
        confirm_item_threshold: u32 = 50; "50"; Int(1, 100000); Yes; "More items than this in one action need a typed confirmation."; "25";
        max_ws_per_identity: u32 = 4; "4"; Int(1, 64); Yes; "Admin WebSockets (console tabs) one identity may hold open at once; more answers 429 limit."; "25";
        max_map_subs_per_conn: u32 = 1; "1"; Int(1, 16); Yes; "Map subscriptions per admin WebSocket; more answers SubscriptionError LIMIT."; "25";
        max_map_subs_total: u32 = 16; "16"; Int(1, 1024); Yes; "Map subscriptions across all admin WebSockets; more answers SubscriptionError LIMIT."; "25";
        map_tier_a_m: u32 = 1200; "1200"; Note("100..=100000, <= map_tier_b_m"); Yes; "Live map: a subscription rectangle up to this size (metres, the larger side) gets tier A, every enabled entity at 5 Hz."; "25";
        map_tier_b_m: u32 = 6000; "6000"; Int(100, 100_000); Yes; "Live map: a rectangle up to this size gets tier B (players, monsters, NPCs at about 1.9 Hz); a larger one tier C (players, bosses and 128 m bins at 0.5 Hz)."; "25";
        map_visit_budget: u32 = 30_000; "30000"; Int(100, 100_000_000); Yes; "Live map: entity reads per game tick across all admin map subscriptions (a cell visit costs 1/16); a frame that does not fit continues on the next tick."; "25";
        map_max_entities: u32 = 4000; "4000"; Int(100, 1_000_000); Yes; "Live map: a tier A or B frame with more entities than this is sent as tier C and flagged degraded."; "25";
        allow_backup_download_remote: bool = false; "false"; Any; Yes; "Allow the P-level backup download of the admin console from a non-loopback peer (Plan/25 §9.7); false: loopback only."; "25";
    } extra { pub alerts: AlertsConfig, }
    "admin.alerts" AlertsConfig {
        tick_p99_ms: u32 = 60; "60"; Int(1, 1000); Yes; "Dashboard alert: the tick p99 stays above this many milliseconds (Plan/25 §9.1)."; "25";
        tick_p99_for_s: u32 = 60; "60"; Int(10, 3600); Yes; "Dashboard alert: how long the tick p99 must stay above the limit before the alert fires."; "25";
        backup_max_age_h: u32 = 26; "26"; ZeroOr(1, 8760); Yes; "Dashboard alert: the newest backup is older than this many hours; 0 = off."; "25";
        disk_min_gb: u32 = 2; "2"; ZeroOr(1, 100_000); Yes; "Dashboard alert: the free space of the data directory is below this many GB; 0 = off."; "25";
        failed_logins: u32 = 5; "5"; Int(1, 100_000); Yes; "Dashboard alert: more than this many failed remote staff logins within failed_logins_window_min."; "25";
        failed_logins_window_min: u32 = 10; "10"; Int(1, 1440); Yes; "Window of the failed staff login alert in minutes."; "25";
    }
    "web" WebConfig {
        public_assets: bool = true; "true"; Any; No; "Serve /res and /gen without login (D6, PD-09). Keep the server on a private network: the assets are proprietary."; "24";
        page_title: String = "[VS22] V15".to_owned(); "\"[VS22] V15\""; Len(1, 64); No; "HTML title of the client (NEUZ_TITLE)."; "24";
        register_url: String = String::new(); "\"\""; Note("empty or URL"); No; "Optional registration link on the login screen."; "24";
        asset_cookie_ttl_s: u32 = 43_200; "43200"; Int(60, 604_800); No; "Sliding validity of the asset cookie."; "24";
        trusted_proxies: Vec<String> = Vec::new(); "[]"; Note("CIDR networks"); No; "Peers whose X-Forwarded-* headers are trusted: the only client-IP rule."; "24";
        max_tcp_per_ip: u32 = 64; "64"; Any; No; "Accept-level connection cap per IP; 0 = unlimited."; "24";
        max_tcp_total: u32 = 8192; "8192"; Any; No; "Accept-level connection cap; 0 = unlimited."; "24";
        req_per_ip_per_sec: u32 = 300; "300"; Any; No; "Static-asset request rate per IP; 0 = class off."; "24";
        req_per_ip_burst: u32 = 2000; "2000"; Note(">= req_per_ip_per_sec"); No; "Static-asset burst per IP."; "24";
        api_per_ip_per_sec: u32 = 5; "5"; Any; No; "API request rate per IP (/api/login, /api/register); 0 = class off."; "24";
        api_per_ip_burst: u32 = 30; "30"; Note(">= api_per_ip_per_sec"); No; "API burst per IP."; "24";
        gen_waits_per_ip: u32 = 8; "8"; Any; No; "Concurrent /gen cache-miss waits per IP; 0 = unlimited."; "24";
        hsts_max_age_s: u32 = 0; "0"; Any; No; "HSTS max-age on TLS; 0 = off."; "24";
        csp: bool = true; "true"; Any; No; "Emit the Content-Security-Policy header."; "24";
    }
    "assets" AssetsConfig {
        prewarm: bool = true; "true"; Any; No; "Build derived assets in the background after start."; "24";
        gzip_raw: String = "all".to_owned(); "\"all\""; Choice(&["all", "lazy", "off"]); No; "Pre-compressed gzip variants of raw files."; "24";
        disk_budget_mb: u32 = 4096; "4096"; Int(100, 100_000); No; "Budget of cache/ in MB (reported by `flyff check`); optional assets are refused above it."; "24";
        interactive_timeout_s: u32 = 30; "30"; Int(1, 600); No; "Seconds a /gen request waits for a build before 503."; "24";
        fingerprint_mode: String = "stat".to_owned(); "\"stat\""; Choice(&["stat", "content"]); No; "Fingerprint inputs: file stats or content."; "24";
        packs: String = "off".to_owned(); "\"off\""; Choice(&["on", "off"]); No; "Per-land and per-skeleton pack endpoints (M8)."; "24";
        pack_shared_min_lands: u32 = 8; "8"; Int(1, 1000); No; "Shared-pack threshold in lands."; "24";
    }
    "paths" PathsConfig {
        resource_dir: String = "Resource".to_owned(); "\"Resource\""; NonEmpty; No; "The original data (never written, D6); --resource and FLYFF_RESOURCE override."; "10";
        overrides_dir: String = "overrides".to_owned(); "\"overrides\""; NonEmpty; No; "Files that shadow Resource/."; "10";
        npc_scripts_dir: String = "scripts/npc".to_owned(); "\"scripts/npc\""; NonEmpty; No; "Transpiled NPC dialogs (restored from the embedded copy when missing)."; "10";
        cache_dir: String = "cache".to_owned(); "\"cache\""; NonEmpty; No; "Derived assets (deletable)."; "10";
    }
    "data" DataConfig {
        strict: bool = false; "false"; Any; No; "Error diagnostics of the data loader fail the start."; "14";
        cp1252_lossy: bool = false; "false"; Any; No; "Replace non-CP1252 characters by '?' as the original did (PL-132)."; "14";
    }
    "log" LogConfig {
        level: String = "info".to_owned(); "\"info\""; Note("tracing filter, e.g. \"info,flyff_game=debug\""); Yes; "Console and file filter; RUST_LOG wins when set."; "10";
        format: String = "text".to_owned(); "\"text\""; Choice(&["text", "json"]); No; "File format (the console is always text)."; "10";
        console: String = "auto".to_owned(); "\"auto\""; Choice(&["auto", "plain", "off"]); No; "stderr output: auto = ANSI when stderr is a TTY, plain, off."; "10";
        dir: String = "logs".to_owned(); "\"logs\""; NonEmpty; No; "Directory of flyff.log and the rotated files."; "10";
        file_max_mib: u32 = 64; "64"; Int(1, 4096); No; "Rotate flyff.log above this size (it also rotates at UTC midnight)."; "10";
        file_keep: u32 = 20; "20"; Int(1, 1000); No; "Rotated files kept."; "10";
        max_age_days: u32 = 30; "30"; Int(1, 3650); No; "Rotated files older than this are deleted."; "10";
        client_warn_per_min: u32 = 10; "10"; Any; Yes; "Client-caused WARN lines per session per minute; 0 = unlimited."; "10";
    }
    "debug" DebugConfig {
        record_inputs: bool = false; "false"; Any; No; "Record every game-thread input for `flyff replay`."; "10";
        replay_dir: String = "logs/replay".to_owned(); "\"logs/replay\""; NonEmpty; No; "One sub-directory per recorded run."; "10";
        replay_max_mib: u32 = 2048; "2048"; Any; No; "Recording stops (with a warning) at this compressed size."; "10";
        digest_every_ticks: u32 = 15; "15"; Any; No; "State digest cadence in the replay log; 0 = never."; "10";
        seed: u64 = 0; "0"; Any; No; "Game RNG seed; 0 = random from the OS at every start."; "10";
        dev_surfaces: bool = false; "false"; Any; No; "Serve the /dev/* surfaces without --dev."; "10";
    }
    "guild" GuildConfig {
        create_requires_quest: bool = false; "false"; Any; No; "Guild creation at NPC Helena needs QUEST_CREGUILD (D1, PD-06); the quest data is empty in this repack."; "21";
    }
    "gm" GmConfig {
        grants: Vec<toml::Table> = Vec::new(); "[]"; Note("entries {account | character | ip, level}"); No; "GM grants that override the database authority (D5, PL-036). Evaluated in order, the last matching entry wins."; "23";
    }
}

impl AuthConfig {
    /// `auth.second_password` is reserved (F-053): `true` warns and behaves as `false`.
    pub fn second_password_effective(&self) -> bool {
        false
    }
}

impl GmConfig {
    /// The grants as the typed [`flyff_common::gm::GmConfig`] (23 §1.2) after the startup validation of
    /// 23 §1.3 step 5 (`flyff_common::gm::validate_gm_config`): every problem of every entry, or the typed list.
    pub fn typed(&self) -> Result<flyff_common::gm::GmConfig, Vec<String>> {
        let mut problems = Vec::new();
        let mut grants = Vec::with_capacity(self.grants.len());
        for (i, entry) in self.grants.iter().enumerate() {
            match toml::Value::Table(entry.clone()).try_into::<flyff_common::gm::GmGrant>() {
                Ok(g) => grants.push(g),
                Err(e) => problems.push(format!("grant {}: {}", i + 1, e.message())),
            }
        }
        let typed = flyff_common::gm::GmConfig { grants };
        if let Err(more) = flyff_common::gm::validate_gm_config(&typed) {
            problems.extend(more);
        }
        if problems.is_empty() {
            Ok(typed)
        } else {
            Err(problems)
        }
    }
}
