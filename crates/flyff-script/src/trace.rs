//! The oracle trace API (Plan/14 §3 and §11 item 3).
//!
//! [`run`] drives a [`Scanner`] with a sequence of operations and records, for every operation, the
//! token type, the token text, the returned value (floats as their `f32` bit pattern) and the cursor.
//! The oracle harness (`tools/oracle`, not part of this task) feeds the same operations to the
//! original `CScanner` and diffs the traces; here the trace also powers the property tests.

use crate::scanner::{Scanner, ScannerOptions, TokenType};
use std::sync::Arc;

/// One scanner operation (the `comma` flag selects comma mode).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraceOp {
    /// `GetToken(bComma)`.
    Token {
        /// Comma mode.
        comma: bool,
    },
    /// `GetNumber(bComma)`.
    Number {
        /// Comma mode.
        comma: bool,
    },
    /// `GetInt64(bComma)`.
    Int64 {
        /// Comma mode.
        comma: bool,
    },
    /// `GetFloat(bComma)`.
    Float {
        /// Comma mode.
        comma: bool,
    },
    /// `GetHex(bComma)`.
    Hex {
        /// Comma mode.
        comma: bool,
    },
    /// `GetLastFull()`.
    LastFull,
}

/// What an operation returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraceValue {
    /// `GetToken`, `GetLastFull`: no value besides the token.
    None,
    /// `GetNumber`, `GetHex`.
    U32(u32),
    /// `GetInt64`.
    I64(i64),
    /// `GetFloat` as raw `f32` bits (NaN-safe comparison).
    F32Bits(u32),
}

/// The state after one operation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TraceEvent {
    /// The operation.
    pub op: TraceOp,
    /// Token type after the operation.
    pub ty: TokenType,
    /// Token text after the operation.
    pub text: String,
    /// Returned value.
    pub value: TraceValue,
    /// Cursor (byte offset) after the operation.
    pub pos: usize,
    /// `tok == FINISHED` after the operation.
    pub finished: bool,
}

/// Runs `ops` over `input` with default [`ScannerOptions`].
pub fn run(input: &str, ops: &[TraceOp]) -> Vec<TraceEvent> {
    run_with(input, ScannerOptions::default(), ops)
}

/// Runs `ops` over `input` with explicit options.
pub fn run_with(input: &str, opts: ScannerOptions, ops: &[TraceOp]) -> Vec<TraceEvent> {
    let mut sc = Scanner::new(input, Arc::from("trace"), opts);
    let mut out = Vec::with_capacity(ops.len());
    for &op in ops {
        let value = match op {
            TraceOp::Token { comma } => {
                sc.get_token(comma);
                TraceValue::None
            }
            TraceOp::Number { comma } => TraceValue::U32(sc.get_number(comma)),
            TraceOp::Int64 { comma } => TraceValue::I64(sc.get_int64(comma)),
            TraceOp::Float { comma } => TraceValue::F32Bits(sc.get_float(comma).to_bits()),
            TraceOp::Hex { comma } => TraceValue::U32(sc.get_hex(comma)),
            TraceOp::LastFull => {
                sc.get_last_full();
                TraceValue::None
            }
        };
        out.push(TraceEvent {
            op,
            ty: sc.token_type(),
            text: sc.token().to_owned(),
            value,
            pos: sc.pos(),
            finished: sc.finished(),
        });
        // The trace is for comparison, not for diagnostics: keep memory bounded.
        let _ = sc.take_diagnostics();
    }
    out
}
