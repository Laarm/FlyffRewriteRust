//! The motion system (Plan/31 §12): the motion table codec, the frame clock, attribute detection and
//! motion selection.

pub mod attr;
pub mod clock;
pub mod ctrl_clock;
pub mod motion_table;
pub mod select;

pub use attr::{attrs_firing, is_attr, is_attr_hit, is_attr_o3d};
pub use clock::{MotionClock, ObjClock};
pub use ctrl_clock::{CTRL_HALF_DELAY_FRAMES, CtrlClock, CtrlStep};
pub use motion_table::{
    ClipId, ClipMeta, ClipSource, MOTION_TABLE_VERSION, ModelKey, ModelMotions, MotionAttr,
    MotionTable, Slot,
};
