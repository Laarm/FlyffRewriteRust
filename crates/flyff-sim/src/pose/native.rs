//! The server-side builders (Plan/31 §13.1; WP-SIM-12): native parsed files are converted into the same
//! `FSK1`/`FMO1` packs the browser receives from its decode workers, and those packs go through the same
//! views into [`SimSkeleton`]/[`SimClip`] — there is exactly one conversion path, so the server and the
//! browser evaluate the same `f32` bits (`pack_motion_bit_exact`, Plan/30 §13.4).

use super::clip::SimClip;
use super::skeleton::SimSkeleton;
use crate::types::SimError;
use flyff_formats::error::FormatError;
use flyff_formats::pack::motion::{self, MotionView};
use flyff_formats::pack::skeleton::{self, SkeletonView};
use flyff_formats::{AniFile, ChrFile, O3dFile};

/// A failure of the native build: the pack builder/validator, or the sim conversion.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PoseBuildError {
    /// The pack builder or the pack view rejected the data.
    #[error("pack: {0}")]
    Format(#[from] FormatError),
    /// [`SimSkeleton::from_view`] / [`SimClip::from_view`] rejected the pack.
    #[error("pose: {0}")]
    Sim(#[from] SimError),
}

fn skeleton_from_bytes(bytes: &[u8]) -> Result<SimSkeleton, PoseBuildError> {
    let view = SkeletonView::parse(bytes)?;
    Ok(SimSkeleton::from_view(&view)?)
}

fn clip_from_bytes(bytes: &[u8]) -> Result<SimClip, PoseBuildError> {
    let view = MotionView::parse(bytes)?;
    Ok(SimClip::from_view(&view)?)
}

impl SimSkeleton {
    /// Builds the `FSK1` of a parsed `.chr` (`pack::skeleton::build_from_chr`) and converts it.
    pub fn from_chr(chr: &ChrFile) -> Result<SimSkeleton, PoseBuildError> {
        skeleton_from_bytes(&skeleton::build_from_chr(chr)?)
    }

    /// Parses the bytes of a `.chr` file and converts it (the boss rigs of Plan/24 §10.9).
    pub fn from_chr_bytes(bytes: &[u8]) -> Result<SimSkeleton, PoseBuildError> {
        let chr = flyff_formats::chr::parse(bytes)?.value;
        SimSkeleton::from_chr(&chr)
    }

    /// The embedded skeleton of an `.o3d` (35 corpus files); `Ok(None)` when it has none.
    pub fn from_o3d_embedded(o3d: &O3dFile) -> Result<Option<SimSkeleton>, PoseBuildError> {
        skeleton::build_from_o3d(o3d)?
            .map(|b| skeleton_from_bytes(&b))
            .transpose()
    }
}

impl SimClip {
    /// Builds the `FMO1` of a parsed `.ani` (`pack::motion::build_from_ani`; the file must have been parsed
    /// with `KeyMode::Load`) and converts it.
    pub fn from_ani(ani: &AniFile) -> Result<SimClip, PoseBuildError> {
        clip_from_bytes(&motion::build_from_ani(ani)?)
    }

    /// Parses the bytes of an `.ani` file (loading the keys) and converts it.
    pub fn from_ani_bytes(bytes: &[u8]) -> Result<SimClip, PoseBuildError> {
        let ani = flyff_formats::ani::parse(
            bytes,
            &flyff_formats::AniOptions {
                keys: flyff_formats::KeyMode::Load,
            },
        )?
        .value;
        SimClip::from_ani(&ani)
    }

    /// The embedded motion of an `.o3d` (flag bit 0); `Ok(None)` when it has none.
    pub fn from_o3d_embedded(o3d: &O3dFile) -> Result<Option<SimClip>, PoseBuildError> {
        motion::build_from_o3d(o3d)?
            .map(|b| clip_from_bytes(&b))
            .transpose()
    }

    /// The node clip of an `.o3d` with node animation (flag bit 2); `Ok(None)` for other files.
    pub fn from_o3d_nodes(o3d: &O3dFile) -> Result<Option<SimClip>, PoseBuildError> {
        motion::build_nodes_from_o3d(o3d)?
            .map(|b| clip_from_bytes(&b))
            .transpose()
    }
}
