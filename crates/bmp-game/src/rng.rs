//! Randomness and core numeric helpers of `BMMAIN.EXE`.
//!
//! This module reproduces, bit for bit, the Microsoft C 6/7 runtime random number generator
//! (`srand`/`rand`), the game's single random entry point `random_range` (176b:0cc7) and the small
//! numeric/text helpers that game logic relies on (clamping, long arithmetic with MSC semantics,
//! German upper-casing and the thousands-separated number formatter).
//!
//! All semantics, addresses and the caller census of `random_range` are documented in
//! `docs/re/logic/rng.md`.
//!
//! Integer conventions of the original (16-bit large model, MSC):
//! * `int` is `i16`, `long` is `i32`, `char` is signed (`i8`).
//! * All arithmetic wraps; there are no overflow checks.
//! * Integer division by zero raises CPU exception 0, which the MSC runtime turns into
//!   "run-time error R6003 - integer divide by 0" and terminates the program. The ports below
//!   panic in that situation (or return `None` in the `checked_*` variants).

/// Initial value of the `rand()` state at DS:9720 in the data segment image (before any `srand`).
pub const RAND_INITIAL_STATE: u32 = 1;

/// LCG multiplier used by MSC `rand()` (4a01:17be).
pub const RAND_MULTIPLIER: u32 = 0x0003_43FD;

/// LCG increment used by MSC `rand()` (4a01:17be).
pub const RAND_INCREMENT: u32 = 0x0026_9EC3;

/// Largest value returned by `rand()`.
pub const RAND_MAX: i16 = 0x7FFF;

/// Message of the MSC runtime error raised by an integer division by zero.
const R6003: &str = "run-time error R6003 - integer divide by 0";

/// Deterministic random number generator with exactly the behaviour of the game's MSC runtime.
///
/// The whole generator state is the 32-bit value stored at DS:9720 (low word) / DS:9722
/// (high word). It is not part of the save game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rng {
    state: u32,
}

impl Default for Rng {
    /// The state the program starts with (DS:9720 = 1, DS:9722 = 0).
    fn default() -> Self {
        Self {
            state: RAND_INITIAL_STATE,
        }
    }
}

impl Rng {
    /// Creates a generator seeded like `srand(seed)`.
    ///
    /// Origin: BMMAIN.EXE 4a01:17ac (crt_srand)
    pub fn new(seed: u16) -> Self {
        let mut rng = Self::default();
        rng.srand(seed);
        rng
    }

    /// Creates a generator from a raw 32-bit state (DS:9722:DS:9720).
    pub fn from_state(state: u32) -> Self {
        Self { state }
    }

    /// Returns the raw 32-bit state (DS:9722:DS:9720).
    pub fn state(&self) -> u32 {
        self.state
    }

    /// `srand(seed)`: the low state word becomes `seed`, the high word becomes 0.
    ///
    /// Origin: BMMAIN.EXE 4a01:17ac (crt_srand)
    pub fn srand(&mut self, seed: u16) {
        self.state = u32::from(seed);
    }

    /// `rand()`: `state = state * 0x343FD + 0x269EC3` (mod 2^32), returns `(state >> 16) & 0x7FFF`.
    ///
    /// Origin: BMMAIN.EXE 4a01:17be (crt_rand)
    pub fn rand(&mut self) -> i16 {
        self.state = self
            .state
            .wrapping_mul(RAND_MULTIPLIER)
            .wrapping_add(RAND_INCREMENT);
        ((self.state >> 16) & 0x7FFF) as i16
    }

