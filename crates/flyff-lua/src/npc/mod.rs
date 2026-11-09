//! The NPC dialog scripts (Plan/20 §3.3, §3.7-§3.9, §5.5, §6): the transpiled `content/npc/<script>.lua` files
//! loaded into one sandboxed state, the `C` define table, the dialog key map, the helper table and the call
//! entry point the game's dialog runtime uses.
//!
//! ```text
//! return { format = 1, script = "kepe_heron", defines = {...}, [0] = function(d) ... end, [1] = ..., }
//! ```
//!
//! Every file runs in its **own environment** (reads fall back to the trimmed standard library, a write to a name
//! of it is an error), is checked (`format == 1`, `defines` known, function keys within
//! [`DIALOG_KEY_LINES`]) and registered under its lower-case stem. A file that does not load is not registered:
//! the NPCs bound to it behave like scriptless NPCs (KQ-26) and the load report names it (§3.8).
//!
//! [`NpcRuntime`] is the cheap-to-clone (reference counted) handle of the loaded scripts. The game takes one
//! per request, so the host it passes to [`NpcRuntime::call`] may borrow the whole game state — including the
//! [`crate::LuaHost`] the handle came from — and a reload during a dialog cannot invalidate a running call (the
//! old registry lives until the call returns).

pub mod ctx;
pub mod helper;

pub use ctx::{HostError, ScriptHost};
pub use helper::{
    ArgKind, CtxReq, HELPER_COUNT, HELPERS, Helper, HelperSig, HelperStatus, dump_table,
};

use crate::clock::LuaClock;
use crate::limits::LuaLimits;
use crate::sandbox::{ConstantResolver, Sandbox, SandboxSpec, classify};
use crate::{LuaFault, ScriptSources};
use flyff_data::dialog_text::DialogText;
use flyff_data::{DiagCode, Diagnostic, Severity};
use mlua::chunk::ChunkMode;
use mlua::{Function, Table, Value};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

/// The `WorldDialog.txt` lines that are dialog keys: 0..=43, 865 and 866 (46 entries,
/// src:WORLDDIALOG/NpcScript.cpp:296-343 `g_nKeys`). The function keys of an NPC file are these line numbers.
pub const DIALOG_KEY_LINES: [u16; 46] = {
    let mut a = [0u16; 46];
    let mut i = 0;
    while i < 44 {
        a[i] = i as u16;
        i += 1;
    }
    a[44] = 865;
    a[45] = 866;
    a
};

/// A loaded NPC script (an index into the registry of one [`NpcRuntime`]; not stable across reloads).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NpcScriptId(pub u32);

/// The dialog keys: the **text** of each key line of `WorldDialog.txt` mapped to its line number (Plan/20 §5.5).
/// A duplicate text keeps the first line (`map::insert`); an empty key line disables every NPC dialog (the
/// original refused to start; the rewrite keeps the process up).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DialogKeyMap {
    by_text: BTreeMap<String, u16>,
    text_of: BTreeMap<u16, String>,
    disabled: bool,
}

impl DialogKeyMap {
    /// The map of a host without game data: no key, disabled.
    pub(crate) const DISABLED: DialogKeyMap = DialogKeyMap {
        by_text: BTreeMap::new(),
        text_of: BTreeMap::new(),
        disabled: true,
    };

