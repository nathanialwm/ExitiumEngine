//! [`Guid`], a stable 128-bit id for assets and anything else that is saved.
//!
//! A `Guid` names the same thing across runs, files and machines, unlike a
//! [`Handle`](crate::Handle), which only means something inside one process.
//! This module never generates GUIDs: `exitium_core` has no random source.
//! The asset database mints them, and [`Guid::from_u128`] wraps a value that
//! already exists.
//!
//! # Text form
//!
//! [`Display`](fmt::Display) writes the 32 hex digits of the `u128`, most
//! significant first and in lowercase, split by dashes into groups of 8, 4,
//! 4, 4 and 12 digits. That is 36 characters, the usual UUID spelling:
//!
//! ```text
//! 67e55044-10b1-426f-9247-bb680e5fe0c8
//! ```
//!
//! [`FromStr`] accepts exactly that form and nothing else. Uppercase digits,
//! braces, a `urn:uuid:` prefix, surrounding whitespace and the 32-digit form
//! without dashes are all rejected with a [`ParseGuidError`] that says what
//! is wrong and where. So every `Guid` has exactly one spelling, and the
//! round trip is exact in both directions:
//!
//! - `guid.to_string().parse::<Guid>() == Ok(guid)` for every `Guid`.
//! - `text.parse::<Guid>()?.to_string() == text` for every `text` that parses.
//!
//! The text and JSON formats and MCP write a `Guid` in this form (see
//! `docs/specs/reflect-model.md`), so changing it is a format change.
//! Because every text has the same width, case and dash positions, comparing
//! two texts byte by byte gives the same order as comparing the values
//! ([`Ord`]).
//!
//! ```
//! use exitium_core::Guid;
//!
//! // Grouping the literal's digits 8-4-4-4-12 makes it read like the text.
//! let guid = Guid::from_u128(0x67e55044_10b1_426f_9247_bb680e5fe0c8);
//! assert_eq!(guid.to_string(), "67e55044-10b1-426f-9247-bb680e5fe0c8");
//! assert_eq!("67e55044-10b1-426f-9247-bb680e5fe0c8".parse::<Guid>(), Ok(guid));
//!
//! // Only the exact form parses.
//! assert!("67E55044-10B1-426F-9247-BB680E5FE0C8".parse::<Guid>().is_err());
//! assert!("67e5504410b1426f9247bb680e5fe0c8".parse::<Guid>().is_err());
//! ```

use std::fmt;
use std::str::FromStr;

/// A stable 128-bit id, written as `67e55044-10b1-426f-9247-bb680e5fe0c8`.
///
/// Build one with [`Guid::from_u128`]. Equality, ordering and hashing are
/// those of the `u128`. The [module docs](crate::guid) define the text form
/// that [`Display`](fmt::Display) writes and [`FromStr`] reads.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Guid(u128);

impl Guid {
    /// The all-zero GUID, `00000000-0000-0000-0000-000000000000`. It is the
    /// [`Default`], and the usual placeholder for "no GUID".
    pub const NIL: Self = Self(0);

    /// Wraps a `u128`. Every value is a valid `Guid`.
    #[must_use]
    #[inline]
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// Returns the inner `u128`.
    #[must_use]
    #[inline]
    pub const fn as_u128(self) -> u128 {
        self.0
    }

    /// Whether this is [`NIL`](Self::NIL).
    #[must_use]
    #[inline]
    pub const fn is_nil(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for Guid {
    /// Writes the 36-character text form described in the
    /// [module docs](crate::guid).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            value >> 96,
            (value >> 80) & 0xffff,
            (value >> 64) & 0xffff,
            (value >> 48) & 0xffff,
            value & 0xffff_ffff_ffff,
        )
    }
}

impl fmt::Debug for Guid {
    /// Shows the text form, `Guid(67e55044-10b1-426f-9247-bb680e5fe0c8)`,
    /// rather than the raw integer.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Guid({self})")
    }
}

/// Length of the text form, in characters.
const TEXT_LEN: usize = 36;

/// Whether character `index` of the text form is a dash rather than a digit.
const fn is_dash_index(index: usize) -> bool {
    matches!(index, 8 | 13 | 18 | 23)
}

/// The value of a lowercase hex digit, or `None` for any other character,
/// uppercase digits included.
const fn lowercase_hex_digit(c: char) -> Option<u32> {
    match c {
        '0'..='9' => Some(c as u32 - '0' as u32),
        'a'..='f' => Some(c as u32 - 'a' as u32 + 10),
        _ => None,
    }
}

impl FromStr for Guid {
    type Err = ParseGuidError;

