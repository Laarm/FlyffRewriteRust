//! `HVal` and the serde bridge between Lua values and Rust types (Plan/20 §3.1, §4.3). No `mlua` type is
//! public: callers pass [`HVal`] arguments and get `serde` types back.

use crate::LuaFault;
use mlua::{Lua, LuaSerdeExt, MultiValue, Value};
use serde::de::DeserializeOwned;

/// A value passed from Rust into a Lua call (Plan/20 §3.1).
#[derive(Clone, Debug, PartialEq)]
pub enum HVal {
    /// `nil`.
    Nil,
    /// A boolean.
    Bool(bool),
    /// An integer (Lua 5.3 integer subtype).
    Int(i64),
    /// A float.
    Float(f64),
    /// A string.
    Str(String),
}

impl From<bool> for HVal {
    fn from(v: bool) -> Self {
        HVal::Bool(v)
    }
}

impl From<i64> for HVal {
    fn from(v: i64) -> Self {
        HVal::Int(v)
    }
}

impl From<i32> for HVal {
    fn from(v: i32) -> Self {
        HVal::Int(i64::from(v))
    }
}

impl From<u32> for HVal {
    fn from(v: u32) -> Self {
        HVal::Int(i64::from(v))
    }
}

impl From<f64> for HVal {
    fn from(v: f64) -> Self {
        HVal::Float(v)
    }
}

impl From<&str> for HVal {
    fn from(v: &str) -> Self {
        HVal::Str(v.to_owned())
    }
}

impl From<String> for HVal {
    fn from(v: String) -> Self {
        HVal::Str(v)
    }
}

/// An `HVal` as a Lua value of `lua`.
pub(crate) fn to_value(lua: &Lua, v: &HVal) -> mlua::Result<Value> {
    Ok(match v {
        HVal::Nil => Value::Nil,
        HVal::Bool(b) => Value::Boolean(*b),
        HVal::Int(i) => Value::Integer(*i),
        HVal::Float(f) => Value::Number(*f),
        HVal::Str(s) => Value::String(lua.create_string(s)?),
    })
}

/// The results of a Lua call as a Rust value: no result is `nil`, one result is converted as it is, several
/// results become a sequence so that a tuple type receives them in order
/// (`let (ok, date): (bool, String) = ...`). A `nil` result converts to `Option::None`.
pub(crate) fn convert<R: DeserializeOwned>(lua: &Lua, results: MultiValue) -> Result<R, LuaFault> {
    let value = match results.len() {
        0 => Value::Nil,
        1 => results.into_iter().next().unwrap_or(Value::Nil),
        _ => {
            let t = lua
                .create_sequence_from(results)
                .map_err(|e| LuaFault::Conversion(e.to_string()))?;
            Value::Table(t)
        }
    };
    lua.from_value::<R>(value)
        .map_err(|e| LuaFault::Conversion(e.to_string()))
}

/// One Lua value as a Rust value (`LuaHost::global`).
pub(crate) fn convert_one<R: DeserializeOwned>(lua: &Lua, value: Value) -> Result<R, LuaFault> {
    lua.from_value::<R>(value)
        .map_err(|e| LuaFault::Conversion(e.to_string()))
}
