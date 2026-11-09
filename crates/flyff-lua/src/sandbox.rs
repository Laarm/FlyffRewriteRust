//! One sandboxed Lua 5.3 state (Plan/20 §3.3): the standard-library subset, the removed and replaced globals,
//! the protected base, the deterministic `os` / `math.random` / `pairs`, the VFS-backed `dofile` / `loadfile`
//! / `require`, the limits, and the `TRACE` / `ERROR` / `print` logging.
//!
//! A script never sees the real global table. It runs with `env` as its `_ENV`: its own globals live in `env`
//! (readable by [`Sandbox::global`]), everything else is looked up in the *base* (`_G` of the state, holding the
//! trimmed standard library), and assigning to a name of the base is an error — so a family script owns its
//! globals (the original design: `CheckOpenTime` is defined by two families) but cannot replace `pairs` or
//! `os`.
//!
//! | Global | Treatment |
//! |---|---|
//! | `io`, `debug`, `coroutine`, `string.dump`, `package.loadlib`, `package.cpath` | absent |
//! | `os.execute`, `exit`, `getenv`, `remove`, `rename`, `tmpname`, `setlocale` | removed |
//! | `os.date`, `time`, `clock`, `difftime` | deterministic ([`crate::clock`]) |
//! | `math.random`, `math.randomseed` | per-state stream ([`crate::rng`]) |
//! | `pairs` | ordered |
//! | `dofile`, `loadfile`, `require` | VFS-backed (`.\\LuaFunc\\X.lua` → `luafunc/x.lua`) |
//! | `load` | text only; the environment defaults to the script's `env` |
//! | `print`, `TRACE`, `ERROR` | `tracing` |
//! | `collectgarbage` | `count`, `step`, `collect` only |

use crate::LuaFault;
use crate::clock::{self, LuaClock};
use crate::limits::{self, Meter};
use crate::rng::{self, LuaRng};
use flyff_vfs::Vfs;
use mlua::chunk::ChunkMode;
use mlua::{Function, Lua, LuaOptions, MultiValue, StdLib, Table, Value, Variadic};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

/// What a state is built from.
pub(crate) struct SandboxSpec {
    /// The label in log lines (`"event"`, `"npc"`).
    pub label: String,
    /// The RNG stream name (`"lua_event"`).
    pub stream: String,
    /// `GameInit.seed`.
    pub seed: u64,
    /// The memory limit of the state.
    pub memory_limit: usize,
    /// Time source.
    pub clock: Rc<dyn LuaClock>,
    /// The resource tree behind `dofile` / `require`.
    pub vfs: Option<Arc<Vfs>>,
    /// The read-only define table `C` of the NPC state (Plan/20 §6.2): resolves `C.NAME`.
    pub constants: Option<ConstantResolver>,
}

/// Resolves a define name for `C.NAME` (`None`: undefined constant).
pub(crate) type ConstantResolver = Rc<dyn Fn(&str) -> Option<i64>>;

/// A sandboxed state with its script environment.
pub(crate) struct Sandbox {
    pub lua: Lua,
    /// The `_ENV` of every chunk loaded into this state: the script's own globals.
    pub env: Table,
    pub meter: Rc<Meter>,
    /// The stream behind `math.random` (read by the tests of the stream derivation).
    #[allow(dead_code)]
    pub rng: Rc<LuaRng>,
    /// The trimmed standard library every script environment falls back to.
    base: Table,
    /// The names of `base`: a script cannot assign them.
    protected: Rc<BTreeSet<String>>,
}

/// Maps an mlua error to a [`LuaFault`]; an exhausted instruction budget wins over whatever the hook's error
/// became on its way out (it may be wrapped in a callback error or turned into text by a `pcall`).
pub(crate) fn classify(e: &mlua::Error, meter: &Meter) -> LuaFault {
    if meter.tripped() {
        return LuaFault::Limit(format!("instruction budget of {} exceeded", meter.budget()));
    }
    match e {
        mlua::Error::MemoryError(m) => LuaFault::Limit(format!("memory limit exceeded: {m}")),
        mlua::Error::SyntaxError { message, .. } => LuaFault::Syntax(message.clone()),
        mlua::Error::CallbackError { cause, .. } => classify(cause, meter),
        // the auxiliary library raises its own memory failures as plain errors (`string.rep` buffers)
        other => {
            let text = other.to_string();
            if text.contains("not enough memory") {
                LuaFault::Limit(format!("memory limit exceeded: {text}"))
            } else {
                LuaFault::Runtime(text)
            }
        }
    }
}

