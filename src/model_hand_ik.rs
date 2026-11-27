use glam::{EulerRot, Mat4, Quat, Vec3};

use super::{BoneRotation, BoneTranslation, Model, Rig, add_bone_translations, model_transforms};

#[derive(Clone, Copy)]
pub(crate) struct ArmPose<'a> {
    pub clip: Option<usize>,
    pub time: f32,
    pub blend: Option<(Option<usize>, f32, f32)>,
    pub translations: &'a [BoneTranslation],
    pub world: Mat4,
}

struct Arm {
    upper: usize,
    lower: usize,
    wrist: usize,
}

const PALM: Vec3 = Vec3::new(0.058, 0.0, 0.0);
const GRIP: &[(&str, f32)] = &[
    ("mixamorig:RightHandIndex1", 24.0), ("mixamorig:RightHandIndex2", 42.0), ("mixamorig:RightHandIndex3", 22.0),
    ("mixamorig:RightHandMiddle1", 29.0), ("mixamorig:RightHandMiddle2", 43.0), ("mixamorig:RightHandMiddle3", 25.0),
    ("mixamorig:RightHandRing1", 33.0), ("mixamorig:RightHandRing2", 46.0), ("mixamorig:RightHandRing3", 28.0),
    ("mixamorig:RightHandPinky1", 35.0), ("mixamorig:RightHandPinky2", 45.0), ("mixamorig:RightHandPinky3", 29.0),
    ("mixamorig:RightHandThumb1", 16.0), ("mixamorig:RightHandThumb2", 21.0), ("mixamorig:RightHandThumb3", 12.0),
];

pub(crate) struct HandMarkers {
    pub palm: Vec3,
    pub wrist: Vec3,
    pub index_tip: Option<Vec3>,
    pub thumb_tip: Option<Vec3>,
}

impl Model {
    pub(crate) fn has_door_hand(&self) -> bool {
        self.rig.as_ref().is_some_and(|rig| find_arm(rig).is_some())
    }

    pub(crate) fn reach_door_handle(
        &self,
        pose: ArmPose<'_>,
        offsets: &mut Vec<BoneRotation>,
        target: Vec3,
        toward: Vec3,
        blend: f32,
    ) -> Option<()> {
        let rig = self.rig.as_ref()?;
        let arm = find_arm(rig)?;
        let frames = self.hand_frames(rig, pose, offsets);
        let shoulder = origin(frames[arm.upper]);
        let elbow = origin(frames[arm.lower]);
        let wrist = origin(frames[arm.wrist]);
        let shoulder_basis = basis(frames[arm.upper])?;
        let elbow_basis = basis(frames[arm.lower])?;
        let wrist_basis = basis(frames[arm.wrist])?;

        let inverse = pose.world.inverse();
        let direction = inverse.transform_vector3(toward).try_normalize()?;
        let palm_orientation =
            Quat::from_rotation_arc(wrist_basis * Vec3::X, direction) * wrist_basis;
        let grip = inverse.transform_point3(target) - palm_orientation * PALM;
        let wanted = wrist.lerp(grip, blend);
        let upper = elbow - shoulder;
        let lower = wrist - elbow;
        let upper_len = upper.length();
        let lower_len = lower.length();
        if upper_len < 0.025 || lower_len < 0.025 || !inverse.is_finite() {
            return None;
        }
        let axis = (wanted - shoulder).try_normalize()?;
        let distance = wanted.distance(shoulder).clamp(
            (upper_len - lower_len).abs() + 0.006,
            upper_len + lower_len - 0.006,
        );
        let along = (upper_len.powi(2) - lower_len.powi(2) + distance.powi(2)) / (2.0 * distance);
        let perpendicular = |point: Vec3| (point - axis * point.dot(axis)).try_normalize();
        let bend = perpendicular(upper).or_else(|| perpendicular(-Vec3::Y))?;
        let solved_upper =
            axis * along + bend * (upper_len.powi(2) - along.powi(2)).max(0.0).sqrt();
        let solved_lower = axis * distance - solved_upper;

        let shoulder_turn =
            Quat::from_rotation_arc(upper.try_normalize()?, solved_upper.try_normalize()?);
        let elbow_turn = Quat::from_rotation_arc(
            (shoulder_turn * lower).try_normalize()?,
            solved_lower.try_normalize()?,
        );
        let inherited_elbow = shoulder_turn * elbow_basis;
        let inherited_wrist = elbow_turn * shoulder_turn * wrist_basis;
        let wrist_goal = wrist_basis.slerp(palm_orientation, blend);
        for (joint, turn) in [
            (
                arm.upper,
                shoulder_basis.conjugate() * shoulder_turn * shoulder_basis,
            ),
            (
                arm.lower,
                inherited_elbow.conjugate() * elbow_turn * inherited_elbow,
            ),
            (arm.wrist, inherited_wrist.conjugate() * wrist_goal),
        ] {
            let rotation = turn.normalize().to_euler(EulerRot::XYZ);
            offsets.push(BoneRotation {
                bone: rig.nodes[joint].name.clone()?,
                degrees: [rotation.0, rotation.1, rotation.2].map(f32::to_degrees),
            });
        }
        let curl = ((blend - 0.56) / 0.44).clamp(0.0, 1.0);
        let curl = curl * curl * (3.0 - 2.0 * curl);
        if curl > 0.0 {
            for &(bone, degrees) in GRIP {
                if rig.nodes.iter().any(|node| node.name.as_deref() == Some(bone)) {
                    offsets.push(BoneRotation {
                        bone: bone.into(),
                        degrees: [0.0, 0.0, -degrees * curl],
                    });
                }
            }
        }
        Some(())
    }