    /// Builds the map from the dialog line table. `DialogKeyEmpty` diagnostics: one `Error` per empty key line; a
    /// table without any line (no `WorldDialog.txt`) is one `Warning`. Either way the map is
    /// [`disabled`](DialogKeyMap::is_disabled).
    pub fn build(dialog: &DialogText) -> (DialogKeyMap, Vec<Diagnostic>) {
        let mut map = DialogKeyMap::default();
        let mut diags = Vec::new();
        if dialog.is_empty() {
            map.disabled = true;
            diags.push(key_diag(
                Severity::Warning,
                "no WorldDialog.txt was loaded: the NPC dialogs are disabled".to_owned(),
            ));
            return (map, diags);
        }
        for line in DIALOG_KEY_LINES {
            let text = dialog.get(u32::from(line));
            if text.is_empty() {
                map.disabled = true;
                diags.push(key_diag(
                    Severity::Error,
                    format!("WorldDialog.txt line {line} is a dialog key and is empty: the NPC dialogs are disabled"),
                ));
                continue;
            }
            map.text_of.insert(line, text.to_owned());
            map.by_text.entry(text.to_owned()).or_insert(line);
        }
        (map, diags)
    }

    /// A map from explicit `(line, text)` pairs (tests); nothing is disabled.
    pub fn from_pairs(pairs: &[(u16, &str)]) -> DialogKeyMap {
        let mut map = DialogKeyMap::default();
        for (line, text) in pairs {
            map.text_of.insert(*line, (*text).to_owned());
            map.by_text.entry((*text).to_owned()).or_insert(*line);
        }
        map
    }

    /// The key line whose text is `text` (exact, case-sensitive), `None` when disabled or not a key.
    pub fn line_of(&self, text: &str) -> Option<u16> {
        if self.disabled {
            return None;
        }
        self.by_text.get(text).copied()
    }

    /// The text of key line `line`.
    pub fn text_of(&self, line: u16) -> Option<&str> {
        self.text_of.get(&line).map(String::as_str)
    }

    /// `true` when an empty key line (or a missing table) disabled the dialogs.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// The number of distinct key texts.
    pub fn len(&self) -> usize {
        self.by_text.len()
    }

    /// `true` when no key text is known.
    pub fn is_empty(&self) -> bool {
        self.by_text.is_empty()
    }
}

fn key_diag(severity: Severity, message: String) -> Diagnostic {
    Diagnostic {
        severity,
        code: DiagCode::DialogKeyEmpty,
        file: Some(Arc::from("WorldDialog.txt")),
        line: 0,
        message,
    }
}

/// What the NPC scripts need besides their sources: the key map and the define resolver. Kept by the host for
/// reloads.
#[derive(Clone)]
pub(crate) struct NpcInputs {
    pub keys: DialogKeyMap,
    pub constants: ConstantResolver,
}

/// The outcome of loading a set of NPC files.
#[derive(Debug, Default)]
pub struct NpcLoadReport {
    /// Warnings and errors (syntax errors, undefined constants, odd keys).
    pub diagnostics: Vec<Diagnostic>,
    /// The files that were not registered, with the reason.
    pub failed: Vec<(String, String)>,
    /// The number of registered scripts.
    pub loaded: usize,
}

/// Counters of the NPC state (the `/metrics` source, Plan/20 §3.10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NpcStats {
    /// Calls of NPC functions (outermost and nested).
    pub calls: u64,
    /// Calls that failed (each marks its script faulted).
    pub faults: u64,
}

struct Script {
    stem: String,
    funcs: BTreeMap<u16, Function>,
    faulted: Cell<bool>,
    sha256: [u8; 32],
}

/// The loaded NPC state (see the module documentation).
pub(crate) struct NpcState {
    // dropped first: the function handles outlive nothing
    scripts: Vec<Script>,
    by_name: BTreeMap<String, NpcScriptId>,
    keys: DialogKeyMap,
    limits: LuaLimits,
    methods: Table,
    depth: Cell<u32>,
    /// A limit that tripped inside a nested call: it keeps its class across the host boundary.
    pending: RefCell<Option<LuaFault>>,
    calls: Cell<u64>,
    faults: Cell<u64>,
    sandbox: Sandbox,
}

struct DepthGuard<'a>(&'a Cell<u32>);

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

fn file_diag(severity: Severity, code: DiagCode, file: &str, message: String) -> Diagnostic {
    Diagnostic {
        severity,
        code,
        file: Some(Arc::from(file)),
        line: 0,
        message,
    }
}

