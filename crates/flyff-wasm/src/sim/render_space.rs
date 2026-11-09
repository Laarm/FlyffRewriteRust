//! `render_space`: the left-handed (game) to three.js mapping of everything `SimWorld` writes into the render
//! view (Plan/01 §8, 31 §13.4; rule B8 "one implementation per language").
//!
//! The mapping itself is `flyff_formats::space` (`S·M·S`, `(x, y, z) -> (x, y, -z)`); the pose runtime
//! applies it to palettes and attachments ([`flyff_sim::pose::palette::render_rows`]) and the view builder to
//! the mover world matrices ([`flyff_sim::client::view::world_rows`]). This module is the named seam the
//! plan calls `flyff-wasm::render_space`.

pub use flyff_sim::client::view::{world_matrix_lh, world_rows};
pub use flyff_sim::pose::palette::{render_pos, render_rows};

use flyff_sim::pose::Mat43;

/// A left-handed 4x3 matrix as the 12 floats of the three.js rows (3 RGBA32F texels).
pub fn mat_lh_to_rh(m: &Mat43) -> [f32; 12] {
    render_rows(m)
}