    /// Parses exactly the text that [`Display`](fmt::Display) writes. The
    /// length is checked first, then each character from the left, and the
    /// first problem found is returned.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let len = text.chars().count();
        if len != TEXT_LEN {
            return Err(ParseGuidError::InvalidLength { len });
        }
        let mut value = 0_u128;
        for (index, found) in text.chars().enumerate() {
            if is_dash_index(index) {
                if found != '-' {
                    return Err(ParseGuidError::InvalidChar { index, found });
                }
            } else if let Some(digit) = lowercase_hex_digit(found) {
                value = (value << 4) | u128::from(digit);
            } else {
                return Err(ParseGuidError::InvalidChar { index, found });
            }
        }
        Ok(Self(value))
    }
}

/// Why a text is not a valid [`Guid`]. Returned by `Guid`'s [`FromStr`] impl.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseGuidError {
    /// The text is not 36 characters long.
    InvalidLength {
        /// Length of the text, in characters.
        len: usize,
    },
    /// A character is not allowed where it is. Indices 8, 13, 18 and 23 must
    /// hold `-`, and every other index a lowercase hex digit (`0-9`, `a-f`).
    InvalidChar {
        /// Character index of the first wrong character. Every character
        /// before it is ASCII, so this is also its byte offset.
        index: usize,
        /// The character found there.
        found: char,
    },
}

impl fmt::Display for ParseGuidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::InvalidLength { len } => write!(
                f,
                "invalid GUID: expected 36 characters \
                 (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx), found {len}"
            ),
            Self::InvalidChar { index, found } => {
                let expected = if is_dash_index(index) {
                    "'-'"
                } else {
                    "a lowercase hex digit"
                };
                write!(
                    f,
                    "invalid GUID: expected {expected} at index {index}, found {found:?}"
                )
            }
        }
    }
}

