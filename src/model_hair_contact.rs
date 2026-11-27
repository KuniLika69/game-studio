use glam::{Mat4, Vec3};

use super::{Model, ModelRegion, RigNode};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum HairContacts {
    #[default]
    Off,
    Head,
    HeadAndShoulders,
}

#[derive(Clone, Debug)]
pub(super) struct HairRig {
    anchors: [Option<HairAnchor>; 4],
}

#[derive(Clone, Copy, Debug)]
struct HairAnchor {
    bone: usize,
    rest_inverse: Mat4,
    center: Vec3,
    radii: Vec3,
}

#[derive(Clone, Copy, Default)]
pub(super) struct HairColliders {
    ellipsoids: [Option<Ellipsoid>; 4],
}

#[derive(Clone, Copy)]
struct Ellipsoid {
    rest: Mat4,
    posed: Mat4,
    center: Vec3,
    radii: Vec3,
}

impl HairRig {
    pub(super) fn prepare(model: &Model) -> Option<Self> {
        let rig = model.rig.as_ref()?;
        let find = |aliases: &[&str]| {
            rig.nodes.iter().enumerate().find(|(_, node)| {
                node.name.as_deref().is_some_and(|name| aliases.contains(&name))
            })
        };
        let tagged_face = model.primitives.iter()
            .filter(|mesh| mesh.region == ModelRegion::StudioFace)
            .flat_map(|mesh| &mesh.vertices);
        let (min, max, count) = tagged_face.fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY), 0_usize),
            |(min, max, count), point| {
                let point = Vec3::from_array(point.position);
                (min.min(point), max.max(point), count + 1)
            },
        );
        if count < 12 {
            return None;
        }
        let bind = |(bone, node): (usize, &RigNode), center, radii| HairAnchor {
            bone,
            rest_inverse: node.rest.inverse(),
            center,
            radii,
        };
        let head = find(&["mixamorig:Head", "head", "head_01"])?;
        let extent = (max - min) * 0.5;
        let center = (min + max) * 0.5 - Vec3::new(0.0, 0.0, extent.z * 0.05);
        let head = bind(head, center, (extent * Vec3::new(0.93, 0.91, 0.90)).max(Vec3::splat(0.025)));
        let height = model.bounds_max[1] - model.bounds_min[1];
        let neck = find(&["mixamorig:Neck", "neck", "neck_01"]).map(|node| {
            let joint = node.1.rest.w_axis.truncate();
            let center = joint.lerp(Vec3::new(center.x, min.y, center.z), 0.46);
            let radii = Vec3::new(0.067, 0.080, 0.059) * (height / 1.72);
            bind(node, center, radii.max(Vec3::splat(0.025)))
        });
        let shoulder = |aliases| {
            let node = find(aliases)?;
            let center = node.1.rest.w_axis.truncate();
            let radii = Vec3::new(0.083, 0.060, 0.079) * (height / 1.72);
            Some(bind(node, center, radii.max(Vec3::splat(0.025))))
        };
        Some(Self {
            anchors: [
                Some(head),
                neck,
                shoulder(&["mixamorig:LeftArm", "upper_arm_l", "upper_arm.L"]),
                shoulder(&["mixamorig:RightArm", "upper_arm_r", "upper_arm.R"]),
            ],
        })
    }

    pub(super) fn pose(&self, globals: &[Mat4], quality: HairContacts) -> HairColliders {
        let limit = match quality {
            HairContacts::Off => 0,
            HairContacts::Head => 1,
            HairContacts::HeadAndShoulders => self.anchors.len(),
        };
        let mut ellipsoids = [None; 4];
        for (slot, anchor) in ellipsoids.iter_mut().zip(self.anchors).take(limit) {
            *slot = anchor.and_then(|anchor| {
                let posed = globals[anchor.bone] * anchor.rest_inverse;
                let rest = posed.inverse();
                rest.is_finite().then_some(Ellipsoid {
                    rest, posed, center: anchor.center, radii: anchor.radii,
                })
            });
        }
        HairColliders { ellipsoids }
    }
}

