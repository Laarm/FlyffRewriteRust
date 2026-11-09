//! `event::flags`: the runtime event-flag store `g_eLocal` (Plan/22 §3, F-298, WP-EVENT-02; ref/15 §3.1).
//!
//! One byte per event id, 1024 of them (the legacy `CFlyffEvent`). Ids `0..=499` are the named slots of
//! `[events]` (`EventId`) and the unnamed `events.flag.<id>` overrides; they come from the effective
//! [`LiveConfig`] at start and follow every live change ([`EventServices`]'s [`LiveConsumer`]). Ids `500..=1023`
//! belong to the generic-event timer (22 §6): they are not part of `LiveConfig` and never persisted.
//!
//! * [`EventFlags::init`] builds the store from the initial `LiveConfig` (overrides already folded in by doc 10),
//! * [`EventServices::set_flag`] / [`set_flag`] validate, store, broadcast `SetLocalEvent` to everybody in a world
//!   and report whether the value changed,
//! * the join batch carries the raw array (`FlyffEvent`, position 14: [`super::join::FlyffEventStep`]),
//! * [`EventFlagQuery`] (15 §13.2) lets the NPC load filter of a newly created layer read it.
//!
//! A GM or administrator change of an id below 500 is not made here: it goes through doc 10's live change
//! (`AdminCmd::SetEvent` / `SetRuntime`), which writes the `runtime_override` row and calls the consumer; only ids
//! from 500 are set directly with [`FlagOrigin::Gm`] / [`FlagOrigin::Admin`] (22 §3.3).

use super::EventServices;
use crate::ctx::{GameCtx, Services};
use crate::live::{LiveConfig, LiveConsumer};
use crate::outbox::Audience;
use crate::world::EventFlagQuery;
use flyff_common::ObjId;
use flyff_common::config::EventId;
use flyff_proto::pb;

/// Size of the store (`MAX_EVENT`, src:_Common/flyffevent.h).
pub const MAX_EVENT: usize = 1024;

/// First id of the generic-event range (22 §6): never part of `LiveConfig`, never persisted.
pub const FIRST_GENERIC_ID: u16 = 500;

/// Who changes a flag (22 §3.1). Only the origin decides about the `Protected` rule; none of them persists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlagOrigin {
    /// The start-up values.
    Config,
    /// A game master (`/localevent`).
    Gm {
        /// The account.
        by: String,
    },
    /// An administrator (admin API or console).
    Admin {
        /// The actor.
        by: String,
    },
    /// The generic-event timer (22 §6.2).
    GenericTimer,
    /// A live change of the effective `LiveConfig` (a GM/admin `set`, a file reload).
    Live,
}

impl FlagOrigin {
    fn is_operator(&self) -> bool {
        matches!(self, FlagOrigin::Gm { .. } | FlagOrigin::Admin { .. })
    }

    /// The actor for the log line.
    fn label(&self) -> String {
        match self {
            FlagOrigin::Config => "config".to_owned(),
            FlagOrigin::Gm { by } => format!("gm:{by}"),
            FlagOrigin::Admin { by } => format!("admin:{by}"),
            FlagOrigin::GenericTimer => "generic".to_owned(),
            FlagOrigin::Live => "live".to_owned(),
        }
    }
}

/// Why a flag change was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FlagError {
    /// The id is not below [`MAX_EVENT`].
    #[error("event ids go up to {}", MAX_EVENT - 1)]
    OutOfRange,
    /// Event 1 (`EVE_18`) cannot be changed by a GM or an administrator (src:_Interface/FuncTextCmd.cpp:2576-2586).
    #[error("event 1 cannot be changed")]
    Protected,
}

/// The event-flag array (`CFlyffEvent`, 03 §4.1).
#[derive(Clone, PartialEq, Eq)]
pub struct EventFlags(pub(crate) [u8; MAX_EVENT]);

impl Default for EventFlags {
    fn default() -> Self {
        EventFlags([0; MAX_EVENT])
    }
}

impl std::fmt::Debug for EventFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.nonzero()).finish()
    }
}

/// The effective state of the id `id < 500` in `live`: an `event_flags` entry (named or not), else the `[events]`
/// default of a named slot, else 0 (22 §3.1 steps 2 and 3).
fn live_state(live: &LiveConfig, id: u16) -> u8 {
    if let Some(v) = live.event_flags.get(&id) {
        return *v;
    }
    live.events
        .initial_flags()
        .iter()
        .find(|(e, _)| e.id() == id)
        .map_or(0, |(_, v)| *v)
}