    /// `random_range(lo, hi)`: `rand() % (hi - lo + 1) + lo` with 16-bit signed arithmetic.
    ///
    /// Exactly: `span = (hi - lo + 1) as i16` (wrapping), `r = rand()` (0..=0x7FFF),
    /// `IDIV` gives `r % span` with the sign of the dividend (always >= 0, so effectively
    /// `r % |span|`), and the result is `(r % span + lo) as i16` (wrapping).
    ///
    /// Consequences that callers must reproduce:
    /// * `hi < lo` does not fail: e.g. `random_range(5, 2)` has span -2 and returns 5 or 6.
    /// * Spans above 0x7FFF wrap negative: `random_range(0, 0x8FFF)` has span -0x7000 and
    ///   therefore returns 0..=0x6FFF.
    /// * `rand()` is consumed before the division, also when the span is zero.
    ///
    /// # Panics
    /// When `hi - lo + 1 == 0` (mod 2^16), e.g. `random_range(1, 0)`; the original dies with
    /// MSC runtime error R6003. Use [`Rng::checked_random_range`] to observe this case.
    ///
    /// Origin: BMMAIN.EXE 176b:0cc7 (random_range)
    pub fn random_range(&mut self, lo: i16, hi: i16) -> i16 {
        self.checked_random_range(lo, hi).expect(R6003)
    }

    /// Like [`Rng::random_range`], but returns `None` (after consuming one `rand()` value, as
    /// the original does before trapping) when the span is zero.
    ///
    /// Origin: BMMAIN.EXE 176b:0cc7 (random_range)
    pub fn checked_random_range(&mut self, lo: i16, hi: i16) -> Option<i16> {
        let r = self.rand();
        let span = hi.wrapping_sub(lo).wrapping_add(1);
        if span == 0 {
            return None;
        }
        // r is never i16::MIN, so the remainder cannot overflow.
        Some((r % span).wrapping_add(lo))
    }

    /// `random_seed_from_time()`: `srand((unsigned) time(NULL))`, i.e. only the low 16 bits of
    /// the MSC `time_t` value seed the generator.
    ///
    /// `time` is the MSC `time()` result: local DOS date/time converted by `__loctotime_t`
    /// (4a01:1596), which adds the `TZ` offset (MSC default when `TZ` is unset: PST8PDT, i.e.
    /// +8 h, with the US daylight-saving rule of `_isindst`).
    ///
    /// Origin: BMMAIN.EXE 176b:0cf0 (random_seed_from_time)
    pub fn seed_from_time(&mut self, time: i32) {
        self.srand(time as u16);
    }

    /// `random_chance_pick(n, default, flag, lo, hi)`.
    ///
    /// Draws `random_range(1, n)`. If the draw is greater than `*flag`, `*flag` is set to **1**
    /// (not to the draw) and the low byte of `random_range(lo, hi)` is returned; otherwise
    /// `default` is returned and only one `rand()` is consumed. All byte arguments are
    /// zero-extended to `int` before the calls and the comparison.
    ///
    /// # Panics
    /// When `n == 0` (span 0 in the first `random_range`), or when `hi - lo + 1 == 0`
    /// (impossible for zero-extended bytes).
    ///
    /// Origin: BMMAIN.EXE 3e3a:21ef (random_chance_pick)
    pub fn chance_pick(&mut self, n: u8, default: u8, flag: &mut u8, lo: u8, hi: u8) -> u8 {
        let draw = self.random_range(1, i16::from(n));
        if draw > i16::from(*flag) {
            *flag = 1;
            self.random_range(i16::from(lo), i16::from(hi)) as u8
        } else {
            default
        }
    }
}

/// `*p = clamp(*p, lo, hi)` for `int`: first raised to `lo`, then lowered to `hi` (signed
/// compares). With `lo > hi` the result is always `hi`.
///
/// Origin: BMMAIN.EXE 4091:1009 (clamp_int)
pub fn clamp_int(value: &mut i16, lo: i16, hi: i16) {
    if *value < lo {
        *value = lo;
    }
    if *value > hi {
        *value = hi;
    }
}

/// `*p = clamp(*p, lo, hi)` for signed `char` (same order of checks as [`clamp_int`]).
///
/// Origin: BMMAIN.EXE 4091:1033 (clamp_char)
pub fn clamp_char(value: &mut i8, lo: i8, hi: i8) {
    if *value < lo {
        *value = lo;
    }
    if *value > hi {
        *value = hi;
    }
}

/// `|a - b|` of two longs, computed as `b - a` when `b > a` (signed), else `a - b`,
/// with 32-bit wrap-around (the full long is returned in DX:AX; many callers use only AX).
///
/// Origin: BMMAIN.EXE 4091:10d5 (long_abs_diff)
pub fn long_abs_diff(a: i32, b: i32) -> i32 {
    if b > a {
        b.wrapping_sub(a)
    } else {
        a.wrapping_sub(b)
    }
}

