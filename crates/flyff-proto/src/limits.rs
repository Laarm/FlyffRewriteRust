//! Generated-limit checks for client-to-server messages (Plan/11 §5.7, §6 `(flyff.max_len)`).
//!
//! `cargo xtask gen-proto` writes `impl Limits for pb::<Message>` (in `src/gen/limits.rs`) for every
//! message reachable from `ClientEnvelope`. The gateway calls [`Limits::check_limits`] right after
//! `decode_client` and answers a violation with `Kick{PROTOCOL_ERROR}`.

/// A field exceeded its `(flyff.max_len)` cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {actual} exceeds the limit of {max}")]
pub struct LimitViolation {
    /// `Message.field`.
    pub field: &'static str,
    /// The cap: UTF-8 bytes for `string`/`bytes`, entries for `repeated`.
    pub max: u32,
    /// The actual size.
    pub actual: usize,
}

/// Length caps of a decoded message.
pub trait Limits {
    /// `Ok` when every capped field is within its cap.
    fn check_limits(&self) -> Result<(), LimitViolation>;
}

/// `string`/`bytes` field: `len` (UTF-8 bytes) must be `<= max`.
pub fn check_bytes(field: &'static str, len: usize, max: u32) -> Result<(), LimitViolation> {
    check(field, len, max)
}

/// `repeated` field: the number of entries must be `<= max`.
pub fn check_count(field: &'static str, len: usize, max: u32) -> Result<(), LimitViolation> {
    check(field, len, max)
}

fn check(field: &'static str, len: usize, max: u32) -> Result<(), LimitViolation> {
    if u32::try_from(len).is_ok_and(|l| l <= max) {
        Ok(())
    } else {
        Err(LimitViolation {
            field,
            max,
            actual: len,
        })
    }
}
