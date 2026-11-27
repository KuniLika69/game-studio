use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};

use super::{
    BoneRotation, BoneTranslation, FootLockInput, LocalTransform, Model, Rig, model_transforms,
};

const LIMBS: [&str; 4] = ["Front.L", "Front.R", "Rear.L", "Rear.R"];

#[derive(Clone, Copy, Debug)]
pub(crate) struct GroundHit {
    pub height: f32,
    pub normal: Vec3,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) struct PawContact {
    pub limb: &'static str,
    pub stance: f32,
    pub authored_gap_m: f32,
    pub solved_gap_m: f32,
    pub solved_position_m: [f32; 3],
    pub ground_height_m: f32,
    pub ground_normal: [f32; 3],
}

#[derive(Clone, Copy)]
struct Leg {
    upper: usize,
    lower: usize,
    paw: usize,
    sole: Vec3,
}

#[derive(Clone, Copy)]
struct Anatomy {
    pelvis: usize,
    legs: [Leg; 4],
}

impl Anatomy {
    fn read(model: &Model, rig: &Rig) -> Option<Self> {
        let find = |name: &str| model.pose_target(rig, name);
        let pelvis = find("Pelvis")?;
        let leg = |suffix| {
            let upper = find(&format!("Leg.{suffix}"))?;
            let lower = find(&format!("Knee.{suffix}"))?;
            let paw = find(&format!("Paw.{suffix}"))?;
            if rig.nodes[upper].parent != Some(pelvis)
                || rig.nodes[lower].parent != Some(upper)
                || rig.nodes[paw].parent != Some(lower)
            {
                return None;
            }
            let mut lowest = Vec3::new(0.0, f32::INFINITY, 0.0);
            for mesh in rig.meshes.iter().filter(|mesh| mesh.node == paw) {
                for vertex in &mesh.vertices {
                    let point = Vec3::from_array(vertex.position);
                    if point.y < lowest.y {
                        lowest = point;
                    }
                }
            }
            lowest.is_finite().then_some(Leg {
                upper,
                lower,
                paw,
                sole: lowest,
            })
        };
        Some(Self {
            pelvis,
            legs: [
                leg(LIMBS[0])?,
                leg(LIMBS[1])?,
                leg(LIMBS[2])?,
                leg(LIMBS[3])?,
            ],
        })
    }
}

#[derive(Clone, Copy, Default)]
struct Plant {
    anchor: Option<Vec2>,
    weight: f32,
}

#[derive(Default)]
pub(crate) struct PawLock {
    identity: Option<u64>,
    anatomy: Option<Anatomy>,
    plants: [Plant; 4],
    body_lift: Option<f32>,
    reports: [Option<PawContact>; 4],
}

impl PawLock {
    pub fn reset(&mut self) {
        self.plants = [Plant::default(); 4];
        self.body_lift = None;
        self.reports = [None; 4];
    }

    pub fn reports(&self) -> [Option<PawContact>; 4] {
        self.reports
    }