impl NpcState {
    /// Builds the state from `sources`: one sandbox (`lua_npc` stream, `C` table), one environment and one
    /// registration per file that loads. Files that fail are reported, not registered.
    pub(crate) fn build(
        sources: &ScriptSources,
        inputs: &NpcInputs,
        limits: LuaLimits,
        seed: u64,
        clock: Rc<dyn LuaClock>,
    ) -> Result<(NpcState, NpcLoadReport), LuaFault> {
        let sandbox = Sandbox::new(SandboxSpec {
            label: "npc".to_owned(),
            stream: "lua_npc".to_owned(),
            seed,
            memory_limit: limits.npc_memory_bytes,
            clock,
            vfs: None,
            constants: Some(Rc::clone(&inputs.constants)),
        })
        .map_err(|e| LuaFault::Runtime(e.to_string()))?;
        let methods =
            ctx::install_methods(&sandbox.lua).map_err(|e| LuaFault::Runtime(e.to_string()))?;
        let mut state = NpcState {
            scripts: Vec::new(),
            by_name: BTreeMap::new(),
            keys: inputs.keys.clone(),
            limits,
            methods,
            depth: Cell::new(0),
            pending: RefCell::new(None),
            calls: Cell::new(0),
            faults: Cell::new(0),
            sandbox,
        };
        let mut report = NpcLoadReport::default();
        for (stem, file) in &sources.files {
            match state.load_script(stem, &file.text, &file.sha256, inputs, &mut report) {
                Ok(script) => {
                    let id = NpcScriptId(u32::try_from(state.scripts.len()).unwrap_or(u32::MAX));
                    state.by_name.insert(stem.clone(), id);
                    state.scripts.push(script);
                }
                Err(fault) => {
                    tracing::error!(
                        target: "lua",
                        script = %stem,
                        error = %fault,
                        "NPC script failed to load: it is not registered"
                    );
                    let code = match &fault {
                        LuaFault::Syntax(_) => DiagCode::LuaSyntax,
                        LuaFault::Limit(_) => DiagCode::LuaLimit,
                        _ => DiagCode::LuaError,
                    };
                    report.diagnostics.push(file_diag(
                        Severity::Error,
                        code,
                        &file.rel_path,
                        format!("{}: {fault}", file.rel_path),
                    ));
                    report.failed.push((stem.clone(), fault.to_string()));
                }
            }
        }
        report.loaded = state.scripts.len();
        Ok((state, report))
    }

