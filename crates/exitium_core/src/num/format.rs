//! Text for [`Num`]: the notations incremental games display, and parsing.
//!
//! [`Num::format`] and [`Num::format_into`] write a value as a [`NumFormat`]
//! says, and `Display` writes [`NumFormat::DEFAULT`]. [`Num::parse`] reads
//! plain and scientific text back, and `{:e}` writes the text that reads back
//! to exactly the same value.
//!
//! # Rounding
//!
//! Every digit comes from Rust's own float formatting (`{:.N}` and `{:.Ne}`),
//! which rounds the exact binary value at the last digit shown, ties to even.
//! Each value is rounded once: `1234.9999` with two digits after the point is
//! `1.23e3`, where rounding first to `1.2350e3` and then again would give
//! `1.24e3`.
//!
//! The engineering, suffix and letter notations show one to three digits
//! before the point, so the number of significant digits to keep depends on
//! the exponent, and rounding can change the exponent: `99.996e3` rounds to
//! `100.00e3`, and `999.996e3` to `1.00e6`. `group_digits` finds the digits in
//! up to three steps:
//!
//! 1. Guess the exponent with `log10`, which can be one off next to a power
//!    of ten, and format with the digits that exponent needs. Rounding never
//!    lowers the exponent, so when the result has the guessed exponent, the
//!    guess was right, or one too high with rounding carried up to a power of
//!    ten. A result with the guessed exponent that is not a power of ten is
//!    kept. This is the common case.
//! 2. Otherwise, format with the most digits any exponent needs, three before
//!    the point. Unless that rounding carried up to a power of ten, its
//!    exponent is exact. If the exponent needs three digits, that result is
//!    kept.
//! 3. Otherwise, format again with the digits the exponent from step 2
//!    needs, so the digits shown are rounded once. After a carry in step 2,
//!    rounding with fewer digits reaches the same power of ten.
//!
//! Nothing here allocates: digits pass through small fixed buffers on the
//! stack, and `core::fmt` formats floats on the stack as well.

use std::fmt::{self, Write as _};
use std::ops::Range;
use std::str::FromStr;

use super::Num;

/// How [`Num::format`] writes a value of 1000 or more.
///
/// Smaller values are written in plain form in every notation, such as `999`
/// or `0.25`; [`NumFormat`] has the exact rule. The examples here show two
/// digits after the decimal point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Notation {
    /// Every digit, with no exponent: `1234567.00`. [`Num::MAX`] has 309
    /// digits before the point.
    Plain,
    /// One digit before the point and a power of ten: `1.23e45`.
    Scientific,
    /// One to three digits before the point and a power of ten that is a
    /// multiple of three: `1.23e3`, `12.35e3`, `123.46e3`, `1.00e6`.
    Engineering,
    /// One to three digits and a suffix naming the power of a thousand, on
    /// the short scale: `1.23K`, `45.60M`, `789.00B`, `1.00Qa`. [`Num::MAX`]
    /// is `179.77UCe`.
    ///
    /// | From | Suffixes, each a thousand times the one before |
    /// |---|---|
    /// | 1e3 | K, M, B, T, Qa, Qi, Sx, Sp, Oc, No |
    /// | 1e33 | Dc, UDc, DDc, TDc, QaDc, QiDc, SxDc, SpDc, OcDc, NoDc |
    /// | 1e63 | Vg, UVg, DVg, TVg, QaVg, QiVg, SxVg, SpVg, OcVg, NoVg |
    /// | 1e93 | Tg, UTg, DTg, TTg, QaTg, QiTg, SxTg, SpTg, OcTg, NoTg |
    /// | 1e123 | Qag, UQag, DQag, TQag, QaQag, QiQag, SxQag, SpQag, OcQag, NoQag |
    /// | 1e153 | Qig, UQig, DQig, TQig, QaQig, QiQig, SxQig, SpQig, OcQig, NoQig |
    /// | 1e183 | Sxg, USxg, DSxg, TSxg, QaSxg, QiSxg, SxSxg, SpSxg, OcSxg, NoSxg |
    /// | 1e213 | Spg, USpg, DSpg, TSpg, QaSpg, QiSpg, SxSpg, SpSpg, OcSpg, NoSpg |
    /// | 1e243 | Ocg, UOcg, DOcg, TOcg, QaOcg, QiOcg, SxOcg, SpOcg, OcOcg, NoOcg |
    /// | 1e273 | Nog, UNog, DNog, TNog, QaNog, QiNog, SxNog, SpNog, OcNog, NoNog |
    /// | 1e303 | Ce, UCe |
    ///
    /// From `Dc` on, a suffix is a units prefix (`U` 1, `D` 2, `T` 3, `Qa` 4,
    /// `Qi` 5, `Sx` 6, `Sp` 7, `Oc` 8, `No` 9) before a tens name (`Dc`
    /// decillion, `Vg` vigintillion, `Tg` trigintillion, `Qag`, `Qig`, `Sxg`,
    /// `Spg`, `Ocg`, `Nog` nonagintillion, `Ce` centillion). So `QaDc` is a
    /// quattuordecillion, 1e45.
    Suffix,
    /// One to three digits and letters naming the power of a thousand: `a` is
    /// 1e3, `b` 1e6 and so on to `z` (1e78), then `aa` (1e81), `ab`, ..., `az`,
    /// `ba`, ...: `1.23a`, `4.56z`, `7.89aa`. [`Num::MAX`] is `179.77cx`.
    Letter,
}

/// How to write a [`Num`] as text: a [`Notation`] and how many digits to show
/// after the decimal point.
///
/// A value is written in plain form when its magnitude, rounded to
/// `small_precision` digits after the point, is below 1000: `0.50`,
/// `-12.25`, `999.99`. Anything larger is written in `notation` with
/// `precision` digits after the point. When rounding carries a value up to
/// the next power of a thousand, it moves to the next exponent, suffix or
/// letter: in suffix notation `999.996` is `1.00K`, not `1000.00`, and
/// `999_996` is `1.00M`. Whether a value carries is decided at
/// `small_precision`, so a larger `precision` shows more digits than the
/// value was rounded to: with `small_precision: 2` and `precision: 5`,
/// `999.996` is `1.00000K`.
///
/// Rounding is to the nearest digit shown, ties to even, as in Rust's own
/// float formatting. A value that rounds to zero is written without a sign,
/// so there is no `-0.00`. Precisions above
/// [`MAX_PRECISION`](Self::MAX_PRECISION) count as that maximum.
///
/// ```
/// use exitium_core::{Notation, Num, NumFormat};
///
/// let gold = NumFormat { notation: Notation::Suffix, precision: 2, small_precision: 0 };
/// assert_eq!(Num::from(999).format(&gold), "999");
/// assert_eq!(Num::from(999.7).format(&gold), "1.00K"); // rounds up into the next suffix
/// assert_eq!(Num::from(1234.5).format(&gold), "1.23K");
/// assert_eq!(Num::from(-2.5e9).format(&gold), "-2.50B");
/// assert_eq!(Num::MAX.format(&gold), "179.77UCe");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NumFormat {
    /// How values of 1000 and above are written.
    pub notation: Notation,
    /// Digits after the decimal point for values of 1000 and above.
    pub precision: u8,
    /// Digits after the decimal point for values below 1000.
    pub small_precision: u8,
}

