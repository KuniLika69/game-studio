use glam::{EulerRot, Mat4, Quat, Vec3, Vec4};

use super::{
    Animation, BoneRotation, BoneTranslation, LocalTransform, Model, Rig, animation_pose,
    model_transforms,
};

const SIDES: usize = 2;

/// Samples the same pose as the renderer, before adding the returned bone rotations.
pub struct FootLockInput<'a> {
    pub model: &'a Model,
    pub identity: u64,
    pub clip: Option<usize>,
    pub elapsed: f32,
    pub previous: Option<(Option<usize>, f32, f32)>,
    pub offsets: &'a [BoneRotation],
    pub world_from_model: Mat4,
    pub ground_height: f32,
    pub dt: f32,
    pub enabled: bool,
}

#[derive(Default)]
pub struct FootLock {
    pub(crate) quadruped: super::PawLock,
    identity: Option<u64>,
    legs: [Option<Leg>; SIDES],
    cycles: Vec<Option<GaitCycle>>,
    contacts: [Contact; SIDES],
    translations: Vec<BoneTranslation>,
    pelvis: Option<Pelvis>,
    pelvis_drop: f32,
    plant_events: [Option<Vec3>; SIDES],
}

struct Pelvis {
    joint: usize,
    descendants: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Leg {
    upper: usize,
    lower: usize,
    ankle: usize,
    sole_marker: Vec3,
    sole_forward: Vec3,
    knee_follows_upper: bool,
    ankle_follows_lower: bool,
    ankle_follows_upper: bool,
}

struct GaitCycle {
    duration: f32,
    weights: Vec<[f32; SIDES]>,
    soles: Vec<[Vec3; SIDES]>,
    stance_speed: Option<f32>,
}

#[derive(Clone, Copy, Default)]
struct Contact {
    anchor: Option<Anchor>,
    weight: f32,
}

#[derive(Clone, Copy)]
struct Anchor {
    world_position: Vec3,
    world_rotation: Quat,
}

#[derive(Clone, Copy)]
struct LegTarget {
    position: Vec3,
    rotation: Quat,
    thigh: Vec3,
    shin: Vec3,
}

impl FootLock {
    pub fn has_plant_feet(&self) -> bool {
        self.legs.iter().all(Option::is_some)
    }

    pub fn reset(&mut self) {
        // Gait analysis is derived from the asset and can be reused on the next start.
        self.contacts = [Contact::default(); SIDES];
        self.translations.clear();
        self.pelvis_drop = 0.0;
        self.plant_events = [None; SIDES];
        self.quadruped.reset();
    }

    pub fn take_plants(&mut self) -> [Option<Vec3>; SIDES] {
        std::mem::take(&mut self.plant_events)
    }

    /// The last solve's local pelvis and detached-joint offsets.
    pub fn translations(&self) -> &[BoneTranslation] {
        &self.translations
    }

    fn prepare_model(&mut self, model: &Model, rig: &Rig, identity: u64) {
        if self.identity != Some(identity) || self.cycles.len() != model.animations.len() {
            self.identity = Some(identity);
            self.legs = find_legs(rig);
            self.pelvis = find_pelvis(rig, self.legs);
            self.cycles.clear();
            self.cycles.resize_with(model.animations.len(), || None);
            self.reset();
        }
    }

    pub fn clip_stride_speed(&mut self, model: &Model, identity: u64, clip: usize) -> Option<f32> {
        let rig = model.rig.as_deref()?;
        let animation = model.animations.get(clip)?;
        self.prepare_model(model, rig, identity);
        if !self.has_plant_feet() {
            return None;
        }
        let height = (model.bounds_max[1] - model.bounds_min[1]).abs();
        self.cycles[clip]
            .get_or_insert_with(|| GaitCycle::measure(rig, animation, self.legs, height))
            .stance_speed
    }

    /// Match the pose on the same left and right soles at the start of a new clip.
    /// Returning a clip time keeps the result shared by rendering, IK and footfall events.
    pub fn transition_time(
        &mut self,
        model: &Model,
        identity: u64,
        from: Option<usize>,
        from_time: f32,
        to: usize,
        fallback: f32,
    ) -> Option<f32> {
        let rig = model.rig.as_deref()?;
        if !from_time.is_finite() || !fallback.is_finite() || to >= model.animations.len() {
            return None;
        }
        self.prepare_model(model, rig, identity);
        if !self.has_plant_feet() {
            return None;
        }
        let height = (model.bounds_max[1] - model.bounds_min[1]).abs().max(0.01);
        let from = from.filter(|index| *index < self.cycles.len());
        for index in from.into_iter().chain(std::iter::once(to)) {
            self.cycles[index].get_or_insert_with(|| {
                GaitCycle::measure(rig, &model.animations[index], self.legs, height)
            });
        }
        let source = from.and_then(|index| self.cycles[index].as_ref());
        let outgoing = source
            .and_then(|cycle| cycle.sole_pose(from_time))
            .unwrap_or_else(|| {
                self.legs.map(|leg| {
                    let leg = leg.expect("both legs are available");
                    rig.nodes[leg.ankle].rest.transform_point3(leg.sole_marker)
                })
            });
        let source_pressure = source.map_or([0.0; SIDES], |cycle| cycle.at(from_time));
        let destination = self.cycles[to].as_ref()?;
        if destination.soles.is_empty() {
            return None;
        }
        let regularization = (height * 0.026).powi(2);
        let pressure_cost = (height * 0.036).powi(2);
        let phase_hint = fallback.rem_euclid(destination.duration) / destination.duration;
        let score = |index: usize, feet: [Vec3; SIDES]| {
            let markers = (0..SIDES)
                .map(|side| {
                    let offset = outgoing[side] - feet[side];
                    let pressure = source_pressure[side] - destination.weights[index][side];
                    offset.length_squared()
                        + 0.8 * offset.y * offset.y
                        + pressure_cost * pressure * pressure
                })
                .sum::<f32>();
            let phase = index as f32 / destination.soles.len() as f32;
            let distance = (phase - phase_hint).abs();
            markers + regularization * distance.min(1.0 - distance).powi(2)
        };
        let index = destination
            .soles
            .iter()
            .copied()
            .enumerate()
            .map(|(index, feet)| (index, score(index, feet)))
            .filter(|(_, score)| score.is_finite())
            .min_by(|(_, a), (_, b)| a.total_cmp(b))?
            .0;
        Some(index as f32 * destination.duration / destination.soles.len() as f32)
    }