    /// Runs one file and checks the table it returns.
    fn load_script(
        &self,
        stem: &str,
        text: &str,
        sha256: &[u8; 32],
        inputs: &NpcInputs,
        report: &mut NpcLoadReport,
    ) -> Result<Script, LuaFault> {
        let env = self
            .sandbox
            .new_file_env()
            .map_err(|e| LuaFault::Runtime(e.to_string()))?;
        let chunk = format!("scripts/npc/{stem}.lua");
        self.sandbox.meter.arm(self.limits.load_instructions);
        let returned = self
            .sandbox
            .lua
            .load(text)
            .set_name(format!("@{chunk}"))
            .set_mode(ChunkMode::Text)
            .set_environment(env)
            .call::<Value>(())
            .map_err(|e| classify(&e, &self.sandbox.meter))?;
        let Value::Table(table) = returned else {
            return Err(LuaFault::Runtime(
                "the script must return a table".to_owned(),
            ));
        };
        let fail = |m: String| LuaFault::Runtime(m);
        match table.raw_get::<Value>("format") {
            Ok(Value::Integer(1)) => {}
            Ok(other) => {
                return Err(fail(format!(
                    "unsupported script format ({} instead of 1)",
                    describe(&other)
                )));
            }
            Err(e) => return Err(fail(e.to_string())),
        }
        match table.raw_get::<Value>("script") {
            Ok(Value::String(s)) if s.to_string_lossy().eq_ignore_ascii_case(stem) => {}
            Ok(other) => report.diagnostics.push(file_diag(
                Severity::Warning,
                DiagCode::LuaError,
                &chunk,
                format!(
                    "{chunk}: the script field ({}) does not name the file",
                    describe(&other)
                ),
            )),
            Err(e) => return Err(fail(e.to_string())),
        }
        // the constants the script claims to use must exist (an unknown one is an error when its function runs)
        if let Ok(Value::Table(defines)) = table.raw_get::<Value>("defines") {
            for v in defines.sequence_values::<String>().flatten() {
                if (inputs.constants)(&v).is_none() {
                    report.diagnostics.push(file_diag(
                        Severity::Error,
                        DiagCode::NpcScriptUndefinedConstant,
                        &chunk,
                        format!("{chunk}: undefined constant {v}"),
                    ));
                }
            }
        }
        let mut funcs = BTreeMap::new();
        let mut odd = Vec::new();
        table
            .for_each(|k: Value, v: Value| {
                if let Value::Integer(line) = k {
                    match (u16::try_from(line), v) {
                        (Ok(l), Value::Function(f)) if DIALOG_KEY_LINES.contains(&l) => {
                            funcs.insert(l, f);
                        }
                        _ => odd.push(line),
                    }
                }
                Ok(())
            })
            .map_err(|e| LuaFault::Runtime(e.to_string()))?;
        if !odd.is_empty() {
            odd.sort_unstable();
            report.diagnostics.push(file_diag(
                Severity::Warning,
                DiagCode::LuaError,
                &chunk,
                format!("{chunk}: ignored entries at non-key lines {odd:?}"),
            ));
        }
        Ok(Script {
            stem: stem.to_owned(),
            funcs,
            faulted: Cell::new(false),
            sha256: *sha256,
        })
    }

    fn script(&self, id: NpcScriptId) -> Option<&Script> {
        self.scripts.get(usize::try_from(id.0).ok()?)
    }

    fn note_limit(&self, fault: &LuaFault) {
        if matches!(fault, LuaFault::Limit(_)) {
            self.pending
                .borrow_mut()
                .get_or_insert_with(|| fault.clone());
        }
    }

    /// `call_npc` (Plan/20 §6.1, §3.7-§3.8).
    pub(crate) fn call(
        &self,
        id: NpcScriptId,
        key_line: u16,
        host: &mut dyn ScriptHost,
    ) -> Result<(), LuaFault> {
        let script = self
            .script(id)
            .ok_or_else(|| LuaFault::Runtime(format!("unknown NPC script id {}", id.0)))?;
        if script.faulted.get() {
            return Err(LuaFault::NpcFaulted(script.stem.clone()));
        }
        let Some(func) = script.funcs.get(&key_line) else {
            tracing::debug!(
                target: "lua.npc",
                script = %script.stem,
                key_line,
                "the script has no function for this key"
            );
            return Ok(());
        };
        let depth = self.depth.get();
        if depth >= self.limits.nesting_depth {
            let fault = LuaFault::Limit("nesting".to_owned());
            self.note_limit(&fault);
            return Err(fault);
        }
        if depth == 0 {
            self.sandbox.meter.arm(self.limits.npc_call_instructions);
            self.pending.borrow_mut().take();
        }
        self.depth.set(depth + 1);
        let _guard = DepthGuard(&self.depth);
        self.calls.set(self.calls.get() + 1);
        match ctx::run_function(&self.sandbox.lua, &self.methods, func, host) {
            Ok(()) => Ok(()),
            Err(e) => {
                let mut fault = classify(&e, &self.sandbox.meter);
                self.note_limit(&fault);
                if depth == 0 && !matches!(fault, LuaFault::Limit(_)) {
                    // a limit hit by a nested call came back through the host as a plain host error
                    if let Some(p) = self.pending.borrow_mut().take() {
                        fault = p;
                    }
                }
                script.faulted.set(true);
                self.faults.set(self.faults.get() + 1);
                tracing::error!(
                    target: "lua",
                    script = %script.stem,
                    key_line,
                    error = %fault,
                    "NPC script function failed: the script is disabled until it is reloaded"
                );
                Err(fault)
            }
        }
    }
}

