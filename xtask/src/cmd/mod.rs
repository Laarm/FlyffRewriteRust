//! One module per `cargo xtask` command (Plan/02 §4.1).

pub mod admin_gen;
pub mod build_wasm;
pub mod build_web;
pub mod config_docs;
pub mod coverage;
pub mod dep_rules;
pub mod dev;
pub mod dist;
pub mod doctor;
pub mod gen_data_types;
pub mod gen_defs;
pub mod gen_lint;
pub mod gen_proto;
pub mod lint_det;
pub mod npc_transpile;
