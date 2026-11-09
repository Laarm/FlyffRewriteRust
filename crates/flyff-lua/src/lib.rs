//! `flyff-lua` — tier G (Plan/20, D2): the `mlua` (Lua 5.3, vendored) host. The VM is `!Send` and is built
//! **on** the game thread from a [`LuaInit`] description (Plan/01 A5).
//!
//! * `sandbox`: one sandboxed state (standard-library subset, protected base, ordered `pairs`, `TRACE` /
//!   `ERROR`, VFS-backed `dofile` / `loadfile` / `require`),
//! * [`clock`]: [`LuaClock`] and the deterministic `os.date` / `os.time` / `os.clock`,
//! * [`rng`]: `math.random` on a named per-state stream, [`limits`]: instruction and memory budgets,
//! * [`family`] + [`LuaHost`]: the five live script families of the Resource folder (`Event`, `Tax`,
//!   `SecretRoom`, `RainbowRace`, `GuildCombat1to1`) with `call` / `call_opt` / `global`, the fault policy
//!   (a Lua error is logged, the family is `Faulted`, nothing exits) and `reload`,
//! * [`value`]: [`HVal`] arguments and the serde bridge for results; no `mlua` type is public.
//!
//! The configuration-only scripts (`PartyDungeon`, `MonsterSkill`, ...) are *not* run here: `flyff-data`
//! extracts them once at load into `GameData.lua` (Plan/14 §6.17). The NPC dialog state and the 100 ScriptLib
//! helpers (Plan/20 §5-§7) arrive with their work packages; [`ScriptSources`] and [`ReloadTarget::Npc`] are
//! their descriptions.

#![warn(missing_docs)]

pub mod clock;
pub mod family;
pub mod limits;
pub mod npc;
pub mod rng;
pub(crate) mod sandbox;

pub mod value;

pub use clock::{CivilFields, CivilTime, LuaClock};
pub use family::{FamilyStats, FamilyStatus, ScriptFamily};
pub use limits::LuaLimits;
pub use npc::{
    DIALOG_KEY_LINES, DialogKeyMap, Helper, HostError, NpcLoadReport, NpcRuntime, NpcScriptId,
    NpcStats, ScriptHost,
};
pub use value::HVal;

use family::FamilyState;
use flyff_common::UnixMs;
use flyff_data::lua_config::LuaSource;
use flyff_data::{DiagCode, Diagnostic, GameData, Severity};
use flyff_vfs::{Vfs, hint_for};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

/// One NPC script file read at start-up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFile {
    /// Path relative to the script directory (`/`-separated).
    pub rel_path: String,
    /// File text (UTF-8).
    pub text: String,
    /// SHA-256 of the file bytes (reported by `flyff check` against `MANIFEST.txt`).
    pub sha256: [u8; 32],
}

/// The pre-read `scripts/npc/*.lua` files, keyed by lower-case stem (Plan/20 §3.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptSources {
    /// Label used in log lines.
    pub dir_label: String,
    /// Files by lower-case stem.
    pub files: BTreeMap<String, ScriptFile>,
}

impl ScriptSources {
    /// Reads every `*.lua` file directly inside `dir`. Called by `flyff-server` during start-up,
    /// never from the game thread. A missing directory yields an empty set.
    #[allow(clippy::disallowed_methods)] // start-up I/O in the server's main thread, not game logic
    pub fn read(dir: &Path) -> Result<ScriptSources, std::io::Error> {
        let mut out = ScriptSources {
            dir_label: dir.display().to_string(),
            files: BTreeMap::new(),
        };
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let path = entry?.path();
            let is_lua = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lua"));
            if !is_lua || !path.is_file() {
                continue;
            }
            let bytes = std::fs::read(&path)?; // det-ok: start-up I/O in flyff-server's main thread
            let Some(stem) = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
            else {
                continue;
            };
            let rel_path = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let sha256: [u8; 32] = Sha256::digest(&bytes).into();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            out.files.insert(
                stem,
                ScriptFile {
                    rel_path,
                    text,
                    sha256,
                },
            );
        }
        Ok(out)
    }
}

/// Which scripts an admin reload targets (Plan/20 §3.9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReloadTarget {
    /// All NPC dialog scripts.
    Npc,
    /// One NPC script by stem.
    NpcOne(String),
    /// One live family (`/lua event`, `/lua rr`).
    Family(ScriptFamily),
    /// All live families and the NPC scripts.
    All,
}

