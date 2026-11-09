//! The load pipeline (Plan/14 §5, feature `load`): `load_all` runs the loaders in the original order and
//! builds the immutable [`crate::GameData`].
//!
//! * [`ctx`]: `LoadCtx` (VFS reads with source tracking, the script tables, the diagnostics and the
//!   partially built data),
//! * [`prj`]: the `Masquerade.prj` driver (raw scanner, keys dispatched in file order),
//! * [`pipeline`]: `load_all`, the phases P0-P8 and the step table,
//! * [`steps`]: the glue of each loader (read the file, parse, store, count), one file per group,
//! * [`log`]: the `logs/data-load.jsonl` writer.
//!
//! Files under this directory are exempt from the wasm-safety source rules (they use `std::fs` and the
//! clock), see `xtask/lint/deps-allow.toml` `[io_exempt]`.

pub mod ctx;
pub mod log;
pub mod pipeline;
pub mod prj;
pub mod steps;

pub use ctx::{LoadCtx, Need};
pub use pipeline::{LoadError, LoadOutput, Phase, StepRecord, StepStatus, load_all};
pub use prj::{PrjEntry, PrjKey, parse_prj};
