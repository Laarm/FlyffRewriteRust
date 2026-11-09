//! Parser/codec version constants (Plan/30 §2.3).
//!
//! Each constant is bumped when its output changes for some input. `flyff-assets` folds the relevant
//! constants into its per-kind builder versions (D6 fingerprints); packed formats carry [`PACK`] in their
//! headers.

/// Model parsers (`o3d`, `chr`, `ani`, `lnd`, `dyo`, `sfx`, text world formats).
pub const PARSERS: u32 = 1;
/// BC1/2/3 decoder output.
pub const BC_DECODE: u32 = 1;
/// BC1/2/3 encoder output.
pub const BC_ENCODE: u32 = 1;
/// Packed render/sim blobs.
pub const PACK: u32 = 1;
/// Mip-chain completion output.
pub const MIPS: u32 = 1;