/// The outcome of a reload (Plan/20 §3.9).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReloadReport {
    /// The parts that were reloaded (`"event"`, ...).
    pub ok: Vec<String>,
    /// The parts that failed, with the error; their previous state was kept.
    pub failed: Vec<(String, String)>,
    /// `true` when at least one failure left an older working version in place.
    pub kept_previous: bool,
}

/// Description of the Lua environment, `Send` so it can cross to the game thread (Plan/01 A5).
#[derive(Clone, Debug, Default)]
pub struct LuaInit {
    /// Pre-read `scripts/npc/*.lua`.
    pub npc_sources: ScriptSources,
    /// Resource limits.
    pub limits: LuaLimits,
    /// `GameInit.seed`; per-state RNG streams derive from it.
    pub seed: u64,
    /// The resource tree behind the scripts' `dofile` / `require` (Plan/14 §2.8). `None`: a script that
    /// `dofile`s a helper fails to load (the family is `Faulted`).
    pub vfs: Option<Arc<Vfs>>,
}

/// Errors of the Lua host.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LuaError {
    /// The Lua runtime raised an error.
    #[error("lua error: {0}")]
    Lua(String),
}

impl From<mlua::Error> for LuaError {
    fn from(e: mlua::Error) -> Self {
        LuaError::Lua(e.to_string())
    }
}

/// Why a Lua call or load did not produce a value (Plan/20 §3.1). Never fatal: the host logs it and the
/// consumer uses its documented defaults.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LuaFault {
    /// The chunk does not compile.
    #[error("syntax: {0}")]
    Syntax(String),
    /// A Lua runtime error.
    #[error("runtime: {0}")]
    Runtime(String),
    /// An instruction, memory or nesting limit was hit.
    #[error("limit: {0}")]
    Limit(String),
    /// The family is `Faulted` and refuses calls until a reload succeeds.
    #[error("family {0:?} faulted")]
    Faulted(ScriptFamily),
    /// The family has no script.
    #[error("family {0:?} missing")]
    Missing(ScriptFamily),
    /// The result could not be converted to the requested Rust type.
    #[error("conversion: {0}")]
    Conversion(String),
    /// The NPC script failed earlier and refuses calls until it is reloaded (Plan/20 §3.8).
    #[error("NPC script {0} faulted")]
    NpcFaulted(String),
}

fn diag(severity: Severity, code: DiagCode, file: &str, message: String) -> Diagnostic {
    Diagnostic {
        severity,
        code,
        file: Some(Arc::from(file)),
        line: 0,
        message,
    }
}

fn fault_code(f: &LuaFault) -> DiagCode {
    match f {
        LuaFault::Syntax(_) => DiagCode::LuaSyntax,
        LuaFault::Limit(_) => DiagCode::LuaLimit,
        _ => DiagCode::LuaError,
    }
}

/// The Lua VM host. `!Send` by construction (`mlua::Lua` without the `send` feature).
pub struct LuaHost {
    init: LuaInit,
    clock: Option<Rc<dyn LuaClock>>,
    families: BTreeMap<ScriptFamily, FamilyState>,
    /// The NPC dialog scripts (`None` for a host built without game data).
    npc: Option<NpcRuntime>,
    npc_inputs: Option<npc::NpcInputs>,
}

impl LuaHost {
    /// A host without families: every family is `Missing`. Enough for the static-link proof of WP-OPS-50 and
    /// for tests of the evaluation helpers; the game builds its host with [`LuaHost::boot`].
    pub fn new(init: LuaInit) -> Result<Self, LuaError> {
        Ok(Self {
            init,
            clock: None,
            families: BTreeMap::new(),
            npc: None,
            npc_inputs: None,
        })
    }

