//! The transient sandbox VM of the static configuration scripts (Plan/14 §6.17, Addendum R2, rule X6).
//!
//! One fresh Lua 5.3 state per script: the libraries `base`, `table`, `string`, `math` and `utf8` only (no
//! `io`, `debug`, `coroutine`, `package`; `string.dump` and bytecode loading are gone), an instruction-count
//! hook (50 M instructions → [`VmError::Limit`]), a 64 MiB memory limit, and a VFS-backed `dofile`,
//! `loadfile` and `require` (`.\\LuaFunc\\X.lua` resolves case-insensitively below the Resource root, the
//! `overrides/` layer first, Plan/14 §2.8). The source handed to Lua is the decoded UTF-8 text.
//!
//! `os.time`, `os.date` and `os.clock` raise an error: a load must not depend on the wall clock (the data
//! fingerprint and exports are byte-stable); none of the seven static scripts uses them. `TRACE`, `ERROR`
//! and `print` are no-ops. A Lua error never exits the process (the original did `exit(0)`).
//!
//! The VM is `!Send` and lives only for the duration of one [`run_config_script`] call plus the reads of
//! the caller; the live VMs belong to `flyff-lua` on the game thread.

use flyff_vfs::{Vfs, VfsPath, require_candidates};
use mlua::chunk::ChunkMode;
use mlua::{HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Value, VmState};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The instruction budget of one script load (Plan/14 §6.17).
pub const INSTRUCTION_LIMIT: u64 = 50_000_000;
/// The memory limit of the extraction VM (Plan/14 §6.17).
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// How many instructions pass between two hook calls.
const HOOK_EVERY: u32 = 1000;
/// Deepest `dofile`/`require` nesting.
const MAX_DEPTH: u32 = 16;

/// Why a script could not be executed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VmError {
    /// The chunk does not compile (`LuaSyntax`).
    Syntax {
        /// The message of Lua.
        message: String,
        /// 1-based line, 0 when unknown.
        line: u32,
    },
    /// A runtime error (`LuaError`).
    Runtime {
        /// The message of Lua, with the position.
        message: String,
        /// 1-based line, 0 when unknown.
        line: u32,
    },
    /// The instruction or memory limit was hit (`LuaLimit`).
    Limit(String),
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::Syntax { message, .. } | VmError::Runtime { message, .. } => {
                f.write_str(message)
            }
            VmError::Limit(m) => f.write_str(m),
        }
    }
}

/// The instruction counter shared with the hook.
#[derive(Debug, Default)]
struct Budget {
    used: Cell<u64>,
    tripped: Cell<bool>,
}

/// A finished script: the VM (its globals hold the configuration) and the files it `dofile`d.
pub struct Executed {
    /// The VM; read the configuration from [`Executed::globals`].
    pub lua: Lua,
    /// The helper files that were read, normalised, in order of first use.
    pub deps: Vec<VfsPath>,
}

impl Executed {
    /// The global table of the script.
    pub fn globals(&self) -> Table {
        self.lua.globals()
    }
}

/// Maps an mlua error to a [`VmError`] (an exhausted budget wins over whatever the hook's error became).
fn classify(e: &mlua::Error, budget: &Budget) -> VmError {
    if budget.tripped.get() {
        return VmError::Limit(format!(
            "instruction limit of {INSTRUCTION_LIMIT} exceeded (endless loop?)"
        ));
    }
    match e {
        mlua::Error::MemoryError(m) => VmError::Limit(format!("memory limit exceeded: {m}")),
        mlua::Error::SyntaxError { message, .. } => VmError::Syntax {
            line: message_line(message),
            message: message.clone(),
        },
        mlua::Error::CallbackError { cause, .. } => classify(cause, budget),
        other => {
            let message = other.to_string();
            VmError::Runtime {
                line: message_line(&message),
                message,
            }
        }
    }
}

/// The first `:<digits>:` of a Lua message (`event.lua:12: unexpected symbol`), else 0.
fn message_line(msg: &str) -> u32 {
    let bytes = msg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes.get(i) == Some(&b':') {
            let start = i + 1;
            let mut j = start;
            while bytes.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            if j > start && bytes.get(j) == Some(&b':') {
                return msg
                    .get(start..j)
                    .and_then(|d| d.parse::<u32>().ok())
                    .unwrap_or(0);
            }
        }
        i += 1;
    }
    0
}

