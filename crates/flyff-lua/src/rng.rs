//! `math.random` / `math.randomseed` of a Lua state on its own named RNG stream (Plan/20 §3.6, 10 §4.8).
//!
//! Each state draws from `Pcg32Rng::new(derive(stream_tag(name)), stream = stream_tag(name))` with `name` in
//! `lua_event`, `lua_tax`, `lua_secret_room`, `lua_rainbow_race`, `lua_guild_combat_1to1`, `lua_npc` and
//! `derive(tag) = splitmix64(seed ^ tag)`, i.e. flyff-common's [`named_stream`], identical to `RngSet::derive`.
//! The streams are independent: draws in one state never move another.
//!
//! Lua 5.3 semantics: `random()` is a float in `[0, 1)`, `random(m)` an integer in `[1, m]`, `random(m, n)` an
//! integer in `[m, n]`; an empty interval is an error. Integers come from `next_u32() % span` (the
//! `xRandom(n)` mapping of 10 §4.8) when the span fits 32 bits, else from `next_u64() % span`.

use flyff_common::rng::{GameRng, Pcg32Rng, derive_seed, named_stream, stream_tag};
use mlua::{Lua, Value, Variadic};
use std::cell::RefCell;
use std::rc::Rc;

/// The stream of one Lua state.
#[derive(Debug)]
pub struct LuaRng {
    rng: RefCell<Pcg32Rng>,
    tag: u64,
}

impl LuaRng {
    /// The stream `name` (`"lua_event"`, ...) of `seed`.
    pub fn new(seed: u64, name: &str) -> Rc<LuaRng> {
        Rc::new(LuaRng {
            rng: RefCell::new(named_stream(seed, name)),
            tag: stream_tag(name),
        })
    }

    /// The next raw 32-bit value of the stream.
    pub fn next_u32(&self) -> u32 {
        self.rng.borrow_mut().next_u32()
    }

    fn next_u64(&self) -> u64 {
        self.rng.borrow_mut().next_u64()
    }

    /// A float in `[0, 1)` with 53 random bits.
    fn unit(&self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// An integer in `[lo, hi]` (`lo <= hi`).
    fn between(&self, lo: i64, hi: i64) -> i64 {
        let span = (i128::from(hi) - i128::from(lo) + 1) as u128;
        let offset = if span <= u128::from(u32::MAX) {
            u128::from(self.next_u32()) % span
        } else {
            u128::from(self.next_u64()) % span
        };
        (i128::from(lo) + offset as i128) as i64
    }

    /// `math.randomseed(x)`: reseeds this stream only.
    fn reseed(&self, x: i64) {
        *self.rng.borrow_mut() = Pcg32Rng::new(derive_seed(x as u64, self.tag), self.tag);
    }
}

/// An argument that must be an integer (floats with an integer value are accepted, as in Lua 5.3).
fn int_arg(v: &Value, pos: usize, func: &str) -> mlua::Result<i64> {
    let bad = |msg: &str| mlua::Error::runtime(format!("bad argument #{pos} to '{func}' ({msg})"));
    match v {
        Value::Integer(i) => Ok(*i),
        Value::Number(n) if n.fract() == 0.0 && n.abs() < 9.2e18 => Ok(*n as i64),
        Value::Number(_) => Err(bad("number has no integer representation")),
        Value::String(s) => s
            .to_str()
            .ok()
            .and_then(|t| t.trim().parse::<f64>().ok())
            .filter(|n| n.fract() == 0.0 && n.abs() < 9.2e18)
            .map(|n| n as i64)
            .ok_or_else(|| bad("number expected, got string")),
        other => Err(bad(&format!("number expected, got {}", other.type_name()))),
    }
}

/// Replaces `math.random` and `math.randomseed` of `lua` (Plan/20 §3.6).
pub(crate) fn install(lua: &Lua, rng: &Rc<LuaRng>) -> mlua::Result<()> {
    let math: mlua::Table = lua.globals().get("math")?;
    let r = Rc::clone(rng);
    math.set(
        "random",
        lua.create_function(move |_, args: Variadic<Value>| match args.len() {
            0 => Ok(Value::Number(r.unit())),
            1 => {
                let hi = int_arg(&args[0], 1, "random")?;
                if hi < 1 {
                    return Err(mlua::Error::runtime(
                        "bad argument #1 to 'random' (interval is empty)",
                    ));
                }
                Ok(Value::Integer(r.between(1, hi)))
            }
            2 => {
                let (lo, hi) = (
                    int_arg(&args[0], 1, "random")?,
                    int_arg(&args[1], 2, "random")?,
                );
                if lo > hi {
                    return Err(mlua::Error::runtime(
                        "bad argument #2 to 'random' (interval is empty)",
                    ));
                }
                Ok(Value::Integer(r.between(lo, hi)))
            }
            _ => Err(mlua::Error::runtime("wrong number of arguments")),
        })?,
    )?;
    let r = Rc::clone(rng);
    math.set(
        "randomseed",
        lua.create_function(move |_, x: Value| {
            r.reseed(match &x {
                Value::Number(n) => *n as i64,
                other => int_arg(other, 1, "randomseed")?,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}