    pub fn corrections(&mut self, input: FootLockInput<'_>) -> Vec<BoneRotation> {
        self.translations.clear();
        let Some(rig) = input.model.rig.as_deref() else {
            self.reset();
            return Vec::new();
        };
        if !input.enabled || !input.dt.is_finite() || !input.ground_height.is_finite() {
            self.reset();
            return Vec::new();
        }
        self.prepare_model(input.model, rig, input.identity);
        if self.legs.iter().all(Option::is_none) {
            return Vec::new();
        }

        let Some(world_rotation) = frame_rotation(input.world_from_model) else {
            self.reset();
            return Vec::new();
        };
        let inverse = input.world_from_model.inverse();
        let sole_y = input
            .world_from_model
            .transform_point3(Vec3::new(0.0, input.model.bounds_min[1], 0.0))
            .y;
        let world_height = (input.model.bounds_max[1] - input.model.bounds_min[1]).abs()
            * input.world_from_model.y_axis.truncate().length();
        let reach = (world_height * 0.36).clamp(0.20, 1.3);
        if !inverse.is_finite() || (sole_y - input.ground_height).abs() > 0.16 {
            self.reset();
            return Vec::new();
        }

        let model_height = (input.model.bounds_max[1] - input.model.bounds_min[1]).abs();
        let mut weights = self.gait_at(input.model, rig, input.clip, input.elapsed, model_height);
        if let Some((previous, time, blend)) = input.previous
            && blend.is_finite()
            && blend < 1.0
        {
            let old = self.gait_at(input.model, rig, previous, time, model_height);
            let blend = blend.clamp(0.0, 1.0);
            weights = std::array::from_fn(|side| old[side] + (weights[side] - old[side]) * blend);
        }
        if weights.iter().all(|&weight| weight <= 0.001)
            && self.contacts.iter().all(|contact| contact.anchor.is_none())
            && self.pelvis_drop <= 0.0001
        {
            return Vec::new();
        }

        let locals = input.model.blended_locals(
            rig,
            input.clip.and_then(|clip| input.model.animations.get(clip)),
            input.elapsed,
            input.previous,
            input.offsets,
        );
        let mut globals = model_transforms(rig, &locals);
        let mut targets = [None; SIDES];
        let dt = input.dt.clamp(0.0, 0.055);
        for (side, leg) in self.legs.into_iter().enumerate() {
            let Some(leg) = leg else { continue };
            let ankle = globals[leg.ankle];
            let Some(ankle_rotation) = frame_rotation(ankle) else {
                continue;
            };
            let marker_offset = ankle.transform_vector3(leg.sole_marker);
            let marker = origin(ankle) + marker_offset;
            let world_marker = input.world_from_model.transform_point3(marker);
            let authored_rotation = world_rotation * ankle_rotation;
            let contact = &mut self.contacts[side];
            let mut confidence = smooth(0.21, 0.78, weights[side]);
            if let Some(anchor) = contact.anchor {
                confidence *= 1.0
                    - smooth(
                        reach * 0.78,
                        reach,
                        anchor.world_position.distance(world_marker),
                    );
            } else if confidence > 0.48 && dt > 0.0 {
                let planted =
                    world_marker + Vec3::Y * (input.ground_height - sole_y).clamp(-0.045, 0.045);
                contact.anchor = Some(Anchor {
                    world_position: planted,
                    world_rotation: authored_rotation,
                });
                self.plant_events[side] = Some(planted);
            }
            let limit = if confidence > contact.weight {
                dt / 0.048
            } else {
                dt / 0.082
            };
            contact.weight += (confidence - contact.weight).clamp(-limit, limit);
            if contact.weight <= 0.001 && confidence <= 0.001 {
                *contact = Contact::default();
            }
            let Some(anchor) = contact.anchor.as_mut() else {
                continue;
            };
            // Retain the support point and ankle tilt while the planted sole swivels.
            let from = anchor.world_rotation * leg.sole_forward;
            let to = authored_rotation * leg.sole_forward;
            let yaw = from.cross(to).y.atan2(from.x * to.x + from.z * to.z);
            let swivel = smooth(12.0_f32.to_radians(), 27.0_f32.to_radians(), yaw.abs());
            let yaw_step = yaw * swivel * (1.0 - (-dt * 38.0).exp());
            anchor.world_rotation = Quat::from_rotation_y(yaw_step) * anchor.world_rotation;
            let rotation = ankle_rotation.slerp(
                world_rotation.conjugate() * anchor.world_rotation,
                contact.weight * 0.58,
            );
            // Roll the foot around the planted sole instead of dragging the sole around a fixed ankle.
            let planted_marker = marker.lerp(
                inverse.transform_point3(anchor.world_position),
                contact.weight,
            );
            let position = planted_marker - rotation * (ankle_rotation.conjugate() * marker_offset);
            let hip = origin(globals[leg.upper]);
            let knee = origin(globals[leg.lower]);
            targets[side] = Some(LegTarget {
                position,
                rotation,
                thigh: knee - hip,
                shin: origin(ankle) - knee,
            });
        }
        self.adjust_pelvis(rig, &mut globals, targets, model_height, dt);
        let mut result = Vec::with_capacity(6);
        for (leg, target) in self.legs.into_iter().zip(targets) {
            if let (Some(leg), Some(target)) = (leg, target)
                && let Some(offsets) = solve_leg(rig, leg, &globals, target)
            {
                result.extend(offsets.rotations);
                self.translations
                    .extend(offsets.translations.into_iter().flatten());
            }
        }
        result
    }