    /// Builds the host: the NPC dialog state first (Plan/20 §3.4), then the five family states from
    /// `data.lua.live_scripts` in [`ScriptFamily`] order. A family without a source is `Missing`; one whose
    /// load fails is `Faulted`; an NPC file that does not load is not registered. Everything is reported in the
    /// returned diagnostics. Nothing here can fail the start-up: a broken Event.lua means neutral rates, a
    /// broken NPC file a silent NPC, not a dead server.
    pub fn boot(
        init: LuaInit,
        data: &GameData,
        clock: Rc<dyn LuaClock>,
    ) -> (LuaHost, Vec<Diagnostic>) {
        let cx = family::cx_of(&init, Rc::clone(&clock));
        let mut diags = Vec::new();
        let (npc, npc_inputs) = Self::boot_npc(&init, data, &clock, &mut diags);
        let mut families = BTreeMap::new();
        for f in ScriptFamily::ALL {
            let st = match data.lua.live_scripts.get(f.logical_name()) {
                None => FamilyState::missing(),
                Some(src) => {
                    let (st, fault) = FamilyState::load(&cx, f, src);
                    if let Some(fault) = fault {
                        tracing::error!(
                            target: "lua",
                            family = f.logical_name(),
                            error = %fault,
                            "Lua family failed to load"
                        );
                        diags.push(diag(
                            Severity::Error,
                            fault_code(&fault),
                            &src.path,
                            format!("{}: {fault}", src.path),
                        ));
                    }
                    st
                }
            };
            families.insert(f, st);
        }
        (
            LuaHost {
                init,
                clock: Some(clock),
                families,
                npc,
                npc_inputs,
            },
            diags,
        )
    }

    /// The NPC state of [`LuaHost::boot`]: the key map from `WorldDialog.txt`, the `C` resolver over the define
    /// table, the scripts of `init.npc_sources`.
    fn boot_npc(
        init: &LuaInit,
        data: &GameData,
        clock: &Rc<dyn LuaClock>,
        diags: &mut Vec<Diagnostic>,
    ) -> (Option<NpcRuntime>, Option<npc::NpcInputs>) {
        let (keys, key_diags) = DialogKeyMap::build(&data.dialog_text);
        // an empty host (no dialog table and no scripts, e.g. the M1 skeleton and most unit tests) is silent
        if !data.dialog_text.is_empty() || !init.npc_sources.files.is_empty() {
            diags.extend(key_diags);
        }
        let defines = Arc::clone(&data.defines);
        let inputs = npc::NpcInputs {
            keys,
            constants: Rc::new(move |name: &str| defines.get(name).map(i64::from)),
        };
        match npc::NpcState::build(
            &init.npc_sources,
            &inputs,
            init.limits,
            init.seed,
            Rc::clone(clock),
        ) {
            Ok((state, load)) => {
                diags.extend(load.diagnostics);
                (Some(NpcRuntime(Rc::new(state))), Some(inputs))
            }
            Err(fault) => {
                tracing::error!(target: "lua", error = %fault, "the NPC state could not be built");
                diags.push(diag(
                    Severity::Error,
                    DiagCode::LuaError,
                    "scripts/npc",
                    format!("the NPC state could not be built: {fault}"),
                ));
                (None, Some(inputs))
            }
        }
    }

    /// The init description the host was built from.
    pub fn init(&self) -> &LuaInit {
        &self.init
    }

    /// The time source of the families (`None` for a host built with [`LuaHost::new`]).
    pub fn clock(&self) -> Option<Rc<dyn LuaClock>> {
        self.clock.clone()
    }

    // ---- NPC scripts (Plan/20 §6, §7) -------------------------------------------------------------------------

    /// The handle of the loaded NPC scripts; `None` for a host built without game data
    /// ([`LuaHost::new`]). The game takes one per request so that its host can borrow everything else.
    pub fn npc_runtime(&self) -> Option<NpcRuntime> {
        self.npc.clone()
    }

    /// The script `script_key` (`NpcDef.dialog_key`); `None` for an NPC without a script (KQ-26).
    pub fn npc_script(&self, script_key: &str) -> Option<NpcScriptId> {
        self.npc.as_ref()?.script(script_key)
    }

    /// `true` when the script has a function at `key_line` (an empty function counts).
    pub fn has_key(&self, s: NpcScriptId, key_line: u16) -> bool {
        self.npc.as_ref().is_some_and(|n| n.has_key(s, key_line))
    }

    /// The dialog key map (disabled and empty without game data).
    pub fn dialog_keys(&self) -> &DialogKeyMap {
        static NO_KEYS: DialogKeyMap = DialogKeyMap::DISABLED;
        self.npc.as_ref().map_or(&NO_KEYS, NpcRuntime::dialog_keys)
    }

    /// Runs the NPC function at `key_line` against `host` ([`NpcRuntime::call`]).
    pub fn call_npc(
        &self,
        s: NpcScriptId,
        key_line: u16,
        host: &mut dyn ScriptHost,
    ) -> Result<(), LuaFault> {
        self.npc
            .as_ref()
            .ok_or_else(|| LuaFault::Runtime("the host has no NPC state".to_owned()))?
            .call(s, key_line, host)
    }