/// Upper-cases a NUL-terminated code page 437 string the way the game does.
///
/// Copies bytes up to (not including) the first NUL or the end of `src`. Bytes `a`..=`z` become
/// `A`..=`Z`; `0x81` (u umlaut) becomes `0x9A`, `0x84` (a umlaut) becomes `0x8E`, `0x94`
/// (o umlaut) becomes `0x99`. Everything else (including `0xE1`, sharp s) is copied unchanged.
/// The original writes a terminating NUL and returns the destination pointer.
///
/// Origin: BMMAIN.EXE 4091:105d (text_to_upper_de)
pub fn text_to_upper_de(src: &[u8]) -> Vec<u8> {
    src.iter()
        .take_while(|&&c| c != 0)
        .map(|&c| match c {
            b'a'..=b'z' => c - 0x20,
            0x81 => 0x9A,
            0x84 => 0x8E,
            0x94 => 0x99,
            _ => c,
        })
        .collect()
}

/// Inputs of [`format_long_thousands`] that the original reads from globals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NumberFormat {
    /// DS:07b2 != 0: suppress the thousands separators.
    pub no_separators: bool,
    /// The long at DS:224e/2250 equals 4: use `,` instead of `.` as separator.
    pub comma_separator: bool,
    /// DS:079c (signed byte): minimum field width for non-negative values; the field is
    /// left-filled with `^` (0x5E). Negative numbers are never padded.
    pub pad_width: i8,
}

/// Fill character used by [`format_long_thousands`] for padding (rendered by the game fonts as
/// a digit-wide blank).
pub const NUMBER_PAD_CHAR: u8 = b'^';

/// Size of the scratch area modelling DS:a306.. (digit buffer at +0, output at +12).
const FMT_SCRATCH: usize = 12 + 2 * 128 + 4;
/// Offset of the output string (DS:a312) relative to the digit buffer (DS:a306).
const FMT_OUT: usize = 12;

/// Converts a long to a decimal string with thousands separators (German style `1.234.567`).
///
/// Algorithm of the original (buffers DS:a306 = reversed digits, DS:a312 = result):
/// 1. If the value is negative it is negated (wrapping; `i32::MIN` stays negative) and a sign
///    flag is set.
/// 2. While the value is non-zero: if separators are enabled and the count of emitted bytes is
///    3, 7 or 11, emit the separator; then emit `'0' + value % 10` (MSC remainder, sign of the
///    dividend) and divide by 10 (truncating).
/// 3. Negative: the bytes are reversed behind a leading `-`. Otherwise the buffer is padded with
///    `^` up to `pad_width` bytes (a zero value yields `0` in the first padded position, so
///    `0` with `pad_width <= 0` is the empty string) and reversed.
///
/// `i32::MIN` therefore formats with garbage digits (`'0' + negative remainder`), exactly as in
/// the original. Returns the bytes without the terminating NUL.
///
/// Origin: BMMAIN.EXE 176b:0681 (format_long_thousands)
pub fn format_long_thousands(value: i32, fmt: NumberFormat) -> Vec<u8> {
    let mut mem = [0u8; FMT_SCRATCH];
    let mut v = value;
    let mut negative = false;
    let mut d: i16 = 0;
    if v < 0 {
        v = v.wrapping_neg();
        negative = true;
    }
    while v != 0 {
        if !fmt.no_separators && (d == 3 || d == 7 || d == 11) {
            mem[d as usize] = if fmt.comma_separator { b',' } else { b'.' };
            d += 1;
        }
        let digit = lrem(v, 10);
        mem[d as usize] = (digit as u8).wrapping_add(b'0');
        d += 1;
        v = ldiv(v, 10);
    }
    if negative {
        let mut e = d;
        while e > 0 {
            mem[FMT_OUT + e as usize] = mem[(d - e) as usize];
            e -= 1;
        }
        mem[FMT_OUT] = b'-';
    } else {
        let mut e = d;
        while i16::from(fmt.pad_width) > e {
            mem[e as usize] = if e == 0 { b'0' } else { NUMBER_PAD_CHAR };
            d += 1;
            e += 1;
        }
        d -= 1;
        let mut e = d;
        while e >= 0 {
            mem[FMT_OUT + e as usize] = mem[(d - e) as usize];
            e -= 1;
        }
    }
    let len = (d + 1) as usize;
    mem[FMT_OUT + len] = 0;
    mem[FMT_OUT..FMT_OUT + len].to_vec()
}

