//! The prop tables (Plan/14 §6.3-6.6): items, skills, party skills, add-skills, movers, ctrls, motions and
//! jobs. Each module holds the row type and, under feature `load`, its pure parser; the glue that stores
//! the rows is in `load/steps/props.rs`.

pub mod ctrl;
pub mod item;
pub mod item_index;
pub mod job;
pub mod motion;
pub mod mover;
pub mod skill;
pub mod skill_add;
