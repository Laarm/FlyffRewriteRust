//! `flyff-wasm` — tier S (Plan/31, 30, 16, 40, D7): the browser module, built by
//! `cargo xtask build-wasm` into `web/src/wasm/pkg/`. It exposes exactly three areas:
//!
//! 1. [`decode`] (Plan/30 §14): the `flyff-formats` decoders for the Web Workers (`decode_o3d`, `decode_chr`,
//!    `decode_ani`, `decode_lnd`, `decode_sfx`, `decode_dds`, `decode_dds_bc`, `formats_info`),
//! 2. [`sim`] (Plan/31 §17): `SimWorld`, the deterministic simulation,
//! 3. [`rules`] (Plan/16 §18.4): pure formulas on plain data.
//!
//! The decoders (WP-FMT-16/17), `sim` (WP-SIM-13..17) and the M3 part of `rules` (WP-MOVER-13: `RulesContext`) are done. Besides
//! the decoders the module exports [`version`], [`hello`] and [`selftest_det`]. No `#[wasm_bindgen]` function
//! may panic (the release profile aborts, 02 §11.4): untrusted input returns `Result<_, JsError>`.
//!
//! Features: `dev-panic-hook` installs `console_error_panic_hook` (dev builds only), `testkit` exports
//! the helpers of the Node determinism runner (never in shipped builds).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
#![deny(clippy::iter_over_hash_type)]
#![warn(missing_docs)]

pub mod decode;
pub mod det;
pub mod item_rules;
pub mod lobby_rules;
pub mod pet_rules;

pub mod sim;

pub mod rules;

use wasm_bindgen::prelude::wasm_bindgen;

/// The crate version (workspace version), shown by the placeholder web page.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Greeting used by the placeholder page and the web tests to prove that the module is callable.
#[wasm_bindgen]
pub fn hello(name: &str) -> String {
    format!("Hello, {name}! (flyff-wasm {})", env!("CARGO_PKG_VERSION"))
}

/// Hash of a fixed `libm`/`f32` computation sequence derived from `seed`; identical natively, under
/// Node and in the browser (Plan/02 §9.5). Returned to JavaScript as a `BigInt`.
#[wasm_bindgen]
pub fn selftest_det(seed: u32) -> u64 {
    det::selftest_det(seed)
}

/// Runs on module instantiation in dev builds: Rust panic messages reach the console before the trap.
#[cfg(feature = "dev-panic-hook")]
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}
