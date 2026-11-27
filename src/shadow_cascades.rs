use glam::{Mat4, Vec3};

pub(super) const SPLIT: f32 = 18.0;
pub(super) const TRANSITION: f32 = 3.0;
const FAR: f32 = 64.0;

pub(super) fn light_projection(
    target: Vec3,
    extent: f32,
    resolution: u32,
    light_direction: Vec3,
) -> (Mat4, f32) {
    let vertical = if light_direction.y.abs() > 0.97 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let right = vertical.cross(light_direction).normalize();
    let up = light_direction.cross(right);
    let texel = extent * 2.0 / resolution as f32;
    let offset = |axis: Vec3| {
        let coordinate = target.dot(axis);
        axis * ((coordinate / texel).round() * texel - coordinate)
    };
    let center = target + offset(right) + offset(up);
    let distance = extent + 54.0;
    let view = Mat4::look_at_rh(center + light_direction * distance, center, up);
    let projection = Mat4::orthographic_rh(-extent, extent, -extent, extent, 0.1, 2.0 * distance);
    (projection * view, texel)
}

pub(super) fn projections(
    eye: Vec3,
    target: Vec3,
    aspect: f32,
    resolution: u32,
    light_direction: Vec3,
) -> [(Mat4, f32); 2] {
    let forward = (target - eye).normalize();
    let half_fov = 20.0_f32.to_radians();
    let corner_scale = half_fov.tan() * aspect.hypot(1.0);
    [
        (0.04, SPLIT + TRANSITION * 0.5),
        (SPLIT - TRANSITION * 0.5, FAR),
    ]
    .map(|(start, end)| {
        let center = eye + forward * ((start + end) * 0.5);
        // Радиус охватывает дальние углы фрустума и не меняется при поворотах камеры.
        let extent = ((end - start) * 0.5).hypot(corner_scale * end).ceil() + 1.0;
        light_projection(center, extent, resolution, light_direction)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a5_cascades_cover_overlap_and_offer_finer_near_texels() {
        let eye = Vec3::new(3.0, 4.0, 5.0);
        let target = Vec3::ZERO;
        let [near, far] = projections(eye, target, 16.0 / 9.0, 2048, Vec3::Y);
        assert!(near.1 < far.1 && near.1 > 0.0);
        let world = eye + (target - eye).normalize() * SPLIT;
        for (projection, _) in [near, far] {
            let clip = projection.project_point3(world);
            assert!(clip.x.abs() < 1.0 && clip.y.abs() < 1.0 && (0.0..=1.0).contains(&clip.z));
        }
    }

    #[test]
    fn a5_shadow_projection_does_not_shift_for_subtexel_camera_motion() {
        let (old, texel) = light_projection(Vec3::ZERO, 16.0, 2048, Vec3::Y);
        let (shifted, _) = light_projection(Vec3::X * (texel * 0.2), 16.0, 2048, Vec3::Y);
        assert!(old.abs_diff_eq(shifted, 1e-6));
    }

    #[test]
    fn a5_zenith_and_low_sun_projections_are_finite() {
        for sun in [Vec3::Y, Vec3::new(-1.0, 0.12, 0.0).normalize()] {
            for (projection, texel) in projections(Vec3::splat(3.0), Vec3::ZERO, 2.0, 1024, sun) {
                assert!(projection.is_finite() && texel.is_finite());
            }
        }
    }
}