fn rt(msg: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(msg)
}

/// A value as `tostring` shows it (for the log functions).
fn text_of(v: &Value) -> String {
    v.to_string()
        .unwrap_or_else(|_| format!("<{}>", v.type_name()))
}

impl Sandbox {
    /// Builds the state. Nothing runs yet.
    pub(crate) fn new(spec: SandboxSpec) -> mlua::Result<Sandbox> {
        let lua = Lua::new_with(
            StdLib::TABLE
                | StdLib::STRING
                | StdLib::MATH
                | StdLib::OS
                | StdLib::UTF8
                | StdLib::PACKAGE,
            LuaOptions::default(),
        )?;
        lua.set_memory_limit(spec.memory_limit)?;
        let meter = Meter::new();
        limits::install_hook(&lua, &meter)?;
        let rng = LuaRng::new(spec.seed, &spec.stream);
        let base = lua.globals();

        // ---- trim the standard library ----
        if let Ok(string) = base.get::<Table>("string") {
            string.set("dump", Value::Nil)?;
        }
        for name in [
            "dofile",
            "loadfile",
            "load",
            "print",
            "collectgarbage",
            "pairs",
        ] {
            base.set(name, Value::Nil)?;
        }
        clock::install_os(&lua, &spec.clock)?;
        rng::install(&lua, &rng)?;

        // ---- logging ----
        let label = spec.label.clone();
        base.set(
            "print",
            lua.create_function(move |_, args: Variadic<Value>| {
                let line = args.iter().map(text_of).collect::<Vec<_>>().join("\t");
                tracing::debug!(target: "lua", family = %label, "{line}");
                Ok(())
            })?,
        )?;
        let label = spec.label.clone();
        base.set(
            "TRACE",
            lua.create_function(move |_, msg: Option<Value>| {
                let line = msg.as_ref().map(text_of).unwrap_or_default();
                tracing::debug!(target: "lua.trace", family = %label, "{line}");
                Ok(())
            })?,
        )?;
        // the original wrote ERROR() to the error log, but EventFunc.lua calls it on every normal state change:
        // it is logged at info level so that real faults stay greppable (Plan/20 §3.3, Q11)
        let label = spec.label.clone();
        base.set(
            "ERROR",
            lua.create_function(move |_, msg: Option<Value>| {
                let line = msg.as_ref().map(text_of).unwrap_or_default();
                tracing::info!(target: "lua.error", family = %label, "{line}");
                Ok(())
            })?,
        )?;

        // ---- collectgarbage ----
        base.set(
            "collectgarbage",
            lua.create_function(|lua, opt: Option<String>| {
                Ok(match opt.as_deref().unwrap_or("collect") {
                    "count" => Value::Number(lua.used_memory() as f64 / 1024.0),
                    "collect" => {
                        lua.gc_collect()?;
                        Value::Integer(0)
                    }
                    "step" => Value::Boolean(lua.gc_step()?),
                    _ => Value::Integer(0),
                })
            })?,
        )?;

        // ---- ordered pairs ----
        base.set("pairs", lua.create_function(ordered_pairs)?)?;

        // ---- the script environment, load / dofile / loadfile / require ----
        let env = lua.create_table()?;
        let default_env = env.clone();
        base.set(
            "load",
            lua.create_function(move |lua, args: Variadic<Value>| {
                load_text(lua, &args, &default_env)
            })?,
        )?;
        install_files(&lua, &base, &env, spec.vfs.as_ref())?;

        // once the budget is exhausted `pcall` / `xpcall` refuse to run: a script that wraps an endless loop
        // in `pcall` inside another endless loop must not swallow the hook's error forever
        let m = Rc::clone(&meter);
        base.set(
            "__tripped",
            lua.create_function(move |_, ()| Ok(m.tripped()))?,
        )?;
        lua.load(
            r#"
            local tripped = __tripped
            __tripped = nil
            local rawpcall, rawxpcall = pcall, xpcall
            function pcall(...)
                if tripped() then error("instruction budget exceeded", 0) end
                return rawpcall(...)
            end
            function xpcall(...)
                if tripped() then error("instruction budget exceeded", 0) end
                return rawxpcall(...)
            end
            "#,
        )
        .set_name("=sandbox")
        .set_mode(ChunkMode::Text)
        .exec()?;

        if let Some(resolve) = spec.constants {
            base.set("C", constants_table(&lua, resolve)?)?;
        }

        // the names of the base are protected: a script defines its own globals in `env` but cannot replace
        // `pairs`, `os`, `string`, ...
        let mut names = BTreeSet::new();
        base.for_each(|k: Value, _: Value| {
            if let Value::String(s) = k {
                names.insert(s.to_string_lossy());
            }
            Ok(())
        })?;
        let protected = Rc::new(names);
        protect_env(&lua, &env, &base, &protected)?;

        Ok(Sandbox {
            lua,
            env,
            meter,
            rng,
            base,
            protected,
        })
    }