/// Lower/upper bound rounding of the value spinner: when the long at DS:224e/2250 is greater
/// than 1 and the initial value is non-zero, both bounds are truncated toward zero to a multiple
/// of 5 (`if b % 5 != 0 { b = b / 5 * 5 }`, MSC signed long semantics).
///
/// Origin: BMMAIN.EXE 4091:0ba5 (money_round_display, bound rounding prologue only)
pub fn spinner_round_bound(bound: i32, option_224e: i32, initial_value: i32) -> i32 {
    if option_224e > 1 && initial_value != 0 && lrem(bound, 5) != 0 {
        lmul(ldiv(bound, 5), 5)
    } else {
        bound
    }
}

/// Signed 32-bit division truncating toward zero; `i32::MIN / -1` wraps to `i32::MIN`.
/// Returns `None` for a zero divisor (R6003 in the original).
///
/// Origin: BMMAIN.EXE 4a01:1a4c (crt_afldiv)
pub fn checked_ldiv(a: i32, b: i32) -> Option<i32> {
    if b == 0 {
        None
    } else {
        Some(a.wrapping_div(b))
    }
}

/// `_aFldiv`: signed 32-bit division truncating toward zero (`i32::MIN / -1 == i32::MIN`).
///
/// # Panics
/// On a zero divisor (R6003).
///
/// Origin: BMMAIN.EXE 4a01:1a4c (crt_afldiv)
pub fn ldiv(a: i32, b: i32) -> i32 {
    checked_ldiv(a, b).expect(R6003)
}

/// `_aFlrem`: signed 32-bit remainder with the sign of the dividend (`i32::MIN % -1 == 0`).
///
/// # Panics
/// On a zero divisor (R6003).
///
/// Origin: BMMAIN.EXE 4a01:1b18 (crt_aflrem)
pub fn lrem(a: i32, b: i32) -> i32 {
    if b == 0 {
        panic!("{R6003}");
    }
    a.wrapping_rem(b)
}

