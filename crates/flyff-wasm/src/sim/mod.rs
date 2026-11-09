//! Area 2 of the module: the browser `SimWorld` (Plan/31 §17, D7).
//!
//! All behaviour lives in [`flyff_sim::client::ClientSim`] (command buffer, stepping, events, render view,
//! preview movers, recorder) so that native tests, the vector runner and bots execute the code the browser
//! executes; this module is the `wasm-bindgen` shell: argument conversion, `JsError` mapping, the pointers
//! the TypeScript side re-acquires its typed-array views from after every call (B4), the worker-side chunk
//! validator and, with the feature `testkit`, the determinism exports `det_run_vectors` /
//! `det_run_vectors_raw` / `det_alloc` / `det_free`.
//!
//! | file | plan name | content |
//! |---|---|---|
//! | `world.rs` | `sim_world.rs` | `SimWorld`, `decode_chunk_check` |
//! | `render_space.rs` | `render_space.rs` | the left-handed to three.js matrix mapping (calls `flyff_sim::pose`) |
//! | `vectors.rs` | (testkit) | `det_run_vectors` |
//! | `raw.rs` | (testkit) | the raw twin for non-JS hosts |
//!
//! `sim_cmd.rs` (command decoder), `client_host.rs` (queue drain host, events) and `render_view.rs` are
//! `flyff_sim::client::{commands, events, view}`.

pub mod render_space;

mod world;

pub use world::{SimWorld, decode_chunk_check};