    /// A fresh script environment: reads fall back to the trimmed standard library, writes to a name of it are
    /// an error. One per NPC file (Plan/20 §3.3), so one file cannot see or replace another's globals.
    pub(crate) fn new_file_env(&self) -> mlua::Result<Table> {
        let env = self.lua.create_table()?;
        protect_env(&self.lua, &env, &self.base, &self.protected)?;
        Ok(env)
    }

    /// Compiles and runs `text` as the chunk `@<chunk_name>` in the script environment, with an instruction
    /// budget of `budget`.
    pub(crate) fn run_source(
        &self,
        chunk_name: &str,
        text: &str,
        budget: u64,
    ) -> Result<(), LuaFault> {
        self.meter.arm(budget);
        self.lua
            .load(text)
            .set_name(format!("@{chunk_name}"))
            .set_mode(ChunkMode::Text)
            .set_environment(self.env.clone())
            .exec()
            .map_err(|e| classify(&e, &self.meter))
    }

    /// Calls `func` with `args` under an instruction budget.
    pub(crate) fn call(
        &self,
        func: &Function,
        args: MultiValue,
        budget: u64,
    ) -> Result<MultiValue, LuaFault> {
        self.meter.arm(budget);
        func.call::<MultiValue>(args)
            .map_err(|e| classify(&e, &self.meter))
    }

    /// The script global `name` (raw: only what the scripts defined).
    pub(crate) fn global(&self, name: &str) -> mlua::Result<Value> {
        self.env.raw_get(name)
    }

    /// Bytes in use by the state.
    pub(crate) fn memory_used(&self) -> usize {
        self.lua.used_memory()
    }
}

/// Gives `env` its metatable (reads fall back to `base`, writes to a protected name fail) and its `_G`.
fn protect_env(
    lua: &Lua,
    env: &Table,
    base: &Table,
    protected: &Rc<BTreeSet<String>>,
) -> mlua::Result<()> {
    let protected = Rc::clone(protected);
    let meta = lua.create_table()?;
    meta.set("__index", base.clone())?;
    meta.set(
        "__newindex",
        lua.create_function(move |_, (t, k, v): (Table, Value, Value)| {
            if let Value::String(s) = &k {
                let name = s.to_string_lossy();
                if protected.contains(&name) {
                    return Err(rt(format!(
                        "cannot assign to the protected global '{name}'"
                    )));
                }
            }
            t.raw_set(k, v)
        })?,
    )?;
    env.set_metatable(Some(meta))?;
    env.raw_set("_G", env.clone())?;
    Ok(())
}

/// The read-only table `C` (Plan/20 §6.2): `C.NAME` is the define `NAME`, looked up on first use and cached; an
/// unknown name is the Lua error `undefined constant NAME`. `C` itself stays empty, so assigning to it (cached
/// name or not) always reaches `__newindex`.
fn constants_table(lua: &Lua, resolve: ConstantResolver) -> mlua::Result<Table> {
    let cache = lua.create_table()?;
    let cache_meta = lua.create_table()?;
    cache_meta.set(
        "__index",
        lua.create_function(move |_, (t, k): (Table, Value)| {
            let Value::String(name) = &k else {
                return Err(rt("constant names are strings"));
            };
            let name = name.to_string_lossy();
            match resolve(&name) {
                Some(v) => {
                    t.raw_set(k, v)?;
                    Ok(v)
                }
                None => Err(rt(format!("undefined constant {name}"))),
            }
        })?,
    )?;
    cache.set_metatable(Some(cache_meta))?;
    let c = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set("__index", cache)?;
    meta.set(
        "__newindex",
        lua.create_function(|_, _: MultiValue| -> mlua::Result<()> {
            Err(rt("the constant table C is read-only"))
        })?,
    )?;
    meta.set("__metatable", false)?;
    c.set_metatable(Some(meta))?;
    Ok(c)
}

