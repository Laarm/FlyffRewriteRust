//! Build identity (Plan/02 §8.7): `BUILD_ID`, target triple, profile.
//!
//! `xtask` injects `FLYFF_BUILD_ID` (`<version>+<git-sha-12>[.dirty]`), `FLYFF_TARGET` and
//! `FLYFF_PROFILE` for dist builds; plain cargo builds fall back to `<version>+dev` and values derived
//! from `cfg!`.

use flyff_proto::PROTO_HASH;

/// `<version>+<git-sha-12>[.dirty]`, or `<version>+dev` for plain cargo builds.
pub const BUILD_ID: &str = match option_env!("FLYFF_BUILD_ID") {
    Some(id) => id,
    None => concat!(env!("CARGO_PKG_VERSION"), "+dev"),
};

/// Target description, e.g. `x86_64-unknown-linux-musl`.
pub fn target() -> String {
    if let Some(t) = option_env!("FLYFF_TARGET") {
        return t.to_owned();
    }
    let env = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    format!(
        "{}-unknown-{}-{}",
        std::env::consts::ARCH,
        std::env::consts::OS,
        env
    )
}

/// Cargo profile name (`dist`, `release`, `fasttest`, `dev`); falls back to a guess from `cfg!`.
pub fn profile() -> &'static str {
    match option_env!("FLYFF_PROFILE") {
        Some(p) => p,
        None if cfg!(debug_assertions) => "dev",
        None => "release",
    }
}

/// The one-line identity printed by `flyff version` and `flyff --version`
/// (`flyff <BUILD_ID> (proto <PROTO_HASH>, web <web build id>, <target>, <profile>)`).
pub fn version_line(web_build_id: &str) -> String {
    format!(
        "flyff {BUILD_ID} (proto {PROTO_HASH}, web {web_build_id}, {}, {})",
        target(),
        profile()
    )
}
