//! [`Num`], the engine's number type for incremental games.
//!
//! `Num` wraps an `f64`, so it reaches about 1.8e308 with 15 to 17
//! significant digits. It is a newtype rather than a type alias so the
//! backing type can be swapped later without changing game code.
//!
//! # NaN and infinity policy
//!
//! A `Num` is always finite: never NaN, never infinite, and never `-0.0`.
//! Every conversion and every operation sends its raw `f64` result through
//! [`Num::from_f64`], which enforces that with three rules:
//!
//! - **Overflow saturates.** A result that would be `+inf` becomes
//!   [`Num::MAX`], and one that would be `-inf` becomes [`Num::MIN`]. This
//!   covers dividing a non-zero value by zero and `log10` of zero.
//! - **Undefined results are zero.** A result that would be NaN becomes
//!   [`Num::ZERO`]. This covers `0 / 0`, `sqrt` and `log10` of a negative
//!   number, and a negative base raised to a fractional power.
//! - **Zero is unsigned.** `-0.0` becomes `0.0`.
//!
//! Any other result is kept exactly, subnormals included, so finite math
//! rounds the same as plain `f64`.
//!
//! Saturation keeps `x - x` at zero at every magnitude: an overflowed value
//! is `MAX`, so it cancels like any other finite value instead of becoming
//! `inf - inf = NaN`. The cost is that saturating steps do not associate:
//! `(MAX + MAX) - MAX` is zero, while `MAX + (MAX - MAX)` is `MAX`.
//!
//! # Equality, ordering and hashing
//!
//! Ruling out NaN makes `==` reflexive, and ruling out `-0.0` makes two
//! values equal exactly when their bits are equal. So `Num` implements
//! [`Eq`], [`Ord`] (the numeric order, which is total) and [`Hash`] (on the
//! bits), and works as a map key or in a plain `sort()`.
//!
//! ```
//! use exitium_core::Num;
//!
//! assert_eq!(Num::from(5), Num::from(5.0)); // integer literals convert exactly
//!
//! let big = Num::from(1e300);
//! assert_eq!(big * big, Num::MAX); // saturates instead of reaching inf
//! assert_eq!(big * big - big * big, Num::ZERO); // MAX - MAX, not inf - inf
//! assert_eq!(Num::ZERO / Num::ZERO, Num::ZERO); // undefined becomes zero
//! assert_eq!(Num::ONE / Num::ZERO, Num::MAX);
//! assert!(Num::from(-1.0) < Num::ZERO);
//! ```
//!
//! # Text
//!
//! [`Num::format`] writes a value as a [`NumFormat`] says: in plain,
//! scientific, engineering, suffix (`1.23K`) or letter (`1.23a`) notation,
//! with a chosen number of digits after the point. `Display` writes
//! [`NumFormat::DEFAULT`], such as `1.23e3`. [`Num::parse`] reads plain and
//! scientific text back.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::iter::{Product, Sum};
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

mod format;

pub use format::{Notation, NumFormat, ParseNumError};

/// The engine's number type: an `f64` that is always finite.
///
/// Build one with [`Num::from_f64`], or `Num::from` on an `f64`, `i32`,
/// `u32`, `i64` or `u64`, or read one from text with [`Num::parse`].
/// Arithmetic saturates instead of overflowing and never produces NaN; the
/// [module docs](crate::num) give the exact policy.
// Invariant: the f64 is finite and not -0.0. Only `from_f64`, the constants
// and the derived `Default` (0.0) build a `Num`, and the `Eq`, `Ord` and
// `Hash` impls rely on it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Num(f64);

impl Num {
    /// Zero. Also the [`Default`] value and the result of undefined operations.
    pub const ZERO: Self = Self(0.0);

    /// One.
    pub const ONE: Self = Self(1.0);

    /// The largest value, about 1.8e308. Positive overflow saturates here.
    pub const MAX: Self = Self(f64::MAX);

    /// The most negative value, `-MAX`. Negative overflow saturates here.
    pub const MIN: Self = Self(f64::MIN);

