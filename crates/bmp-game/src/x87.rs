//! Exact model of the x87 extended-precision arithmetic used by `BMMAIN.EXE`.
//!
//! The game's floating-point code runs on the MSC floating-point emulator (`INT 34h..3Dh`), which
//! behaves like an 8087: every intermediate result lives in an 80-bit register with a 64-bit
//! mantissa and is rounded to nearest-even after each instruction. Only a store to a `double`
//! (`fstp m64`) rounds to 53 bits. Evaluating the same expression in `f64` rounds every step to
//! 53 bits instead, which moves results that land next to an integer across it and changes what
//! `_ftol` returns. [`F80`] reproduces the register arithmetic bit for bit for the operations the
//! ported code needs (see `docs/re/logic/ai.md` §2.6).
//!
//! Only finite values are modelled: zero and normal numbers. Infinities, NaNs and denormals
//! cannot arise from the game's operands and panic.

use std::ops::{Add, Mul, Sub};

/// Mantissa bits that `fstp m64` drops from the 64-bit register mantissa.
const DOUBLE_DROPPED_BITS: u32 = 64 - 53;

/// A finite x87 register value: `(-1)^negative * mantissa * 2^(exponent - 63)`.
///
/// `mantissa` has bit 63 set for every non-zero value; zero is `mantissa == 0, exponent == 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct F80 {
    negative: bool,
    exponent: i32,
    mantissa: u64,
}

impl F80 {
    /// Positive zero.
    pub const ZERO: F80 = F80 {
        negative: false,
        exponent: 0,
        mantissa: 0,
    };

    /// `fild m32`: exact.
    pub fn from_i32(v: i32) -> F80 {
        round(v < 0, u128::from(v.unsigned_abs()), 0)
    }

    /// `fild m16`: exact.
    pub fn from_i16(v: i16) -> F80 {
        F80::from_i32(i32::from(v))
    }

    /// `fld m64`: exact widening of a double.
    ///
    /// # Panics
    /// On infinities, NaNs and denormals.
    pub fn from_f64(v: f64) -> F80 {
        let bits = v.to_bits();
        let negative = bits >> 63 != 0;
        let biased = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1 << 52) - 1);
        match biased {
            0 if fraction == 0 => F80::ZERO,
            1..=0x7fe => round(negative, u128::from(fraction | 1 << 52), biased - 1023 - 52),
            _ => panic!("x87 model: {v} is not a finite normal double"),
        }
    }

    /// `fstp m64`: rounds the mantissa to 53 bits, nearest-even.
    ///
    /// # Panics
    /// When the value does not fit a normal double.
    pub fn to_f64(self) -> f64 {
        if self.mantissa == 0 {
            return 0.0;
        }
        let half = 1u64 << (DOUBLE_DROPPED_BITS - 1);
        let rest = self.mantissa & ((1 << DOUBLE_DROPPED_BITS) - 1);
        let mut kept = self.mantissa >> DOUBLE_DROPPED_BITS;
        let mut exponent = self.exponent;
        if rest > half || (rest == half && kept & 1 == 1) {
            kept += 1;
            if kept == 1 << 53 {
                kept >>= 1;
                exponent += 1;
            }
        }
        let biased = exponent + 1023;
        assert!(
            (1..=0x7fe).contains(&biased),
            "x87 model: {self:?} does not fit a normal double"
        );
        let bits =
            u64::from(self.negative) << 63 | (biased as u64) << 52 | (kept & ((1 << 52) - 1));
        f64::from_bits(bits)
    }

    /// `_ftol`: truncates toward zero (`fistp m64` under chop rounding) and returns the low
    /// 32 bits of the 64-bit integer.
    ///
    /// # Panics
    /// When the value is outside the `fistp m64` range.
    ///
    /// Origin: BMMAIN.EXE 4a01:3156 (_ftol; Ghidra name crt_fp_emul_init)
    pub fn ftol(self) -> i32 {
        if self.exponent < 0 {
            return 0;
        }
        assert!(
            self.exponent <= 62,
            "x87 model: {self:?} is outside the fistp m64 range"
        );
        let magnitude = (self.mantissa >> (63 - self.exponent)) as i64;
        let value = if self.negative { -magnitude } else { magnitude };
        value as i32
    }
}

/// `fmul`: product rounded to nearest-even at 64 mantissa bits.
impl Mul for F80 {
    type Output = F80;

    fn mul(self, rhs: F80) -> F80 {
        let product = u128::from(self.mantissa) * u128::from(rhs.mantissa);
        round(
            self.negative != rhs.negative,
            product,
            self.exponent + rhs.exponent - 126,
        )
    }
}

/// `fadd`: sum rounded to nearest-even at 64 mantissa bits.
impl Add for F80 {
    type Output = F80;

    fn add(self, rhs: F80) -> F80 {
        if self.mantissa == 0 {
            return rhs;
        }
        if rhs.mantissa == 0 {
            return self;
        }
        let (big, small) = if self.exponent >= rhs.exponent {
            (self, rhs)
        } else {
            (rhs, self)
        };
        // 62 guard bits below the mantissa plus a sticky bit make the single final rounding
        // exact: an inexact aligned operand always yields an odd sum, never a rounding tie.
        let x = u128::from(big.mantissa) << 62;
        let shift = (big.exponent - small.exponent) as u32;
        let s = u128::from(small.mantissa) << 62;
        let s = if shift >= 128 {
            1
        } else {
            let lost = s & ((1u128 << shift) - 1) != 0;
            (s >> shift) | u128::from(lost)
        };
        let scale = big.exponent - 63 - 62;
        if big.negative == small.negative {
            round(big.negative, x + s, scale)
        } else if x >= s {
            round(big.negative, x - s, scale)
        } else {
            round(small.negative, s - x, scale)
        }
    }
}