    /// `true` when the script failed and is refused until it is reloaded.
    pub fn npc_faulted(&self, s: NpcScriptId) -> bool {
        self.npc.as_ref().is_some_and(|n| n.faulted(s))
    }

    // ---- live families (Plan/20 §4) ---------------------------------------------------------------------------

    /// Where family `f` stands.
    pub fn family_status(&self, f: ScriptFamily) -> FamilyStatus {
        self.families
            .get(&f)
            .map_or(FamilyStatus::Missing, FamilyState::status)
    }

    /// The call counters of `f`.
    pub fn family_stats(&self, f: ScriptFamily) -> FamilyStats {
        self.families
            .get(&f)
            .map_or_else(FamilyStats::default, |s| FamilyStats {
                calls: s.calls.get(),
                faults: s.faults.get(),
            })
    }

    /// The source `f` runs (`None` when `Missing`).
    pub fn family_source(&self, f: ScriptFamily) -> Option<&LuaSource> {
        self.families.get(&f).and_then(|s| s.source.as_ref())
    }

    /// Calls the global function `func` of family `f` (Plan/20 §4.3). Several Lua results become a sequence, so
    /// `R` may be a tuple; a single `nil` converts to `Option::None`. A missing function is
    /// `Err(LuaFault::Runtime)` (use [`LuaHost::call_opt`] for optional getters). A runtime error or an
    /// exhausted limit returns `Err` and puts the family into `Faulted`.
    pub fn call<R: DeserializeOwned>(
        &self,
        f: ScriptFamily,
        func: &str,
        args: &[HVal],
    ) -> Result<R, LuaFault> {
        self.call_inner(f, func, args)?
            .ok_or_else(|| LuaFault::Runtime(format!("function '{func}' is not defined")))
    }

    /// Like [`LuaHost::call`], but `Ok(None)` when the function does not exist (the getters missing in
    /// `EventFunc.lua`, KQ-11).
    pub fn call_opt<R: DeserializeOwned>(
        &self,
        f: ScriptFamily,
        func: &str,
        args: &[HVal],
    ) -> Result<Option<R>, LuaFault> {
        self.call_inner(f, func, args)
    }

    /// `true` when family `f` is `Ready` and its script defines the global function `name` (a pure lookup in the
    /// family's environment: nothing is called and no counter moves). `false` for a missing function, a global that
    /// is not a function and a family that is not up. The host-API coverage test of RK-M7-02 uses it.
    pub fn has_function(&self, f: ScriptFamily, name: &str) -> bool {
        self.ready(f)
            .ok()
            .and_then(|st| st.sandbox.as_ref())
            .is_some_and(|sb| matches!(sb.global(name), Ok(mlua::Value::Function(_))))
    }

    fn ready(&self, f: ScriptFamily) -> Result<&FamilyState, LuaFault> {
        let st = self.families.get(&f).ok_or(LuaFault::Missing(f))?;
        match st.status() {
            FamilyStatus::Ready => Ok(st),
            FamilyStatus::Missing => Err(LuaFault::Missing(f)),
            FamilyStatus::Faulted { .. } => Err(LuaFault::Faulted(f)),
        }
    }

    fn call_inner<R: DeserializeOwned>(
        &self,
        f: ScriptFamily,
        func: &str,
        args: &[HVal],
    ) -> Result<Option<R>, LuaFault> {
        let st = self.ready(f)?;
        let sb = st
            .sandbox
            .as_ref()
            .ok_or_else(|| LuaFault::Runtime("family has no state".to_owned()))?;
        let Ok(mlua::Value::Function(function)) = sb.global(func) else {
            return Ok(None);
        };
        let mut lua_args = mlua::MultiValue::new();
        for a in args {
            lua_args.push_back(
                value::to_value(&sb.lua, a).map_err(|e| LuaFault::Conversion(e.to_string()))?,
            );
        }
        st.calls.set(st.calls.get() + 1);
        match sb.call(
            &function,
            lua_args,
            self.init.limits.family_call_instructions,
        ) {
            Ok(results) => value::convert::<R>(&sb.lua, results).map(Some),
            Err(fault) => {
                let at = self
                    .clock
                    .as_ref()
                    .map_or_else(Default::default, |c| c.game_ms());
                st.fault(f, at, &fault);
                Err(fault)
            }
        }
    }