    pub fn solve(
        &mut self,
        input: FootLockInput<'_>,
        probe: impl Fn(Vec2) -> GroundHit,
    ) -> (Vec<BoneRotation>, Option<BoneTranslation>) {
        self.reports = [None; 4];
        if !input.enabled {
            self.reset();
            return (Vec::new(), None);
        }
        let model = input.model;
        let Some(rig) = model.rig.as_deref() else {
            return (Vec::new(), None);
        };
        if self.identity != Some(input.identity) {
            self.reset();
            self.identity = Some(input.identity);
            self.anatomy = Anatomy::read(model, rig);
        }
        let Some(anatomy) = self.anatomy else {
            return (Vec::new(), None);
        };
        let transform = input.world_from_model;
        let inv = transform.inverse();
        let actor_rotation = rotation(transform);
        let scale = transform.y_axis.truncate().length();
        let clip = input.clip.and_then(|index| model.animations.get(index));
        let mut locals =
            model.blended_locals(rig, clip, input.elapsed, input.previous, input.offsets);
        let mut globals = model_transforms(rig, &locals);
        let authored = anatomy
            .legs
            .map(|leg| transform.transform_point3(globals[leg.paw].transform_point3(leg.sole)));

        let mut targets = [Vec3::ZERO; 4];
        let mut terrain = [GroundHit {
            height: 0.0,
            normal: Vec3::Y,
        }; 4];
        let mut min_lift = f32::NEG_INFINITY;
        let mut max_lift = f32::INFINITY;
        let mut mean = 0.0;
        let mut support = 0.0;
        for (index, leg) in anatomy.legs.into_iter().enumerate() {
            let leg_scale = (origin(globals[leg.upper]).distance(origin(globals[leg.lower]))
                + origin(globals[leg.lower]).distance(origin(globals[leg.paw])))
                * scale;
            let stance = pose_stance(clip, input.elapsed, index);
            let stance = input.previous.map_or(stance, |(clip, time, fade)| {
                let old = pose_stance(
                    clip.and_then(|slot| model.animations.get(slot)),
                    time,
                    index,
                );
                old + (stance - old) * fade.clamp(0.0, 1.0)
            });
            let plant = &mut self.plants[index];
            let at = horizontal(authored[index]);
            if stance > 0.24 {
                let anchor = plant.anchor.get_or_insert(at);
                // A hard anchor is only valid while the hip can still reach it during turns.
                // Excess stride advances it by the overflow rather than snapping the paw home.
                *anchor = at + (*anchor - at).clamp_length_max(leg_scale * 0.30);
            } else if stance < 0.015 {
                plant.anchor = None;
            }
            plant.weight = stance;
            let xz = at.lerp(plant.anchor.unwrap_or(at), stance);
            let hit = probe(xz);
            let free_lift = (authored[index].y - input.ground_height).max(0.0);
            let sole_y = hit.height + (1.0 - stance) * free_lift;
            targets[index] = Vec3::new(xz.x, sole_y, xz.y);
            terrain[index] = hit;

            if stance > 0.48 {
                let hip = transform.transform_point3(origin(globals[leg.upper]));
                let paw_rotation = slope_rotation(
                    actor_rotation * rotation(globals[leg.paw]),
                    hit.normal,
                    stance,
                );
                let ankle = targets[index] - paw_rotation * (leg.sole * scale);
                let delta = ankle - hip;
                let max_down =
                    ((leg_scale * 0.998).powi(2) - delta.x * delta.x - delta.z * delta.z)
                        .max(0.0)
                        .sqrt();
                max_lift = max_lift.min(delta.y + max_down);
                min_lift = min_lift.max(delta.y + leg_scale * 0.44);
                mean += (hit.height - input.ground_height) * stance;
                support += stance;
            }
        }
        let mut lift = if support > 0.0 {
            mean / support
        } else {
            self.body_lift.unwrap_or(0.0)
        };
        lift = lift
            .max(min_lift.min(max_lift))
            .min(max_lift)
            .clamp(-0.19 * scale, 0.23 * scale);
        let old = self.body_lift.unwrap_or(lift);
        let alpha = 1.0 - (-24.0 * input.dt).exp();
        let body_lift = (old + (lift - old) * alpha)
            .min(max_lift)
            .max(min_lift.min(max_lift));
        let body_lift = body_lift.clamp(-0.22 * scale, 0.25 * scale);
        self.body_lift = Some(body_lift);
        let parent = rig.nodes[anatomy.pelvis]
            .parent
            .map_or(Mat4::IDENTITY, |parent| globals[parent]);
        let local_lift = parent
            .inverse()
            .transform_vector3(inv.transform_vector3(Vec3::Y * body_lift));
        if let LocalTransform::Trs { translation, .. } = &mut locals[anatomy.pelvis] {
            *translation += local_lift;
        }
        globals = model_transforms(rig, &locals);

        let mut corrections = Vec::with_capacity(12);
        for (index, leg) in anatomy.legs.into_iter().enumerate() {
            let base = rotation(globals[leg.paw]);
            let weight = self.plants[index].weight;
            let desired = actor_rotation.conjugate()
                * slope_rotation(actor_rotation * base, terrain[index].normal, weight);
            let target = inv.transform_point3(targets[index]) - desired * leg.sole;
            let parent = rig.nodes[leg.upper]
                .parent
                .map_or(Quat::IDENTITY, |p| rotation(globals[p]));
            let forward = parent * if index >= 2 { -Vec3::Z } else { Vec3::Z };
            let Some((upper, lower, reached)) = solve_leg(leg, &globals, target, forward) else {
                continue;
            };
            for (node, local) in [
                (leg.upper, parent.conjugate() * upper),
                (leg.lower, upper.conjugate() * lower),
                (leg.paw, lower.conjugate() * desired),
            ] {
                if let LocalTransform::Trs { rotation: old, .. } = locals[node] {
                    let delta = (old.conjugate() * local).normalize();
                    let (x, y, z) = delta.to_euler(EulerRot::XYZ);
                    corrections.push(BoneRotation {
                        bone: rig.nodes[node].name.clone().expect("named leg node"),
                        degrees: [x.to_degrees(), y.to_degrees(), z.to_degrees()],
                    });
                }
            }
            let actual = transform.transform_point3(reached + desired * leg.sole);
            let under = probe(horizontal(actual));
            self.reports[index] = Some(PawContact {
                limb: LIMBS[index],
                stance: weight,
                authored_gap_m: authored[index].y - probe(horizontal(authored[index])).height,
                solved_gap_m: actual.y - under.height,
                solved_position_m: actual.to_array(),
                ground_height_m: under.height,
                ground_normal: under.normal.to_array(),
            });
        }
        let translate = BoneTranslation {
            bone: "Pelvis".into(),
            translation: local_lift.to_array(),
        };
        (corrections, Some(translate))
    }
}