/// `fsub`: difference rounded to nearest-even at 64 mantissa bits.
impl Sub for F80 {
    type Output = F80;

    fn sub(self, rhs: F80) -> F80 {
        self + F80 {
            negative: !rhs.negative,
            ..rhs
        }
    }
}

/// Rounds `magnitude * 2^scale` to a 64-bit mantissa, nearest-even.
fn round(negative: bool, magnitude: u128, scale: i32) -> F80 {
    if magnitude == 0 {
        return F80::ZERO;
    }
    let top = 127 - magnitude.leading_zeros() as i32;
    if top <= 63 {
        return F80 {
            negative,
            exponent: scale + top,
            mantissa: (magnitude << (63 - top)) as u64,
        };
    }
    let shift = (top - 63) as u32;
    let half = 1u128 << (shift - 1);
    let rest = magnitude & ((1u128 << shift) - 1);
    let mut kept = magnitude >> shift;
    let mut exponent = scale + top;
    if rest > half || (rest == half && kept & 1 == 1) {
        kept += 1;
        if kept == 1 << 64 {
            kept >>= 1;
            exponent += 1;
        }
    }
    F80 {
        negative,
        exponent,
        mantissa: kept as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_arithmetic_is_exact() {
        let p = F80::from_i32(7) * F80::from_i32(-3);
        assert_eq!(p.ftol(), -21);
        assert_eq!(p, F80::from_i32(-21));
        assert_eq!(F80::from_i16(-5) + F80::from_i32(5), F80::ZERO);
        assert_eq!((F80::from_i32(1000) - F80::from_i16(1)).ftol(), 999);
        assert_eq!(F80::from_f64(-2.5).to_f64(), -2.5);
        assert_eq!(F80::from_f64(0.01).to_f64(), 0.01);
    }

    #[test]
    fn products_round_to_nearest_even_at_64_bits() {
        // (2^32 + 1)^2 = 2^64 + 2^33 + 1 needs 65 bits: the dropped bit is exactly half an
        // ulp and the kept mantissa is even, so the tie rounds down.
        let a = F80::from_f64(4_294_967_297.0);
        let sq = a * a;
        assert_eq!(sq.exponent, 64);
        assert_eq!(sq.mantissa, (1 << 63) | (1 << 32));
        // (2^32 + 1) * (2^32 + 3) = 2^64 + 2^34 + 3: again a tie, now with an odd kept
        // mantissa, so it rounds up.
        let c = F80::from_f64(4_294_967_299.0);
        assert_eq!((a * c).mantissa, (1 << 63) | (1 << 33) | 2);
    }

    #[test]
    fn sums_use_one_correct_rounding() {
        // 2^63 + 0.5 is a tie between 2^63 and 2^63 + 1 at 64 bits; nearest-even keeps 2^63.
        let big = F80::from_f64(9_223_372_036_854_775_808.0);
        assert_eq!(big + F80::from_f64(0.5), big);
        // 2^63 + 0.75 lies above the tie and rounds up.
        let up = big + F80::from_f64(0.75);
        assert_eq!(up.mantissa, (1 << 63) + 1);
        // Subtracting a far smaller operand stays below the original value.
        let one = F80::from_i32(1);
        let tiny = F80::from_f64(f64::from_bits(0x3c00_0000_0000_0000)); // 2^-63
        let below = one - tiny;
        assert_eq!(below.exponent, -1);
        assert_eq!(below.mantissa, u64::MAX - 1);
        let far_below = one - F80::from_f64(1e-300);
        assert_eq!(far_below, one);
    }

    #[test]
    fn extended_precision_differs_from_double_evaluation() {
        // 1 + (2^-53 + 2^-64): f64 rounds once and goes up to 1 + 2^-52. The register keeps
        // 1 + 2^-53 (a tie at 64 bits, rounded to even), and the fstp then meets a tie at
        // 53 bits and rounds to even: 1.0.
        let b = f64::from_bits(0x3ca0_0200_0000_0000);
        assert_eq!(1.0 + b, 1.0 + f64::EPSILON);
        let x = (F80::from_i32(1) + F80::from_f64(b)).to_f64();
        assert_eq!(x, 1.0);
    }

    #[test]
    fn to_f64_rounds_ties_to_even() {
        // Mantissa 2^63 + 2^10: the 11 dropped bits are exactly half, kept mantissa even.
        let tie_even = F80 {
            negative: false,
            exponent: 0,
            mantissa: (1 << 63) | (1 << 10),
        };
        assert_eq!(tie_even.to_f64(), 1.0);
        // Same tie with an odd kept mantissa rounds up.
        let tie_odd = F80 {
            negative: false,
            exponent: 0,
            mantissa: (1 << 63) | (1 << 11) | (1 << 10),
        };
        assert_eq!(tie_odd.to_f64(), 1.0 + 2.0 * f64::EPSILON);
    }

    #[test]
    fn ftol_truncates_toward_zero() {
        assert_eq!(F80::from_f64(2.75).ftol(), 2);
        assert_eq!(F80::from_f64(-2.75).ftol(), -2);
        assert_eq!(F80::from_f64(0.999).ftol(), 0);
        assert_eq!(F80::from_f64(-0.5).ftol(), 0);
        assert_eq!(F80::from_f64(4_294_967_301.0).ftol(), 5); // low 32 bits
    }
}
