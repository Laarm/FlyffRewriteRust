//! C numeric conversions as the original executables performed them (Plan/14 §3.5).
//!
//! The server was built with MSVC; its `atoi`, `_atoi64` and `atof` come from the UCRT. The functions
//! here model that behaviour on byte strings and never panic.

/// MSVC `isspace` in the C locale: space, `\t`, `\n`, `\v`, `\f`, `\r`.
#[inline]
pub fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// Splits leading white space and an optional sign: `(negative, rest)`.
fn sign_and_rest(s: &[u8]) -> (bool, &[u8]) {
    let mut i = 0;
    while s.get(i).copied().is_some_and(c_isspace) {
        i += 1;
    }
    let rest = s.get(i..).unwrap_or_default();
    match rest.first() {
        Some(b'-') => (true, rest.get(1..).unwrap_or_default()),
        Some(b'+') => (false, rest.get(1..).unwrap_or_default()),
        _ => (false, rest),
    }
}

/// `atoi`: skips leading `isspace`, an optional sign and decimal digits, stopping at the first
/// non-digit. Overflow **saturates** to `i32::MAX`/`i32::MIN` (UCRT `strtol`). The flag is `true` when
/// the value saturated (Warning `NumberOverflow`).
pub fn c_atoi_checked(s: &[u8]) -> (i32, bool) {
    let (neg, digits) = sign_and_rest(s);
    // Accumulate in i64 with an early clamp: no overflow is possible below the clamp.
    const CLAMP: i64 = 1 << 40;
    let mut acc: i64 = 0;
    for &b in digits {
        if !b.is_ascii_digit() {
            break;
        }
        acc = (acc.saturating_mul(10).saturating_add(i64::from(b - b'0'))).min(CLAMP);
    }
    let v = if neg { -acc } else { acc };
    match i32::try_from(v) {
        Ok(v) => (v, false),
        Err(_) => (if neg { i32::MIN } else { i32::MAX }, true),
    }
}

/// [`c_atoi_checked`] without the overflow flag.
pub fn c_atoi(s: &[u8]) -> i32 {
    c_atoi_checked(s).0
}

/// `_atoi64`: like [`c_atoi_checked`] with 64-bit saturation.
pub fn c_atoi64_checked(s: &[u8]) -> (i64, bool) {
    let (neg, digits) = sign_and_rest(s);
    let mut acc: u64 = 0;
    let mut overflow = false;
    for &b in digits {
        if !b.is_ascii_digit() {
            break;
        }
        match acc
            .checked_mul(10)
            .and_then(|a| a.checked_add(u64::from(b - b'0')))
        {
            Some(a) => acc = a,
            None => {
                overflow = true;
                break;
            }
        }
    }
    if overflow {
        return (if neg { i64::MIN } else { i64::MAX }, true);
    }
    if neg {
        // `i64::MIN` has magnitude 2^63.
        if acc > (1u64 << 63) {
            (i64::MIN, true)
        } else {
            (0i64.wrapping_sub_unsigned(acc), false)
        }
    } else {
        match i64::try_from(acc) {
            Ok(v) => (v, false),
            Err(_) => (i64::MAX, true),
        }
    }
}

/// [`c_atoi64_checked`] without the overflow flag.
pub fn c_atoi64(s: &[u8]) -> i64 {
    c_atoi64_checked(s).0
}

/// Case-insensitive ASCII prefix test.
fn starts_with_ci(s: &[u8], pat: &[u8]) -> bool {
    s.len() >= pat.len() && s.iter().zip(pat).all(|(a, b)| a.to_ascii_lowercase() == *b)
}

/// `atof`: skips `isspace`, an optional sign, then the longest prefix matching
/// `digits [. digits] | . digits` with an optional exponent `[eE][+-]?digits` (only accepted when it has
/// at least one digit), or case-insensitive `infinity`/`inf`/`nan`. Anything else gives `0.0`.
/// Callers cast the result with `as f32`, as the C code did. Hex floats are not parsed (Plan/14 §3.5).
pub fn c_atof(s: &[u8]) -> f64 {
    let (neg, rest) = sign_and_rest(s);
    let value = if starts_with_ci(rest, b"infinity") || starts_with_ci(rest, b"inf") {
        f64::INFINITY
    } else if starts_with_ci(rest, b"nan") {
        f64::NAN
    } else {
        match parse_decimal_prefix(rest) {
            Some(v) => v,
            // "If no conversion could be performed, zero is returned": a positive zero, whatever the
            // sign character was.
            None => return 0.0,
        }
    };
    if neg { -value } else { value }
}

/// Parses the longest decimal-float prefix of `s` (no sign); `None` when there is none.
fn parse_decimal_prefix(s: &[u8]) -> Option<f64> {
    let digits = |from: usize| -> usize {
        s.get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let int_len = digits(0);
    let mut end = int_len;
    let mut frac_len = 0;
    if s.get(end) == Some(&b'.') {
        frac_len = digits(end + 1);
        // "5." is a valid prefix ("5"); "." alone is not.
        if int_len > 0 || frac_len > 0 {
            end += 1 + frac_len;
        }
    }
    if int_len == 0 && frac_len == 0 {
        return None;
    }
    if matches!(s.get(end), Some(b'e' | b'E')) {
        let mut e = end + 1;
        if matches!(s.get(e), Some(b'+' | b'-')) {
            e += 1;
        }
        let exp_digits = digits(e);
        if exp_digits > 0 {
            end = e + exp_digits;
        }
    }
    s.get(..end)
        .and_then(|p| core::str::from_utf8(p).ok())
        .and_then(|t| t.parse::<f64>().ok())
}

/// `sscanf(s, "%x", &n)`: parses the longest prefix of hexadecimal digits (an optional `0x` prefix is
/// skipped), wrapping into 32 bits. `None` when there is no digit (the original left `n`
/// uninitialised, Plan/14 §4.3).
pub fn c_sscanf_x(s: &[u8]) -> Option<u32> {
    let mut rest = s;
    if rest.len() >= 3
        && rest.first() == Some(&b'0')
        && matches!(rest.get(1), Some(b'x' | b'X'))
        && rest.get(2).is_some_and(u8::is_ascii_hexdigit)
    {
        rest = rest.get(2..).unwrap_or_default();
    }
    let mut acc: u32 = 0;
    let mut any = false;
    for &b in rest {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => break,
        };
        acc = acc.wrapping_shl(4) | u32::from(d);
        any = true;
    }
    any.then_some(acc)
}