impl EventFlags {
    /// The store of the initial `LiveConfig` (22 §3.1): all zero, then the named `[events]` slots, then the
    /// `events.flag.<id>` entries; ids from 500 start at 0 (the generic timer sets them at its first run).
    pub fn init(live: &LiveConfig) -> EventFlags {
        let mut f = EventFlags::default();
        for id in 0..FIRST_GENERIC_ID {
            f.0[usize::from(id)] = live_state(live, id);
        }
        f
    }

    /// The state of `id`; an id of 1024 or more is 0 (the legacy bounds check).
    pub fn get(&self, id: u16) -> u8 {
        self.0.get(usize::from(id)).copied().unwrap_or(0)
    }

    /// `g_eLocal.GetState(id) != 0`.
    pub fn is_on(&self, id: EventId) -> bool {
        self.get(id.id()) != 0
    }

    /// The raw array in the legacy byte layout (the `FlyffEvent` message).
    pub fn bytes(&self) -> &[u8; MAX_EVENT] {
        &self.0
    }

    /// The ids whose state is not 0, ascending.
    pub fn nonzero(&self) -> impl Iterator<Item = (u16, u8)> + '_ {
        self.0
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0)
            .filter_map(|(i, v)| u16::try_from(i).ok().map(|i| (i, *v)))
    }

    /// Writes `state` into the slot (range check only) and returns whether it changed.
    pub(crate) fn store(&mut self, id: u16, state: u8) -> Result<bool, FlagError> {
        let slot = self
            .0
            .get_mut(usize::from(id))
            .ok_or(FlagError::OutOfRange)?;
        let changed = *slot != state;
        *slot = state;
        Ok(changed)
    }
}

impl EventFlagQuery for EventFlags {
    fn is_on(&self, id: EventId) -> bool {
        EventFlags::is_on(self, id)
    }
}

impl EventServices {
    /// `set_flag` (22 §3.2): validates `id < 1024` (`OutOfRange`) and, for a GM or administrator, `id != 1`
    /// (`Protected`); an unchanged value changes nothing and sends nothing. A change is stored and broadcast as
    /// `SetLocalEvent{id, state}` to everybody who is in a world; an id from 500 (generic factors) also
    /// recomputes the modifiers.
    ///
    /// Persistence is not done here: ids below 500 are persisted by doc 10's live change that leads to
    /// [`LiveConsumer::on_live_change`], ids from 500 never.
    pub fn set_flag(
        &mut self,
        ctx: &mut GameCtx<'_>,
        id: u16,
        state: u8,
        origin: FlagOrigin,
    ) -> Result<bool, FlagError> {
        if usize::from(id) >= MAX_EVENT {
            return Err(FlagError::OutOfRange);
        }
        if origin.is_operator() && id == EventId::Eve18.id() {
            return Err(FlagError::Protected);
        }
        let changed = self.flags.store(id, state)?;
        if !changed {
            return Ok(false);
        }
        tracing::info!(id, state, by = %origin.label(), "event flag changed");
        ctx.broadcast(
            None,
            ObjId(0),
            Audience::AllOnline,
            pb::SetLocalEvent {
                event_id: i32::from(id),
                state: u32::from(state),
            },
        );
        if id >= FIRST_GENERIC_ID {
            self.recompute_modifiers(ctx);
        }
        Ok(true)
    }

    /// Recomputes the cached [`EventModifiers`](super::EventModifiers) from their sources (22 §4.3: the Event.lua
    /// getters; the generic, lord and weather parts keep the values their services stored).
    pub fn recompute_modifiers(&mut self, ctx: &GameCtx<'_>) {
        self.lua_events.refresh(ctx.lua, ctx.data);
        let (exp, item_drop) = self.generic.factors(ctx.data, &self.flags);
        self.lua_events.set_generic_factors(exp, item_drop);
    }
}

/// `set_flag` of 22 §3.1 as a free function on doc 10's context pair (`/localevent` for ids from 500, the generic
/// timer).
pub fn set_flag(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    id: u16,
    state: u8,
    origin: FlagOrigin,
) -> Result<bool, FlagError> {
    s.event.set_flag(ctx, id, state, origin)
}

impl LiveConsumer for EventServices {
    /// 22 §3.3: for every id whose effective value differs between `old` and `new` (the named keys of `[events]`
    /// and the `events.flag.<id>` entries) `set_flag(.., FlagOrigin::Live)` runs: validation, the `SetLocalEvent`
    /// broadcast and the modifier recompute. An unchanged key sends nothing.
    fn on_live_change(&mut self, ctx: &mut GameCtx<'_>, old: &LiveConfig, new: &LiveConfig) {
        for id in 0..FIRST_GENERIC_ID {
            let (was, now) = (live_state(old, id), live_state(new, id));
            if was != now {
                // `Live` is never `Protected` and ids below 500 are in range
                let _ = self.set_flag(ctx, id, now, FlagOrigin::Live);
            }
        }
    }
}
