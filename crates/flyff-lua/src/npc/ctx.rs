//! The host side of the helper API and the scoped `d` object of an NPC function (Plan/20 §6.1).
//!
//! * [`ScriptHost`]: the one dispatch method `call(Helper, &[HVal])` the game implements.
//! * [`HostError`]: a fatal host problem; it becomes a Lua runtime error (policy §3.8). Game-rule refusals are
//!   **not** errors: the host returns a value (or nothing) as the original did.
//! * `run_function`: builds the `d` table for one call (a table whose metatable maps every `d:Name(...)` to the
//!   per-call dispatch closure), checks the arity and converts the arguments per [`ArgKind`], calls the host and
//!   converts the result. No `mlua` type is visible to the host.
//!
//! The dispatch closure borrows the host for the duration of the call only (mlua's `Lua::scope`), so the game can
//! pass `&mut` references to its own state; a nested call (`Run`, flows) builds a second `d` over a re-borrow of
//! the same host.

use super::helper::{ArgKind, CtxReq, HELPERS, Helper, HelperSig};
use crate::value::HVal;
use mlua::{Function, Lua, MultiValue, Table, Value};

/// A fatal problem of the host while executing a helper (Plan/20 §6.1). Becomes a Lua runtime error and, at the
/// top of the call, a faulted NPC script (§3.8).
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// The message of the Lua error.
    #[error("{0}")]
    Fatal(String),
}

impl From<crate::LuaFault> for HostError {
    fn from(f: crate::LuaFault) -> Self {
        HostError::Fatal(f.to_string())
    }
}

/// The helper API as the game implements it (Plan/20 §6.1).
pub trait ScriptHost {
    /// Executes one helper. `args` are already converted per [`ArgKind`] (`TextId` stays an id, the host resolves
    /// the text). A helper with `returns_value == false` ignores the returned value; a value helper's result is
    /// pushed to the script (`HVal::Nil` becomes `nil`).
    fn call(&mut self, h: Helper, args: &[HVal]) -> Result<HVal, HostError>;
}

fn rt(msg: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(msg.into())
}

/// Converts one Lua argument per its kind.
fn convert_arg(sig: &HelperSig, i: usize, v: &Value) -> mlua::Result<HVal> {
    let kind = sig.kind_of(i);
    let bad = |what: &str| {
        rt(format!(
            "bad argument #{} to '{}' ({what})",
            i + 1,
            sig.name
        ))
    };
    match (kind, v) {
        (ArgKind::Float, Value::Integer(n)) => Ok(HVal::Float(*n as f64)),
        (ArgKind::Float, Value::Number(n)) => Ok(HVal::Float(*n)),
        (ArgKind::Float, other) => Err(bad(&format!("number expected, got {}", other.type_name()))),
        (ArgKind::Int | ArgKind::TextId, Value::Integer(n)) => {
            if kind == ArgKind::TextId && *n < 0 {
                Err(bad("text id must not be negative"))
            } else {
                Ok(HVal::Int(*n))
            }
        }
        (ArgKind::Int | ArgKind::TextId, Value::Number(n)) => {
            if n.fract() == 0.0 && n.abs() < 9.2e18 {
                if kind == ArgKind::TextId && *n < 0.0 {
                    Err(bad("text id must not be negative"))
                } else {
                    Ok(HVal::Int(*n as i64))
                }
            } else {
                Err(bad("number has no integer representation"))
            }
        }
        (_, other) => Err(bad(&format!("number expected, got {}", other.type_name()))),
    }
}

/// The result of a helper as Lua values.
fn push_result(sig: &HelperSig, lua: &Lua, r: HVal) -> mlua::Result<MultiValue> {
    if !sig.returns_value {
        return Ok(MultiValue::new());
    }
    let v = match r {
        HVal::Nil => Value::Nil,
        HVal::Bool(b) => Value::Integer(i64::from(b)),
        HVal::Int(i) => Value::Integer(i),
        HVal::Float(f) => Value::Number(f),
        HVal::Str(s) => Value::String(lua.create_string(&s)?),
    };
    Ok(MultiValue::from_iter([v]))
}

/// The dispatch of `d:Name(...)`: `code` is the table index of the helper.
fn dispatch(
    lua: &Lua,
    host: &mut dyn ScriptHost,
    code: usize,
    args: &MultiValue,
) -> mlua::Result<MultiValue> {
    let sig = HELPERS
        .get(code)
        .ok_or_else(|| rt(format!("unknown helper index {code}")))?;
    if !sig.accepts(args.len()) {
        return Err(rt(format!(
            "wrong number of arguments to '{}' ({} expected, got {})",
            sig.name,
            sig.arities_text(),
            args.len()
        )));
    }
    let mut converted: Vec<HVal> = Vec::with_capacity(args.len());
    for (i, v) in args.iter().enumerate() {
        converted.push(convert_arg(sig, i, v)?);
    }
    debug_assert!(matches!(sig.ctx, CtxReq::None | CtxReq::Player));
    let result = host
        .call(sig.helper, &converted)
        .map_err(|HostError::Fatal(msg)| rt(msg))?;
    push_result(sig, lua, result)
}

/// Runs `function(d)` with a fresh `d` bound to `host`. `methods` is the shared metatable created by
/// [`install_methods`]. The scope ends when the function returns: a `d` kept by the script is dead afterwards.
pub(crate) fn run_function(
    lua: &Lua,
    methods: &Table,
    function: &Function,
    host: &mut dyn ScriptHost,
) -> mlua::Result<()> {
    lua.scope(|scope| {
        let call = scope.create_function_mut(|lua, mut args: MultiValue| {
            // `d.__call(code, ...)`: the first value is the helper index
            let code = match args.pop_front() {
                Some(Value::Integer(i)) => usize::try_from(i).unwrap_or(usize::MAX),
                _ => return Err(rt("the dialog object is used incorrectly")),
            };
            dispatch(lua, &mut *host, code, &args)
        })?;
        let d = lua.create_table_with_capacity(0, 1)?;
        d.raw_set("__call", call)?;
        d.set_metatable(Some(methods.clone()))?;
        function.call::<()>(d)
    })
}

/// Builds the shared metatable of `d`: `__index` holds one Lua closure per helper that forwards to
/// `self.__call(index, ...)`; the other keys of `d` cannot be written.
pub(crate) fn install_methods(lua: &Lua) -> mlua::Result<Table> {
    let names = lua.create_table_with_capacity(HELPERS.len(), 0)?;
    for (i, h) in HELPERS.iter().enumerate() {
        names.raw_set(i + 1, h.name)?;
    }
    let meta: Table = lua
        .load(
            r#"
            local names = ...
            local methods = {}
            for index = 1, #names do
                local code = index - 1
                methods[names[index]] = function(self, ...)
                    return self.__call(code, ...)
                end
            end
            return {
                __index = methods,
                __newindex = function() error("the dialog object is read-only", 2) end,
                __metatable = false,
            }
            "#,
        )
        .set_name("=npc-dialog-object")
        .call(names)?;
    Ok(meta)
}
