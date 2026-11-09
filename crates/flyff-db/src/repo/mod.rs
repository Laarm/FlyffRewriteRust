//! SQL per domain (Plan/13 §1.3 `src/repo/`): every function runs on the writer connection inside a command
//! savepoint (writes) or on a reader connection (the [`crate::queries`]).

pub mod accounts;
pub mod banks;
pub mod characters;
pub mod guilds;
pub mod houses;
pub mod messaging;
pub mod meta;
pub mod party_names;
pub mod realm;
pub mod relations;
pub mod siege;
pub mod snapshot;