fn pose_stance(clip: Option<&super::Animation>, elapsed: f32, limb: usize) -> f32 {
    let Some(clip) = clip else { return 1.0 };
    let (bias, contact) = match clip.name.as_str() {
        "Walk" => ([0.0, 0.5, 0.75, 0.25][limb], 0.64),
        "Run" => ([0.0, 0.5, 0.5, 0.0][limb], 0.51),
        "Idle" => return 1.0,
        _ => return 0.0,
    };
    let phase = (elapsed / clip.duration.max(0.001) + bias).rem_euclid(1.0);
    ((phase + 0.016) / 0.052).clamp(0.0, 1.0)
        * (1.0 - ((phase - contact + 0.028) / 0.074).clamp(0.0, 1.0))
}

fn solve_leg(
    leg: Leg,
    globals: &[Mat4],
    desired: Vec3,
    forward: Vec3,
) -> Option<(Quat, Quat, Vec3)> {
    let hip = origin(globals[leg.upper]);
    let knee = origin(globals[leg.lower]);
    let ankle = origin(globals[leg.paw]);
    let upper = knee - hip;
    let lower = ankle - knee;
    let [a, b] = [upper.length(), lower.length()];
    let direction = (desired - hip).try_normalize()?;
    if a < 0.001 || b < 0.001 {
        return None;
    }
    let reach = hip
        .distance(desired)
        .clamp((a - b).abs() + 0.0005, (a + b) * 0.998);
    let along = (a * a + reach * reach - b * b) / (2.0 * reach);
    let normal = |vector: Vec3| (vector - direction * vector.dot(direction)).try_normalize();
    let default_pole = normal(forward)?;
    let pole = normal(upper)
        .filter(|authored| authored.dot(default_pole) > 0.25)
        .map_or(default_pole, |authored| {
            default_pole.lerp(authored, 0.7).normalize()
        });
    let mid = hip + direction * along + pole * (a * a - along * along).max(0.0).sqrt();
    let end = hip + direction * reach;
    let upper_swing = Quat::from_rotation_arc(upper / a, (mid - hip).normalize());
    let lower_swing = Quat::from_rotation_arc(upper_swing * (lower / b), (end - mid).normalize());
    Some((
        upper_swing * rotation(globals[leg.upper]),
        lower_swing * upper_swing * rotation(globals[leg.lower]),
        end,
    ))
}

