//! [`Rect`], an axis-aligned rectangle with `f32` coordinates.

use super::Vec2;

/// An axis-aligned rectangle, stored as its `min` and `max` corners.
///
/// `min` is the corner with the smaller x and y, whichever way the y axis
/// points. The rect is half-open: it contains a point `p` when
/// `min.x <= p.x < max.x` and `min.y <= p.y < max.y`. So rects that only
/// share an edge, such as neighbouring tiles or UI cells, never both contain
/// a point on that edge, and they do not [intersect](Rect::intersects).
///
/// # Empty rects
///
/// A rect is [empty](Rect::is_empty) unless `min < max` on both axes. That
/// covers a zero width or height, `min > max` on an axis (an inverted rect,
/// which the public fields allow), and any NaN coordinate. An empty rect
/// contains no point, intersects nothing, and [`union`](Rect::union) ignores
/// it. The [`Default`] rect has both corners at the origin, so it is empty.
///
/// ```
/// use exitium_core::math::{Rect, Vec2};
///
/// let tile = Rect::from_min_size(Vec2::new(0.0, 0.0), Vec2::splat(16.0));
/// let next = Rect::from_min_size(Vec2::new(16.0, 0.0), Vec2::splat(16.0));
/// assert!(tile.contains(Vec2::new(0.0, 15.5)));
/// assert!(!tile.contains(Vec2::new(16.0, 8.0))); // the max edge is outside
/// assert!(next.contains(Vec2::new(16.0, 8.0))); // and belongs to the next tile
/// assert!(!tile.intersects(next)); // sharing an edge is not overlapping
/// assert_eq!(tile.union(next), Rect::new(Vec2::ZERO, Vec2::new(32.0, 16.0)));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// The corner with the smallest coordinates, an inclusive bound.
    pub min: Vec2,
    /// The corner with the largest coordinates, an exclusive bound.
    pub max: Vec2,
}

impl Rect {
    /// A rect with exactly these corners. Use
    /// [`from_corners`](Self::from_corners) when their order is not known.
    #[must_use]
    #[inline]
    pub const fn new(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    /// The rect spanned by two opposite corners, given in either order.
    #[must_use]
    #[inline]
    pub fn from_corners(a: Vec2, b: Vec2) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// The rect with corner `min` and the given `size`. A negative size on
    /// an axis gives an empty rect.
    #[must_use]
    #[inline]
    pub fn from_min_size(min: Vec2, size: Vec2) -> Self {
        Self {
            min,
            max: min + size,
        }
    }

    /// The rect of the given `size` centered on `center`. A negative size on
    /// an axis gives an empty rect.
    #[must_use]
    #[inline]
    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        let half = size * 0.5;
        Self {
            min: center - half,
            max: center + half,
        }
    }

    /// `max.x - min.x`. Negative for a rect inverted on x.
    #[must_use]
    #[inline]
    pub const fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    /// `max.y - min.y`. Negative for a rect inverted on y.
    #[must_use]
    #[inline]
    pub const fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    /// The [`width`](Self::width) and [`height`](Self::height).
    #[must_use]
    #[inline]
    pub const fn size(self) -> Vec2 {
        Vec2::new(self.width(), self.height())
    }

    /// The point halfway between the corners.
    #[must_use]
    #[inline]
    pub const fn center(self) -> Vec2 {
        Vec2::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }

    /// Whether the rect contains no point, because `min < max` fails on an
    /// axis. The [type docs](Rect#empty-rects) list the cases.
    #[must_use]
    #[inline]
    pub const fn is_empty(self) -> bool {
        // A negated `<`, so that a NaN coordinate counts as empty.
        !(self.min.x < self.max.x && self.min.y < self.max.y)
    }

    /// Whether `point` is inside: `min <= point < max` on both axes. A point
    /// on a `min` edge is inside, one on a `max` edge is not, and an empty
    /// rect contains nothing.
    #[must_use]
    #[inline]
    pub const fn contains(self, point: Vec2) -> bool {
        self.min.x <= point.x
            && point.x < self.max.x
            && self.min.y <= point.y
            && point.y < self.max.y
    }

    /// Whether some point is inside both rects. Rects that only share an
    /// edge or a corner do not intersect, and an empty rect intersects
    /// nothing.
    #[must_use]
    #[inline]
    pub const fn intersects(self, other: Self) -> bool {
        // Two non-empty rects overlap when, on both axes, each one starts
        // before the other ends.
        !self.is_empty()
            && !other.is_empty()
            && self.min.x < other.max.x
            && other.min.x < self.max.x
            && self.min.y < other.max.y
            && other.min.y < self.max.y
    }

