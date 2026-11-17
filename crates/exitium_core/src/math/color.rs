//! [`Color`], a linear RGBA color, and [`ParseColorError`] for hex colors.

use std::fmt;

/// A color as linear red, green and blue plus alpha, each an `f32`.
///
/// Linear values are proportional to light intensity, so blending and
/// lighting math on them is correct. Image files, hex codes and color
/// pickers store sRGB-encoded values instead, which are not linear: `0x80`
/// is about 22% intensity, not 50%. [`from_srgba_u8`](Color::from_srgba_u8),
/// [`to_srgba_u8`](Color::to_srgba_u8) and [`from_hex`](Color::from_hex)
/// convert with the sRGB transfer function of IEC 61966-2-1.
///
/// Alpha is opacity, from 0 (transparent) to 1 (opaque). It is never
/// sRGB-encoded, and it is straight, not premultiplied: `r`, `g` and `b` do
/// not include it.
///
/// Channels from 0 to 1 are displayable. Values above 1 (HDR) or below 0 are
/// allowed and kept; [`to_srgba_u8`](Color::to_srgba_u8) clamps them.
///
/// ```
/// use exitium_core::math::Color;
///
/// let orange = Color::from_hex("#ff8800").unwrap();
/// assert_eq!(orange, Color::from_srgba_u8(0xff, 0x88, 0x00, 0xff));
/// assert_eq!(orange.to_srgba_u8(), [0xff, 0x88, 0x00, 0xff]);
/// assert!((orange.g - 0.246).abs() < 0.001); // 0x88 is 25% intensity, not 53%
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    /// Red, linear.
    pub r: f32,
    /// Green, linear.
    pub g: f32,
    /// Blue, linear.
    pub b: f32,
    /// Alpha: 0 is transparent, 1 is opaque.
    pub a: f32,
}

impl Color {
    /// Opaque white. Also the [`Default`], because multiplying by white
    /// changes nothing, so a default tint leaves a sprite as it was drawn.
    pub const WHITE: Self = Self::linear_rgba(1.0, 1.0, 1.0, 1.0);

    /// Opaque black.
    pub const BLACK: Self = Self::linear_rgba(0.0, 0.0, 0.0, 1.0);

    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self::linear_rgba(0.0, 0.0, 0.0, 0.0);

    /// A color from linear channels, stored as given.
    #[must_use]
    #[inline]
    pub const fn linear_rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// A color from sRGB-encoded bytes, as in image files and color pickers.
    /// `r`, `g` and `b` are decoded to linear. `a` is only scaled to 0..=1.
    #[must_use]
    pub fn from_srgba_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        let decode = |byte: u8| srgb_to_linear(f32::from(byte) / 255.0);
        Self {
            r: decode(r),
            g: decode(g),
            b: decode(b),
            a: f32::from(a) / 255.0,
        }
    }

    /// The color as sRGB-encoded bytes `[r, g, b, a]`, the inverse of
    /// [`from_srgba_u8`](Self::from_srgba_u8): the round trip returns the
    /// same bytes for every input. Each channel is clamped to 0..=1 and
    /// rounded to the nearest byte; NaN becomes 0.
    #[must_use]
    pub fn to_srgba_u8(self) -> [u8; 4] {
        let encode = |linear: f32| unit_to_u8(linear_to_srgb(linear.clamp(0.0, 1.0)));
        [
            encode(self.r),
            encode(self.g),
            encode(self.b),
            unit_to_u8(self.a.clamp(0.0, 1.0)),
        ]
    }

    /// Parses a hex color as written in CSS and image editors: an optional
    /// `#`, then 3, 4, 6 or 8 hex digits in either case, for `RGB`, `RGBA`,
    /// `RRGGBB` or `RRGGBBAA`. Each pair of digits is an sRGB-encoded byte,
    /// so the result equals [`from_srgba_u8`](Self::from_srgba_u8) on the
    /// bytes. In the short forms each digit is repeated (`f80` is `ff8800`).
    /// Without an alpha digit the color is opaque. Anything else, surrounding
    /// whitespace included, is an error.
    ///
    /// ```
    /// use exitium_core::math::{Color, ParseColorError};
    ///
    /// assert_eq!(Color::from_hex("#F80"), Color::from_hex("ff8800"));
    /// let glass = Color::from_hex("#ff880080").unwrap();
    /// assert_eq!(glass.to_srgba_u8(), [0xff, 0x88, 0x00, 0x80]);
    /// assert_eq!(
    ///     Color::from_hex("#ff 800"),
    ///     Err(ParseColorError::InvalidChar { index: 3, found: ' ' }),
    /// );
    /// ```
    pub fn from_hex(text: &str) -> Result<Self, ParseColorError> {
        let digits = text.strip_prefix('#').unwrap_or(text);
        // Index of the first digit in `text`: 1 after a `#`, otherwise 0.
        let start = text.len() - digits.len();
        let len = digits.chars().count();
        if !matches!(len, 3 | 4 | 6 | 8) {
            return Err(ParseColorError::InvalidLength { len });
        }
        let mut values = [0_u8; 8];
        for (i, found) in digits.chars().enumerate() {
            let Some(value) = found.to_digit(16) else {
                let index = start + i;
                return Err(ParseColorError::InvalidChar { index, found });
            };
            values[i] = value as u8;
        }
        let [r, g, b, a] = if len <= 4 {
            let channel = |i: usize| (values[i] << 4) | values[i];
            let alpha = if len == 4 { channel(3) } else { u8::MAX };
            [channel(0), channel(1), channel(2), alpha]
        } else {
            let channel = |i: usize| (values[2 * i] << 4) | values[2 * i + 1];
            let alpha = if len == 8 { channel(3) } else { u8::MAX };
            [channel(0), channel(1), channel(2), alpha]
        };
        Ok(Self::from_srgba_u8(r, g, b, a))
    }
}