// ---- pairs ----------------------------------------------------------------------------------------------

fn key_rank(v: &Value) -> u8 {
    match v {
        Value::Integer(_) => 0,
        Value::Number(_) => 1,
        Value::String(_) => 2,
        Value::Boolean(_) => 3,
        _ => 4,
    }
}

/// The documented key order: integers ascending, floats ascending, strings bytewise, `false` before `true`;
/// other key types (tables, functions) after all of those in traversal order.
fn cmp_keys(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => x.cmp(y),
        (Value::Number(x), Value::Number(y)) => x.total_cmp(y),
        (Value::String(x), Value::String(y)) => {
            let (x, y) = (x.as_bytes(), y.as_bytes());
            x.as_ref().cmp(y.as_ref())
        }
        (Value::Boolean(x), Value::Boolean(y)) => x.cmp(y),
        _ => key_rank(a).cmp(&key_rank(b)),
    }
}

/// `pairs(t)`: array part `1..#t` first, then the remaining keys sorted ([`cmp_keys`]); values are read when
/// visited, a key removed meanwhile is skipped. Lua 5.3 seeds string hashes from addresses, so the built-in
/// order is not reproducible; every `pairs` use of the live scripts walks sequence tables, for which this is
/// the original order (Plan/20 §3.6). A `__pairs` metamethod is honoured like Lua 5.3 does.
fn ordered_pairs(lua: &Lua, t: Value) -> mlua::Result<MultiValue> {
    let Value::Table(table) = t else {
        return Err(rt(format!(
            "bad argument #1 to 'for iterator' (table expected, got {})",
            t.type_name()
        )));
    };
    if let Some(meta) = table.metatable()
        && let Value::Function(f) = meta.raw_get::<Value>("__pairs")?
    {
        let mut r = f.call::<MultiValue>(Value::Table(table))?;
        while r.len() > 3 {
            r.pop_back();
        }
        return Ok(r);
    }
    let n = table.raw_len();
    let mut keys: Vec<Value> = (1..=n as i64).map(Value::Integer).collect();
    let mut rest: Vec<Value> = Vec::new();
    table.for_each(|k: Value, _: Value| {
        let in_array = matches!(&k, Value::Integer(i) if *i >= 1 && *i as usize <= n);
        if !in_array {
            rest.push(k);
        }
        Ok(())
    })?;
    rest.sort_by(cmp_keys);
    keys.extend(rest);
    let walk = table.clone();
    let mut next = 0usize;
    let iter = lua.create_function_mut(move |_, (_state, _ctl): (Value, Value)| {
        while let Some(k) = keys.get(next) {
            next += 1;
            let v: Value = walk.raw_get(k.clone())?;
            if !v.is_nil() {
                return Ok(MultiValue::from_iter([k.clone(), v]));
            }
        }
        Ok(MultiValue::new())
    })?;
    Ok(MultiValue::from_iter([
        Value::Function(iter),
        Value::Table(table),
        Value::Nil,
    ]))
}

// ---- load, dofile, loadfile, require ----------------------------------------------------------------------------