impl NumFormat {
    /// The most digits shown after the point; a larger precision counts as
    /// this. Sixteen digits after the point in scientific notation are 17
    /// significant digits, enough to tell any two values apart.
    pub const MAX_PRECISION: u8 = 16;

    /// The format `Display` uses: scientific notation with two digits after
    /// the point, so `1234.5` is `1.23e3` and `0.5` is `0.50`.
    pub const DEFAULT: Self = Self::new(Notation::Scientific);

    /// `notation` with two digits after the point for every value.
    #[must_use]
    pub const fn new(notation: Notation) -> Self {
        Self {
            notation,
            precision: 2,
            small_precision: 2,
        }
    }
}

impl Default for NumFormat {
    /// [`NumFormat::DEFAULT`].
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The error [`Num::parse`] returns for text that is not a decimal number in
/// plain or scientific form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseNumError;

impl fmt::Display for ParseNumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid number: expected a decimal like -1.5 or 2.5e30")
    }
}

impl std::error::Error for ParseNumError {}

impl Num {
    /// Writes `self` as text in `format`, into a new `String`.
    ///
    /// [`format_into`](Self::format_into) writes into an existing buffer
    /// instead. [`NumFormat`] describes the rules.
    ///
    /// ```
    /// use exitium_core::{Notation, Num, NumFormat};
    ///
    /// let x = Num::from(123_456.789);
    /// let shown = |notation| x.format(&NumFormat::new(notation));
    /// assert_eq!(shown(Notation::Plain), "123456.79");
    /// assert_eq!(shown(Notation::Scientific), "1.23e5");
    /// assert_eq!(shown(Notation::Engineering), "123.46e3");
    /// assert_eq!(shown(Notation::Suffix), "123.46K");
    /// assert_eq!(shown(Notation::Letter), "123.46a");
    /// ```
    #[must_use]
    pub fn format(self, format: &NumFormat) -> String {
        let mut text = String::new();
        self.format_into(format, &mut text)
            .expect("formatting into a String cannot fail");
        text
    }

    /// Writes `self` as text in `format` to the end of `out`.
    ///
    /// This allocates nothing itself, so it suits counters that refresh every
    /// frame: keep one `String` per label, clear it and write into it, and it
    /// stops growing after the first frames.
    ///
    /// ```
    /// use exitium_core::{Notation, Num, NumFormat};
    ///
    /// let format = NumFormat::new(Notation::Suffix);
    /// let mut label = String::new();
    /// for gold in [Num::from(950), Num::from(1_250_000)] {
    ///     label.clear();
    ///     gold.format_into(&format, &mut label)?;
    /// }
    /// assert_eq!(label, "1.25M");
    /// # Ok::<(), std::fmt::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error only if `out` does.
    pub fn format_into(self, format: &NumFormat, out: &mut impl fmt::Write) -> fmt::Result {
        let precision = usize::from(format.precision.min(NumFormat::MAX_PRECISION));
        let small_precision = usize::from(format.small_precision.min(NumFormat::MAX_PRECISION));
        let negative = self.as_f64() < 0.0;
        let mut magnitude = self.as_f64().abs();

        if magnitude < PLAIN_BELOW {
            let mut plain = StackStr::new();
            write!(plain, "{magnitude:.small_precision$}")?;
            let plain = plain.as_str()?;
            let before_point = plain.find('.').unwrap_or(plain.len());
            if before_point <= 3 {
                if negative && plain.bytes().any(|b| matches!(b, b'1'..=b'9')) {
                    out.write_char('-')?;
                }
                return out.write_str(plain);
            }
            // Rounding carried up to exactly 1000, which `notation` writes.
            magnitude = PLAIN_BELOW;
        }

        if negative {
            out.write_char('-')?;
        }
        match format.notation {
            Notation::Plain => write!(out, "{magnitude:.precision$}"),
            Notation::Scientific => write!(out, "{magnitude:.precision$e}"),
            Notation::Engineering => {
                let thousands = write_grouped(magnitude, precision, out)?;
                write!(out, "e{}", 3 * thousands)
            }
            Notation::Suffix => {
                let thousands = write_grouped(magnitude, precision, out)?;
                // `Num::MAX` is below 1e309, so `thousands` is at most 102,
                // the length of the table.
                out.write_str(SUFFIXES[thousands - 1])
            }
            Notation::Letter => {
                let thousands = write_grouped(magnitude, precision, out)?;
                write_letters(thousands, out)
            }
        }
    }

    /// Parses a decimal number in plain or scientific form: an optional sign,
    /// digits with an optional decimal point, and an optional exponent, such
    /// as `42`, `-0.5`, `1.23e45` or `4E-7`.
    ///
    /// This reads back what [`Notation::Plain`], [`Notation::Scientific`] and
    /// [`Notation::Engineering`] write, to the digits they show. For an exact
    /// round trip, write the value with `{:e}`, which gives the shortest text
    /// that reads back to the same value. The result goes through
    /// [`Num::from_f64`], so text outside the range saturates: `1e999` is
    /// [`MAX`](Num::MAX), `-1e999` is [`MIN`](Num::MIN), and `1e-999` and
    /// `-0` are [`ZERO`](Num::ZERO).
    ///
    /// ```
    /// use exitium_core::Num;
    ///
    /// assert_eq!(Num::parse("1.5e3"), Ok(Num::from(1500)));
    /// assert_eq!("-42".parse::<Num>(), Ok(Num::from(-42)));
    /// assert_eq!(Num::parse("1e999"), Ok(Num::MAX));
    /// assert!(Num::parse("1.5K").is_err());
    ///
    /// let third = Num::ONE / Num::from(3);
    /// assert_eq!(Num::parse(&format!("{third:e}")), Ok(third));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`ParseNumError`] for anything else, including empty text,
    /// spaces, digit separators (`1_000`, `1,000`), suffixes and letters
    /// (`1.5K`), `inf` and `NaN`.
    pub fn parse(text: &str) -> Result<Self, ParseNumError> {
        // Rust's float parser also takes `inf`, `infinity` and `nan`. Allowing
        // only these characters rules those out and leaves the rest of the
        // syntax to it.
        let scientific = text
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-'));
        if !scientific {
            return Err(ParseNumError);
        }
        text.parse::<f64>()
            .map(Self::from_f64)
            .map_err(|_| ParseNumError)
    }
}

