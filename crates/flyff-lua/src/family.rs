//! The live script families (Plan/20 §3.2, §4): one sandboxed state per family, created from
//! `GameData.lua.live_scripts`, with the fault state machine of §3.8.
//!
//! ```text
//! Missing  ── no source (the script file is absent or was rejected by the data bridge)
//! Ready    ── loaded; calls run
//! Faulted  ── the load or a call failed; every call returns LuaFault::Faulted until a reload succeeds
//! ```
//!
//! A family call never re-enters the game (Lua sees only `TRACE` / `ERROR` and the standard library), so calls
//! are safe from any game phase.

use crate::clock::LuaClock;
use crate::limits::LuaLimits;
use crate::sandbox::{Sandbox, SandboxSpec};
use crate::{LuaFault, LuaInit};
use flyff_common::GameMs;
use flyff_data::lua_config::LuaSource;
use flyff_vfs::Vfs;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// A live script family (Plan/20 §3.2). The order is the load order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScriptFamily {
    /// `Event.lua` + `LuaFunc/EventFunc.lua`: timed events, the rates (doc 22).
    Event,
    /// `Tax.lua` + `LuaFunc/TaxFunc.lua` (doc 22).
    Tax,
    /// `SecretRoom.lua` + `LuaFunc/SecretRoomFunc.lua` (doc 22).
    SecretRoom,
    /// `RainbowRace.lua` + `LuaFunc/RainbowRaceFunc.lua` (doc 22).
    RainbowRace,
    /// `GuildCombat1TO1.lua` (doc 21).
    GuildCombat1to1,
}

impl ScriptFamily {
    /// Every family in load order.
    pub const ALL: [ScriptFamily; 5] = [
        ScriptFamily::Event,
        ScriptFamily::Tax,
        ScriptFamily::SecretRoom,
        ScriptFamily::RainbowRace,
        ScriptFamily::GuildCombat1to1,
    ];

    /// The key in `GameData.lua.live_scripts` (Plan/14 §6.17).
    pub fn logical_name(self) -> &'static str {
        match self {
            ScriptFamily::Event => "event",
            ScriptFamily::Tax => "tax",
            ScriptFamily::SecretRoom => "secret_room",
            ScriptFamily::RainbowRace => "rainbow_race",
            ScriptFamily::GuildCombat1to1 => "guild_combat_1to1",
        }
    }

    /// The RNG stream of the state (10 §4.8).
    pub fn stream_name(self) -> &'static str {
        match self {
            ScriptFamily::Event => "lua_event",
            ScriptFamily::Tax => "lua_tax",
            ScriptFamily::SecretRoom => "lua_secret_room",
            ScriptFamily::RainbowRace => "lua_rainbow_race",
            ScriptFamily::GuildCombat1to1 => "lua_guild_combat_1to1",
        }
    }

    /// The default file below the Resource root (the VFS is case-insensitive).
    pub fn default_file(self) -> &'static str {
        match self {
            ScriptFamily::Event => "Event.lua",
            ScriptFamily::Tax => "Tax.lua",
            ScriptFamily::SecretRoom => "SecretRoom.lua",
            ScriptFamily::RainbowRace => "RainbowRace.lua",
            ScriptFamily::GuildCombat1to1 => "GuildCombat1TO1.lua",
        }
    }
}

/// Where a family stands (Plan/20 §3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FamilyStatus {
    /// Loaded; calls run.
    Ready,
    /// No script (consumers use their documented defaults; not an error).
    Missing,
    /// The load or a call failed; calls return [`LuaFault::Faulted`] until a reload succeeds.
    Faulted {
        /// In-game time of the failure.
        at: GameMs,
        /// The error text.
        error: String,
    },
}

/// Per-family call counters (the `/metrics` source, Plan/20 §3.10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FamilyStats {
    /// Calls that ran (`call` / `call_opt` of an existing function).
    pub calls: u64,
    /// Faults that put the family into `Faulted`.
    pub faults: u64,
}

/// The shared context of building a family state.
pub(crate) struct FamilyCx {
    pub seed: u64,
    pub limits: LuaLimits,
    pub clock: Rc<dyn LuaClock>,
    pub vfs: Option<Arc<Vfs>>,
}

/// One family: its status, its sandbox and the source it was built from.
pub(crate) struct FamilyState {
    pub status: RefCell<FamilyStatus>,
    pub sandbox: Option<Sandbox>,
    pub source: Option<LuaSource>,
    pub calls: Cell<u64>,
    pub faults: Cell<u64>,
}

impl FamilyState {
    /// A family without a script.
    pub(crate) fn missing() -> FamilyState {
        FamilyState {
            status: RefCell::new(FamilyStatus::Missing),
            sandbox: None,
            source: None,
            calls: Cell::new(0),
            faults: Cell::new(0),
        }
    }

    /// Builds the state of `family` from `source` and runs the script. A load failure still yields a state
    /// (`Faulted`, with the failure) so that boot keeps it; a reload discards it and keeps the old one.
    pub(crate) fn load(
        cx: &FamilyCx,
        family: ScriptFamily,
        source: &LuaSource,
    ) -> (FamilyState, Option<LuaFault>) {
        let failed = |fault: LuaFault| {
            let st = FamilyState {
                status: RefCell::new(FamilyStatus::Faulted {
                    at: cx.clock.game_ms(),
                    error: fault.to_string(),
                }),
                sandbox: None,
                source: Some(source.clone()),
                calls: Cell::new(0),
                faults: Cell::new(1),
            };
            (st, Some(fault))
        };
        let sandbox = match Sandbox::new(SandboxSpec {
            label: family.logical_name().to_owned(),
            stream: family.stream_name().to_owned(),
            seed: cx.seed,
            memory_limit: cx.limits.family_memory_bytes,
            clock: Rc::clone(&cx.clock),
            vfs: cx.vfs.clone(),
            constants: None,
        }) {
            Ok(sb) => sb,
            Err(e) => return failed(LuaFault::Runtime(e.to_string())),
        };
        if let Err(fault) =
            sandbox.run_source(&source.path, &source.text, cx.limits.load_instructions)
        {
            return failed(fault);
        }
        (
            FamilyState {
                status: RefCell::new(FamilyStatus::Ready),
                sandbox: Some(sandbox),
                source: Some(source.clone()),
                calls: Cell::new(0),
                faults: Cell::new(0),
            },
            None,
        )
    }

    pub(crate) fn status(&self) -> FamilyStatus {
        self.status.borrow().clone()
    }

    /// Puts the family into `Faulted` after a failed call.
    pub(crate) fn fault(&self, family: ScriptFamily, at: GameMs, fault: &LuaFault) {
        self.faults.set(self.faults.get() + 1);
        let error = fault.to_string();
        tracing::error!(
            target: "lua",
            family = family.logical_name(),
            error = %error,
            "Lua family faulted: calls are refused until it is reloaded"
        );
        *self.status.borrow_mut() = FamilyStatus::Faulted { at, error };
    }
}

/// Makes a [`LuaInit`] usable where only the pieces are needed.
pub(crate) fn cx_of(init: &LuaInit, clock: Rc<dyn LuaClock>) -> FamilyCx {
    FamilyCx {
        seed: init.seed,
        limits: init.limits,
        clock,
        vfs: init.vfs.clone(),
    }
}
