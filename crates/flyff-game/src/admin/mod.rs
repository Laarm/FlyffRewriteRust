//! The game-thread side of the admin console (Plan/25 §3.15, §11.5; WP-ADMIN-06): live subscriptions, read-only
//! queries and the scaffolding of the console's commands.
//!
//! **Observers and commands are different inputs** (ADM-D4). `GameInput::AdminView{conn, op}` carries subscriptions
//! and read-only queries; it is **not recorded** in the replay log (`GameInput::is_recorded`) and must not touch world
//! state or wake a monster (15 §5.6). The mutating commands are recorded `GameInput::Admin` inputs
//! (`AdminCmd`, the variants of 25 §11.5).
//!
//! | file | content | owner |
//! |---|---|---|
//! | [`views`] | the registry [`views::AdminViews`], `AdminViewOp`, `ViewSpec`, cadence, round-robin, the visit budget; roster, layers, status and entity views | WP-ADMIN-06 |
//! | [`events`] | [`events::AdminSink`], [`events::AdminEvent`] and the value types of roster, layers, status, entity detail | WP-ADMIN-06 |
//! | [`types`] | `AdminConnId`, `AdminFailure`, `AdminErrorCode`, `ContainerLoc` | WP-ADMIN-06 |
//! | [`map`] | the frame types of 25 §3.15, `MapFlags::of_mover`, **the hooks `open` / `update` / `collect`** | types: 06; bodies: WP-ADMIN-07 |
//! | [`watch`] | `CharacterDetail`, `ContainerSnapshot`, **the hooks `open` / `update` / `collect`** | types: 06; bodies: WP-ADMIN-16 |
//! | [`chat_tap`] | `ChatFilter`, `ChatLine`, **the hooks `open` / `update`**, the tap | types: 06; bodies: WP-ADMIN-19 |
//! | [`queries`] | `AdminQuery`, `AdminQueryResult`, `run`; one area file per group of queries | 06 (world queries), then 16, 17, 20, 23, 26, 30 |
//! | [`actions`] | the payloads and bodies of the new `AdminCmd` variants; one area file per group | 06 (stubs), then 12, 16, 20, 23, 26, 27, 30, 31 |
//!
//! Later work packages edit **only the files they own** (the table), replacing stub bodies; see `NOTES.md`,
//! "Admin console" for the variant-to-file map.

pub mod actions;
pub mod chat_tap;
pub mod events;
pub mod map;
pub mod queries;
pub mod types;
pub mod views;
pub mod watch;

pub use events::{
    AdminEvent, AdminSink, Delivery, EntityDetail, LayerInfo, Pushed, RosterDelta, RosterEntry,
    SharedSink, SlotKey, StatusTick, SubError, SubErrorCode,
};
pub use queries::{AdminQuery, AdminQueryResult};
pub use types::{AdminConnId, AdminErrorCode, AdminFailure, ContainerLoc};
pub use views::{AdminViewOp, AdminViews, ViewLimits, ViewSpec, VisitBudget};