    fn adjust_pelvis(
        &mut self,
        rig: &Rig,
        globals: &mut [Mat4],
        targets: [Option<LegTarget>; SIDES],
        height: f32,
        dt: f32,
    ) {
        let Some(pelvis) = &self.pelvis else { return };
        let mut required_drop: f32 = 0.0;
        for (leg, target) in self.legs.into_iter().zip(targets) {
            if let (Some(leg), Some(target)) = (leg, target) {
                let delta = origin(globals[leg.upper]) - target.position;
                let reach = target.thigh.length() + target.shin.length() - 0.002;
                let vertical_reach = (reach * reach - delta.x * delta.x - delta.z * delta.z)
                    .max(0.0)
                    .sqrt();
                required_drop = required_drop.max(delta.y - vertical_reach);
            }
        }
        let target_drop = required_drop.min(height * 0.09);
        let speed = if target_drop > self.pelvis_drop {
            70.0
        } else {
            17.0
        };
        self.pelvis_drop += (target_drop - self.pelvis_drop) * (1.0 - (-dt * speed).exp());
        if self.pelvis_drop <= 0.0001 {
            self.pelvis_drop = 0.0;
            return;
        }
        let drop = Vec3::NEG_Y * self.pelvis_drop;
        if let Some(offset) = local_translation(rig, globals, pelvis.joint, drop) {
            self.translations.push(offset);
            for &index in &pelvis.descendants {
                globals[index].w_axis += Vec4::from((drop, 0.0));
            }
        }
    }

    fn gait_at(
        &mut self,
        model: &Model,
        rig: &Rig,
        clip: Option<usize>,
        elapsed: f32,
        height: f32,
    ) -> [f32; SIDES] {
        let Some(index) = clip.filter(|&index| index < self.cycles.len()) else {
            return [0.0; SIDES];
        };
        self.cycles[index]
            .get_or_insert_with(|| {
                GaitCycle::measure(rig, &model.animations[index], self.legs, height)
            })
            .at(elapsed)
    }
}

impl GaitCycle {
    fn measure(rig: &Rig, clip: &Animation, legs: [Option<Leg>; SIDES], height: f32) -> Self {
        let duration = clip.duration;
        let mut result = Self {
            duration,
            weights: Vec::new(),
            soles: Vec::new(),
            stance_speed: None,
        };
        if !duration.is_finite() || duration < 0.18 {
            return result;
        }
        let count = ((duration * 64.0).ceil() as usize).clamp(48, 256);
        let interval = duration / count as f32;
        let sampled: Vec<[Vec3; SIDES]> = (0..count)
            .map(|key| {
                let locals = animation_pose(rig, Some(clip), key as f32 * interval);
                let bones = model_transforms(rig, &locals);
                result.soles.push(legs.map(|leg| {
                    leg.map_or(Vec3::ZERO, |leg| {
                        bones[leg.ankle].transform_point3(leg.sole_marker)
                    })
                }));
                legs.map(|leg| leg.map_or(Vec3::ZERO, |leg| origin(bones[leg.ankle])))
            })
            .collect();
        result.weights.resize(count, [0.0; SIDES]);
        let mut stance_speeds = Vec::with_capacity(count);
        for side in 0..SIDES {
            let mut levels: Vec<_> = sampled.iter().map(|feet| feet[side].y).collect();
            levels.sort_by(f32::total_cmp);
            let low = levels[count / 12];
            let high = levels[count * 23 / 24];
            let swing_height = high - low;
            let (min_z, max_z) = sampled
                .iter()
                .map(|feet| feet[side].z)
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), z| {
                    (min.min(z), max.max(z))
                });
            if max_z - min_z < height * 0.035 || swing_height < height * 0.014 {
                continue;
            }
            let lags: Vec<_> = (0..count)
                .map(|key| {
                    (sampled[(key + count - 1) % count][side].z
                        - sampled[(key + 1) % count][side].z)
                        / (2.0 * interval)
                })
                .collect();
            let strongest = lags.iter().copied().fold(0.0_f32, f32::max);
            for key in 0..count {
                let pressure = 1.0
                    - smooth(
                        low + (swing_height * 0.14).max(height * 0.008),
                        low + (swing_height * 0.43).max(height * 0.025),
                        sampled[key][side].y,
                    );
                let backwards = smooth(-strongest * 0.025, strongest * 0.24 + 0.0001, lags[key]);
                result.weights[key][side] = pressure * backwards;
                if result.weights[key][side] >= 0.82 {
                    let before = result.soles[(key + count - 1) % count][side];
                    let after = result.soles[(key + 1) % count][side];
                    let speed = (before.z - after.z) / (2.0 * interval);
                    if (height * 0.12..=height * 5.5).contains(&speed) {
                        stance_speeds.push(speed);
                    }
                }
            }
        }
        if stance_speeds.len() >= 8 {
            stance_speeds.sort_by(f32::total_cmp);
            result.stance_speed = Some(stance_speeds[stance_speeds.len() / 2]);
        }
        result
    }

    fn at(&self, elapsed: f32) -> [f32; SIDES] {
        let Some((from, to, mix)) = self.frames(elapsed) else {
            return [0.0; SIDES];
        };
        std::array::from_fn(|side| {
            self.weights[from][side] + (self.weights[to][side] - self.weights[from][side]) * mix
        })
    }

    fn sole_pose(&self, elapsed: f32) -> Option<[Vec3; SIDES]> {
        let (from, to, mix) = self.frames(elapsed)?;
        Some(std::array::from_fn(|side| {
            self.soles[from][side].lerp(self.soles[to][side], mix)
        }))
    }

    fn frames(&self, elapsed: f32) -> Option<(usize, usize, f32)> {
        let count = self.weights.len();
        if count == 0 || !elapsed.is_finite() {
            return None;
        }
        let time = elapsed.rem_euclid(self.duration) / self.duration * count as f32;
        let from = time.floor() as usize % count;
        Some((from, (from + 1) % count, time.fract()))
    }
}