impl FromStr for Num {
    type Err = ParseNumError;

    /// Same as [`Num::parse`].
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl fmt::Display for Num {
    /// Writes [`NumFormat::DEFAULT`]. A precision in the format string
    /// replaces both of its precisions, and width, fill, alignment and the
    /// `+` and `0` flags work as they do for other numbers.
    ///
    /// ```
    /// use exitium_core::Num;
    ///
    /// assert_eq!(Num::from(1234.5).to_string(), "1.23e3");
    /// assert_eq!(format!("{:.1}", Num::from(0.25)), "0.2"); // a tie, to even
    /// assert_eq!(format!("{:>8}", Num::from(-5)), "   -5.00");
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut format = NumFormat::DEFAULT;
        if let Some(precision) = f.precision() {
            let precision = u8::try_from(precision).unwrap_or(u8::MAX);
            format.precision = precision;
            format.small_precision = precision;
        }
        // Scientific text is short enough for `StackStr`, and `pad_integral`
        // writes the sign, so format the magnitude.
        let mut text = StackStr::new();
        self.abs().format_into(&format, &mut text)?;
        let text = text.as_str()?;
        let shows_zero = !text.bytes().any(|b| matches!(b, b'1'..=b'9'));
        f.pad_integral(*self >= Self::ZERO || shows_zero, "", text)
    }
}

impl fmt::LowerExp for Num {
    /// Writes the inner `f64` in scientific form, such as `1.5e30`. With no
    /// precision this is the shortest text that [`Num::parse`] reads back to
    /// exactly the same value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerExp::fmt(&self.as_f64(), f)
    }
}

impl fmt::UpperExp for Num {
    /// Same as [`LowerExp`](fmt::LowerExp), with `E`: `1.5E30`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperExp::fmt(&self.as_f64(), f)
    }
}

/// Values whose magnitude rounds to less than this are written in plain form.
const PLAIN_BELOW: f64 = 1000.0;

/// [`Notation::Suffix`] names. `SUFFIXES[n]` names `1000^(n + 1)`: `K` for a
/// thousand, then the short-scale n-illion, so `M` (n = 1) is a million and
/// `Dc` (n = 10) a decillion. The docs of `Notation::Suffix` show this table.
#[rustfmt::skip]
const SUFFIXES: [&str; 102] = [
    "K", "M", "B", "T", "Qa", "Qi", "Sx", "Sp", "Oc", "No",
    "Dc", "UDc", "DDc", "TDc", "QaDc", "QiDc", "SxDc", "SpDc", "OcDc", "NoDc",
    "Vg", "UVg", "DVg", "TVg", "QaVg", "QiVg", "SxVg", "SpVg", "OcVg", "NoVg",
    "Tg", "UTg", "DTg", "TTg", "QaTg", "QiTg", "SxTg", "SpTg", "OcTg", "NoTg",
    "Qag", "UQag", "DQag", "TQag", "QaQag", "QiQag", "SxQag", "SpQag", "OcQag", "NoQag",
    "Qig", "UQig", "DQig", "TQig", "QaQig", "QiQig", "SxQig", "SpQig", "OcQig", "NoQig",
    "Sxg", "USxg", "DSxg", "TSxg", "QaSxg", "QiSxg", "SxSxg", "SpSxg", "OcSxg", "NoSxg",
    "Spg", "USpg", "DSpg", "TSpg", "QaSpg", "QiSpg", "SxSpg", "SpSpg", "OcSpg", "NoSpg",
    "Ocg", "UOcg", "DOcg", "TOcg", "QaOcg", "QiOcg", "SxOcg", "SpOcg", "OcOcg", "NoOcg",
    "Nog", "UNog", "DNog", "TNog", "QaNog", "QiNog", "SxNog", "SpNog", "OcNog", "NoNog",
    "Ce", "UCe",
];

/// Room for the digits `write_grouped` shows: three before the point and up
/// to [`NumFormat::MAX_PRECISION`] after it.
const MAX_DIGITS: usize = 3 + NumFormat::MAX_PRECISION as usize;

/// Writes `magnitude` (1000 or more) as one to three digits, then the point
/// and `precision` digits, and returns its power of a thousand for the
/// caller to name.
fn write_grouped(
    magnitude: f64,
    precision: usize,
    out: &mut impl fmt::Write,
) -> Result<usize, fmt::Error> {
    let digits = group_digits(magnitude, precision)?;
    let before_point = digits.exp % 3 + 1;
    out.write_str(digits.text(0..before_point)?)?;
    if precision > 0 {
        out.write_char('.')?;
        out.write_str(digits.text(before_point..before_point + precision)?)?;
    }
    Ok(digits.exp / 3)
}

/// The significant digits of `magnitude` (1000 or more) for one to three
/// digits before the point and `precision` after it, rounded once. The
/// module docs explain the three steps.
fn group_digits(magnitude: f64, precision: usize) -> Result<Digits, fmt::Error> {
    let guess = magnitude.log10() as usize;
    let guessed = Digits::scientific(magnitude, precision + guess % 3)?;
    if guessed.exp == guess && !guessed.is_power_of_ten() {
        return Ok(guessed);
    }
    // An exponent two above a multiple of three puts three digits before the
    // point, the most any exponent needs.
    let widest = Digits::scientific(magnitude, precision + 2)?;
    let extra_before_point = widest.exp % 3;
    if extra_before_point == 2 {
        return Ok(widest);
    }
    Digits::scientific(magnitude, precision + extra_before_point)
}

/// Writes [`Notation::Letter`]'s name for `thousands` (1 or more) in
/// bijective base 26: 1 is `a`, 26 is `z`, 27 is `aa` and 53 is `ba`.
fn write_letters(thousands: usize, out: &mut impl fmt::Write) -> fmt::Result {
    // Least significant first. Sixteen letters are enough for any `usize`.
    let mut letters = [b'a'; 16];
    let mut len = 0;
    let mut rest = thousands;
    while rest > 0 {
        rest -= 1;
        letters[len] = b'a' + (rest % 26) as u8;
        rest /= 26;
        len += 1;
    }
    letters[..len]
        .iter()
        .rev()
        .try_for_each(|&letter| out.write_char(char::from(letter)))
}

/// Significant decimal digits of a magnitude of 1 or more, as ASCII: the
/// digits `d1 d2 d3 ...` stand for `d1.d2d3... × 10^exp`. Positions past the
/// last digit hold zeros, so a slice past it reads as zero padding.
struct Digits {
    ascii: [u8; MAX_DIGITS],
    exp: usize,
}

