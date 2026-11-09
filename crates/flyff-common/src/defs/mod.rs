//! Generated legacy define constants (`define*.h`), Plan/14 §10.9, Plan/01 ADR-014, WP-DATA-19.
//!
//! The 17 header modules, `all.rs` and `manifest.json` are produced by `cargo xtask gen-defs --resource
//! DIR` from the original `Resource/` headers (the same scan the server runs at start-up) and are
//! committed, so builds never need Resource. Names and values only. A value is the 32-bit pattern of the
//! header's `#define` (so `WBS_NOFOCUS 0x80000000L` is `2_147_483_648`). A name defined by two headers is
//! generated once, in the header that defined it first (first definition wins, like the original).
//!
//! * `flyff_common::defs::item::II_WEA_SWO_WOODEN` etc.: one module per header, named like the header
//!   (`define`, `neuz`, `quest`, `job`, `item`, `world`, `itemkind`, `lang`, `obj`, `attribute`, `skill`,
//!   `text`, `sound`, `resdata`, `wndstyle`, `lordskill`, `honor`),
//! * [`ALL`]: every constant sorted by name, [`BY_HEADER`]: the same grouped in file order,
//! * [`lookup`]: name -> value,
//! * `flyff_data::check::define_drift` compares these constants with the runtime define table (P8).
//!
//! The `ALL` table is `&[(&str, i64)]`; values are never negative here (bit patterns).

// The generated modules carry their own `#![allow(non_upper_case_globals, missing_docs)]`; they are
// excluded from rustfmt because the generator writes the canonical shape itself.
#[rustfmt::skip]
mod all;
#[rustfmt::skip]
pub mod attribute;
#[rustfmt::skip]
pub mod define;
#[rustfmt::skip]
pub mod honor;
#[rustfmt::skip]
pub mod item;
#[rustfmt::skip]
pub mod itemkind;
#[rustfmt::skip]
pub mod job;
#[rustfmt::skip]
pub mod lang;
#[rustfmt::skip]
pub mod lordskill;
#[rustfmt::skip]
pub mod neuz;
#[rustfmt::skip]
pub mod obj;
#[rustfmt::skip]
pub mod quest;
#[rustfmt::skip]
pub mod resdata;
#[rustfmt::skip]
pub mod skill;
#[rustfmt::skip]
pub mod sound;
#[rustfmt::skip]
pub mod text;
#[rustfmt::skip]
pub mod wndstyle;
#[rustfmt::skip]
pub mod world;

pub use all::{ALL, BY_HEADER};

/// The constants of one header in file order (see [`BY_HEADER`]).
#[derive(Debug, Clone, Copy)]
pub struct HeaderDefs {
    /// Module name (`item`, `skill`, ...).
    pub module: &'static str,
    /// The header file name as the original code spells it (`defineItem.h`).
    pub file: &'static str,
    /// `(name, value)` in file order.
    pub entries: &'static [(&'static str, i64)],
}

/// Looks a constant up by its legacy name (binary search over [`ALL`]).
pub fn lookup(name: &str) -> Option<i64> {
    ALL.binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .and_then(|i| ALL.get(i))
        .map(|(_, v)| *v)
}