/// `_aFlmul`: 32-bit multiplication, low 32 bits of the product (same for signed and unsigned).
///
/// Origin: BMMAIN.EXE 4a01:1ae6 (crt_aflmul)
pub fn lmul(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

/// `_aFuldiv`: unsigned 32-bit division.
///
/// # Panics
/// On a zero divisor (R6003).
///
/// Origin: BMMAIN.EXE 4a01:1c10 (crt_afuldiv)
pub fn uldiv(a: u32, b: u32) -> u32 {
    a.checked_div(b).expect(R6003)
}

/// `_aFulrem`: unsigned 32-bit remainder.
///
/// # Panics
/// On a zero divisor (R6003).
///
/// Origin: BMMAIN.EXE 4a01:1c70 (crt_afulrem)
pub fn ulrem(a: u32, b: u32) -> u32 {
    a.checked_rem(b).expect(R6003)
}

/// `_aFlshl`: 32-bit shift left by `CL` (only the low byte counts; the loop shifts one bit per
/// iteration, so counts >= 32 give 0).
///
/// Origin: BMMAIN.EXE 4a01:1bb8 (crt_aflshl)
pub fn lshl(a: i32, count: u8) -> i32 {
    if count >= 32 {
        0
    } else {
        a.wrapping_shl(u32::from(count))
    }
}

/// `_aFlshr`: 32-bit **arithmetic** shift right by `CL` (`SAR`/`RCR` loop, low byte of the
/// count; counts >= 32 give 0 or -1).
///
/// Origin: BMMAIN.EXE 4a01:1bc4 (crt_aflshr)
pub fn lshr(a: i32, count: u8) -> i32 {
    a >> u32::from(count.min(31))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference sequence of MSC `rand()` after `srand(1)` (also the power-on state).
    const MSC_SEED1: [i16; 10] = [
        41, 18467, 6334, 26500, 19169, 15724, 11478, 29358, 26962, 24464,
    ];

    #[test]
    fn rand_matches_msc_reference_sequence() {
        let mut rng = Rng::new(1);
        for &want in &MSC_SEED1 {
            assert_eq!(rng.rand(), want);
        }
        let mut rng = Rng::default();
        for &want in &MSC_SEED1 {
            assert_eq!(rng.rand(), want);
        }
    }

    #[test]
    fn srand_clears_high_word() {
        let mut rng = Rng::from_state(0xDEAD_BEEF);
        rng.srand(0x1234);
        assert_eq!(rng.state(), 0x1234);
        assert_eq!(rng.rand(), Rng::new(0x1234).rand());
    }

    #[test]
    fn rand_state_update() {
        let mut rng = Rng::new(0);
        rng.rand();
        assert_eq!(rng.state(), 0x0026_9EC3);
        assert_eq!(Rng::new(0).rand(), 0x26);
    }

    #[test]
    fn random_range_basic() {
        // seed 1: first rand() is 41
        assert_eq!(Rng::new(1).random_range(0, 9), 1);
        assert_eq!(Rng::new(1).random_range(1, 6), 41 % 6 + 1);
        assert_eq!(Rng::new(1).random_range(-5, 5), 41 % 11 - 5);
        assert_eq!(Rng::new(1).random_range(7, 7), 7);
        let mut rng = Rng::new(42);
        for _ in 0..10_000 {
            let v = rng.random_range(-3, 12);
            assert!((-3..=12).contains(&v));
        }
    }

    #[test]
    fn random_range_reversed_and_wrapped_spans() {
        // hi < lo: span -2, 41 % -2 == 1 -> 6
        assert_eq!(Rng::new(1).random_range(5, 2), 6);
        // span 0x9000 wraps to -0x7000: result is rand % 0x7000
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..1000 {
            let v = a.random_range(0, 0x8FFF_u16 as i16);
            let r = b.rand();
            assert_eq!(v, r % 0x7000);
        }
        // span 0x8000 (-32768): rand unchanged
        let mut a = Rng::new(3);
        let mut b = Rng::new(3);
        assert_eq!(a.random_range(0, 0x7FFF), b.rand());
        // wrap-around of the final addition
        assert_eq!(Rng::new(1).random_range(0x7FF0, 0x7FFF), 0x7FF0 + 41 % 16);
    }

    #[test]
    fn random_range_zero_span_consumes_rand() {
        let mut rng = Rng::new(1);
        assert_eq!(rng.checked_random_range(1, 0), None);
        assert_eq!(rng.rand(), MSC_SEED1[1]);
        assert_eq!(Rng::new(1).checked_random_range(i16::MIN, i16::MAX), None);
    }

    #[test]
    #[should_panic(expected = "R6003")]
    fn random_range_zero_span_panics() {
        Rng::new(1).random_range(1, 0);
    }

    #[test]
    fn seed_from_time_uses_low_word() {
        let mut rng = Rng::default();
        rng.seed_from_time(0x2C5A_1234);
        assert_eq!(rng.state(), 0x1234);
    }

    #[test]
    fn chance_pick_semantics() {
        // seed 1: draws 41, 18467
        let mut flag = 0u8;
        let v = Rng::new(1).chance_pick(100, 9, &mut flag, 10, 20);
        assert_eq!(flag, 1);
        assert_eq!(v as i16, 18467 % 11 + 10);
        // draw (41 % 50 + 1 = 42) not greater than flag 200 -> default, one rand consumed
        let mut flag = 200u8;
        let mut rng = Rng::new(1);
        assert_eq!(rng.chance_pick(50, 9, &mut flag, 10, 20), 9);
        assert_eq!(flag, 200);
        assert_eq!(rng.rand(), MSC_SEED1[1]);
    }

    #[test]
    fn clamp_helpers() {
        let mut v = -5i16;
        clamp_int(&mut v, 0, 10);
        assert_eq!(v, 0);
        let mut v = 50i16;
        clamp_int(&mut v, 0, 10);
        assert_eq!(v, 10);
        let mut v = 5i16;
        clamp_int(&mut v, 10, 0); // lo > hi: ends at hi
        assert_eq!(v, 0);
        let mut c = -100i8;
        clamp_char(&mut c, -10, 10);
        assert_eq!(c, -10);
        let mut c = 120i8;
        clamp_char(&mut c, -10, 10);
        assert_eq!(c, 10);
    }

    #[test]
    fn long_abs_diff_cases() {
        assert_eq!(long_abs_diff(10, 3), 7);
        assert_eq!(long_abs_diff(3, 10), 7);
        assert_eq!(long_abs_diff(-70000, 70000), 140000);
        assert_eq!(long_abs_diff(i32::MIN, i32::MAX), -1); // wraps
    }

    #[test]
    fn upper_de() {
        assert_eq!(
            text_to_upper_de(b"M\x81ller br\x84u K\x94ln \xe1z\0junk"),
            b"M\x9aLLER BR\x8eU K\x99LN \xe1Z".to_vec()
        );
    }

    #[test]
    fn long_math_semantics() {
        assert_eq!(ldiv(-7, 2), -3);
        assert_eq!(lrem(-7, 2), -1);
        assert_eq!(lrem(7, -2), 1);
        assert_eq!(ldiv(i32::MIN, -1), i32::MIN);
        assert_eq!(lrem(i32::MIN, -1), 0);
        assert_eq!(checked_ldiv(1, 0), None);
        assert_eq!(lmul(0x10000, 0x10000), 0);
        assert_eq!(lmul(-3, 5), -15);
        assert_eq!(uldiv(0xFFFF_FFFF, 16), 0x0FFF_FFFF);
        assert_eq!(ulrem(0xFFFF_FFFF, 16), 15);
        assert_eq!(lshl(1, 31), i32::MIN);
        assert_eq!(lshl(1, 40), 0);
        assert_eq!(lshr(-16, 2), -4);
        assert_eq!(lshr(-16, 200), -1);
        assert_eq!(lshr(16, 200), 0);
    }

    fn fmt(v: i32) -> String {
        String::from_utf8(format_long_thousands(v, NumberFormat::default())).unwrap()
    }

    #[test]
    fn format_thousands() {
        assert_eq!(fmt(0), "");
        assert_eq!(fmt(7), "7");
        assert_eq!(fmt(1234), "1.234");
        assert_eq!(fmt(999), "999");
        assert_eq!(fmt(1_000_000), "1.000.000");
        assert_eq!(fmt(-1_234_567), "-1.234.567");
        assert_eq!(fmt(i32::MAX), "2.147.483.647");
        let comma = NumberFormat {
            comma_separator: true,
            ..Default::default()
        };
        assert_eq!(format_long_thousands(1234, comma), b"1,234");
        let plain = NumberFormat {
            no_separators: true,
            ..Default::default()
        };
        assert_eq!(format_long_thousands(1234567, plain), b"1234567");
    }

    #[test]
    fn format_padding() {
        let w = |pad_width| NumberFormat {
            pad_width,
            ..Default::default()
        };
        assert_eq!(format_long_thousands(0, w(1)), b"0");
        assert_eq!(format_long_thousands(0, w(3)), b"^^0");
        assert_eq!(format_long_thousands(42, w(5)), b"^^^42");
        assert_eq!(format_long_thousands(1234, w(7)), b"^^1.234");
        assert_eq!(format_long_thousands(-42, w(5)), b"-42");
        assert_eq!(format_long_thousands(123456, w(2)), b"123.456");
    }

    #[test]
    fn format_min_long_garbage() {
        // '0' + (-8) == '(' etc., exactly like the original
        let s = format_long_thousands(i32::MIN, NumberFormat::default());
        assert_eq!(s[0], b'-');
        assert_eq!(*s.last().unwrap(), b'(');
    }

    #[test]
    fn spinner_bounds() {
        assert_eq!(spinner_round_bound(1234, 2, 1), 1230);
        assert_eq!(spinner_round_bound(-1234, 2, 1), -1230);
        assert_eq!(spinner_round_bound(1234, 1, 1), 1234);
        assert_eq!(spinner_round_bound(1234, 2, 0), 1234);
    }
}