    /// Reads the global `name` of family `f` by name (no dotted paths: read the whole table into a Rust type
    /// and index it there). `Ok(None)` when the script did not define it.
    pub fn global<R: DeserializeOwned>(
        &self,
        f: ScriptFamily,
        name: &str,
    ) -> Result<Option<R>, LuaFault> {
        let st = self.ready(f)?;
        let sb = st
            .sandbox
            .as_ref()
            .ok_or_else(|| LuaFault::Runtime("family has no state".to_owned()))?;
        match sb.global(name) {
            Ok(mlua::Value::Nil) => Ok(None),
            Ok(v) => value::convert_one::<R>(&sb.lua, v).map(Some),
            Err(e) => Err(LuaFault::Runtime(e.to_string())),
        }
    }

    /// Runs `body` with the wall clock of the families pinned to `wall` and unpins afterwards, also when
    /// `body` unwinds (22 §2.4 rule 4: a replayed minute is evaluated with its own time). Without a clock
    /// `body` simply runs.
    pub fn with_wall_pinned<T>(&self, wall: UnixMs, body: impl FnOnce(&LuaHost) -> T) -> T {
        struct Unpin<'a>(Option<&'a Rc<dyn LuaClock>>);
        impl Drop for Unpin<'_> {
            fn drop(&mut self) {
                if let Some(c) = self.0 {
                    c.unpin();
                }
            }
        }
        if let Some(c) = &self.clock {
            c.pin(wall);
        }
        let _guard = Unpin(self.clock.as_ref());
        body(self)
    }

    /// Bytes in use by all families and the NPC state.
    pub fn memory_used(&self) -> u64 {
        self.families
            .values()
            .filter_map(|s| s.sandbox.as_ref())
            .map(|sb| sb.memory_used() as u64)
            .sum::<u64>()
            + self.npc.as_ref().map_or(0, NpcRuntime::memory_used)
    }

    // ---- reload (Plan/20 §3.9) --------------------------------------------------------------------------------

    /// Reloads `target`: the family scripts are re-read from the resource tree and re-executed from scratch in a
    /// fresh state (Lua-side state such as `tEvent[i].State` restarts at its initial values, so the consumer
    /// must re-run its evaluation afterwards). Only a state that loads completely replaces the old one; a
    /// failure keeps the previous version and is listed in [`ReloadReport::failed`]. A family that was
    /// `Faulted` or `Missing` and still fails stays so.
    ///
    /// The NPC targets (Plan/20 §3.9) compile every file of the target into a fresh state first and swap the
    /// whole registry only when all of them load: `npc_sources` is the new directory content (`None`: the sources
    /// the host already has, which clears the faulted flags), for `NpcOne(stem)` only that file is taken from it.
    /// Open dialogs survive: their offers are strings, the next request runs the new function.
    pub fn reload(
        &mut self,
        target: ReloadTarget,
        npc_sources: Option<ScriptSources>,
    ) -> ReloadReport {
        let mut report = ReloadReport::default();
        let (which, npc_target): (Vec<ScriptFamily>, Option<Option<String>>) = match target {
            ReloadTarget::Family(f) => (vec![f], None),
            ReloadTarget::All => (ScriptFamily::ALL.to_vec(), Some(None)),
            ReloadTarget::Npc => (Vec::new(), Some(None)),
            ReloadTarget::NpcOne(stem) => (Vec::new(), Some(Some(stem.to_ascii_lowercase()))),
        };
        if let Some(only) = npc_target {
            self.reload_npc(only, npc_sources, &mut report);
        }
        for f in which {
            let name = f.logical_name().to_owned();
            match self.reload_family(f) {
                Ok(true) => report.ok.push(name),
                Ok(false) => {} // no script, none before: nothing to reload
                Err(msg) => {
                    let had_working = self
                        .families
                        .get(&f)
                        .is_some_and(|s| matches!(s.status(), FamilyStatus::Ready));
                    report.kept_previous |= had_working;
                    tracing::warn!(target: "lua", family = %name, error = %msg, "Lua reload failed");
                    report.failed.push((name, msg));
                }
            }
        }
        report
    }

    /// The NPC part of [`LuaHost::reload`]. A host without game data has no NPC state and nothing to reload.
    fn reload_npc(
        &mut self,
        only: Option<String>,
        given: Option<ScriptSources>,
        report: &mut ReloadReport,
    ) {
        let (Some(inputs), Some(clock)) = (self.npc_inputs.clone(), self.clock.clone()) else {
            return;
        };
        let current = &self.init.npc_sources;
        let candidate = match (&only, given) {
            (None, Some(new)) => new,
            (None, None) | (Some(_), None) => current.clone(),
            (Some(stem), Some(new)) => {
                let mut merged = current.clone();
                match new.files.get(stem) {
                    Some(file) => {
                        merged.files.insert(stem.clone(), file.clone());
                    }
                    None => {
                        merged.files.remove(stem);
                    }
                }
                merged
            }
        };
        if candidate.files.is_empty() && self.npc.as_ref().is_none_or(|n| n.script_count() == 0) {
            return; // no script, none before: nothing to reload
        }
        let name = only.clone().unwrap_or_else(|| "npc".to_owned());
        match npc::NpcState::build(&candidate, &inputs, self.init.limits, self.init.seed, clock) {
            Ok((state, load)) if load.failed.is_empty() => {
                for d in &load.diagnostics {
                    tracing::warn!(target: "lua", "{d}");
                }
                self.npc = Some(NpcRuntime(Rc::new(state)));
                self.init.npc_sources = candidate;
                report.ok.push(name);
            }
            Ok((_, load)) => {
                report.kept_previous |= self.npc.is_some();
                tracing::warn!(
                    target: "lua",
                    failed = load.failed.len(),
                    "NPC reload failed: the previous scripts stay"
                );
                report.failed.extend(load.failed);
            }
            Err(fault) => {
                report.kept_previous |= self.npc.is_some();
                report.failed.push((name, fault.to_string()));
            }
        }
    }

    /// `Ok(true)` reloaded, `Ok(false)` there is no script and there never was one.
    fn reload_family(&mut self, f: ScriptFamily) -> Result<bool, String> {
        let clock = self
            .clock
            .clone()
            .ok_or_else(|| "the host has no clock (it was not booted with game data)".to_owned())?;
        let vfs = self
            .init
            .vfs
            .clone()
            .ok_or_else(|| "the host has no resource tree".to_owned())?;
        let known = self.families.get(&f).and_then(|s| s.source.as_ref());
        let path = known.map_or_else(|| f.default_file().to_owned(), |s| s.path.clone());
        if known.is_none() && !vfs.exists(&path) {
            // the file index is built at start: a script added later needs a restart
            return Ok(false);
        }
        let key = flyff_common::normalize_resource_path(&path).map_err(|e| e.to_string())?;
        let text = vfs
            .read_text_with(&path, hint_for(&key), false)
            .map_err(|e| format!("{path}: {e}"))?
            .text;
        let deps = flyff_data::lua_config::vm::scan_dofiles(&text)
            .iter()
            .filter_map(|d| flyff_vfs::lua_path_key(d).ok())
            .map(|p| p.as_str().to_owned())
            .collect();
        let source = LuaSource {
            path: key,
            text,
            deps,
        };
        let cx = family::cx_of(&self.init, clock);
        match FamilyState::load(&cx, f, &source) {
            (st, None) => {
                // the counters survive a reload
                let old = self.families.get(&f);
                st.calls.set(old.map_or(0, |o| o.calls.get()));
                st.faults.set(old.map_or(0, |o| o.faults.get()));
                self.families.insert(f, st);
                Ok(true)
            }
            (_, Some(fault)) => Err(fault.to_string()),
        }
    }

    // ---- scratch evaluation (tests, the static-link proof) ----------------------------------------------------

    /// The Lua version string (`"Lua 5.3"`).
    pub fn lua_version(&self) -> Result<String, LuaError> {
        self.eval_string("return _VERSION")
    }

    /// Runs `chunk` in a throw-away pure state (`table`, `string`, `math`, `utf8`; no `os`, no `io`) and
    /// returns its (first) result converted to a string.
    pub fn eval_string(&self, chunk: &str) -> Result<String, LuaError> {
        Ok(scratch()?.load(chunk).eval::<String>()?)
    }

    /// Runs `chunk` like [`LuaHost::eval_string`] and returns its integer result.
    pub fn eval_i64(&self, chunk: &str) -> Result<i64, LuaError> {
        Ok(scratch()?.load(chunk).eval::<i64>()?)
    }
}

fn scratch() -> Result<mlua::Lua, LuaError> {
    use mlua::{LuaOptions, StdLib};
    Ok(mlua::Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
        LuaOptions::default(),
    )?)
}
