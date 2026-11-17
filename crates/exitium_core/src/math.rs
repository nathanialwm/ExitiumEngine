//! Math types: glam's vectors and matrices, plus [`Rect`] and [`Color`].
//!
//! The glam types are re-exported under their glam names, so
//! `exitium_core::math::Vec2` *is* `glam::Vec2`, with all of its methods and
//! operators. Engine crates and games import math from here and never depend
//! on glam directly. That keeps one glam version in every build, and it lets
//! `exitium_reflect`, which depends on `exitium_core` alone, register these
//! types under the stable names in `docs/specs/reflect-model.md`.
//!
//! | Types | Scalar | Typical use |
//! |---|---|---|
//! | [`Vec2`], [`Vec3`], [`Vec4`] | `f32` | positions, sizes, velocities, shader values |
//! | [`Mat2`], [`Mat3`], [`Mat4`], [`Affine2`], [`Quat`] | `f32` | 2D transforms, cameras, rotations |
//! | [`IVec2`], [`IVec3`], [`IVec4`] | `i32` | tile and grid coordinates |
//! | [`UVec2`], [`UVec3`], [`UVec4`] | `u32` | texture sizes and pixel coordinates |
//! | [`DVec2`] | `f64` | sim positions that need more precision than `f32` |
//!
//! [`Mat2`] is here because it is the type of [`Affine2`]'s `matrix2` field.
//! glam's other scalar types are not built at all (see the glam entry in the
//! workspace `Cargo.toml`). [`Rect`] and [`Color`] are the 2D types that glam
//! lacks.
//!
//! # SIMD
//!
//! glam uses the SIMD instructions that stable Rust exposes: SSE2 on x86 and
//! x86_64, NEON on aarch64, and `simd128` on wasm32 when that target feature
//! is enabled. Elsewhere it runs scalar code with the same size and alignment
//! for every type, so a value's layout never depends on the target. The two
//! paths can group sums differently (a SIMD `Vec4::dot` adds `(x + z) + (y +
//! w)`), so `f32` results can differ in the last bit between targets.
//!
//! ```
//! use exitium_core::math::{Affine2, Rect, Vec2};
//!
//! let to_world = Affine2::from_translation(Vec2::new(10.0, 0.0));
//! let point = to_world.transform_point2(Vec2::new(1.0, 2.0));
//! assert_eq!(point, Vec2::new(11.0, 2.0));
//!
//! let screen = Rect::from_min_size(Vec2::ZERO, Vec2::new(320.0, 180.0));
//! assert!(screen.contains(point));
//! ```

mod color;
mod rect;

pub use color::{Color, ParseColorError};
pub use rect::Rect;

pub use glam::{
    Affine2, DVec2, IVec2, IVec3, IVec4, Mat2, Mat3, Mat4, Quat, UVec2, UVec3, UVec4, Vec2, Vec3,
    Vec4,
};