impl HairColliders {
    pub(super) fn active(self) -> bool {
        self.ellipsoids[0].is_some()
    }

    pub(super) fn fit(self, base: Vec3, mut posed: Vec3) -> Vec3 {
        for _ in 0..2 {
            let mut moved = false;
            for shape in self.ellipsoids.into_iter().flatten() {
                let baseline = (shape.rest.transform_point3(base) - shape.center) / shape.radii;
                let minimum = baseline.length_squared().min(1.0);
                let displaced = (shape.rest.transform_point3(posed) - shape.center) / shape.radii;
                if displaced.length_squared() + 1e-6 < minimum {
                    let direction = displaced.try_normalize()
                        .or_else(|| baseline.try_normalize()).unwrap_or(Vec3::Z);
                    posed = shape.posed.transform_point3(
                        shape.center + direction * minimum.sqrt() * shape.radii
                    );
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        posed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hair_collision_respects_attachment_and_does_not_push_already_fitted_hair() {
        let fit = HairColliders { ellipsoids: [Some(Ellipsoid {
            rest: Mat4::IDENTITY, posed: Mat4::IDENTITY, center: Vec3::ZERO,
            radii: Vec3::new(0.08, 0.10, 0.09),
        }), None, None, None] };
        let base = Vec3::new(0.092, 0.0, 0.0);
        let enters = Vec3::new(0.065, 0.0, 0.0);
        assert!((fit.fit(base, enters).x - 0.08).abs() < 1e-6);
        assert_eq!(fit.fit(base, base), base);
        assert!((fit.fit(enters, Vec3::X * 0.06).x - enters.x).abs() < 1e-6);
        assert_eq!(fit.fit(Vec3::ZERO, Vec3::ZERO), Vec3::ZERO);
    }

    #[test]
    fn both_cast_profiles_prepare_limits_for_live_head_neck_and_shoulder_rotations() {
        use crate::secondary_motion::State;
        let rigs = [
            ("emma", include_bytes!("../demo_assets/emma.glb").as_slice()),
            ("ryan", include_bytes!("../demo_assets/ryan.glb").as_slice()),
        ];
        for (name, bytes) in rigs {
            let model = crate::model::decode_glb(bytes).unwrap();
            let hair = model.hair_rig.as_ref().unwrap();
            assert!(hair.anchors.iter().all(Option::is_some));
            let rig = model.rig.as_ref().unwrap();
            let globals = rig.nodes.iter().map(|joint| joint.rest).collect::<Vec<_>>();
            let high = hair.pose(&globals, HairContacts::HeadAndShoulders);
            let low = hair.pose(&globals, HairContacts::Head);
            assert_eq!(high.ellipsoids.iter().flatten().count(), 4);
            assert_eq!(low.ellipsoids.iter().flatten().count(), 1);
            let mut driver = State::default();
            let dt = 1.0 / 60.0;
            for i in 0..25 {
                driver.advance_in_air(dt, Vec3::Z * 3.5, i as f32 * 7.2, Vec3::X * 10.0);
            }
            for _ in 0..12 {
                driver.advance_in_air(dt, Vec3::ZERO, 180.0, -Vec3::X * 8.0);
            }
            let plain = driver.pose(1.12);
            let fitted = super::super::SecondaryPose { hair_contacts: HairContacts::HeadAndShoulders, ..plain };
            let baselines = model.pose_with_secondary(None, 0.0, &[], plain).unwrap();
            let contact = model.pose_with_secondary(None, 0.0, &[], fitted).unwrap();
            let mut peak = 0.0_f32;
            let mut corrected = 0;
            for (base, output) in baselines.iter().flatten().zip(contact.iter().flatten()) {
                let delta = Vec3::from_array(base.position).distance(Vec3::from_array(output.position));
                peak = peak.max(delta);
                corrected += usize::from(delta >= 0.00005);
                assert!(delta.is_finite() && delta < 0.065);
            }
            eprintln!("{name} changed hair vertices {corrected}; max correction {peak:.5} m");
        }
    }
}