    /// The smallest rect that contains both. An empty rect contains no
    /// point, so it is ignored: `a.union(empty)` is `a`, and the union of
    /// two empty rects is empty.
    #[must_use]
    #[inline]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    fn r(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Rect {
        Rect::new(v(min_x, min_y), v(max_x, max_y))
    }

    /// Every rect whose corner coordinates are 0, 1, 2 or 3, empty and
    /// inverted ones included.
    fn grid_rects() -> Vec<Rect> {
        const COORDS: [f32; 4] = [0.0, 1.0, 2.0, 3.0];
        let mut rects = Vec::new();
        for min_x in COORDS {
            for min_y in COORDS {
                for max_x in COORDS {
                    for max_y in COORDS {
                        rects.push(r(min_x, min_y, max_x, max_y));
                    }
                }
            }
        }
        rects
    }

    /// Bitwise equality, so that rects with NaN coordinates compare equal
    /// to themselves.
    fn same(a: Rect, b: Rect) -> bool {
        let bits = |rect: Rect| [rect.min.x, rect.min.y, rect.max.x, rect.max.y].map(f32::to_bits);
        bits(a) == bits(b)
    }

    #[test]
    fn constructors() {
        let inverted = Rect::new(v(5.0, 8.0), v(1.0, 2.0));
        assert_eq!((inverted.min, inverted.max), (v(5.0, 8.0), v(1.0, 2.0)));

        let expected = r(1.0, 2.0, 5.0, 8.0);
        let corner_pairs = [
            (v(1.0, 2.0), v(5.0, 8.0)),
            (v(5.0, 8.0), v(1.0, 2.0)),
            (v(1.0, 8.0), v(5.0, 2.0)),
            (v(5.0, 2.0), v(1.0, 8.0)),
        ];
        for (a, b) in corner_pairs {
            assert_eq!(Rect::from_corners(a, b), expected, "{a} {b}");
        }
        assert_eq!(Rect::from_min_size(v(1.0, 2.0), v(4.0, 6.0)), expected);
        assert_eq!(Rect::from_center_size(v(3.0, 5.0), v(4.0, 6.0)), expected);
        assert_eq!(Rect::default(), r(0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn from_corners_with_a_nan_corner_is_empty() {
        let nan = f32::NAN;
        let corner_pairs = [
            (v(nan, 0.0), v(1.0, 1.0)),
            (v(0.0, 0.0), v(nan, 1.0)),
            (v(0.0, nan), v(1.0, 1.0)),
            (v(0.0, 0.0), v(1.0, nan)),
        ];
        for (a, b) in corner_pairs {
            assert!(Rect::from_corners(a, b).is_empty(), "{a} {b}");
        }
    }

    #[test]
    fn from_min_size_loses_a_size_below_f32_precision() {
        // At 1e8 the gap between f32 values is 8, so `1e8 + 1` is `1e8`.
        let rect = Rect::from_min_size(v(1e8, 0.0), v(1.0, 1.0));
        assert_eq!(rect.max.x, rect.min.x);
        assert!(rect.is_empty());
    }

    #[test]
    fn measurements() {
        let rect = r(1.0, 2.0, 5.0, 8.0);
        assert_eq!(rect.width(), 4.0);
        assert_eq!(rect.height(), 6.0);
        assert_eq!(rect.size(), v(4.0, 6.0));
        assert_eq!(rect.center(), v(3.0, 5.0));
        assert_eq!(r(5.0, 8.0, 1.0, 2.0).size(), v(-4.0, -6.0));
    }

    #[test]
    fn contains_is_half_open() {
        let rect = r(0.0, 0.0, 4.0, 2.0);
        let inside = [
            v(0.0, 0.0),
            v(2.0, 0.0),
            v(0.0, 1.0),
            v(3.999, 1.999),
            v(-0.0, -0.0),
        ];
        let outside = [
            v(4.0, 0.0),
            v(0.0, 2.0),
            v(4.0, 2.0),
            v(4.0, 1.0),
            v(2.0, 2.0),
            v(-0.001, 1.0),
            v(2.0, -0.001),
            v(f32::NAN, 1.0),
            v(2.0, f32::NAN),
        ];
        for point in inside {
            assert!(rect.contains(point), "{point}");
        }
        for point in outside {
            assert!(!rect.contains(point), "{point}");
        }
    }

    #[test]
    fn empty_rects() {
        let big = r(-10.0, -10.0, 10.0, 10.0);
        let empties = [
            Rect::default(),
            r(0.0, -1.0, 0.0, 1.0),
            r(-1.0, 0.0, 1.0, 0.0),
            r(1.0, -1.0, -1.0, 1.0),
            r(-1.0, 1.0, 1.0, -1.0),
            r(f32::NAN, 0.0, 1.0, 1.0),
            r(0.0, 0.0, 1.0, f32::NAN),
            r(0.0, f32::NAN, 1.0, 1.0),
            r(0.0, 0.0, f32::NAN, 1.0),
            // Far outside `big`, so merging them would grow the union.
            r(100.0, 100.0, 100.0, 100.0),
            r(50.0, 50.0, -50.0, -50.0),
            Rect::from_min_size(v(0.0, 0.0), v(-1.0, 1.0)),
            Rect::from_center_size(v(0.0, 0.0), v(1.0, -1.0)),
        ];
        assert!(!big.is_empty());
        for empty in empties {
            assert!(empty.is_empty(), "{empty:?}");
            assert!(!empty.contains(empty.min), "{empty:?}");
            assert!(!empty.contains(empty.center()), "{empty:?}");
            assert!(!empty.intersects(big), "{empty:?}");
            assert!(!big.intersects(empty), "{empty:?}");
            assert!(!empty.intersects(empty), "{empty:?}");
            assert_eq!(empty.union(big), big, "{empty:?}");
            assert_eq!(big.union(empty), big, "{empty:?}");
            for other in empties {
                let union = empty.union(other);
                assert!(
                    same(union, empty) || same(union, other),
                    "{empty:?} {other:?}"
                );
            }
        }
    }

    #[test]
    fn infinite_bounds() {
        let inf = f32::INFINITY;
        let everything = r(-inf, -inf, inf, inf);
        assert!(!everything.is_empty());
        assert!(everything.contains(v(0.0, 0.0)));
        assert!(everything.contains(v(-inf, -inf)));
        assert!(!everything.contains(v(inf, 0.0)), "the max edge is outside");
        assert!(!everything.contains(v(f32::NAN, 0.0)));
        assert!(everything.intersects(r(0.0, 0.0, 1.0, 1.0)));
        assert_eq!(everything.union(r(0.0, 0.0, 1.0, 1.0)), everything);
    }

    #[test]
    fn intersects() {
        let a = r(0.0, 0.0, 4.0, 4.0);
        let cases = [
            (r(2.0, 2.0, 6.0, 6.0), true),
            (r(1.0, 1.0, 2.0, 2.0), true),
            (r(-1.0, -1.0, 5.0, 5.0), true),
            (r(-1.0, 1.0, 5.0, 2.0), true),
            (a, true),
            (r(4.0, 0.0, 8.0, 4.0), false),
            (r(0.0, -4.0, 4.0, 0.0), false),
            (r(4.0, 4.0, 8.0, 8.0), false),
            (r(5.0, 0.0, 8.0, 4.0), false),
        ];
        for (b, expected) in cases {
            assert_eq!(a.intersects(b), expected, "{b:?}");
            assert_eq!(b.intersects(a), expected, "{b:?}, swapped");
        }
    }

    #[test]
    fn union_is_the_bounding_rect() {
        let a = r(1.0, 1.0, 2.0, 2.0);
        let b = r(5.0, -3.0, 6.0, 1.5);
        let bounds = r(1.0, -3.0, 6.0, 2.0);
        assert_eq!(a.union(b), bounds);
        assert_eq!(b.union(a), bounds);
        assert_eq!(bounds.union(a), bounds);
        // The empty default is a neutral start, so the origin is not pulled in.
        assert_eq!(
            [a, b].into_iter().fold(Rect::default(), Rect::union),
            bounds
        );
    }

    #[test]
    fn methods_agree_with_contains_on_a_grid() {
        // Steps of 0.5 land on every grid edge and inside every grid cell.
        let steps = || (-1_i8..=7).map(|step| f32::from(step) * 0.5);
        let points: Vec<Vec2> = steps()
            .flat_map(|x| steps().map(move |y| v(x, y)))
            .collect();
        let rects = grid_rects();
        for &a in &rects {
            let a_has_point = points.iter().any(|&p| a.contains(p));
            assert_eq!(a.is_empty(), !a_has_point, "{a:?}");
            if a_has_point {
                assert_eq!(a.union(a), a, "{a:?}");
            }
            for &b in &rects {
                let shared = points.iter().any(|&p| a.contains(p) && b.contains(p));
                assert_eq!(a.intersects(b), shared, "{a:?} {b:?}");
                let union = a.union(b);
                let covered: Vec<Vec2> = points
                    .iter()
                    .copied()
                    .filter(|&p| a.contains(p) || b.contains(p))
                    .collect();
                if covered.is_empty() {
                    assert!(union.is_empty(), "{a:?} {b:?}");
                    continue;
                }
                // The bounding box of the covered points. Corners are whole
                // numbers and points step by 0.5, so each max edge is half a
                // step past the last covered point.
                let fold = |init: f32, pick: fn(f32, f32) -> f32, axis: fn(Vec2) -> f32| {
                    covered.iter().map(|&p| axis(p)).fold(init, pick)
                };
                let bounds = r(
                    fold(f32::INFINITY, f32::min, |p| p.x),
                    fold(f32::INFINITY, f32::min, |p| p.y),
                    fold(f32::NEG_INFINITY, f32::max, |p| p.x) + 0.5,
                    fold(f32::NEG_INFINITY, f32::max, |p| p.y) + 0.5,
                );
                assert_eq!(union, bounds, "{a:?} {b:?}");
                assert_eq!(b.union(a), union, "{a:?} {b:?}, swapped");
            }
        }
    }
}