impl Default for Color {
    /// [`Color::WHITE`].
    fn default() -> Self {
        Self::WHITE
    }
}

/// The sRGB decoding curve: an encoded value in 0..=1 to linear 0..=1.
fn srgb_to_linear(encoded: f32) -> f32 {
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// The sRGB encoding curve, the inverse of [`srgb_to_linear`].
fn linear_to_srgb(linear: f32) -> f32 {
    if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// Scales 0..=1 to the nearest byte. `as` saturates, and turns NaN into 0.
fn unit_to_u8(value: f32) -> u8 {
    (value * 255.0).round() as u8
}

/// Why a text is not a valid hex color. Returned by [`Color::from_hex`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseColorError {
    /// The text after the optional `#` is not 3, 4, 6 or 8 characters long.
    InvalidLength {
        /// Number of characters after the optional `#`.
        len: usize,
    },
    /// A character after the optional `#` is not a hex digit.
    InvalidChar {
        /// Character index of the first wrong character in the whole text,
        /// counting the `#`. Every character before it is ASCII, so this is
        /// also its byte offset.
        index: usize,
        /// The character found there.
        found: char,
    },
}

impl fmt::Display for ParseColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::InvalidLength { len } => write!(
                f,
                "invalid hex color: expected 3, 4, 6 or 8 hex digits \
                 after an optional '#', found {len}"
            ),
            Self::InvalidChar { index, found } => write!(
                f,
                "invalid hex color: expected a hex digit at index {index}, found {found:?}"
            ),
        }
    }
}

