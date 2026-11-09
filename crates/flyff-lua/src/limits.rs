//! Resource limits of a Lua state (Plan/20 §3.7): an instruction budget counted by a VM hook every
//! [`HOOK_EVERY`] instructions (deterministic, so replays fail identically), a memory limit per state, and the
//! nesting depth of NPC dialog calls. Violations are [`crate::LuaFault::Limit`], never an abort.

use mlua::{HookTriggers, Lua, VmState};
use std::cell::Cell;
use std::rc::Rc;

/// Instructions between two hook calls.
pub const HOOK_EVERY: u32 = 1000;

/// Resource limits (Plan/20 §3.7). Constants of the host, not configuration keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LuaLimits {
    /// VM instructions per top-level NPC call (including nested `Run` / flows).
    pub npc_call_instructions: u64,
    /// VM instructions per family call.
    pub family_call_instructions: u64,
    /// VM instructions per script load (family or NPC file); the budget of 14 §6.17.
    pub load_instructions: u64,
    /// Memory budget of one family state.
    pub family_memory_bytes: usize,
    /// Memory budget of the NPC state.
    pub npc_memory_bytes: usize,
    /// NPC dialog nesting (`Run`, flow re-entry).
    pub nesting_depth: u32,
}

impl Default for LuaLimits {
    fn default() -> Self {
        Self {
            npc_call_instructions: 1_000_000,
            family_call_instructions: 10_000_000,
            load_instructions: 50_000_000,
            family_memory_bytes: 8 * 1024 * 1024,
            npc_memory_bytes: 24 * 1024 * 1024,
            nesting_depth: 16,
        }
    }
}

/// The instruction counter shared between a state and its hook.
#[derive(Debug)]
pub(crate) struct Meter {
    used: Cell<u64>,
    budget: Cell<u64>,
    tripped: Cell<bool>,
}

impl Meter {
    pub(crate) fn new() -> Rc<Meter> {
        Rc::new(Meter {
            used: Cell::new(0),
            budget: Cell::new(u64::MAX),
            tripped: Cell::new(false),
        })
    }

    /// Starts a new budgeted run.
    pub(crate) fn arm(&self, budget: u64) {
        self.used.set(0);
        self.budget.set(budget);
        self.tripped.set(false);
    }

    /// `true` when the last run exhausted its budget.
    pub(crate) fn tripped(&self) -> bool {
        self.tripped.get()
    }

    /// The budget of the current run.
    pub(crate) fn budget(&self) -> u64 {
        self.budget.get()
    }
}

/// Installs the instruction hook of `meter` on `lua`.
pub(crate) fn install_hook(lua: &Lua, meter: &Rc<Meter>) -> mlua::Result<()> {
    let m = Rc::clone(meter);
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(HOOK_EVERY),
        move |_lua, _debug| {
            let used = m.used.get().saturating_add(u64::from(HOOK_EVERY));
            m.used.set(used);
            if used > m.budget.get() {
                m.tripped.set(true);
                return Err(mlua::Error::runtime("instruction budget exceeded"));
            }
            Ok(VmState::Continue)
        },
    )
}
