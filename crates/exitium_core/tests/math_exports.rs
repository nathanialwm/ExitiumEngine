//! Checks that every math type is reachable from outside the crate, under
//! `exitium_core::math` and, for the engine's own types, the crate root.

use exitium_core::math;

#[test]
fn glam_types_are_reexported() {
    assert_eq!(math::Vec2::ZERO.x, 0.0_f32);
    assert_eq!(math::Vec3::ZERO.z, 0.0_f32);
    assert_eq!(math::Vec4::ZERO.w, 0.0_f32);
    assert_eq!(math::Mat2::IDENTITY.x_axis, math::Vec2::X);
    assert_eq!(math::Mat3::IDENTITY.z_axis, math::Vec3::Z);
    assert_eq!(math::Mat4::IDENTITY.w_axis, math::Vec4::W);
    assert_eq!(math::Affine2::IDENTITY.matrix2, math::Mat2::IDENTITY);
    assert_eq!(math::Quat::IDENTITY.w, 1.0_f32);
    assert_eq!(math::IVec2::NEG_ONE.x, -1_i32);
    assert_eq!(math::IVec3::NEG_ONE.z, -1_i32);
    assert_eq!(math::IVec4::NEG_ONE.w, -1_i32);
    assert_eq!(math::UVec2::MAX.x, u32::MAX);
    assert_eq!(math::UVec3::MAX.z, u32::MAX);
    assert_eq!(math::UVec4::MAX.w, u32::MAX);
    assert_eq!(math::DVec2::ONE.y, 1.0_f64);
}

#[test]
fn engine_math_types_are_at_the_crate_root() {
    let rect: exitium_core::Rect = math::Rect::default();
    assert!(rect.is_empty());
    let color: exitium_core::Color = math::Color::WHITE;
    assert_eq!(color.to_srgba_u8(), [255; 4]);
    let error: exitium_core::ParseColorError = math::ParseColorError::InvalidLength { len: 2 };
    assert_eq!(math::Color::from_hex("#ff"), Err(error));
}