impl std::error::Error for ParseColorError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_and_default() {
        assert_eq!(Color::WHITE.to_srgba_u8(), [255, 255, 255, 255]);
        assert_eq!(Color::BLACK.to_srgba_u8(), [0, 0, 0, 255]);
        assert_eq!(Color::TRANSPARENT.to_srgba_u8(), [0, 0, 0, 0]);
        assert_eq!(Color::from_srgba_u8(255, 255, 255, 255), Color::WHITE);
        assert_eq!(Color::from_srgba_u8(0, 0, 0, 255), Color::BLACK);
        assert_eq!(Color::from_srgba_u8(0, 0, 0, 0), Color::TRANSPARENT);
        assert_eq!(Color::default(), Color::WHITE);
        let color = Color::linear_rgba(0.1, 0.2, 0.3, 0.4);
        assert_eq!([color.r, color.g, color.b, color.a], [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn byte_round_trip_is_exact() {
        // Every byte value in every channel, with the channels all different.
        for v in 0..=u8::MAX {
            let bytes = [v, v.wrapping_add(85), v.wrapping_add(170), u8::MAX - v];
            let [r, g, b, a] = bytes;
            assert_eq!(Color::from_srgba_u8(r, g, b, a).to_srgba_u8(), bytes);
        }
    }

    #[test]
    fn decoding_matches_reference_values() {
        // Reference values from the IEC 61966-2-1 formulas in f64. Byte 10
        // is on the linear segment, byte 11 the first on the curve.
        let cases = [
            (0, 0.0),
            (10, 0.003_035_27),
            (11, 0.003_346_536),
            (128, 0.215_860_5),
            (188, 0.502_886_5),
            (255, 1.0),
        ];
        for (byte, linear) in cases {
            let color = Color::from_srgba_u8(byte, byte, byte, byte);
            assert!((color.r - linear).abs() < 1e-6, "{byte}: {}", color.r);
            assert_eq!((color.g, color.b), (color.r, color.r), "{byte}");
            assert_eq!(color.a, f32::from(byte) / 255.0, "{byte}: alpha is linear");
        }
    }

    #[test]
    fn encoding_matches_reference_values() {
        // (linear value, its sRGB byte, its alpha byte). 0.001 is on the
        // linear segment; alpha is only scaled, and 127.5 rounds up.
        let cases = [
            (0.0, 0, 0),
            (0.001, 3, 0),
            (0.18, 118, 46),
            (0.5, 188, 128),
            (1.0, 255, 255),
        ];
        for (linear, byte, alpha) in cases {
            let color = Color::linear_rgba(linear, linear, linear, linear);
            assert_eq!(color.to_srgba_u8(), [byte, byte, byte, alpha], "{linear}");
        }
    }

    #[test]
    fn to_srgba_u8_clamps_and_maps_nan_to_zero() {
        let wild = Color::linear_rgba(4.0, -1.0, f32::NAN, 2.0);
        assert_eq!(wild.to_srgba_u8(), [255, 0, 0, 255]);
        let infinite = Color::linear_rgba(f32::INFINITY, f32::NEG_INFINITY, 0.0, -0.5);
        assert_eq!(infinite.to_srgba_u8(), [255, 0, 0, 0]);
        assert_eq!(
            Color::linear_rgba(0.0, 0.0, 0.0, f32::NAN).to_srgba_u8()[3],
            0
        );
    }

    #[test]
    fn from_hex_accepts_every_form() {
        let orange = Ok(Color::from_srgba_u8(0xff, 0x88, 0x00, 0xff));
        let forms = [
            "#ff8800",
            "ff8800",
            "#FF8800",
            "#fF8800",
            "#ff8800ff",
            "#f80",
            "F80",
            "#f80f",
        ];
        for text in forms {
            assert_eq!(Color::from_hex(text), orange, "{text}");
        }
        let rgba = Ok(Color::from_srgba_u8(0x12, 0x34, 0x56, 0x78));
        assert_eq!(Color::from_hex("#12345678"), rgba);
        let short = Ok(Color::from_srgba_u8(0x11, 0x22, 0x33, 0x44));
        assert_eq!(Color::from_hex("#1234"), short);
        assert_eq!(Color::from_hex("#000"), Ok(Color::BLACK));
        assert_eq!(Color::from_hex("#0000"), Ok(Color::TRANSPARENT));
    }

    #[test]
    fn from_hex_reads_every_digit() {
        for (value, digit) in "0123456789abcdef".chars().enumerate() {
            let byte = u8::try_from(value * 0x11).unwrap();
            let expected = Ok(Color::from_srgba_u8(byte, byte, byte, byte));
            for case in [digit, digit.to_ascii_uppercase()] {
                for text in [case.to_string().repeat(4), case.to_string().repeat(8)] {
                    assert_eq!(Color::from_hex(&text), expected, "{text}");
                }
            }
        }
    }

    #[test]
    fn from_hex_rejects_other_text() {
        use ParseColorError::{InvalidChar, InvalidLength};
        let cases = [
            ("", InvalidLength { len: 0 }),
            ("#", InvalidLength { len: 0 }),
            ("#ff", InvalidLength { len: 2 }),
            ("#ff880", InvalidLength { len: 5 }),
            ("#ff8800f", InvalidLength { len: 7 }),
            ("#ff8800ff0", InvalidLength { len: 9 }),
            // Without a leading `#`, the space is one of the characters.
            (" #fff", InvalidLength { len: 5 }),
            // Characters are counted, not bytes: `é` is two bytes.
            ("#ffffé", InvalidLength { len: 5 }),
            (
                "#fff ",
                InvalidChar {
                    index: 4,
                    found: ' ',
                },
            ),
            (
                "##fff",
                InvalidChar {
                    index: 1,
                    found: '#',
                },
            ),
            (
                "#ff8g00",
                InvalidChar {
                    index: 4,
                    found: 'g',
                },
            ),
            (
                "0xff",
                InvalidChar {
                    index: 1,
                    found: 'x',
                },
            ),
            (
                "#éff",
                InvalidChar {
                    index: 1,
                    found: 'é',
                },
            ),
        ];
        for (text, error) in cases {
            assert_eq!(Color::from_hex(text), Err(error), "{text:?}");
        }
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            ParseColorError::InvalidLength { len: 5 }.to_string(),
            "invalid hex color: expected 3, 4, 6 or 8 hex digits after an optional '#', found 5"
        );
        assert_eq!(
            ParseColorError::InvalidChar {
                index: 4,
                found: 'g'
            }
            .to_string(),
            "invalid hex color: expected a hex digit at index 4, found 'g'"
        );
    }
}