/// The handle of the loaded NPC scripts (see the module documentation). Cloning is a reference-count bump.
#[derive(Clone)]
pub struct NpcRuntime(pub(crate) Rc<NpcState>);

impl NpcRuntime {
    /// The script `script_key` (the lower-case NPC prefix, `NpcDef.dialog_key`), `None` for the NPCs without a
    /// script (KQ-26).
    pub fn script(&self, script_key: &str) -> Option<NpcScriptId> {
        self.0.by_name.get(script_key).copied()
    }

    /// The stem of a registered script.
    pub fn script_name(&self, id: NpcScriptId) -> Option<&str> {
        self.0.script(id).map(|s| s.stem.as_str())
    }

    /// The stems of all registered scripts, ascending.
    pub fn script_names(&self) -> impl Iterator<Item = &str> {
        self.0.by_name.keys().map(String::as_str)
    }

    /// The number of registered scripts.
    pub fn script_count(&self) -> usize {
        self.0.scripts.len()
    }

    /// `true` when the script has a function at `key_line` (an empty function counts; the original
    /// `CWorldDialog::Find`).
    pub fn has_key(&self, id: NpcScriptId, key_line: u16) -> bool {
        self.0
            .script(id)
            .is_some_and(|s| s.funcs.contains_key(&key_line))
    }

    /// The key lines the script has a function for, ascending.
    pub fn script_keys(&self, id: NpcScriptId) -> Vec<u16> {
        self.0
            .script(id)
            .map(|s| s.funcs.keys().copied().collect())
            .unwrap_or_default()
    }

    /// The SHA-256 of the file the script was loaded from.
    pub fn script_sha256(&self, id: NpcScriptId) -> Option<[u8; 32]> {
        self.0.script(id).map(|s| s.sha256)
    }

    /// The dialog key map.
    pub fn dialog_keys(&self) -> &DialogKeyMap {
        &self.0.keys
    }

    /// Runs the function of `s` at `key_line` with `host` behind `d` (Plan/20 §6.1). A script without a function
    /// there is a no-op. A runtime error, an exhausted limit or a host error returns the fault and marks the
    /// script **faulted** (all its functions are refused until a reload, §3.8); side effects the host already
    /// applied stay. The host may call this again (nested `Run`, quest flows); the nesting depth is limited to
    /// `LuaLimits::nesting_depth` and the instruction budget is shared with the outermost call.
    pub fn call(
        &self,
        s: NpcScriptId,
        key_line: u16,
        host: &mut dyn ScriptHost,
    ) -> Result<(), LuaFault> {
        self.0.call(s, key_line, host)
    }

    /// `true` when the script failed and is refused until it is reloaded.
    pub fn faulted(&self, s: NpcScriptId) -> bool {
        self.0.script(s).is_some_and(|s| s.faulted.get())
    }

    /// Marks the script faulted (the game does this for a failure it detects itself).
    pub fn set_faulted(&self, s: NpcScriptId) {
        if let Some(s) = self.0.script(s) {
            s.faulted.set(true);
        }
    }

    /// The call counters.
    pub fn stats(&self) -> NpcStats {
        NpcStats {
            calls: self.0.calls.get(),
            faults: self.0.faults.get(),
        }
    }

    /// Bytes in use by the NPC state.
    pub fn memory_used(&self) -> u64 {
        self.0.sandbox.memory_used() as u64
    }
}

fn describe(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_owned(),
        Value::String(s) => format!("\"{}\"", s.to_string_lossy()),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.type_name().to_owned(),
    }
}