fn origin(matrix: Mat4) -> Vec3 {
    matrix.w_axis.truncate()
}
fn horizontal(point: Vec3) -> Vec2 {
    Vec2::new(point.x, point.z)
}
fn rotation(matrix: Mat4) -> Quat {
    matrix.to_scale_rotation_translation().1.normalize()
}

fn slope_rotation(base: Quat, normal: Vec3, stance: f32) -> Quat {
    let up = (base * Vec3::Y).normalize();
    let align = Quat::from_rotation_arc(up, normal);
    (Quat::IDENTITY.slerp(align, stance) * base).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature_mesh::{self, Draft, Kind};
    use crate::model::{add_bone_translations, decode_glb};

    fn ground(p: Vec2) -> GroundHit {
        GroundHit {
            height: 0.095 + 0.19 * p.x + 0.115 * p.y,
            normal: Vec3::new(-0.19, 1.0, -0.115).normalize(),
        }
    }

    #[test]
    fn each_of_four_soles_meets_a_different_height_in_the_rendered_pose() {
        let model =
            decode_glb(&creature_mesh::build(&Draft::for_kind(Kind::Quadruped)).unwrap()).unwrap();
        let mut state = PawLock::default();
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::splat(0.92),
            Quat::from_rotation_y(0.42),
            Vec3::new(0.04, -model.bounds_min[1] * 0.92, 0.1),
        );
        let (offsets, translation) = state.solve(
            FootLockInput {
                model: &model,
                identity: 3,
                clip: Some(0),
                elapsed: 0.45,
                previous: None,
                offsets: &[],
                world_from_model: transform,
                ground_height: 0.0,
                dt: 1.0 / 60.0,
                enabled: true,
            },
            ground,
        );
        let rig = model.rig.as_deref().unwrap();
        let anatomy = state.anatomy.unwrap();
        let mut local = model.blended_locals(rig, model.animations.first(), 0.45, None, &offsets);
        add_bone_translations(rig, &mut local, &[translation.unwrap()]);
        let actual = model_transforms(rig, &local);
        let reports = state.reports().map(Option::unwrap);
        for (leg, report) in anatomy.legs.iter().zip(reports) {
            let sole = transform.transform_point3(actual[leg.paw].transform_point3(leg.sole));
            let ground = ground(horizontal(sole));
            let sole_normal = (rotation(transform) * rotation(actual[leg.paw])) * Vec3::Y;
            assert!(
                (sole.y - ground.height).abs() <= 0.0007,
                "{} {sole:?} / {:?}",
                report.limb,
                ground
            );
            assert!(sole_normal.dot(ground.normal) > 0.9999);
            assert!(Vec3::from_array(report.solved_position_m).distance(sole) < 0.0001);
            assert!(report.authored_gap_m.abs() > 0.018);
        }
        let min = reports
            .iter()
            .map(|r| r.ground_height_m)
            .fold(f32::INFINITY, f32::min);
        let max = reports
            .iter()
            .map(|r| r.ground_height_m)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((max - min) > 0.043);
    }

    #[test]
    fn stance_locks_world_position_for_an_in_range_stride() {
        let model =
            decode_glb(&creature_mesh::build(&Draft::for_kind(Kind::Quadruped)).unwrap()).unwrap();
        let mut state = PawLock::default();
        let mut anchors = Vec::new();
        for frame in 0..12 {
            let t = 0.08 + frame as f32 / 60.0;
            state.solve(
                FootLockInput {
                    model: &model,
                    identity: 3,
                    clip: Some(1),
                    elapsed: t,
                    previous: None,
                    offsets: &[],
                    world_from_model: Mat4::from_translation(Vec3::new(
                        0.0,
                        -model.bounds_min[1],
                        t * 0.43,
                    )),
                    ground_height: 0.0,
                    dt: 1.0 / 60.0,
                    enabled: true,
                },
                ground,
            );
            let contact = state.reports()[0].unwrap();
            assert!(contact.stance >= 0.99);
            assert!(contact.solved_gap_m.abs() < 0.001);
            anchors.push(Vec3::from_array(contact.solved_position_m));
        }
        assert!(anchors[0].distance(anchors[11]) < 0.002);
    }
}