/// A fresh, trimmed state with the limits installed.
fn new_state(budget: &Rc<Budget>) -> mlua::Result<Lua> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
        LuaOptions::default(),
    )?;
    lua.set_memory_limit(MEMORY_LIMIT)?;
    let b = Rc::clone(budget);
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(HOOK_EVERY),
        move |_lua, _debug| {
            let used = b.used.get().saturating_add(u64::from(HOOK_EVERY));
            b.used.set(used);
            if used > INSTRUCTION_LIMIT {
                b.tripped.set(true);
                return Err(mlua::Error::runtime("instruction limit exceeded"));
            }
            Ok(VmState::Continue)
        },
    )?;
    let g = lua.globals();
    // string.dump would hand out bytecode; bytecode loading is refused below
    if let Ok(string) = g.get::<Table>("string") {
        string.set("dump", Value::Nil)?;
    }
    for name in ["dofile", "loadfile", "print"] {
        g.set(name, Value::Nil)?;
    }
    // once the budget is exhausted `pcall` / `xpcall` refuse to run: the hook's error must not be swallowed
    // by a script that wraps an endless loop in `pcall` inside another endless loop
    let tripped = Rc::clone(budget);
    g.set(
        "__budget_tripped",
        lua.create_function(move |_, ()| Ok(tripped.tripped.get()))?,
    )?;
    // text-only `load`, no-op logging, no clock
    lua.load(
        r#"
        local tripped = __budget_tripped
        __budget_tripped = nil
        local rawpcall, rawxpcall = pcall, xpcall
        function pcall(...)
            if tripped() then error("instruction limit exceeded", 0) end
            return rawpcall(...)
        end
        function xpcall(...)
            if tripped() then error("instruction limit exceeded", 0) end
            return rawxpcall(...)
        end
        local rawload = load
        function load(chunk, name, mode, env)
            if env == nil then return rawload(chunk, name, "t") end
            return rawload(chunk, name, "t", env)
        end
        function TRACE() end
        function ERROR() end
        function print() end
        local function no_clock(what)
            return function() error(what .. " is not available while the configuration is loaded", 2) end
        end
        os = { time = no_clock("os.time"), date = no_clock("os.date"), clock = no_clock("os.clock") }
        package = { loaded = {} }
        "#,
    )
    .set_name("=sandbox")
    .exec()?;
    Ok(lua)
}

/// The VFS access of the scoped `dofile` / `loadfile` / `require`.
struct Files<'a> {
    vfs: &'a Vfs,
    deps: RefCell<Vec<VfsPath>>,
    depth: Cell<u32>,
}

impl Files<'_> {
    fn note(&self, p: &VfsPath) {
        let mut deps = self.deps.borrow_mut();
        if !deps.contains(p) {
            deps.push(p.clone());
        }
    }

    fn read(&self, arg: &str) -> mlua::Result<(VfsPath, String)> {
        let (key, text) = self
            .vfs
            .read_lua_source(arg)
            .map_err(|e| mlua::Error::runtime(format!("cannot open {arg}: {e}")))?;
        self.note(&key);
        Ok((key, text))
    }

    fn run(&self, lua: &Lua, key: &VfsPath, text: &str) -> mlua::Result<MultiValue> {
        let depth = self.depth.get();
        if depth >= MAX_DEPTH {
            return Err(mlua::Error::runtime(format!(
                "dofile nesting deeper than {MAX_DEPTH} at {}",
                key.as_str()
            )));
        }
        self.depth.set(depth + 1);
        let r = lua
            .load(text)
            .set_name(format!("@{}", key.as_str()))
            .set_mode(ChunkMode::Text)
            .call::<MultiValue>(());
        self.depth.set(depth);
        r
    }
}

/// Runs `text` (the decoded content of the script `chunk_name`) in a fresh sandbox VM.
///
/// `vfs` serves the `dofile`d helpers; the files that were read are returned in [`Executed::deps`] so the
/// caller can register them as sources of the data fingerprint.
pub fn run_config_script(vfs: &Vfs, chunk_name: &str, text: &str) -> Result<Executed, VmError> {
    let budget = Rc::new(Budget::default());
    let lua = new_state(&budget).map_err(|e| classify(&e, &budget))?;
    let files = Files {
        vfs,
        deps: RefCell::new(Vec::new()),
        depth: Cell::new(0),
    };
    let result = lua.scope(|scope| {
        let g = lua.globals();
        let dofile = scope.create_function(|lua, path: String| {
            let (key, text) = files.read(&path)?;
            files.run(lua, &key, &text)
        })?;
        g.set("dofile", dofile)?;
        let loadfile = scope.create_function(|lua, path: String| {
            let (key, text) = match files.read(&path) {
                Ok(v) => v,
                Err(e) => return Ok((Value::Nil, Some(e.to_string()))),
            };
            match lua
                .load(text)
                .set_name(format!("@{}", key.as_str()))
                .set_mode(ChunkMode::Text)
                .into_function()
            {
                Ok(f) => Ok((Value::Function(f), None)),
                Err(e) => Ok((Value::Nil, Some(e.to_string()))),
            }
        })?;
        g.set("loadfile", loadfile)?;
        let require = scope.create_function(|lua, name: String| {
            let package: Table = lua.globals().get("package")?;
            let loaded: Table = package.get("loaded")?;
            let have: Value = loaded.get(name.as_str())?;
            if !matches!(have, Value::Nil) {
                return Ok(have);
            }
            let (key, text) = require_candidates(&name)
                .iter()
                .find_map(|c| files.read(c.as_str()).ok())
                .ok_or_else(|| mlua::Error::runtime(format!("module '{name}' not found")))?;
            let out = files.run(lua, &key, &text)?;
            let value = out.into_iter().next().unwrap_or(Value::Nil);
            let value = if matches!(value, Value::Nil) {
                Value::Boolean(true)
            } else {
                value
            };
            loaded.set(name.as_str(), value.clone())?;
            Ok(value)
        })?;
        g.set("require", require)?;
        lua.load(text)
            .set_name(format!("@{chunk_name}"))
            .set_mode(ChunkMode::Text)
            .exec()
    });
    match result {
        Ok(()) => Ok(Executed {
            lua,
            deps: files.deps.into_inner(),
        }),
        Err(e) => Err(classify(&e, &budget)),
    }
}