impl std::error::Error for ParseGuidError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};

    const SAMPLE: Guid = Guid::from_u128(0x67e55044_10b1_426f_9247_bb680e5fe0c8);
    const SAMPLE_TEXT: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    fn parse(text: &str) -> Result<Guid, ParseGuidError> {
        text.parse()
    }

    /// Every non-zero hex digit at every position, plus both ends of the
    /// range and a few mixed patterns.
    fn samples() -> Vec<Guid> {
        let mut values = vec![
            0,
            1,
            u128::MAX - 1,
            u128::MAX,
            0x55555555_5555_5555_5555_555555555555,
            0xaaaaaaaa_aaaa_aaaa_aaaa_aaaaaaaaaaaa,
            SAMPLE.as_u128(),
        ];
        for position in 0..32 {
            for digit in 1..16_u128 {
                values.push(digit << (4 * position));
            }
        }
        values.into_iter().map(Guid::from_u128).collect()
    }

    #[test]
    fn nil_and_accessors() {
        assert_eq!(Guid::NIL.as_u128(), 0);
        assert_eq!(Guid::default(), Guid::NIL);
        assert!(Guid::NIL.is_nil());
        assert!(!Guid::from_u128(1).is_nil());
        assert_eq!(Guid::from_u128(u128::MAX).as_u128(), u128::MAX);
    }

    #[test]
    fn text_form_golden() {
        let cases = [
            (0, "00000000-0000-0000-0000-000000000000"),
            (1, "00000000-0000-0000-0000-000000000001"),
            (1 << 127, "80000000-0000-0000-0000-000000000000"),
            (u128::MAX, "ffffffff-ffff-ffff-ffff-ffffffffffff"),
            (SAMPLE.as_u128(), SAMPLE_TEXT),
            (
                0x01234567_89ab_cdef_0123_456789abcdef,
                "01234567-89ab-cdef-0123-456789abcdef",
            ),
        ];
        for (value, text) in cases {
            let guid = Guid::from_u128(value);
            assert_eq!(guid.to_string(), text);
        }
    }

    #[test]
    fn round_trip_is_exact() {
        for guid in samples() {
            let text = guid.to_string();
            assert_eq!(text.len(), TEXT_LEN, "{text}");
            assert_eq!(parse(&text), Ok(guid), "{text}");
        }
    }

    #[test]
    fn only_the_canonical_text_parses() {
        // Replace each character of a valid text with each candidate. The
        // expected outcome comes from the format alone: a dash at the four
        // dash indices, a lowercase hex digit everywhere else. The non-ASCII
        // candidates are digits or letters to Unicode-aware checks such as
        // `char::is_numeric` or `char::to_digit`.
        let candidates = "0123456789abcdefABCDEFgxz-_ {}é\0١１ｆ😀";
        for index in 0..TEXT_LEN {
            let dash = [8, 13, 18, 23].contains(&index);
            for replacement in candidates.chars() {
                let mut chars: Vec<char> = SAMPLE_TEXT.chars().collect();
                chars[index] = replacement;
                let text: String = chars.into_iter().collect();
                let allowed = if dash {
                    replacement == '-'
                } else {
                    "0123456789abcdef".contains(replacement)
                };
                match parse(&text) {
                    Ok(guid) => {
                        assert!(allowed, "{text:?} parsed");
                        assert_eq!(guid.to_string(), text, "accepted text is not canonical");
                    }
                    Err(error) => {
                        assert!(!allowed, "{text:?} was rejected: {error}");
                        let expected = ParseGuidError::InvalidChar {
                            index,
                            found: replacement,
                        };
                        assert_eq!(error, expected, "{text:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn wrong_lengths_are_rejected() {
        let longer = format!("{SAMPLE_TEXT}0");
        let cases = [
            ("", 0),
            (&SAMPLE_TEXT[..35], 35),
            (longer.as_str(), 37),
            ("67e5504410b1426f9247bb680e5fe0c8", 32),
            ("{67e55044-10b1-426f-9247-bb680e5fe0c8}", 38),
            ("urn:uuid:67e55044-10b1-426f-9247-bb680e5fe0c8", 45),
            (" 67e55044-10b1-426f-9247-bb680e5fe0c8", 37),
            ("67e55044-10b1-426f-9247-bb680e5fe0c8\n", 37),
        ];
        for (text, len) in cases {
            assert_eq!(
                parse(text),
                Err(ParseGuidError::InvalidLength { len }),
                "{text:?}"
            );
        }
        // Length counts characters, not bytes: 35 characters with a 2-byte
        // one at the end are 36 bytes, and still the wrong length.
        let text = format!("{}é", &SAMPLE_TEXT[..34]);
        assert_eq!(text.len(), TEXT_LEN);
        assert_eq!(parse(&text), Err(ParseGuidError::InvalidLength { len: 35 }));
    }

    #[test]
    fn the_first_bad_character_is_reported() {
        // A bad hex digit before a bad dash, then a bad dash before a bad hex
        // digit: the leftmost one wins either way.
        let cases = [
            ("67E55044+10b1-426f-9247-bb680e5fe0cX", 2, 'E'),
            ("67e55044+10b1-426f-92G7-bb680e5fe0c8", 8, '+'),
        ];
        for (text, index, found) in cases {
            assert_eq!(
                parse(text),
                Err(ParseGuidError::InvalidChar { index, found }),
                "{text:?}"
            );
        }
    }

    #[test]
    fn error_messages() {
        let cases = [
            (
                ParseGuidError::InvalidLength { len: 32 },
                "invalid GUID: expected 36 characters \
                 (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx), found 32",
            ),
            (
                ParseGuidError::InvalidChar {
                    index: 8,
                    found: '1',
                },
                "invalid GUID: expected '-' at index 8, found '1'",
            ),
            (
                ParseGuidError::InvalidChar {
                    index: 0,
                    found: 'A',
                },
                "invalid GUID: expected a lowercase hex digit at index 0, found 'A'",
            ),
            (
                ParseGuidError::InvalidChar {
                    index: 35,
                    found: '\n',
                },
                "invalid GUID: expected a lowercase hex digit at index 35, found '\\n'",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }

    #[test]
    fn debug_shows_the_text_form() {
        assert_eq!(
            format!("{SAMPLE:?}"),
            "Guid(67e55044-10b1-426f-9247-bb680e5fe0c8)"
        );
    }

    #[test]
    fn value_order_matches_text_order() {
        let guids = samples();
        let texts: Vec<String> = guids.iter().map(ToString::to_string).collect();
        for (a, a_text) in guids.iter().zip(&texts) {
            for (b, b_text) in guids.iter().zip(&texts) {
                assert_eq!(a.cmp(b), a_text.cmp(b_text), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn works_as_a_plain_key() {
        let keys = [
            Guid::NIL,
            Guid::from_u128(1),
            SAMPLE,
            Guid::from_u128(u128::MAX),
        ];
        let set: HashSet<Guid> = keys.into_iter().collect();
        let map: BTreeMap<Guid, usize> = keys.into_iter().zip(0..).collect();
        assert_eq!((set.len(), map.len()), (keys.len(), keys.len()));
        // Look each key up through an equal value built another way, so a
        // `Hash` or `Ord` that disagrees with `Eq` misses it.
        for (position, key) in keys.iter().enumerate() {
            let copy = parse(&key.to_string()).unwrap();
            assert!(set.contains(&copy), "{key:?}");
            assert_eq!(map.get(&copy), Some(&position), "{key:?}");
        }
        let absent = Guid::from_u128(2);
        assert!(!set.contains(&absent));
        assert_eq!(map.get(&absent), None);
    }
}