    /// Converts an `f64` under the [NaN and infinity policy](crate::num):
    /// NaN becomes [`ZERO`](Self::ZERO), `inf` and `-inf` become
    /// [`MAX`](Self::MAX) and [`MIN`](Self::MIN), and `-0.0` becomes `0.0`.
    /// Every other value is kept exactly.
    #[must_use]
    #[inline]
    pub const fn from_f64(value: f64) -> Self {
        if value.is_nan() || value == 0.0 {
            // `-0.0 == 0.0`, so this also folds -0.0 into 0.0.
            Self(0.0)
        } else {
            Self(value.clamp(f64::MIN, f64::MAX))
        }
    }

    /// Returns the inner `f64`. It is always finite and never `-0.0`.
    #[must_use]
    #[inline]
    pub const fn as_f64(self) -> f64 {
        self.0
    }

    /// Raises `self` to the power `exponent`.
    ///
    /// Overflow saturates, and a negative base with a fractional exponent
    /// returns [`ZERO`](Self::ZERO).
    #[must_use]
    #[inline]
    pub fn pow(self, exponent: Self) -> Self {
        Self::from_f64(self.0.powf(exponent.0))
    }

    /// Square root. A negative value returns [`ZERO`](Self::ZERO).
    #[must_use]
    #[inline]
    pub fn sqrt(self) -> Self {
        Self::from_f64(self.0.sqrt())
    }

    /// Base-10 logarithm. Zero returns [`MIN`](Self::MIN), and a negative
    /// value returns [`ZERO`](Self::ZERO).
    #[must_use]
    #[inline]
    pub fn log10(self) -> Self {
        Self::from_f64(self.0.log10())
    }

    /// The largest integer less than or equal to `self`.
    #[must_use]
    #[inline]
    pub const fn floor(self) -> Self {
        Self::from_f64(self.0.floor())
    }

    /// The smaller of `self` and `other`. Same result as [`Ord::min`].
    #[must_use]
    #[inline]
    pub const fn min(self, other: Self) -> Self {
        Self::from_f64(self.0.min(other.0))
    }

    /// The larger of `self` and `other`. Same result as [`Ord::max`].
    #[must_use]
    #[inline]
    pub const fn max(self, other: Self) -> Self {
        Self::from_f64(self.0.max(other.0))
    }

    /// Absolute value. [`MIN`](Self::MIN) becomes [`MAX`](Self::MAX).
    #[must_use]
    #[inline]
    pub const fn abs(self) -> Self {
        Self::from_f64(self.0.abs())
    }
}

impl From<f64> for Num {
    /// Same as [`Num::from_f64`]: NaN becomes zero and infinities saturate.
    #[inline]
    fn from(value: f64) -> Self {
        Self::from_f64(value)
    }
}

impl From<i32> for Num {
    /// Always exact. An integer literal with no suffix falls back to `i32`,
    /// so this is the conversion `Num::from(5)` uses.
    #[inline]
    fn from(value: i32) -> Self {
        Self::from_f64(f64::from(value))
    }
}

impl From<u32> for Num {
    /// Always exact.
    #[inline]
    fn from(value: u32) -> Self {
        Self::from_f64(f64::from(value))
    }
}

impl From<i64> for Num {
    /// Exact up to 2^53 in magnitude; larger values round to the nearest `f64`.
    #[inline]
    fn from(value: i64) -> Self {
        Self::from_f64(value as f64)
    }
}

impl From<u64> for Num {
    /// Exact up to 2^53; larger values round to the nearest `f64`.
    #[inline]
    fn from(value: u64) -> Self {
        Self::from_f64(value as f64)
    }
}

impl Add for Num {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self::from_f64(self.0 + rhs.0)
    }
}

impl Sub for Num {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self::from_f64(self.0 - rhs.0)
    }
}

impl Mul for Num {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self::from_f64(self.0 * rhs.0)
    }
}

impl Div for Num {
    type Output = Self;

    /// `0 / 0` is [`ZERO`](Num::ZERO); any other value divided by zero
    /// saturates to [`MAX`](Num::MAX) or [`MIN`](Num::MIN) by its own sign.
    #[inline]
    fn div(self, rhs: Self) -> Self {
        Self::from_f64(self.0 / rhs.0)
    }
}

