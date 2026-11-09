//! The question generator of the arithmetic game (`WORLDSERVER/MiniGameArithmetic.cpp:123-173`, Plan/22 §14.6).
//!
//! The draw order is normative: for `i` in 0..3 the number `random_range(1, 1000)` and then the operator
//! `random_below(4)` (interleaved); a division then draws its divisor ([`divisor`]) at the place where the legacy
//! `Calculate` evaluated it. All arithmetic is `i32` with the C wrap-around (the largest reachable value, 999 x 999
//! x 999, fits anyway).

use flyff_common::rng::GameRng;

/// The 168 primes below 1000 in ascending order (the legacy `InitPrimeNumber` list).
pub const PRIMES: [u16; 168] = primes();

const fn primes() -> [u16; 168] {
    let mut out = [0u16; 168];
    let mut n = 0;
    let mut i: u16 = 2;
    while i < 1000 {
        let mut prime = true;
        let mut j: u16 = 2;
        while j * j <= i {
            if i.is_multiple_of(j) {
                prime = false;
                break;
            }
            j += 1;
        }
        if prime {
            out[n] = i;
            n += 1;
        }
        i += 1;
    }
    out
}

/// `GetOperatorCh`.
fn operator_char(op: u32) -> char {
    match op {
        1 => '-',
        2 => '*',
        3 => '/',
        _ => '+',
    }
}

/// The list `GetDivisor` builds: `[1]` and the prime factors of `a` with multiplicity, found by restarting at the
/// smallest prime after each hit and stopping at the first prime above the rest (`a <= 1` gives `[1]`). The legacy
/// looped forever on a cofactor without a prime below 1000 (unreachable from the generator); here the search stops.
pub fn divisors_of(a: i32) -> Vec<i32> {
    let mut list = vec![1];
    let mut rest = a;
    'search: loop {
        for &p in &PRIMES {
            let p = i32::from(p);
            if p > rest {
                break 'search;
            }
            if rest % p == 0 {
                list.push(p);
                rest /= p;
                continue 'search;
            }
        }
        break;
    }
    list
}

/// `GetDivisor(a)`: `n = random_below(len) + 1`, then `n` times `i = random_below(current len)`, multiply by that entry
/// and remove it (a random product of the prime factors of `a`, 1 included).
pub fn divisor(a: i32, rng: &mut dyn GameRng) -> i32 {
    let mut list = divisors_of(a);
    let n = rng.random_below(u32::try_from(list.len()).unwrap_or(1)) + 1;
    let mut d: i32 = 1;
    for _ in 0..n {
        if list.is_empty() {
            break;
        }
        let i = rng.random_below(u32::try_from(list.len()).unwrap_or(1)) as usize;
        d = d.wrapping_mul(list.remove(i.min(list.len() - 1)));
    }
    d
}

/// `Calculate(a, b, op)`: `+ - *` as C; a division first REPLACES `b` by [`divisor`]`(a)` (the printed operand) and
/// answers `a / b`.
pub fn calc(a: i32, b: &mut i32, op: u32, rng: &mut dyn GameRng) -> i32 {
    match op {
        0 => a.wrapping_add(*b),
        1 => a.wrapping_sub(*b),
        2 => a.wrapping_mul(*b),
        3 => {
            *b = divisor(a, rng);
            a.checked_div(*b).unwrap_or(0)
        }
        _ => -1,
    }
}

/// `MakeQuestion`: the text of the question (`"%d %c %d"` or `"%d %c %d %c %d"`, the REPLACED divisor printed) and its
/// integer answer.
pub fn make_question(rng: &mut dyn GameRng) -> (String, i32) {
    let mut num = [0i32; 3];
    let mut op = [0u32; 3];
    for i in 0..3 {
        num[i] = i32::try_from(rng.random_range(1, 1000)).unwrap_or(1);
        op[i] = rng.random_below(4);
    }
    let (text, result) = if op[2] > 1 {
        // three numbers
        let result = if op[0] < 2 && op[1] > 1 {
            // the later operator (`*`, `/`) binds first
            let mut n2 = num[2];
            let mut r = calc(num[1], &mut n2, op[1], rng);
            num[2] = n2;
            r = calc(num[0], &mut r, op[0], rng);
            r
        } else {
            let mut n1 = num[1];
            let r = calc(num[0], &mut n1, op[0], rng);
            num[1] = n1;
            let mut n2 = num[2];
            let result = calc(r, &mut n2, op[1], rng);
            num[2] = n2;
            result
        };
        let text = format!(
            "{} {} {} {} {}",
            num[0],
            operator_char(op[0]),
            num[1],
            operator_char(op[1]),
            num[2]
        );
        (text, result)
    } else {
        let mut n1 = num[1];
        let result = calc(num[0], &mut n1, op[0], rng);
        num[1] = n1;
        (
            format!("{} {} {}", num[0], operator_char(op[0]), num[1]),
            result,
        )
    };
    (text, result)
}