/// Compiles `text` without running it: the syntax check of the live scripts and their helpers.
pub fn check_syntax(chunk_name: &str, text: &str) -> Result<(), VmError> {
    let budget = Rc::new(Budget::default());
    let lua = new_state(&budget).map_err(|e| classify(&e, &budget))?;
    lua.load(text)
        .set_name(format!("@{chunk_name}"))
        .set_mode(ChunkMode::Text)
        .into_function()
        .map(|_| ())
        .map_err(|e| classify(&e, &budget))
}

/// The files a script `dofile`s: every `dofile("...")` / `dofile('...')` call with a string literal argument
/// that is not inside a comment or another string, in order of appearance, duplicates removed. A scan, not an
/// execution: the live scripts are only compiled at load (Plan/14 §6.17).
pub fn scan_dofiles(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let at = |i: usize| b.get(i).copied();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(c) = at(i) {
        // comments: `--` to the end of the line, `--[[ ... ]]` (any level) to its end
        if c == b'-' && at(i + 1) == Some(b'-') {
            i += 2;
            if let Some(level) = long_bracket_level(b, i) {
                i = skip_long(b, i, level);
            } else {
                while at(i).is_some_and(|c| c != b'\n') {
                    i += 1;
                }
            }
            continue;
        }
        // string literals are skipped as a whole: a "dofile(" inside a string is not a call
        if c == b'"' || c == b'\'' {
            i = skip_quoted(b, i);
            continue;
        }
        if c == b'['
            && let Some(level) = long_bracket_level(b, i)
        {
            i = skip_long(b, i, level);
            continue;
        }
        let call = b.get(i..).is_some_and(|rest| rest.starts_with(b"dofile"))
            && (i == 0 || !at(i - 1).is_some_and(is_ident))
            && !at(i + 6).is_some_and(is_ident);
        if call {
            let mut j = i + 6;
            while at(j).is_some_and(|c| c.is_ascii_whitespace()) {
                j += 1;
            }
            if at(j) == Some(b'(') {
                j += 1;
                while at(j).is_some_and(|c| c.is_ascii_whitespace()) {
                    j += 1;
                }
            }
            if let Some(q) = at(j)
                && (q == b'"' || q == b'\'')
            {
                let end = skip_quoted(b, j);
                if let Some(raw) = text.get(j + 1..end.saturating_sub(1)) {
                    let path = unescape(raw);
                    if !out.contains(&path) {
                        out.push(path);
                    }
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `[[`, `[=[`, ...: the number of `=` when a long bracket opens at `i`.
fn long_bracket_level(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(j) == Some(&b'[')).then_some(j - i - 1)
}

/// Index after the long bracket of `level` that opens at `i` (the end of the text when unterminated).
fn skip_long(b: &[u8], i: usize, level: usize) -> usize {
    let mut j = i + level + 2;
    while j < b.len() {
        if b.get(j) == Some(&b']') {
            let mut k = j + 1;
            let mut eq = 0;
            while b.get(k) == Some(&b'=') {
                eq += 1;
                k += 1;
            }
            if eq == level && b.get(k) == Some(&b']') {
                return k + 1;
            }
        }
        j += 1;
    }
    b.len()
}

/// Index after the quoted string that starts at `i` (a newline ends an unterminated one).
fn skip_quoted(b: &[u8], i: usize) -> usize {
    let q = b.get(i).copied().unwrap_or(b'"');
    let mut j = i + 1;
    while let Some(c) = b.get(j).copied() {
        match c {
            b'\\' => j += 2,
            c if c == q => return j + 1,
            b'\n' => return j,
            _ => j += 1,
        }
    }
    b.len()
}

/// `\\` and `\"` escapes of a string literal (the only ones a path needs).
fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut it = raw.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}