/// `load(chunk [, chunkname [, mode [, env]]])` in text mode only; the environment defaults to the script
/// environment of the state.
fn load_text(lua: &Lua, args: &[Value], default_env: &Table) -> mlua::Result<MultiValue> {
    let fail = |msg: String| -> mlua::Result<MultiValue> {
        Ok(MultiValue::from_iter([
            Value::Nil,
            Value::String(lua.create_string(&msg)?),
        ]))
    };
    // the chunk: a string, or a function that returns the pieces (bytes: a script may hold any byte, and a
    // binary chunk must reach Lua to be refused by the text mode, not die in a UTF-8 check)
    let mut source: Vec<u8> = Vec::new();
    match args.first() {
        Some(Value::String(s)) => source.extend_from_slice(&s.as_bytes()),
        Some(Value::Function(f)) => loop {
            let piece = f.call::<Value>(())?;
            match piece {
                Value::Nil => break,
                Value::String(s) => {
                    let b = s.as_bytes();
                    if b.is_empty() {
                        break;
                    }
                    source.extend_from_slice(&b);
                    if source.len() > 1 << 20 {
                        return fail("load: chunk too large".to_owned());
                    }
                }
                _ => return fail("reader function must return a string".to_owned()),
            }
        },
        _ => {
            return Err(rt(
                "bad argument #1 to 'load' (string or function expected)",
            ));
        }
    }
    let name = match args.get(1) {
        Some(Value::String(s)) => s.to_str().map_err(rt)?.to_string(),
        _ => format!(
            "={}",
            String::from_utf8_lossy(source.get(..40).unwrap_or(&source))
        ),
    };
    let env = match args.get(3) {
        Some(Value::Table(t)) => t.clone(),
        None | Some(Value::Nil) => default_env.clone(),
        Some(_) => return Err(rt("bad argument #4 to 'load' (table expected)")),
    };
    match lua
        .load(source)
        .set_name(name)
        .set_mode(ChunkMode::Text)
        .set_environment(env)
        .into_function()
    {
        Ok(f) => Ok(MultiValue::from_iter([Value::Function(f)])),
        Err(e) => fail(e.to_string()),
    }
}

/// `dofile`, `loadfile` and the single `package.searchers` entry (Plan/14 §2.8, 20 §3.4).
fn install_files(lua: &Lua, base: &Table, env: &Table, vfs: Option<&Arc<Vfs>>) -> mlua::Result<()> {
    let no_tree = || rt("no resource tree is attached to this Lua state");

    let (v, e) = (vfs.cloned(), env.clone());
    base.set(
        "dofile",
        lua.create_function(move |lua, path: String| {
            let vfs = v.as_ref().ok_or_else(no_tree)?;
            let (key, text) = vfs
                .read_lua_source(&path)
                .map_err(|err| rt(format!("cannot open {path}: {err}")))?;
            lua.load(text)
                .set_name(format!("@{}", key.as_str()))
                .set_mode(ChunkMode::Text)
                .set_environment(e.clone())
                .call::<MultiValue>(())
        })?,
    )?;

    let (v, e) = (vfs.cloned(), env.clone());
    base.set(
        "loadfile",
        lua.create_function(move |lua, path: String| {
            let fail = |msg: String| -> mlua::Result<MultiValue> {
                Ok(MultiValue::from_iter([
                    Value::Nil,
                    Value::String(lua.create_string(&msg)?),
                ]))
            };
            let Some(vfs) = v.as_ref() else {
                return fail("no resource tree is attached to this Lua state".to_owned());
            };
            let (key, text) = match vfs.read_lua_source(&path) {
                Ok(r) => r,
                Err(err) => return fail(format!("cannot open {path}: {err}")),
            };
            match lua
                .load(text)
                .set_name(format!("@{}", key.as_str()))
                .set_mode(ChunkMode::Text)
                .set_environment(e.clone())
                .into_function()
            {
                Ok(f) => Ok(MultiValue::from_iter([Value::Function(f)])),
                Err(err) => fail(err.to_string()),
            }
        })?,
    )?;

    // `require` is the standard one; its search path is one searcher built on the VFS
    let package: Table = base.get("package")?;
    let (v, e) = (vfs.cloned(), env.clone());
    let searcher = lua.create_function(move |lua, name: String| {
        let found = v
            .as_ref()
            .and_then(|vfs| vfs.read_require_source(&name).ok());
        let Some((key, text)) = found else {
            let msg = format!("\n\tno file for module '{name}'");
            return Ok(MultiValue::from_iter([Value::String(
                lua.create_string(&msg)?,
            )]));
        };
        let loader = lua
            .load(text)
            .set_name(format!("@{}", key.as_str()))
            .set_mode(ChunkMode::Text)
            .set_environment(e.clone())
            .into_function()?;
        Ok(MultiValue::from_iter([
            Value::Function(loader),
            Value::String(lua.create_string(key.as_str())?),
        ]))
    })?;
    package.set("searchers", lua.create_sequence_from([searcher])?)?;
    package.set("path", "")?;
    package.set("cpath", Value::Nil)?;
    package.set("loadlib", Value::Nil)?;
    Ok(())
}