    pub(crate) fn hand_markers(&self, pose: ArmPose<'_>, offsets: &[BoneRotation]) -> Option<HandMarkers> {
        let rig = self.rig.as_ref()?;
        let arm = find_arm(rig)?;
        let frames = self.hand_frames(rig, pose, offsets);
        let transform = |joint: usize, point| pose.world.transform_point3(frames[joint].transform_point3(point));
        let tip = |name| {
            let index = rig.nodes.iter().position(|node| node.name.as_deref() == Some(name))?;
            Some(transform(index, Vec3::X * 0.013))
        };
        Some(HandMarkers {
            palm: transform(arm.wrist, PALM),
            wrist: transform(arm.wrist, Vec3::ZERO),
            index_tip: tip("mixamorig:RightHandIndex3"),
            thumb_tip: tip("mixamorig:RightHandThumb3"),
        })
    }

    pub(crate) fn door_shoulder(&self, pose: ArmPose<'_>) -> Option<Vec3> {
        let rig = self.rig.as_ref()?;
        let arm = find_arm(rig)?;
        let frames = self.hand_frames(rig, pose, &[]);
        Some(pose.world.transform_point3(origin(frames[arm.upper])))
    }

    fn hand_frames(&self, rig: &Rig, pose: ArmPose<'_>, offsets: &[BoneRotation]) -> Vec<Mat4> {
        let mut local = self.blended_locals(
            rig,
            pose.clip.and_then(|index| self.animations.get(index)),
            pose.time,
            pose.blend,
            offsets,
        );
        add_bone_translations(rig, &mut local, pose.translations);
        model_transforms(rig, &local)
    }
}

fn find_arm(rig: &Rig) -> Option<Arm> {
    let joint = |aliases: &[&str]| {
        rig.skins
            .iter()
            .flat_map(|skin| skin.joints.iter().map(|joint| joint.0))
            .find(|&index| {
                rig.nodes[index]
                    .name
                    .as_deref()
                    .is_some_and(|name| aliases.contains(&name))
            })
    };
    let upper = joint(&["mixamorig:RightArm", "upper_arm_r", "upper_arm.R"])?;
    let lower = joint(&["mixamorig:RightForeArm", "forearm_r", "forearm.R"])?;
    let wrist = joint(&["mixamorig:RightHand", "hand_r", "hand.R"])?;
    (rig.nodes[lower].parent == Some(upper) && rig.nodes[wrist].parent == Some(lower)).then_some(
        Arm {
            upper,
            lower,
            wrist,
        },
    )
}

fn origin(frame: Mat4) -> Vec3 {
    frame.w_axis.truncate()
}

fn basis(frame: Mat4) -> Option<Quat> {
    let x = frame.x_axis.truncate().try_normalize()?;
    let z = x.cross(frame.y_axis.truncate()).try_normalize()?;
    let y = z.cross(x);
    Some(Quat::from_mat3(&glam::Mat3::from_cols(x, y, z)))
}