impl Digits {
    /// `magnitude` rounded to `decimals + 1` significant digits, read back
    /// from Rust's `{:.Ne}` text such as `1.2346e3`.
    fn scientific(magnitude: f64, decimals: usize) -> Result<Self, fmt::Error> {
        let mut text = StackStr::new();
        write!(text, "{magnitude:.decimals$e}")?;
        let (mantissa, exp) = text.as_str()?.split_once('e').ok_or(fmt::Error)?;
        let mut ascii = [b'0'; MAX_DIGITS];
        for (i, digit) in mantissa.bytes().filter(|&b| b != b'.').enumerate() {
            *ascii.get_mut(i).ok_or(fmt::Error)? = digit;
        }
        let exp = exp.parse().map_err(|_| fmt::Error)?;
        Ok(Self { ascii, exp })
    }

    /// Whether the digits are a one and then zeros, so the value is exactly
    /// `10^exp`.
    fn is_power_of_ten(&self) -> bool {
        self.ascii[0] == b'1' && self.ascii[1..].iter().all(|&b| b == b'0')
    }

    /// The digits in `range` as text.
    fn text(&self, range: Range<usize>) -> Result<&str, fmt::Error> {
        let ascii = self.ascii.get(range).ok_or(fmt::Error)?;
        std::str::from_utf8(ascii).map_err(|_| fmt::Error)
    }
}

/// A short string in a fixed array on the stack, so formatting needs no
/// allocation. Writing past its capacity fails with [`fmt::Error`].
struct StackStr {
    bytes: [u8; 32],
    len: usize,
}

impl StackStr {
    const fn new() -> Self {
        Self {
            bytes: [0; 32],
            len: 0,
        }
    }

    fn as_str(&self) -> Result<&str, fmt::Error> {
        std::str::from_utf8(&self.bytes[..self.len]).map_err(|_| fmt::Error)
    }
}

impl fmt::Write for StackStr {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        let slot = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        slot.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;
    use std::collections::HashSet;

    /// The smallest positive subnormal `f64`.
    const TINY: f64 = 5e-324;

    const NOTATIONS: [Notation; 5] = [
        Notation::Plain,
        Notation::Scientific,
        Notation::Engineering,
        Notation::Suffix,
        Notation::Letter,
    ];

    fn format_of(notation: Notation, precision: u8, small_precision: u8) -> NumFormat {
        NumFormat {
            notation,
            precision,
            small_precision,
        }
    }

    /// Asserts that each value formats as its text.
    fn check(format: NumFormat, cases: &[(f64, &str)]) {
        for &(value, text) in cases {
            assert_eq!(
                Num::from(value).format(&format),
                text,
                "{value:e} in {format:?}"
            );
        }
    }

