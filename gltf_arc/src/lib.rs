pub use traversal::*;
pub use types::*;

pub mod canonicalize;
pub mod combiner;
pub mod from_raw;
pub mod primitive_builder;
pub mod to_raw;
pub mod traversal;
pub mod types;

/// Round-trip the matrix through f32 and back to f64 to ensure it will
/// serialize f32-friendly through glTF JSON
pub fn snap_dmat4_to_f32(matrix: glam::DMat4) -> glam::DMat4 {
    // Snap translation directly to f32, then back to f64
    let translation = matrix.w_axis.truncate();
    let snapped_translation = glam::DVec3::new(
        translation.x as f32 as f64,
        translation.y as f32 as f64,
        translation.z as f32 as f64,
    );

    // Turn rotation into an f32 quat as f64 and renormalize, don't just cast
    // the matrix values, to ensure the result is orthonormal
    let rotation = glam::DMat3::from_mat4(matrix);
    let quat = glam::DQuat::from_mat3(&rotation);
    let snapped_quat = glam::DQuat::from_xyzw(
        quat.x as f32 as f64,
        quat.y as f32 as f64,
        quat.z as f32 as f64,
        quat.w as f32 as f64,
    )
    .normalize();

    // Reassemble the matrix
    glam::DMat4::from_rotation_translation(snapped_quat, snapped_translation)
}