fn find_legs(rig: &Rig) -> [Option<Leg>; SIDES] {
    let joints: Vec<_> = rig
        .skins
        .iter()
        .flat_map(|skin| skin.joints.iter().map(|joint| joint.0))
        .collect();
    let names: Vec<_> = rig
        .nodes
        .iter()
        .map(|joint| normalized_name(joint.name.as_deref().unwrap_or_default()))
        .collect();
    std::array::from_fn(|side| {
        let (suffix, prefix) = if side == 0 {
            ("l", "left")
        } else {
            ("r", "right")
        };
        let matches = |node: usize, roots: &[&str]| {
            let name = &names[node];
            roots.iter().any(|root| {
                *name == format!("{root}{suffix}")
                    || *name == format!("{prefix}{root}")
                    || *name == format!("{root}{prefix}")
            })
        };
        let ankle = joints
            .iter()
            .copied()
            .find(|&joint| matches(joint, &["foot", "ankle"]))?;
        let ancestor = |start: usize, roots: &[&str]| {
            let mut cursor = rig.nodes[start].parent;
            while let Some(index) = cursor {
                if joints.contains(&index) && matches(index, roots) {
                    return Some(index);
                }
                cursor = rig.nodes[index].parent;
            }
            None
        };
        let lower_names = ["shin", "calf", "lowerleg", "leg", "knee"];
        let upper_names = ["thigh", "upperleg", "upleg", "femur", "hip"];
        let named = |roots: &[&str]| joints.iter().copied().find(|&joint| matches(joint, roots));
        // Maya's foot transforms are baked outside the usual thigh/calf chain.
        let lower = ancestor(ankle, &lower_names).or_else(|| named(&lower_names))?;
        let upper = ancestor(lower, &upper_names).or_else(|| named(&upper_names))?;
        let toe = named(&["toebase", "toe", "toes", "ball"])
            .filter(|&toe| descends_from(rig, toe, ankle))
            .map(|toe| origin(rig.nodes[toe].rest));
        let ankle_frame = rig.nodes[ankle].rest;
        let ankle_origin = origin(ankle_frame);
        let sole_marker = toe
            .and_then(|toe| {
                let mut midpoint = ankle_origin.lerp(toe, 0.55);
                midpoint.y = toe.y;
                let local = ankle_frame.inverse().transform_point3(midpoint);
                local.is_finite().then_some(local)
            })
            .unwrap_or(Vec3::ZERO);
        let toe_vector = toe.map_or(Vec3::Z, |toe| toe - ankle_origin);
        let model_forward = Vec3::new(toe_vector.x, 0.0, toe_vector.z)
            .try_normalize()
            .unwrap_or(Vec3::Z);
        let sole_forward = frame_rotation(ankle_frame)?.conjugate() * model_forward;
        [upper, lower, ankle]
            .iter()
            .all(|&index| matches!(rig.nodes[index].local, LocalTransform::Trs { .. }))
            .then_some(Leg {
                upper,
                lower,
                ankle,
                sole_marker,
                sole_forward,
                knee_follows_upper: descends_from(rig, lower, upper),
                ankle_follows_lower: descends_from(rig, ankle, lower),
                ankle_follows_upper: descends_from(rig, ankle, upper),
            })
    })
}

fn descends_from(rig: &Rig, node: usize, source: usize) -> bool {
    let mut cursor = rig.nodes[node].parent;
    while let Some(index) = cursor {
        if index == source {
            return true;
        }
        cursor = rig.nodes[index].parent;
    }
    false
}

fn find_pelvis(rig: &Rig, legs: [Option<Leg>; SIDES]) -> Option<Pelvis> {
    let joint = rig.nodes.iter().enumerate().find_map(|(index, node)| {
        let name = normalized_name(node.name.as_deref()?);
        (matches!(name.as_str(), "pelvis" | "hips" | "hip")
            && matches!(node.local, LocalTransform::Trs { .. })
            && rig
                .skins
                .iter()
                .any(|skin| skin.joints.iter().any(|joint| joint.0 == index))
            && legs
                .into_iter()
                .flatten()
                .all(|leg| descends_from(rig, leg.upper, index)))
        .then_some(index)
    })?;
    let descendants = (0..rig.nodes.len())
        .filter(|&index| index == joint || descends_from(rig, index, joint))
        .collect();
    Some(Pelvis { joint, descendants })
}

fn normalized_name(name: &str) -> String {
    let name = name.rsplit([':', '|']).next().unwrap_or(name);
    let value: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    value
        .strip_prefix("mixamorig")
        .or_else(|| value.strip_prefix("def"))
        .unwrap_or(&value)
        .to_owned()
}

struct LegCorrections {
    rotations: [BoneRotation; 3],
    translations: [Option<BoneTranslation>; 2],
}

fn solve_leg(rig: &Rig, leg: Leg, globals: &[Mat4], target: LegTarget) -> Option<LegCorrections> {
    let [upper_frame, lower_frame, ankle_frame] =
        [globals[leg.upper], globals[leg.lower], globals[leg.ankle]];
    let [hip, knee, ankle] = [upper_frame, lower_frame, ankle_frame].map(origin);
    let LegTarget {
        position,
        rotation,
        thigh,
        shin,
    } = target;
    let a = thigh.length();
    let b = shin.length();
    let lower_distance = (a - b).abs() + 0.0005;
    let upper_distance = a + b - 0.0005;
    if lower_distance >= upper_distance || !position.is_finite() {
        return None;
    }
    let toward = (position - hip).try_normalize()?;
    let distance = (position - hip)
        .length()
        .clamp(lower_distance, upper_distance);
    // Carry the authored knee's bend plane with the leg axis. Projecting it onto
    // the new axis can collapse the bend and send the knee sideways when pivoting.
    let raw_axis = (thigh + shin).try_normalize()?;
    let authored_bend = thigh - raw_axis * thigh.dot(raw_axis);
    let forward_bend = Vec3::Z - raw_axis * raw_axis.z;
    let bend_strength = smooth(a * 0.008, a * 0.04, authored_bend.length());
    let pole = forward_bend
        .normalize_or_zero()
        .lerp(authored_bend.normalize_or_zero(), bend_strength);
    let fallback = Vec3::X - raw_axis * raw_axis.x;
    let bend = Quat::from_rotation_arc(raw_axis, toward)
        * pole.try_normalize().or_else(|| fallback.try_normalize())?;
    let along = (a * a - b * b + distance * distance) / (2.0 * distance);
    let sideways = (a * a - along * along).max(0.0).sqrt();
    let desired_thigh = toward * along + bend * sideways;
    let desired_shin = toward * distance - desired_thigh;
    let turn_upper =
        Quat::from_rotation_arc(thigh.try_normalize()?, desired_thigh.try_normalize()?);
    let parent_turn = if leg.knee_follows_upper {
        turn_upper
    } else {
        Quat::IDENTITY
    };
    let turn_lower = Quat::from_rotation_arc(
        (parent_turn * shin).try_normalize()?,
        desired_shin.try_normalize()?,
    );

    // Convert the model-space rotations into post-multiplied, local joint corrections.
    let q_upper = frame_rotation(upper_frame)?;
    let q_lower = parent_turn * frame_rotation(lower_frame)?;
    let ankle_inheritance = if leg.ankle_follows_lower {
        turn_lower * parent_turn
    } else if leg.ankle_follows_upper {
        turn_upper
    } else {
        Quat::IDENTITY
    };
    let q_ankle = ankle_inheritance * frame_rotation(ankle_frame)?;
    let corrections = [
        q_upper.conjugate() * turn_upper * q_upper,
        q_lower.conjugate() * turn_lower * q_lower,
        q_ankle.conjugate() * rotation,
    ];
    let make = |index: usize, rotation: Quat| -> Option<BoneRotation> {
        let rotation = rotation.normalize();
        let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
        Some(BoneRotation {
            bone: rig.nodes[index].name.clone()?,
            degrees: [x, y, z].map(f32::to_degrees),
        })
    };
    let lower_position = if leg.knee_follows_upper {
        None
    } else {
        Some(local_translation(
            rig,
            globals,
            leg.lower,
            hip + desired_thigh - knee,
        )?)
    };
    let ankle_position = if leg.ankle_follows_lower {
        None
    } else {
        let inherited_position = hip + ankle_inheritance * (ankle - hip);
        Some(local_translation(
            rig,
            globals,
            leg.ankle,
            ankle_inheritance.conjugate()
                * (hip + desired_thigh + desired_shin - inherited_position),
        )?)
    };
    Some(LegCorrections {
        rotations: [
            make(leg.upper, corrections[0])?,
            make(leg.lower, corrections[1])?,
            make(leg.ankle, corrections[2])?,
        ],
        translations: [lower_position, ankle_position],
    })
}

