//! Pending grants and removals at the player (Plan/19 §20.4, 13 §7, §17.4 step 8, §18; WP-ECON-09, PL-055).
//!
//! The database holds `pending_grants` (gold and item templates that wait for a character: the operator's CLI, the
//! admin console's pending mode, the guild salary of an offline member) and `pending_removals` (penya or items to
//! take from a character). This module is the game's side:
//!
//! | module | content |
//! |---|---|
//! | [`mod@deliver`] | [`deliver()`] = the `DeliverFn` of the poll P12 (`persist::grants`) and of the enter-world pass: grants grouped by receiver, one `GrantDelivery` batch per receiver and pass |
//! | [`removals`] | [`apply_removals`]: the rows of a character that is in the world (inventory, own bank, pockets, penya), one `RemovalApply` batch per target |
//! | [`join`] | the enter-world pass: [`join::stash`] at the end of `WorldRules::join`, [`join::on_joined`] from `WorldRules::joined` (removals first, then grants) |
//!
//! **Who delivers when.** `Game::new` installs [`deliver()`] in `Services.grants.deliver`; the poll P12 (period
//! `gameplay.grant_poll_s`, `0` = no job) hands it the open grants of the characters in the world that the guard has
//! not seen; the enter-world pass hands it the grants of the `CharacterLoad`. A receiver that is not an in-world
//! avatar with a live untainted session is left alone entirely (the rows stay open, nothing is sent or marked).
//! Online-character removals have no poll (the poll reads grants only): they apply at the next enter-world, as in
//! the original (WP-GM-27 may add one).
//!
//! **Guards.** The ids of a pass enter [`GrantGuard`](crate::persist::grants::GrantGuard) when they go into the
//! batch (`in_flight`) and stay there after the commit for ten minutes (`recent`): a poll result that was read
//! before the commit can arrive after it and must not deliver twice; the admin console's cancel of an id in flight
//! is refused (`delivery_in_progress`). A batch that comes back `Failed` is a persistence fault (13 §16.6): the ids
//! stay in flight for the rest of the run, because a retry would duplicate the item. Nothing here emits
//! `AdminOfflineEdit`: that is the admin console's, under `OnlineRegistry::admin_hold`.

pub mod deliver;
pub mod join;
pub mod removals;

pub use deliver::deliver;
pub use join::{on_joined, stash};
pub use removals::apply_removals;
