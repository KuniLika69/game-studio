use crate::{viewport3d::Camera, world::Collider};

/// Choose a nearby orbit with useful clearance and retain it while skimming an obstacle.
pub fn resolve_boom(
    desired: Camera,
    aspect_clearance: impl Fn(Camera, &[Collider]) -> f32,
    colliders: &[Collider],
    elapsed: f32,
    yaw_offset: &mut f32,
    distance: &mut Option<f32>,
) -> Camera {
    let clear_center = aspect_clearance(desired, colliders);
    let min_comfort = (desired.distance * 0.60).min(2.15);
    let choice = if clear_center >= min_comfort {
        0.0
    } else {
        [-0.72, -0.42, -0.23, 0.0, 0.23, 0.42, 0.72]
            .into_iter()
            .map(|offset: f32| {
                let alternative = Camera {
                    yaw: desired.yaw + offset,
                    ..desired
                };
                let clear = aspect_clearance(alternative, colliders).min(min_comfort * 1.22);
                // Favor small adjustments and resist flipping between shoulders.
                let score = clear - offset.abs() * 0.42 - (*yaw_offset - offset).abs() * 0.28;
                (score, offset)
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(0.0, |(_, offset)| offset)
    };
    *yaw_offset += (choice - *yaw_offset) * (1.0 - (-elapsed * 8.2).exp());
    let mut actual = Camera {
        yaw: desired.yaw + *yaw_offset,
        ..desired
    };
    let clearance = aspect_clearance(actual, colliders);
    let prior = distance.unwrap_or(clearance);
    actual.distance = (prior + (clearance - prior) * (1.0 - (-elapsed * 6.0).exp())).min(clearance);
    *distance = Some(actual.distance);
    actual
}