    /// Edge values, pseudo-random values of every magnitude, and all of
    /// them negated.
    fn samples() -> Vec<f64> {
        let mut values = vec![
            0.0,
            TINY,
            f64::MIN_POSITIVE,
            0.001,
            0.1,
            0.5,
            1.0,
            999.4,
            999.5,
            999.9995,
            1000.0,
            1234.5,
            1e15,
            1e23,
            1e300,
            f64::MAX,
        ];
        // xorshift64, so every run checks the same values.
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        for _ in 0..400 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let raw = f64::from_bits(state);
            if raw.is_finite() {
                values.push(raw.abs());
            }
            // Raw bits are mostly tiny or huge, so also take powers of ten
            // spread evenly from 1e-5 to 1e308.
            values.push(10f64.powf((state % 31_300) as f64 / 100.0 - 5.0));
        }
        let negated: Vec<f64> = values.iter().map(|value| -value).collect();
        values.extend(negated);
        values
    }

    /// The exact significant digits (each 0 to 9) and decimal exponent of a
    /// positive `value`.
    fn exact_digits(value: f64) -> (Vec<u8>, i32) {
        // No f64 has more than 767 significant digits, so 800 places is exact.
        let text = format!("{value:.800e}");
        let (mantissa, exp) = text.split_once('e').unwrap();
        let digits = mantissa
            .bytes()
            .filter(|&b| b != b'.')
            .map(|b| b - b'0')
            .collect();
        (digits, exp.parse().unwrap())
    }

    /// Engineering text for a value of 1000 or more, from its exact digits,
    /// rounded half to even by hand.
    fn reference_engineering(digits: &[u8], exp: i32, precision: usize) -> String {
        let keep = exp.rem_euclid(3) as usize + 1 + precision;
        let mut kept = digits[..keep].to_vec();
        let rest = &digits[keep..];
        let beyond = if rest[1..].iter().any(|&d| d != 0) {
            Ordering::Greater
        } else {
            Ordering::Equal
        };
        let round_up = match rest[0].cmp(&5).then(beyond) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => kept[keep - 1] % 2 == 1,
        };
        let mut exp = exp;
        if round_up {
            if let Some(i) = kept.iter().rposition(|&d| d != 9) {
                kept[i] += 1;
                kept[i + 1..].fill(0);
            } else {
                kept.fill(0);
                kept.insert(0, 1);
                exp += 1;
            }
        }
        let before_point = exp.rem_euclid(3) as usize + 1;
        kept.resize(before_point + precision, 0);
        let text: String = kept.iter().map(|&d| char::from(b'0' + d)).collect();
        let (whole, fraction) = text.split_at(before_point);
        let point = if precision > 0 { "." } else { "" };
        format!("{whole}{point}{fraction}e{}", exp - exp.rem_euclid(3))
    }

    #[test]
    fn plain() {
        check(
            format_of(Notation::Plain, 2, 2),
            &[
                (0.5, "0.50"),
                (-0.5, "-0.50"),
                (12.345, "12.35"), // the f64 is just above 12.345
                (999.994, "999.99"),
                (999.9995, "1000.00"),
                (1234.567, "1234.57"),
                (-1234.567, "-1234.57"),
                (1e15, "1000000000000000.00"),
            ],
        );
        check(
            format_of(Notation::Plain, 0, 2),
            &[
                (12.5, "12.50"),
                (999.996, "1000"),
                (1234.5, "1234"), // an exact tie, to even
                (1235.5, "1236"),
                (-1e6, "-1000000"),
            ],
        );
    }

    #[test]
    fn scientific() {
        check(
            format_of(Notation::Scientific, 2, 2),
            &[
                (1.0, "1.00"),
                (-2.5, "-2.50"),
                (999.99, "999.99"),
                (999.994, "999.99"),
                (999.996, "1.00e3"),
                (999.9995, "1.00e3"),
                (1000.0, "1.00e3"),
                (-1000.0, "-1.00e3"),
                (1234.5, "1.23e3"),
                (1225.0, "1.22e3"), // an exact tie, to even
                (1235.0, "1.24e3"),
                (999_400.0, "9.99e5"),
                (999_500.0, "1.00e6"),
                (1.23e45, "1.23e45"),
                (1e100, "1.00e100"),
            ],
        );
        check(
            format_of(Notation::Scientific, 0, 0),
            &[
                (0.5, "0"),
                (1.5, "2"),
                (2.5, "2"),
                (-0.5, "0"),
                (-0.6, "-1"),
                (999.4, "999"),
                (999.5, "1e3"),
                (1499.0, "1e3"),
                (1500.0, "2e3"),
                (2500.0, "2e3"),
            ],
        );
        check(
            format_of(Notation::Scientific, 3, 3),
            &[
                (999.9994, "999.999"),
                (999.9995, "1.000e3"), // the f64 is just above 999.9995
                (123_456.0, "1.235e5"),
            ],
        );
    }

    #[test]
    fn engineering() {
        check(
            format_of(Notation::Engineering, 2, 2),
            &[
                (999.994, "999.99"),
                (999.996, "1.00e3"),
                (1234.5, "1.23e3"),
                (1234.9999, "1.23e3"), // rounded once, not to 1.2350e3 first
                (9994.0, "9.99e3"),
                (9996.0, "10.00e3"),
                (12_346.0, "12.35e3"),
                (99_994.0, "99.99e3"),
                (99_996.0, "100.00e3"),
                (123_456.0, "123.46e3"),
                (999_994.0, "999.99e3"),
                (999_996.0, "1.00e6"),
                (1.5e10, "15.00e9"),
                (-123_456.0, "-123.46e3"),
            ],
        );
        check(
            format_of(Notation::Engineering, 0, 0),
            &[
                (999.5, "1e3"),
                (8500.0, "8e3"),
                (9500.0, "10e3"),
                (99_500.0, "100e3"),
                (999_500.0, "1e6"),
                (1_234_567.0, "1e6"),
            ],
        );
    }

    #[test]
    fn suffix() {
        check(
            format_of(Notation::Suffix, 2, 0),
            &[
                (999.0, "999"),
                (999.4, "999"),
                (999.5, "1.00K"),
                (-999.5, "-1.00K"),
                (1000.0, "1.00K"),
                (1500.0, "1.50K"),
                (45.6e6, "45.60M"),
                (789e9, "789.00B"),
                (999_994.0, "999.99K"),
                (999_995.0, "1.00M"), // an exact tie, to even
                (1e12, "1.00T"),
                (1e15, "1.00Qa"),
                (1e18, "1.00Qi"),
                (1e21, "1.00Sx"),
                (1e24, "1.00Sp"),
                (1e27, "1.00Oc"),
                (1e30, "1.00No"),
                (1e33, "1.00Dc"),
                (1e36, "1.00UDc"),
                (1.23e45, "1.23QaDc"),
                (1e63, "1.00Vg"),
                (1e93, "1.00Tg"),
                (1e123, "1.00Qag"),
                (1e153, "1.00Qig"),
                (1e183, "1.00Sxg"),
                (1e213, "1.00Spg"),
                (1e243, "1.00Ocg"),
                (1e273, "1.00Nog"),
                (1e300, "1.00NoNog"),
                (1e303, "1.00Ce"),
                (1e306, "1.00UCe"),
            ],
        );
        check(
            format_of(Notation::Suffix, 3, 3),
            &[(999.9995, "1.000K"), (999.9994, "999.999")],
        );
        // Rounding up to the next power of a thousand moves to the next
        // suffix, across each tens name too.
        check(
            format_of(Notation::Suffix, 2, 2),
            &[
                (-999.996, "-1.00K"),
                (999_999.5, "1.00M"),
                (-999_999.5, "-1.00M"),
                (999.996e30, "1.00Dc"),
                (999.996e60, "1.00Vg"),
                (999.996e300, "1.00Ce"),
                (999.996e303, "1.00UCe"),
            ],
        );
        check(
            format_of(Notation::Suffix, 0, 0),
            &[(999_999.5, "1M"), (-999_999.5, "-1M"), (999.996e30, "1Dc")],
        );
        check(
            format_of(Notation::Suffix, 1, 1),
            &[
                (1234.5, "1.2K"),
                (999_850.0, "999.8K"), // an exact tie, to even
                (999_950.0, "1.0M"),   // an exact tie, to even
            ],
        );
        check(
            format_of(Notation::Suffix, 16, 16),
            &[
                (1234.5, "1.2345000000000000K"),
                (1e6 / 3.0, "333.3333333333333139K"),
                (f64::MAX, "179.7693134862315708UCe"),
            ],
        );
    }

    #[test]
    fn letter() {
        check(
            format_of(Notation::Letter, 2, 0),
            &[
                (999.0, "999"),
                (999.5, "1.00a"),
                (1000.0, "1.00a"),
                (12_346.0, "12.35a"),
                (1e6, "1.00b"),
                (1e9, "1.00c"),
                (1e78, "1.00z"),
                (1e81, "1.00aa"),
                (1e84, "1.00ab"),
                (1e156, "1.00az"),
                (1e159, "1.00ba"),
                (-2.5e81, "-2.50aa"),
            ],
        );
        check(
            format_of(Notation::Letter, 2, 2),
            &[
                (-999.996, "-1.00a"),
                (-999_999.5, "-1.00b"),
                (999.996e75, "1.00z"),
                (999.996e78, "1.00aa"),
                (-999.996e78, "-1.00aa"),
                (999.996e153, "1.00az"),
                (999.996e156, "1.00ba"),
            ],
        );
        check(format_of(Notation::Letter, 1, 1), &[(999_950.0, "1.0b")]);
        check(
            format_of(Notation::Letter, 16, 16),
            &[(f64::MAX, "179.7693134862315708cx")],
        );
    }

    #[test]
    fn letters_count_in_bijective_base_26() {
        let cases = [
            (1, "a"),
            (2, "b"),
            (26, "z"),
            (27, "aa"),
            (28, "ab"),
            (52, "az"),
            (53, "ba"),
            (702, "zz"),
            (703, "aaa"),
            (
                usize::MAX,
                if usize::BITS == 64 {
                    "gkgwbylwrxtlpo"
                } else {
                    "mwlqkwu"
                },
            ),
        ];
        for (thousands, expected) in cases {
            let mut letters = String::new();
            write_letters(thousands, &mut letters).unwrap();
            assert_eq!(letters, expected, "{thousands}");
        }
    }

    #[test]
    fn suffix_table_follows_its_documented_pattern() {
        const UNITS: [&str; 10] = ["", "U", "D", "T", "Qa", "Qi", "Sx", "Sp", "Oc", "No"];
        const TENS: [&str; 10] = [
            "", "Dc", "Vg", "Tg", "Qag", "Qig", "Sxg", "Spg", "Ocg", "Nog",
        ];
        assert_eq!(
            SUFFIXES[..10],
            ["K", "M", "B", "T", "Qa", "Qi", "Sx", "Sp", "Oc", "No"]
        );
        // From a decillion (n = 10) on, the n-illion is units then tens.
        for (n, suffix) in SUFFIXES.iter().enumerate().skip(10) {
            let expected = if n < 100 {
                format!("{}{}", UNITS[n % 10], TENS[n / 10])
            } else {
                format!("{}Ce", UNITS[n - 100])
            };
            assert_eq!(*suffix, expected, "1e{}", 3 * (n + 1));
        }
        let distinct: HashSet<_> = SUFFIXES.iter().collect();
        assert_eq!(distinct.len(), SUFFIXES.len());
    }

    #[test]
    fn zero_and_nan_in_every_notation() {
        // `Num::ZERO.log10()` is `MIN`, so an exponent from `log10` would
        // misplace zero. The formatter takes exponents from Rust's own float
        // formatting instead.
        for notation in NOTATIONS {
            assert_eq!(Num::ZERO.format(&format_of(notation, 2, 2)), "0.00");
            assert_eq!(Num::ZERO.format(&format_of(notation, 2, 0)), "0");
            assert_eq!(Num::from(-0.0).format(&format_of(notation, 2, 0)), "0");
            // NaN becomes zero, so it writes as zero.
            for nan in [f64::NAN, -f64::NAN] {
                assert_eq!(Num::from(nan).format(&format_of(notation, 2, 2)), "0.00");
            }
        }
    }

    #[test]
    fn values_that_round_to_zero_have_no_sign() {
        for notation in NOTATIONS {
            let format = format_of(notation, 2, 2);
            for value in [-0.001, -0.004_999, -TINY] {
                assert_eq!(Num::from(value).format(&format), "0.00", "{value:e}");
            }
            assert_eq!(Num::from(-0.006).format(&format), "-0.01");
        }
    }

    #[test]
    fn max_min_and_infinities() {
        let cases = [
            (Notation::Scientific, "1.80e308"),
            (Notation::Engineering, "179.77e306"),
            (Notation::Suffix, "179.77UCe"),
            (Notation::Letter, "179.77cx"),
        ];
        for (notation, text) in cases {
            let format = format_of(notation, 2, 2);
            assert_eq!(Num::MAX.format(&format), text);
            assert_eq!(Num::MIN.format(&format), format!("-{text}"));
            // Infinities saturate, so they write as MAX and MIN.
            assert_eq!(Num::from(f64::INFINITY).format(&format), text);
            assert_eq!(
                Num::from(f64::NEG_INFINITY).format(&format),
                format!("-{text}")
            );
        }
        let format = format_of(Notation::Plain, 2, 2);
        let plain = Num::MAX.format(&format);
        assert_eq!(plain, format!("{:.2}", f64::MAX));
        assert!(plain.starts_with("17976931348623157"));
        assert_eq!(plain.len(), 309 + 3);
        assert_eq!(Num::MIN.format(&format), format!("-{plain}"));
    }

    #[test]
    fn values_below_1000_are_plain_in_every_notation() {
        // Value, small_precision and the text every notation writes.
        let cases = [
            (0.25, 1, "0.2"),   // an exact tie, to even
            (0.35, 1, "0.3"),   // the f64 is just below 0.35
            (0.005, 2, "0.01"), // the f64 is just above 0.005
            (-0.5, 0, "0"),
            (-1.5, 0, "-2"),
            (123.456, 7, "123.4560000"),
            (TINY, 7, "0.0000000"),
            (998.5, 0, "998"), // an exact tie, to even
            (999.0, 2, "999.00"),
            (999.4, 0, "999"),
            (-999.4, 0, "-999"),
            (999.4, 2, "999.40"),
            (999.5, 2, "999.50"),
            (999.994, 2, "999.99"),
            (999.9994, 3, "999.999"),
            (999.9995, 4, "999.9995"),
            (999.9995, 7, "999.9995000"),
        ];
        for (value, small_precision, text) in cases {
            for notation in NOTATIONS {
                let format = format_of(notation, 3, small_precision);
                assert_eq!(
                    Num::from(value).format(&format),
                    text,
                    "{value:e} in {format:?}"
                );
            }
        }
    }

    #[test]
    fn values_that_round_to_1000_carry_in_every_notation() {
        // Value and small_precision; each rounds to 1000 at that precision.
        let cases = [(999.5, 0), (999.996, 2), (999.9995, 2), (999.9995, 3)];
        let carried = [
            (Notation::Plain, "1000.00"),
            (Notation::Scientific, "1.00e3"),
            (Notation::Engineering, "1.00e3"),
            (Notation::Suffix, "1.00K"),
            (Notation::Letter, "1.00a"),
        ];
        for (value, small_precision) in cases {
            for (notation, text) in carried {
                let format = format_of(notation, 2, small_precision);
                assert_eq!(
                    Num::from(value).format(&format),
                    text,
                    "{value:e} in {format:?}"
                );
                assert_eq!(
                    Num::from(-value).format(&format),
                    format!("-{text}"),
                    "{:e} in {format:?}",
                    -value
                );
            }
        }
    }

    #[test]
    fn small_precision_decides_whether_a_value_carries() {
        check(
            format_of(Notation::Suffix, 1, 3),
            &[
                (999.9996, "1.0K"),
                (-999.9996, "-1.0K"),
                (999.9994, "999.999"),
            ],
        );
        // A carried value is written with `precision`, which here shows more
        // digits than it was rounded to. The `NumFormat` docs say so.
        let carried = [
            (Notation::Plain, "1000.00000"),
            (Notation::Scientific, "1.00000e3"),
            (Notation::Engineering, "1.00000e3"),
            (Notation::Suffix, "1.00000K"),
            (Notation::Letter, "1.00000a"),
        ];
        for (notation, text) in carried {
            assert_eq!(Num::from(999.996).format(&format_of(notation, 5, 2)), text);
        }
    }

    /// Values of 1000 or more next to each power of ten, where rounding can
    /// change the exponent, and the large `samples()`, each also negated.
    fn grouped_samples() -> Vec<f64> {
        let mut values = Vec::new();
        for k in 3..=308 {
            for text in [
                format!("1e{k}"),
                format!("9.995e{}", k - 1),
                format!("99.995e{}", k - 2),
                format!("999.995e{}", k - 3),
            ] {
                let near: f64 = text.parse().unwrap();
                values.extend([near.next_down(), near, near.next_up()]);
            }
        }
        values.retain(|&value| value >= 1000.0);
        let negated: Vec<f64> = values.iter().map(|value| -value).collect();
        values.extend(negated);
        // `samples()` is already negated.
        values.extend(samples().into_iter().filter(|value| value.abs() >= 1000.0));
        values
    }

    #[test]
    fn grouped_notations_round_once_at_the_right_digit() {
        for value in grouped_samples() {
            let num = Num::from(value);
            let (digits, exp) = exact_digits(value.abs());
            let sign = if value < 0.0 { "-" } else { "" };
            for precision in 0..=NumFormat::MAX_PRECISION {
                let engineering = format!(
                    "{sign}{}",
                    reference_engineering(&digits, exp, precision.into())
                );
                let (mantissa, exp) = engineering.split_once('e').unwrap();
                let thousands = exp.parse::<usize>().unwrap() / 3;
                let mut letters = String::new();
                write_letters(thousands, &mut letters).unwrap();
                let expected = [
                    (Notation::Engineering, engineering.clone()),
                    (
                        Notation::Suffix,
                        format!("{mantissa}{}", SUFFIXES[thousands - 1]),
                    ),
                    (Notation::Letter, format!("{mantissa}{letters}")),
                ];
                for (notation, text) in expected {
                    assert_eq!(
                        num.format(&format_of(notation, precision, 0)),
                        text,
                        "{value:e} in {notation:?} at precision {precision}"
                    );
                }
            }
        }
    }

    #[test]
    fn precision_above_the_maximum_counts_as_the_maximum() {
        let max = NumFormat::MAX_PRECISION;
        for notation in NOTATIONS {
            for value in [0.1, -2.5e-7, 1234.5678, 1e300] {
                let num = Num::from(value);
                let at_max = num.format(&format_of(notation, max, max));
                assert_eq!(num.format(&format_of(notation, max + 1, max + 1)), at_max);
                assert_eq!(num.format(&format_of(notation, u8::MAX, 200)), at_max);
            }
        }
        let third = Num::from(1.0 / 3.0);
        assert_eq!(
            third.format(&format_of(Notation::Plain, 99, 99)),
            "0.3333333333333333"
        );
    }

    #[test]
    fn format_into_appends_to_the_text_already_there() {
        let mut text = String::from("gold: ");
        Num::from(-1234.5)
            .format_into(&NumFormat::new(Notation::Suffix), &mut text)
            .unwrap();
        assert_eq!(text, "gold: -1.23K");

        for value in samples() {
            let num = Num::from(value);
            for notation in NOTATIONS {
                for (precision, small_precision) in [(0, 0), (2, 2), (1, 3), (16, 0), (5, 16)] {
                    let format = format_of(notation, precision, small_precision);
                    let text = num.format(&format);
                    let mut fresh = String::new();
                    num.format_into(&format, &mut fresh).unwrap();
                    assert_eq!(fresh, text, "{value:e} in {format:?}");
                    let mut label = String::from("gold: ");
                    num.format_into(&format, &mut label).unwrap();
                    assert_eq!(label, format!("gold: {text}"), "{value:e} in {format:?}");
                }
            }
        }
    }

    /// A writer that fails only its `fail_at`th write, counting from 0, so an
    /// error that is dropped instead of passed on turns into `Ok`.
    struct FailOnce {
        writes: usize,
        fail_at: usize,
    }

    impl FailOnce {
        fn at(fail_at: usize) -> Self {
            Self { writes: 0, fail_at }
        }
    }

    impl fmt::Write for FailOnce {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            let this = self.writes;
            self.writes += 1;
            if this == self.fail_at {
                Err(fmt::Error)
            } else {
                Ok(())
            }
        }
    }

    /// Runs `write` into a writer that fails on each of its writes in turn,
    /// and checks that every failure comes back as an error.
    fn assert_passes_on_every_error(name: &str, write: impl Fn(&mut FailOnce) -> fmt::Result) {
        let mut counter = FailOnce::at(usize::MAX);
        write(&mut counter).unwrap();
        assert!(counter.writes > 0, "{name}");
        for fail_at in 0..counter.writes {
            assert_eq!(
                write(&mut FailOnce::at(fail_at)),
                Err(fmt::Error),
                "{name}: write {fail_at} of {}",
                counter.writes
            );
        }
    }

    #[test]
    fn writer_errors_are_passed_on() {
        let values = [
            0.5,
            -0.5,
            -0.001,
            999.996,
            -999.996,
            1234.5,
            -1234.5,
            1e300,
            -1e300,
            f64::MAX,
        ];
        for value in values {
            let num = Num::from(value);
            for notation in NOTATIONS {
                for format in [NumFormat::new(notation), format_of(notation, 0, 0)] {
                    assert_passes_on_every_error(&format!("{value:e} in {format:?}"), |out| {
                        num.format_into(&format, out)
                    });
                }
            }
            assert_passes_on_every_error(&format!("{value:e} in Display"), |out| {
                write!(out, "{num}")
            });
            assert_passes_on_every_error(&format!("{value:e} in padded Display"), |out| {
                write!(out, "{num:*^14}")
            });
        }
    }

    #[test]
    fn display_writes_the_default_format() {
        assert_eq!(NumFormat::default(), NumFormat::DEFAULT);
        assert_eq!(Num::from(0.5).to_string(), "0.50");
        assert_eq!(Num::ZERO.to_string(), "0.00");
        assert_eq!(Num::from(1e-10).to_string(), "0.00");
        assert_eq!(Num::MIN.to_string(), "-1.80e308");
    }

    #[test]
    fn display_takes_precision_width_and_flags() {
        let x = Num::from(1234.5);
        assert_eq!(format!("{x:.0}"), "1e3");
        assert_eq!(format!("{x:.3}"), "1.234e3"); // an exact tie, to even
        assert_eq!(format!("{x:.300}"), format!("{x:.16}"));
        assert_eq!(format!("{:.300}", Num::from(0.1)), "0.1000000000000000");
        assert_eq!(format!("{:.16}", Num::from(1e-10)), "0.0000000001000000");
        assert_eq!(format!("{x:08.1}"), "0001.2e3");
        assert_eq!(format!("{x:3}"), "1.23e3"); // narrower than the text
        let five = Num::from(5);
        assert_eq!(format!("{:>9}", -five), "    -5.00");
        assert_eq!(format!("{five:<7}|"), "5.00   |");
        assert_eq!(format!("{five:^8}"), "  5.00  ");
        assert_eq!(format!("{:^8}", -five), " -5.00  "); // the odd space goes after
        assert_eq!(format!("{five:*>8}"), "****5.00");
        assert_eq!(format!("{:08}", -five), "-0005.00");
        // A `0` fill is not the `0` flag: it goes before the sign.
        assert_eq!(format!("{five:0>8}"), "00005.00");
        assert_eq!(format!("{:0>8}", -five), "000-5.00");
        assert_eq!(format!("{five:+}"), "+5.00");
        assert_eq!(format!("{five:+08}"), "+0005.00");
        assert_eq!(format!("{:+}", Num::ZERO), "+0.00");
        assert_eq!(format!("{:>12}", Num::MAX), "    1.80e308");
        assert_eq!(format!("{:<10}|", Num::MIN), "-1.80e308 |");
        // A value that rounds to zero has no minus sign here either.
        assert_eq!(format!("{:>6}", Num::from(-0.001)), "  0.00");
        assert_eq!(format!("{:+}", Num::from(-0.001)), "+0.00");
    }

    #[test]
    fn exp_formats_write_the_inner_f64() {
        assert_eq!(format!("{:e}", Num::from(1500)), "1.5e3");
        assert_eq!(format!("{:E}", Num::from(1500)), "1.5E3");
        assert_eq!(format!("{:.3E}", Num::from(123_456)), "1.235E5");
        assert_eq!(format!("{:e}", Num::ZERO), "0e0");
        assert_eq!(format!("{:e}", Num::from(-0.0)), "0e0");
        assert_eq!(format!("{:e}", Num::MAX), "1.7976931348623157e308");
        assert_eq!(format!("{:E}", Num::MAX), "1.7976931348623157E308");
        assert_eq!(format!("{:e}", Num::MIN), "-1.7976931348623157e308");
        assert_eq!(format!("{:e}", Num::from(TINY)), "5e-324");
    }

    #[test]
    fn exp_and_max_precision_text_round_trip_exactly() {
        let format = format_of(Notation::Scientific, NumFormat::MAX_PRECISION, 0);
        for value in samples() {
            let num = Num::from(value);
            assert_eq!(Num::parse(&format!("{num:e}")), Ok(num), "{value:e}");
            assert_eq!(Num::parse(&format!("{num:E}")), Ok(num), "{value:e}");
            if value.abs() >= 1000.0 {
                assert_eq!(Num::parse(&num.format(&format)), Ok(num), "{value:e}");
            }
        }
    }

    /// The place value of the last digit `text` shows: 10 for `12.35e3`.
    fn last_digit_unit(text: &str) -> f64 {
        let (mantissa, exp) = text.split_once('e').unwrap_or((text, "0"));
        let after_point = mantissa
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len());
        10f64.powi(exp.parse::<i32>().unwrap() - after_point as i32)
    }

    #[test]
    fn formatted_text_reads_back_as_the_value_shown() {
        // Parsing plain, scientific or engineering text gives a value within
        // half a last digit of the value written (plus the f64 rounding of
        // the parse), and that value formats as the same text again.
        for notation in [Notation::Plain, Notation::Scientific, Notation::Engineering] {
            for precision in [0, 1, 2, 3, 6] {
                let format = format_of(notation, precision, precision);
                for value in samples() {
                    let text = Num::from(value).format(&format);
                    let parsed = Num::parse(&text).unwrap_or_else(|_| panic!("{text:?}"));
                    let error = (parsed.as_f64() - value).abs();
                    let bound = 0.5 * last_digit_unit(&text) + value.abs() * f64::EPSILON;
                    assert!(
                        error <= bound,
                        "{value:e} read back from {text:?} as {parsed:e}"
                    );
                    assert_eq!(parsed.format(&format), text, "{value:e} in {format:?}");
                }
            }
        }
    }

    #[test]
    fn parse_reads_plain_and_scientific_text() {
        let cases = [
            ("0", 0.0),
            ("42", 42.0),
            ("-42", -42.0),
            ("+42", 42.0),
            ("007", 7.0),
            ("1.5", 1.5),
            ("-0.25", -0.25),
            (".5", 0.5),
            ("5.", 5.0),
            ("1e3", 1000.0),
            ("1E3", 1000.0),
            ("1e+3", 1000.0),
            ("1.e3", 1000.0),
            ("-.5e-3", -0.0005),
            ("1e007", 1e7),
            ("2.5e-3", 0.0025),
            ("1.23e45", 1.23e45),
            ("12.35e3", 12_350.0),
            ("1.7976931348623157e308", f64::MAX),
            ("5e-324", TINY),
        ];
        for (text, value) in cases {
            assert_eq!(Num::parse(text), Ok(Num::from(value)), "{text:?}");
            assert_eq!(text.parse::<Num>(), Ok(Num::from(value)), "{text:?}");
        }
        // A thousand digits are read and rounded once.
        let third = format!("0.{}", "3".repeat(998));
        assert_eq!(Num::parse(&third), Ok(Num::from(1.0 / 3.0)));
        assert_eq!(Num::parse(&"9".repeat(1000)), Ok(Num::MAX));
    }

    #[test]
    fn parse_saturates_text_out_of_range() {
        assert_eq!(Num::parse("1e309"), Ok(Num::MAX));
        assert_eq!(Num::parse("1.8e308"), Ok(Num::MAX));
        assert_eq!(Num::parse("-1e999"), Ok(Num::MIN));
        assert_eq!(Num::parse("1e99999999999999999999"), Ok(Num::MAX));
        assert_eq!(Num::parse("-1e99999999999999999999"), Ok(Num::MIN));
        for zero in [
            "-0",
            "-0.0e5",
            "1e-400",
            "-1e-400",
            "1e-99999999999999999999",
        ] {
            let parsed = Num::parse(zero).unwrap();
            assert_eq!(parsed.as_f64().to_bits(), 0, "{zero:?}");
        }
    }

    #[test]
    fn parse_rejects_other_text() {
        let invalid = [
            "", " ", " 1", "1 ", "\t1", "1e3\n", "1\0", "+", "-", ".", "E", "-e", "e3", "1e",
            "1e+", "--1", "+-1", "1e+-3", "1e3.5", "1e 3", "1..2", "1.2.3", "1_000", "1,000",
            "0x10", "1.5K", "1.5e3K", "1.00a", "inf", "+inf", "-inf", "infinity", "NaN", "-NaN",
            "nan", "1e3e3", "\u{661}", "\u{ff11}", "\u{221e}",
        ];
        for text in invalid {
            assert_eq!(Num::parse(text), Err(ParseNumError), "{text:?}");
            assert_eq!(text.parse::<Num>(), Err(ParseNumError), "{text:?}");
        }
        assert_eq!(
            ParseNumError.to_string(),
            "invalid number: expected a decimal like -1.5 or 2.5e30"
        );
    }

    #[test]
    fn parse_error_is_a_thread_safe_std_error() {
        fn assert_error<E: std::error::Error + Send + Sync + 'static>() {}
        assert_error::<ParseNumError>();
    }
}