fn local_translation(
    rig: &Rig,
    globals: &[Mat4],
    index: usize,
    delta: Vec3,
) -> Option<BoneTranslation> {
    let parent = rig.nodes[index]
        .parent
        .map_or(Mat4::IDENTITY, |index| globals[index]);
    let correction = parent.inverse().transform_vector3(delta);
    if !correction.is_finite() {
        return None;
    }
    Some(BoneTranslation {
        bone: rig.nodes[index].name.clone()?,
        translation: correction.to_array(),
    })
}

fn origin(matrix: Mat4) -> Vec3 {
    matrix.w_axis.truncate()
}

fn frame_rotation(matrix: Mat4) -> Option<Quat> {
    if !matrix.is_finite() || matrix.determinant() <= 1e-12 {
        return None;
    }
    let (_, rotation, _) = matrix.to_scale_rotation_translation();
    rotation.is_finite().then(|| rotation.normalize())
}

fn smooth(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use crate::egui;

    use super::super::decode_glb;
    use super::*;

    #[test]
    fn gait_transition_lines_up_soles_on_two_live_rigs() {
        for bytes in [
            include_bytes!("../demo_assets/emma.glb").as_slice(),
            include_bytes!("../demo_assets/ryan.glb").as_slice(),
        ] {
            let model = decode_glb(bytes).unwrap();
            let rig = model.rig.as_deref().unwrap();
            let legs = find_legs(rig).map(Option::unwrap);
            let clip = |needle: &str| {
                model
                    .animations
                    .iter()
                    .position(|clip| clip.name.to_ascii_lowercase().contains(needle))
                    .unwrap()
            };
            let [idle, walking, running] = ["idle", "walk", "run"].map(clip);
            let feet = |clip: usize, time| {
                let locals = animation_pose(rig, Some(&model.animations[clip]), time);
                let bones = model_transforms(rig, &locals);
                legs.map(|leg| bones[leg.ankle].transform_point3(leg.sole_marker))
            };
            let mut feet_lock = FootLock::default();
            for (from, to) in [(idle, walking), (walking, running), (running, idle)] {
                let from_duration = model.animations[from].duration;
                let to_duration = model.animations[to].duration;
                let mut before = 0.0;
                let mut after = 0.0;
                for sample in 0..12 {
                    let phase = sample as f32 / 12.0;
                    let elapsed = phase * from_duration;
                    let fallback = if from == idle || to == idle {
                        0.0
                    } else {
                        phase * to_duration
                    };
                    let selected = feet_lock
                        .transition_time(&model, 1, Some(from), elapsed, to, fallback)
                        .expect("recognized sole positions");
                    assert!((0.0..to_duration).contains(&selected));
                    let outgoing = feet(from, elapsed);
                    let distance = |clock| {
                        let incoming = feet(to, clock);
                        outgoing[0].distance(incoming[0]) + outgoing[1].distance(incoming[1])
                    };
                    before += distance(fallback);
                    after += distance(selected);
                }
                let limit = if to == idle { 1.005 } else { 0.98 };
                assert!(
                    after < before * limit,
                    "{} → {} sole displacement {before:.3} → {after:.3}",
                    model.animations[from].name,
                    model.animations[to].name,
                );
                println!(
                    "{} → {} sole displacement {before:.3} → {after:.3}",
                    model.animations[from].name, model.animations[to].name,
                );
            }
        }
    }

    fn assert_reduced_sliding(model: &Model, clip_name: &str, speed: f32, rate: f32) {
        let rig = model.rig.as_deref().unwrap();
        let legs =
            find_legs(rig).map(|leg| leg.expect("each imported leg needs three named joints"));
        let clip = model
            .animations
            .iter()
            .position(|clip| clip.name == clip_name)
            .unwrap();
        let duration = model.animations[clip].duration;
        let mut lock = FootLock::default();
        let mut previous = [None; SIDES];
        let mut checked = 0;
        let mut drift = 0.0;
        let mut corrected = 0.0;
        let dt = 1.0 / 60.0;
        for frame in 0..(duration / (dt * rate)).ceil() as usize * 2 {
            let time = frame as f32 * dt;
            let elapsed = time * rate;
            let placement =
                Mat4::from_translation(Vec3::new(0.0, -model.bounds_min[1], time * speed));
            let offsets = lock.corrections(FootLockInput {
                model,
                identity: 1,
                clip: Some(clip),
                elapsed,
                previous: None,
                offsets: &[],
                world_from_model: placement,
                ground_height: 0.0,
                dt,
                enabled: true,
            });
            let source = model_transforms(
                rig,
                &animation_pose(rig, Some(&model.animations[clip]), elapsed),
            );
            let mut locals =
                model.blended_locals(rig, Some(&model.animations[clip]), elapsed, None, &offsets);
            super::super::add_bone_translations(rig, &mut locals, lock.translations());
            let target = model_transforms(rig, &locals);
            for (side, leg) in legs.into_iter().enumerate() {
                let anchor = lock.contacts[side]
                    .anchor
                    .filter(|_| lock.contacts[side].weight > 0.91);
                let raw = placement.transform_point3(origin(source[leg.ankle]));
                let posed = placement.transform_point3(origin(target[leg.ankle]));
                if let Some(anchor) = anchor {
                    if let Some((old_anchor, old_raw, old_posed)) = previous[side]
                        && old_anchor == anchor.world_position
                    {
                        drift += raw.distance(old_raw);
                        corrected += posed.distance(old_posed);
                        checked += 1;
                    }
                    previous[side] = Some((anchor.world_position, raw, posed));
                } else {
                    previous[side] = None;
                }
            }
        }
        assert!(
            checked > 10,
            "no sustained contact samples {clip_name}: {checked}"
        );
        assert!(
            drift > 0.025 && corrected < drift * 0.58,
            "planted foot drift {clip_name}: original={drift:.4}, corrected={corrected:.4}, samples={checked}"
        );
    }

    #[test]
    fn imported_walk_and_run_planted_ankles_drift_less_with_ik() {
        let woman = decode_glb(include_bytes!("../demo_assets/woman.glb")).unwrap();
        let man = decode_glb(include_bytes!("../demo_assets/human.glb")).unwrap();
        let emma = decode_glb(include_bytes!("../demo_assets/emma.glb")).unwrap();
        let ryan = decode_glb(include_bytes!("../demo_assets/ryan.glb")).unwrap();
        assert_reduced_sliding(&woman, "Walk_Relaxed", 1.73, 1.0);
        assert_reduced_sliding(&woman, "Run", 4.05, 1.0);
        assert_reduced_sliding(&man, "Walk", 1.12, 1.0);
        assert_reduced_sliding(&man, "Run", 2.14, 1.0);
        assert_reduced_sliding(&emma, "Walk", 3.15, 1.64);
        assert_reduced_sliding(&emma, "Run", 5.70, 1.64);
        assert_reduced_sliding(&ryan, "Walk", 1.30, 1.30);
        assert_reduced_sliding(&ryan, "Run", 5.70, 1.64);
    }

    #[test]
    fn disabling_lock_clears_anchors_without_resampling_asset() {
        let model = decode_glb(include_bytes!("../demo_assets/woman.glb")).unwrap();
        let clip = model
            .animations
            .iter()
            .position(|clip| clip.name == "Walk_Relaxed")
            .unwrap();
        let mut lock = FootLock::default();
        let input = FootLockInput {
            model: &model,
            identity: 4,
            clip: Some(clip),
            elapsed: 0.01,
            previous: None,
            offsets: &[],
            world_from_model: Mat4::IDENTITY,
            ground_height: 0.0,
            dt: 1.0 / 60.0,
            enabled: true,
        };
        for frame in 0..60 {
            lock.corrections(FootLockInput {
                elapsed: frame as f32 * input.dt,
                ..input
            });
            if lock.contacts.iter().any(|contact| contact.anchor.is_some()) {
                break;
            }
        }
        assert!(lock.contacts.iter().any(|contact| contact.anchor.is_some()));
        assert!(lock.cycles[clip].is_some());
        assert!(
            lock.corrections(FootLockInput {
                enabled: false,
                ..input
            })
            .is_empty()
        );
        assert!(lock.contacts.iter().all(|contact| contact.anchor.is_none()));
        assert!(lock.cycles[clip].is_some());
    }

    const FRAME_SECONDS: f32 = 1.0 / 60.0;

    struct CityTrace {
        studio: crate::Studio,
        context: egui::Context,
        frame: u32,
        key: Option<egui::Key>,
        previous: Option<CitySample>,
    }

    #[derive(Clone, Copy)]
    struct CitySample {
        raw: [Vec3; SIDES],
        posed: [Vec3; SIDES],
        raw_sole: [Vec3; SIDES],
        posed_sole: [Vec3; SIDES],
        raw_ankle_forward: [Vec3; SIDES],
        posed_ankle_forward: [Vec3; SIDES],
        contact: [bool; SIDES],
        root: Vec3,
        heading: f32,
        moving: bool,
    }

    #[derive(Default)]
    struct Drift {
        previous: Option<CitySample>,
        raw: f32,
        posed: f32,
        raw_sole: f32,
        posed_sole: f32,
        peak_sole_step: f32,
        peak_planted_step: f32,
        peak_stance_twist: f32,
        peak_change: f32,
        sideways: f32,
        backward: f32,
        frames: usize,
    }

    impl Drift {
        fn observe(&mut self, sample: CitySample) {
            if let Some(previous) = self.previous.replace(sample) {
                let current = self.previous.as_ref().unwrap();
                if current.moving {
                    let travel = current.root - previous.root;
                    let (sin, cos) = current.heading.to_radians().sin_cos();
                    self.sideways += travel.dot(Vec3::new(cos, 0.0, -sin)).abs();
                    self.backward += (-travel.dot(Vec3::new(sin, 0.0, cos))).max(0.0);
                }
                for side in 0..SIDES {
                    let raw_step = current.raw[side] - previous.raw[side];
                    let posed_step = current.posed[side] - previous.posed[side];
                    let raw_sole_step =
                        horizontal_distance(current.raw_sole[side] - previous.raw_sole[side]);
                    let posed_sole_step =
                        horizontal_distance(current.posed_sole[side] - previous.posed_sole[side]);
                    self.peak_sole_step = self.peak_sole_step.max(posed_sole_step);
                    let change = horizontal_distance(posed_step - raw_step);
                    self.peak_change = self.peak_change.max(change);
                    if previous.contact[side] && current.contact[side] {
                        self.raw += horizontal_distance(raw_step);
                        self.posed += horizontal_distance(posed_step);
                        self.raw_sole += raw_sole_step;
                        self.posed_sole += posed_sole_step;
                        self.peak_planted_step = self.peak_planted_step.max(posed_sole_step);
                        let yaw = current.raw_ankle_forward[side]
                            .angle_between(current.posed_ankle_forward[side]);
                        self.peak_stance_twist = self.peak_stance_twist.max(yaw.to_degrees());
                        self.frames += 1;
                    }
                }
            }
        }

        fn report(&self, label: &str) {
            eprintln!(
                "{label}: ankle {:.4}→{:.4}m sole {:.4}→{:.4}m n={} peak sole step {:.4}m/frame (planted {:.4}), peak IK delta {:.4}m/frame, stance ankle twist {:.1}°, travel sideways {:.4}m, backward {:.4}m",
                self.raw,
                self.posed,
                self.raw_sole,
                self.posed_sole,
                self.frames,
                self.peak_sole_step,
                self.peak_planted_step,
                self.peak_change,
                self.peak_stance_twist,
                self.sideways,
                self.backward,
            );
        }

        fn require_plant(&self, label: &str, minimum_pairs: usize, maximum_ratio: f32) {
            self.report(label);
            assert!(
                self.frames >= minimum_pairs && self.raw_sole > 0.025,
                "insufficient measured stance for {label}"
            );
            assert!(
                self.posed_sole <= self.raw_sole * maximum_ratio,
                "sole drift exceeded {:.0}% of the uncorrected pose for {label}",
                maximum_ratio * 100.0
            );
            assert!(
                self.peak_planted_step < 0.025,
                "planted sole moved {:.1} cm in a frame for {label}",
                self.peak_planted_step * 100.0
            );
        }
    }

    impl CityTrace {
        fn new(source: crate::assets::Source) -> Self {
            let mut studio = crate::Studio::default();
            studio.city_demo();
            studio.assign_avatar(source, "Foot lock trace".into());
            studio.game_running = true;
            studio.scene.npcs.clear();
            studio.view = crate::View::Three;
            studio.camera.yaw = std::f32::consts::PI;
            studio.game.z = -10.0;
            Self {
                studio,
                context: egui::Context::default(),
                frame: 0,
                key: None,
                previous: None,
            }
        }

        fn advance(&mut self, key: Option<egui::Key>, sprint: bool, jump: bool) -> CitySample {
            let modifiers = if sprint {
                egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::NONE
            };
            let mut events = Vec::new();
            let mut key_event = |key, pressed| {
                events.push(egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers,
                });
            };
            if self.key != key {
                if let Some(old) = self.key {
                    key_event(old, false);
                }
                if let Some(new) = key {
                    key_event(new, true);
                }
                self.key = key;
            }
            if jump {
                key_event(egui::Key::Space, true);
            }
            let _ = self.context.run(
                egui::RawInput {
                    time: Some(f64::from(self.frame) / 60.0),
                    predicted_dt: FRAME_SECONDS,
                    modifiers,
                    events,
                    ..egui::RawInput::default()
                },
                |ctx| self.studio.advance(ctx),
            );
            self.frame += 1;

            self.studio.view = crate::View::Two;
            let (raw, raw_sole, raw_ankle_forward) =
                world_foot_points(&self.studio.avatar_draw().unwrap());
            self.studio.view = crate::View::Three;
            let draw = self.studio.avatar_draw().unwrap();
            let (posed, posed_sole, posed_ankle_forward) = world_foot_points(&draw);
            // Select consecutive stance frames from the imported clip, regardless of whether IK held an anchor.
            let contact =
                self.studio
                    .foot_locks
                    .borrow()
                    .get(&0)
                    .map_or([false; SIDES], |(_, lock)| {
                        let sample = |clip: Option<usize>, elapsed| {
                            clip.and_then(|clip| lock.cycles.get(clip)?.as_ref())
                                .map_or([0.0; SIDES], |cycle| cycle.at(elapsed))
                        };
                        let current = sample(draw.clip, draw.elapsed);
                        std::array::from_fn(|side| {
                            let weight = draw.blend.map_or(current[side], |blend| {
                                let old = sample(blend.clip, blend.elapsed)[side];
                                old + (current[side] - old) * blend.weight
                            });
                            self.studio.game.grounded && weight >= 0.78
                        })
                    });
            let sample = CitySample {
                raw,
                posed,
                raw_sole,
                posed_sole,
                raw_ankle_forward,
                posed_ankle_forward,
                contact,
                root: self.studio.actor_position(),
                heading: self.studio.avatar_heading,
                moving: matches!(
                    self.studio.avatar_motion,
                    crate::Motion::Walk | crate::Motion::Run
                ),
            };
            self.previous = Some(sample);
            sample
        }

        fn segment(&mut self, key: Option<egui::Key>, sprint: bool, frames: usize) -> Drift {
            let mut drift = Drift {
                previous: self.previous,
                ..Drift::default()
            };
            for _ in 0..frames {
                drift.observe(self.advance(key, sprint, false));
            }
            drift
        }
    }

    fn world_foot_points(
        draw: &crate::ModelDraw<'_>,
    ) -> ([Vec3; SIDES], [Vec3; SIDES], [Vec3; SIDES]) {
        let model = draw.model;
        let rig = model.rig.as_ref().unwrap();
        let legs = find_legs(rig);
        let mut locals = model.blended_locals(
            rig,
            draw.clip.and_then(|clip| model.animations.get(clip)),
            draw.elapsed,
            draw.blend
                .map(|blend| (blend.clip, blend.elapsed, blend.weight)),
            &draw.bone_offsets,
        );
        super::super::add_bone_translations(rig, &mut locals, &draw.bone_translations);
        let frames = model_transforms(rig, &locals);
        let [x, y, z] = draw.rotation.map(f32::to_radians);
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::from_array(draw.scale),
            Quat::from_euler(EulerRot::XYZ, x, y, z),
            Vec3::from_array(draw.position),
        );
        let legs = legs.map(Option::unwrap);
        assert!(
            legs.iter()
                .all(|leg| leg.sole_marker.length_squared() > 1e-6)
        );
        (
            legs.map(|leg| transform.transform_point3(origin(frames[leg.ankle]))),
            legs.map(|leg| (transform * frames[leg.ankle]).transform_point3(leg.sole_marker)),
            legs.map(|leg| {
                let forward = (transform * frames[leg.ankle]).transform_vector3(leg.sole_forward);
                Vec3::new(forward.x, 0.0, forward.z).normalize()
            }),
        )
    }

    fn horizontal_distance(delta: Vec3) -> f32 {
        delta.x.hypot(delta.z)
    }

    #[test]
    fn measured_city_foot_placement_during_real_keyboard_controls() {
        use crate::assets::Source;
        use egui::Key;

        for (source, label) in [(Source::Emma, "Emma"), (Source::Ryan, "Ryan")] {
            for sprint in [false, true] {
                let mut trace = CityTrace::new(source.clone());
                let motion = if sprint { "run" } else { "walk" };
                let start = trace.segment(Some(Key::W), sprint, 33);
                let steady = trace.segment(Some(Key::W), sprint, 150);
                let speed = trace.studio.game.horizontal_speed();
                assert!((speed - if sprint { 3.534 } else { 1.4805 }).abs() < 0.01);
                let stop = trace.segment(None, false, 37);
                assert!(trace.studio.game.horizontal_speed() < 0.01);
                assert_eq!(trace.studio.avatar_motion, crate::Motion::Idle);
                start.require_plant(&format!("{label} {motion} start"), 12, 0.20);
                steady.require_plant(&format!("{label} {motion} steady"), 80, 0.18);
                stop.require_plant(&format!("{label} {motion} stop"), 5, 0.12);
                assert!(
                    stop.peak_change < 0.07,
                    "abrupt ankle correction while stopping {label} {motion}"
                );
            }

            let mut trace = CityTrace::new(source);
            trace.segment(Some(Key::W), false, 63);
            let ninety = trace.segment(Some(Key::D), false, 32);
            assert!((trace.studio.avatar_heading - 270.0).abs() < 3.0);
            let reversal = trace.segment(Some(Key::A), false, 41);
            assert!((trace.studio.avatar_heading - 90.0).abs() < 3.0);
            ninety.require_plant(&format!("{label} 90°"), 24, 0.10);
            reversal.require_plant(&format!("{label} 180°"), 34, 0.10);

            trace.studio.stop();
            trace.studio.game_running = true;
            trace.studio.game.x = 3.15;
            trace.studio.game.z = 10.2;
            trace.studio.camera.yaw = 0.0;
            trace.previous = None;
            let curb = trace.segment(Some(Key::D), false, 42);
            assert!((trace.studio.game.height - 0.07).abs() < 0.015);
            let mut landing = Drift::default();
            let mut airborne = false;
            let mut touched_down = false;
            for frame in 0..80 {
                let sample = trace.advance(Some(Key::W), false, frame == 0);
                if trace.studio.game.grounded && airborne {
                    landing.observe(sample);
                    touched_down = true;
                } else {
                    airborne |= !trace.studio.game.grounded;
                    assert!(
                        trace.studio.foot_locks.borrow()[&0]
                            .1
                            .contacts
                            .iter()
                            .all(|contact| contact.anchor.is_none())
                    );
                }
            }
            assert!(touched_down);
            curb.require_plant(&format!("{label} curb"), 20, 0.22);
            landing.require_plant(&format!("{label} landing"), 12, 0.20);
        }
    }

    #[test]
    fn mira_authored_stride_measures_real_clips_and_preserves_directional_control() {
        use crate::assets::Source;
        use egui::Key;

        let setup = |matched| {
            let mut trace = CityTrace::new(Source::Emma);
            let imported = trace.studio.assets.import_bytes(
                include_bytes!("../demo_assets/character-showcase/mira.glb").to_vec(),
            ).unwrap();
            trace.studio.assign_avatar(imported, "Мира".into());
            let avatar = trace.studio.scene.avatar.as_mut().unwrap();
            avatar.actor_tuning.match_stride = matched;
            avatar.height = 1.73;
            trace
        };
        let mut prior = setup(false);
        let mut matched = setup(true);
        let profile = matched.studio.measured_player_stride().expect("Mira has two calibrated clips");
        // Match the native stance travel while still letting project speed lower its cap.
        assert!(profile.movement.walk_speed < 1.95);
        assert!(profile.movement.sprint_speed > profile.movement.walk_speed + 0.3);
        let walk_prior = prior.segment(Some(Key::W), false, 92);
        let walk_matched = matched.segment(Some(Key::W), false, 92);
        walk_prior.report("Mira baseline walk");
        walk_matched.report("Mira measured walk");
        assert!((matched.studio.game.horizontal_speed() - profile.movement.walk_speed).abs() < 0.02);
        assert!(walk_matched.posed_sole < walk_prior.posed_sole * 0.65);
        let turn_prior = prior.segment(Some(Key::A), false, 50);
        let turn_matched = matched.segment(Some(Key::A), false, 50);
        turn_prior.report("Mira baseline turn");
        turn_matched.report("Mira measured turn");
        assert!(turn_matched.posed_sole < turn_prior.posed_sole * 0.5);
        assert!(turn_matched.peak_stance_twist < 20.0);
        assert!(turn_matched.sideways < 0.07);
        let reversal_prior = prior.segment(Some(Key::D), false, 60);
        let reversal_matched = matched.segment(Some(Key::D), false, 60);
        reversal_prior.report("Mira baseline reversal");
        reversal_matched.report("Mira measured reversal");
        assert!(reversal_matched.posed_sole < reversal_prior.posed_sole * 0.5);
        assert!(reversal_matched.peak_planted_step < reversal_prior.peak_planted_step * 0.4);
        assert!(reversal_matched.peak_planted_step < 0.022);
        assert!(reversal_matched.peak_stance_twist < 20.0);
        assert!(reversal_matched.sideways < 0.06 && reversal_matched.backward < 0.005);
        let delta = (matched.studio.avatar_heading - matched.studio.game.yaw + 180.0)
            .rem_euclid(360.0) - 180.0;
        assert!(delta.abs() < 2.5);
        let before_stop = matched.studio.actor_position();
        let stop = matched.segment(None, false, 45);
        let stop_distance = horizontal_distance(matched.studio.actor_position() - before_stop);
        stop.report("Mira measured stop");
        eprintln!("Mira coast after releasing movement: {:.3}m", stop_distance);
        assert!((0.025..=0.15).contains(&stop_distance));
        assert_eq!(matched.studio.avatar_motion, crate::Motion::Idle);
        eprintln!(
            "Mira aligned movement cap: {}/{} m/s",
            profile.movement.walk_speed, profile.movement.sprint_speed
        );
    }
}