impl Neg for Num {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self::from_f64(-self.0)
    }
}

impl AddAssign for Num {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SubAssign for Num {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl MulAssign for Num {
    #[inline]
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

impl DivAssign for Num {
    #[inline]
    fn div_assign(&mut self, rhs: Self) {
        *self = *self / rhs;
    }
}

impl Sum for Num {
    /// Adds left to right, saturating at each step. The empty sum is zero.
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |total, value| total + value)
    }
}

impl<'a> Sum<&'a Num> for Num {
    fn sum<I: Iterator<Item = &'a Num>>(iter: I) -> Self {
        iter.copied().sum()
    }
}

impl Product for Num {
    /// Multiplies left to right, saturating at each step. The empty product
    /// is one.
    fn product<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ONE, |total, value| total * value)
    }
}

impl<'a> Product<&'a Num> for Num {
    fn product<I: Iterator<Item = &'a Num>>(iter: I) -> Self {
        iter.copied().product()
    }
}

// Sound because a `Num` is never NaN, so `==` is reflexive.
impl Eq for Num {}

impl PartialOrd for Num {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Num {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        // `total_cmp` differs from `==` and `<` only for NaN and for -0.0
        // against 0.0, and a `Num` is never either.
        self.0.total_cmp(&other.0)
    }
}

impl Hash for Num {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Equal `Num`s have equal bits (there is no -0.0), so this agrees
        // with `==`.
        self.0.to_bits().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::hash::DefaultHasher;

    /// The smallest positive subnormal `f64`.
    const TINY: f64 = 5e-324;

    /// Distinct finite values in ascending order: both ends of the range,
    /// values around one, the smallest normal and subnormal of each sign,
    /// and zero.
    const EDGES: [f64; 15] = [
        f64::MIN,
        -1e308,
        -1.5,
        -1.0,
        -0.5,
        -f64::MIN_POSITIVE,
        -TINY,
        0.0,
        TINY,
        f64::MIN_POSITIVE,
        0.5,
        1.0,
        1.5,
        1e308,
        f64::MAX,
    ];

    fn n(value: f64) -> Num {
        Num::from_f64(value)
    }

    fn edges() -> Vec<Num> {
        EDGES.into_iter().map(n).collect()
    }

    /// Panics unless `value` holds the `Num` invariant: finite and not -0.0.
    fn assert_canonical(value: Num) {
        let raw = value.as_f64();
        assert!(raw.is_finite(), "{raw:e} is not finite");
        assert!(raw != 0.0 || raw.is_sign_positive(), "negative zero");
    }

    /// For `pow` and `log10`, whose last bit may differ between platform
    /// math libraries.
    fn assert_close(actual: Num, expected: f64) {
        let actual = actual.as_f64();
        let tolerance = expected.abs() * 1e-12;
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual:e} is not close to {expected:e}"
        );
    }

    fn hash_of(value: Num) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn constants() {
        assert_eq!(Num::ZERO.as_f64().to_bits(), 0, "ZERO must be +0.0");
        assert_eq!(Num::ONE.as_f64(), 1.0);
        assert_eq!(Num::MAX.as_f64(), f64::MAX);
        assert_eq!(Num::MIN.as_f64(), f64::MIN);
        assert_eq!(Num::default(), Num::ZERO);
    }

    #[test]
    fn from_f64_keeps_finite_values_exactly() {
        for value in EDGES {
            assert_eq!(n(value).as_f64().to_bits(), value.to_bits(), "{value:e}");
        }
    }

    #[test]
    fn from_f64_maps_nan_infinities_and_negative_zero() {
        assert_eq!(n(f64::NAN), Num::ZERO);
        assert_eq!(n(-f64::NAN), Num::ZERO);
        assert_eq!(n(f64::INFINITY), Num::MAX);
        assert_eq!(n(f64::NEG_INFINITY), Num::MIN);
        assert_eq!(n(-0.0).as_f64().to_bits(), 0);
    }

    #[test]
    fn from_impls() {
        assert_eq!(Num::from(2.5), n(2.5));
        assert_eq!(Num::from(f64::NAN), Num::ZERO);
        assert_eq!(Num::from(f64::NEG_INFINITY), Num::MIN);
        assert_eq!(Num::from(-7_i32), n(-7.0));
        assert_eq!(Num::from(0_i32).as_f64().to_bits(), 0);
        assert_eq!(Num::from(i32::MIN), n(-(2f64.powi(31))));
        assert_eq!(Num::from(i32::MAX), n(2f64.powi(31) - 1.0));
        assert_eq!(Num::from(0_u32).as_f64().to_bits(), 0);
        assert_eq!(Num::from(u32::MAX), n(2f64.powi(32) - 1.0));
        assert_eq!(Num::from(-7_i64), n(-7.0));
        assert_eq!(Num::from(0_i64).as_f64().to_bits(), 0);
        assert_eq!(Num::from(i64::MIN), n(-(2f64.powi(63))));
        assert_eq!(Num::from(i64::MAX), n(2f64.powi(63)));
        assert_eq!(Num::from(0_u64).as_f64().to_bits(), 0);
        assert_eq!(Num::from(u64::MAX), n(2f64.powi(64)));
        // Above 2^53, integers round to the nearest f64.
        assert_eq!(Num::from((1_u64 << 53) + 1), Num::from(1_u64 << 53));
    }

    #[test]
    fn unsuffixed_integer_literals_convert() {
        // With no suffix, an integer literal falls back to `i32`.
        assert_eq!(Num::from(5), Num::from(5.0));
        assert_eq!(Num::from(-5), Num::from(-5.0));
    }

    #[test]
    fn arithmetic() {
        assert_eq!(n(2.0) + n(3.0), n(5.0));
        assert_eq!(n(2.0) - n(3.0), n(-1.0));
        assert_eq!(n(2.0) * n(-3.0), n(-6.0));
        assert_eq!(n(3.0) / n(-2.0), n(-1.5));
        assert_eq!(-n(2.0), n(-2.0));
        assert_eq!(-Num::MAX, Num::MIN);
        assert_eq!(-Num::MIN, Num::MAX);
    }

    #[test]
    fn assign_ops_match_binary_ops() {
        for a in edges() {
            for b in edges() {
                let mut x = a;
                x += b;
                assert_eq!(x, a + b);
                let mut x = a;
                x -= b;
                assert_eq!(x, a - b);
                let mut x = a;
                x *= b;
                assert_eq!(x, a * b);
                let mut x = a;
                x /= b;
                assert_eq!(x, a / b);
            }
        }
    }

    #[test]
    fn overflow_saturates() {
        assert_eq!(Num::MAX + Num::MAX, Num::MAX);
        assert_eq!(Num::MIN + Num::MIN, Num::MIN);
        assert_eq!(Num::MIN - Num::MAX, Num::MIN);
        assert_eq!(Num::MAX * n(2.0), Num::MAX);
        assert_eq!(Num::MAX * Num::MIN, Num::MIN);
        assert_eq!(n(1e308) * n(10.0), Num::MAX);
        assert_eq!(n(1e308) / n(0.1), Num::MAX);
        assert_eq!(Num::ONE / n(TINY), Num::MAX);
        assert_eq!(n(10.0).pow(n(400.0)), Num::MAX);
        assert_eq!(n(-10.0).pow(n(401.0)), Num::MIN);
        // Below the overflow threshold, rounding is plain f64 rounding.
        assert_eq!(Num::MAX + Num::ONE, Num::MAX);
        assert_eq!(Num::MAX - Num::ONE, Num::MAX);
    }

    #[test]
    fn saturating_steps_do_not_associate() {
        assert_eq!(Num::MAX + Num::MAX - Num::MAX, Num::ZERO);
        assert_eq!(Num::MAX + (Num::MAX - Num::MAX), Num::MAX);
    }

    #[test]
    fn division_by_zero() {
        assert_eq!(Num::ZERO / Num::ZERO, Num::ZERO);
        assert_eq!(Num::ONE / Num::ZERO, Num::MAX);
        assert_eq!(-Num::ONE / Num::ZERO, Num::MIN);
        assert_eq!(n(TINY) / Num::ZERO, Num::MAX);
        assert_eq!(Num::MIN / Num::ZERO, Num::MIN);
        // There is no -0.0 to divide by, so only the dividend's sign counts.
        assert_eq!(Num::ONE / -Num::ZERO, Num::MAX);
        let mut x = Num::ZERO;
        x /= Num::ZERO;
        assert_eq!(x, Num::ZERO);
    }

    #[test]
    fn x_minus_x_is_zero_at_any_magnitude() {
        let large = [
            n(1e308),
            n(-1e308),
            Num::MAX,
            Num::MIN,
            Num::MAX + Num::MAX,
            Num::MIN * n(10.0),
            n(1e300).pow(n(3.0)),
        ];
        for x in large.into_iter().chain(edges()) {
            assert_eq!((x - x).as_f64().to_bits(), 0, "{x:?} - itself");
            assert_eq!((x + -x).as_f64().to_bits(), 0, "{x:?} + its negation");
        }
    }

    #[test]
    fn results_are_never_negative_zero() {
        let zeros = [
            -Num::ZERO,
            Num::ZERO * n(-1.0),
            Num::ZERO / n(-5.0),
            n(-TINY) * n(TINY),
            n(-TINY) / n(2.0),
            n(-1e-300) * n(1e-300),
            n(-TINY).pow(n(3.0)),
            n(-1e-300).pow(n(3.0)),
            (-Num::ZERO).abs(),
            Num::ZERO.floor(),
            n(-0.0).sqrt(),
            [n(-1.0), Num::ONE].into_iter().sum(),
            [Num::ZERO, n(-1.0)].into_iter().product(),
        ];
        for zero in zeros {
            assert_eq!(zero.as_f64().to_bits(), 0, "{zero:?}");
        }
    }

    #[test]
    fn subnormals_are_ordinary_values() {
        let tiny = n(TINY);
        assert!(Num::ZERO < tiny && tiny < n(f64::MIN_POSITIVE));
        assert!(-tiny < Num::ZERO);
        assert_eq!((tiny + tiny).as_f64(), 2.0 * TINY);
        assert_eq!(tiny - tiny, Num::ZERO);
        assert_eq!(tiny / n(2.0), Num::ZERO); // underflows
        assert_eq!(tiny.floor(), Num::ZERO);
        assert_eq!((-tiny).floor(), n(-1.0));
        assert_eq!((-tiny).abs(), tiny);
        assert_close(tiny.log10(), -323.306_215_343_115_8);
    }

    #[test]
    fn undefined_results_are_zero() {
        assert_eq!(n(-4.0).sqrt(), Num::ZERO);
        assert_eq!(Num::MIN.sqrt(), Num::ZERO);
        assert_eq!(n(-1.0).log10(), Num::ZERO);
        assert_eq!(n(-TINY).log10(), Num::ZERO);
        assert_eq!(n(-8.0).pow(n(1.0 / 3.0)), Num::ZERO);
    }

    #[test]
    fn math_functions() {
        assert_close(n(2.0).pow(n(10.0)), 1024.0);
        assert_close(n(4.0).pow(n(-0.5)), 0.5);
        assert_close(n(-2.0).pow(n(3.0)), -8.0);
        assert_eq!(Num::ZERO.pow(Num::ZERO), Num::ONE);
        assert_eq!(Num::ZERO.pow(-Num::ONE), Num::MAX);
        assert_eq!(n(16.0).sqrt(), n(4.0));
        assert_eq!(Num::ZERO.sqrt(), Num::ZERO);
        assert_close(Num::MAX.sqrt(), 1.340_780_792_994_259_6e154);
        assert_close(n(1000.0).log10(), 3.0);
        assert_close(n(1e308).log10(), 308.0);
        assert_close(Num::MAX.log10(), 308.254_715_559_916_7);
        assert_eq!(Num::ZERO.log10(), Num::MIN);
        assert_eq!(n(2.7).floor(), n(2.0));
        assert_eq!(n(-2.7).floor(), n(-3.0));
        assert_eq!(n(-0.5).floor(), n(-1.0));
        assert_eq!(Num::MAX.floor(), Num::MAX);
        assert_eq!(Num::MIN.floor(), Num::MIN);
        assert_eq!(n(-3.0).abs(), n(3.0));
        assert_eq!(Num::MIN.abs(), Num::MAX);
    }

    #[test]
    fn min_and_max_agree_with_ord() {
        for a in edges() {
            for b in edges() {
                assert_eq!(a.min(b), Ord::min(a, b));
                assert_eq!(a.max(b), Ord::max(a, b));
            }
        }
        assert_eq!(Num::MIN.max(Num::MAX), Num::MAX);
        assert_eq!(n(2.0).min(n(-3.0)), n(-3.0));
    }

    #[test]
    fn sum_and_product() {
        let values = [n(1.0), n(2.0), n(3.0), n(4.0)];
        assert_eq!(values.into_iter().sum::<Num>(), n(10.0));
        assert_eq!(values.iter().sum::<Num>(), n(10.0));
        assert_eq!(values.into_iter().product::<Num>(), n(24.0));
        assert_eq!(values.iter().product::<Num>(), n(24.0));
        assert_eq!(std::iter::empty::<Num>().sum::<Num>(), Num::ZERO);
        assert_eq!(std::iter::empty::<Num>().product::<Num>(), Num::ONE);
        assert_eq!([Num::MAX; 3].iter().sum::<Num>(), Num::MAX);
        assert_eq!([Num::MIN; 3].iter().sum::<Num>(), Num::MIN);
        assert_eq!([n(1e300); 3].iter().product::<Num>(), Num::MAX);
        assert_eq!([n(-1e300); 3].iter().product::<Num>(), Num::MIN);
        // Saturation happens per step, as with `+`.
        assert_eq!(
            [Num::MAX, Num::MAX, Num::MIN].iter().sum::<Num>(),
            Num::ZERO
        );
    }

    #[test]
    fn every_operation_keeps_the_invariant() {
        for a in edges() {
            for result in [-a, a.sqrt(), a.log10(), a.floor(), a.abs()] {
                assert_canonical(result);
            }
            for b in edges() {
                for result in [a + b, a - b, a * b, a / b, a.pow(b), a.min(b), a.max(b)] {
                    assert_canonical(result);
                }
            }
        }
    }

    #[test]
    fn ordering_is_total_and_numeric() {
        let values = edges();
        for (i, a) in values.iter().enumerate() {
            for (j, b) in values.iter().enumerate() {
                // EDGES ascends strictly, so index order is the expected order.
                assert_eq!(a.cmp(b), i.cmp(&j), "{a:?} vs {b:?}");
                assert_eq!(a.partial_cmp(b), Some(i.cmp(&j)));
                assert_eq!(a.as_f64().partial_cmp(&b.as_f64()), Some(i.cmp(&j)));
                assert_eq!(a == b, i == j);
                assert_eq!(a < b, i < j);
            }
        }
        let mut shuffled = values.clone();
        shuffled.reverse();
        shuffled.rotate_left(5);
        shuffled.sort();
        assert_eq!(shuffled, values);
    }

    #[test]
    fn equal_values_hash_equally() {
        // Every route to zero must give one value, for both `==` and `Hash`.
        let zeros = [
            Num::ZERO,
            -Num::ZERO,
            n(-0.0),
            n(f64::NAN),
            Num::ZERO * n(-1.0),
            Num::from(0_i64),
            Num::from(0_u64),
        ];
        for zero in zeros {
            assert_eq!(zero, Num::ZERO);
            assert_eq!(zero.cmp(&Num::ZERO), Ordering::Equal);
            assert_eq!(hash_of(zero), hash_of(Num::ZERO));
        }
        assert_eq!(zeros.into_iter().collect::<HashSet<_>>().len(), 1);

        // Saturated results are the same value as the bound.
        assert_eq!(hash_of(Num::MAX * n(2.0)), hash_of(Num::MAX));
        assert_eq!(hash_of(n(f64::NEG_INFINITY)), hash_of(Num::MIN));

        let values = edges();
        assert_eq!(values.iter().collect::<HashSet<_>>().len(), values.len());
    }
}
