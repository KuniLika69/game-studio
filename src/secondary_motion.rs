use glam::{Quat, Vec3};

use crate::{avatar::ActorTuning, model::{HairFollow, SecondaryPose}};

const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
const HAIR_LIMIT: Vec3 = Vec3::new(0.041, 0.023, 0.029);
const TAIL_LIMIT: Vec3 = Vec3::new(0.067, 0.032, 0.044);
const WISP_LIMIT: Vec3 = Vec3::new(0.024, 0.015, 0.019);
const CLOTH_LIMIT: Vec3 = Vec3::new(0.028, 0.021, 0.028);

#[derive(Default)]
pub struct State {
    sample: Option<DriverSample>,
    hair: Spring,
    tail_end: Spring,
    loose_strands: Spring,
    cloth: Spring,
}

#[derive(Clone, Copy)]
struct DriverSample {
    velocity: Vec3,
    yaw: f32,
    turning: f32,
}

#[derive(Default)]
struct Spring {
    offset: Vec3,
    velocity: Vec3,
}

impl State {
    #[cfg(test)]
    pub fn advance(&mut self, dt: f32, velocity: Vec3, yaw_degrees: f32) {
        self.advance_in_air(dt, velocity, yaw_degrees, Vec3::ZERO);
    }

    #[cfg(test)]
    pub fn advance_in_air(&mut self, dt: f32, velocity: Vec3, yaw_degrees: f32, wind: Vec3) {
        self.advance_tuned(dt, velocity, yaw_degrees, wind, ActorTuning::default());
    }

    pub fn advance_tuned(&mut self, dt: f32, velocity: Vec3, yaw_degrees: f32, wind: Vec3, tuning: ActorTuning) {
        if !dt.is_finite() || dt <= 0.0 || !velocity.is_finite() || !yaw_degrees.is_finite() {
            return;
        }
        let yaw = yaw_degrees.rem_euclid(360.0).to_radians();
        let velocity = velocity.clamp_length_max(35.0);
        let previous = self.sample.unwrap_or(DriverSample {
            velocity,
            yaw,
            turning: 0.0,
        });
        let frame_dt = dt.clamp(0.001, 0.12);
        let step_yaw = (yaw - previous.yaw + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let turning = (step_yaw / frame_dt).clamp(-7.0, 7.0);
        let angular_accel = ((turning - previous.turning) / frame_dt).clamp(-59.0, 59.0);
        let to_local = Quat::from_rotation_y(-yaw);
        let linear_accel =
            to_local * ((velocity - previous.velocity) / frame_dt).clamp_length_max(34.0);
        self.sample = Some(DriverSample {
            velocity,
            yaw,
            turning,
        });

        // Rotate relative positions into the new reference frame before applying attachment forces.
        let turn_reference = Quat::from_rotation_y(-step_yaw);
        self.hair.change_reference(turn_reference);
        self.tail_end.change_reference(turn_reference);
        self.loose_strands.change_reference(turn_reference);
        self.cloth.change_reference(turn_reference);
        let rotation_accel = |arm: Vec3| {
            let omega = Vec3::Y * turning;
            (Vec3::Y * angular_accel).cross(arm) + omega.cross(omega.cross(arm))
        };
        let hair_accel = linear_accel + rotation_accel(Vec3::new(0.0, -0.12, -0.17));
        let cloth_accel = linear_accel + rotation_accel(Vec3::new(-0.11, -0.33, 0.045));
        // The assets already have a resting drape. Gravity provides the remaining deflection.
        let mut hair_force = (GRAVITY - hair_accel) * Vec3::new(0.42, 0.172, 0.41);
        let mut cloth_force = (GRAVITY - cloth_accel) * Vec3::new(0.29, 0.163, 0.31);
        if wind.is_finite() && wind != Vec3::ZERO {
            let air = to_local * wind.clamp_length_max(14.0);
            let drag = air * (0.64 + air.length() * 0.04);
            hair_force += drag * Vec3::new(0.52, 0.22, 0.49);
            cloth_force += drag * Vec3::new(0.31, 0.15, 0.34);
        }
        self.hair
            .integrate(frame_dt, hair_force, 139.0 * tuning.hair_stiffness, 13.2 * tuning.hair_damping, HAIR_LIMIT);

        // A long tail follows the displacement of its upper section, with independent
        // inertia farther from the tie. Short cards follow the head more quickly.
        let wind = if wind.is_finite() { wind } else { Vec3::ZERO };
        let relative_air = to_local * (wind - velocity).clamp_length_max(14.0);
        let drag = relative_air * (0.66 + relative_air.length() * 0.048);
        let tail_accel = linear_accel + rotation_accel(Vec3::new(0.0, -0.21, -0.22));
        let tail_stiffness = 88.0 * tuning.hair_stiffness;
        let tail_force = (GRAVITY - tail_accel) * Vec3::new(0.30, 0.124, 0.29)
            + drag * Vec3::new(0.37, 0.16, 0.34)
            + self.hair.offset * (0.55 * tail_stiffness);
        self.tail_end.integrate(
            frame_dt,
            tail_force,
            tail_stiffness,
            10.7 * tuning.hair_damping,
            TAIL_LIMIT,
        );
        let wisp_accel = linear_accel + rotation_accel(Vec3::new(0.08, -0.047, 0.025));
        let wisp_force = (GRAVITY - wisp_accel) * Vec3::new(0.22, 0.10, 0.21)
            + drag * Vec3::new(0.49, 0.18, 0.46);
        self.loose_strands.integrate(
            frame_dt,
            wisp_force,
            342.0 * tuning.hair_stiffness,
            20.0 * tuning.hair_damping,
            WISP_LIMIT,
        );
        self.cloth
            .integrate(frame_dt, cloth_force, 196.0, 18.6, CLOTH_LIMIT);
    }

    pub fn pose(&self, seconds: f32) -> SecondaryPose {
        let hair = self.hair.offset / HAIR_LIMIT;
        let normalized = |spring: &Spring| {
            let normalized = spring.offset / HAIR_LIMIT;
            [normalized.x, normalized.y, -normalized.z]
        };
        let garment = self.cloth.offset / CLOTH_LIMIT;
        SecondaryPose {
            hair_sway: [hair.x, -hair.z],
            hair_lift: hair.y,
            hair_follow: Some(HairFollow {
                tail_end: normalized(&self.tail_end),
                loose_strands: normalized(&self.loose_strands),
            }),
            garment_sway: garment.to_array(),
            ..SecondaryPose::natural(seconds, 0.0)
        }
    }
}

impl Spring {
    fn change_reference(&mut self, rotation: Quat) {
        self.offset = rotation * self.offset;
        self.velocity = rotation * self.velocity;
    }

    fn integrate(&mut self, dt: f32, force: Vec3, stiffness: f32, damping: f32, limit: Vec3) {
        let steps = (dt * 140.0).ceil().max(1.0) as u32;
        let step = dt / steps as f32;
        for _ in 0..steps {
            let acceleration = force - stiffness * self.offset - damping * self.velocity;
            self.velocity += acceleration * step;
            self.offset += self.velocity * step;
            for axis in 0..3 {
                if self.offset[axis].abs() > limit[axis] {
                    let edge = self.offset[axis].signum();
                    self.offset[axis] = edge * limit[axis];
                    if edge * self.velocity[axis] > 0.0 {
                        self.velocity[axis] = 0.0;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acceleration_leaves_hair_and_garment_tips_behind_then_damps() {
        let mut state = State::default();
        state.advance(0.016, Vec3::ZERO, 0.0);
        for step in 1..=12 {
            state.advance(0.016, Vec3::X * (step as f32 * 0.27), 0.0);
        }
        let moving = state.pose(0.0);
        assert!(moving.hair_sway[0] < -0.2);
        assert!(moving.garment_sway[0] < -0.2);
        for _ in 0..200 {
            state.advance(0.016, Vec3::X * 3.24, 0.0);
        }
        let settled = state.pose(0.0);
        assert!(settled.hair_sway[0].abs() < 0.01);
        assert!(settled.garment_sway[0].abs() < 0.01);
        assert!(settled.hair_lift < -0.05);
        assert!(settled.garment_sway[1] < -0.05);
    }

    #[test]
    fn pivot_has_weight_and_wrapping_yaw_does_not_create_a_spike() {
        let mut state = State::default();
        state.advance(0.016, Vec3::ZERO, 179.0);
        state.advance(0.016, Vec3::ZERO, -179.0);
        assert!(state.pose(0.0).hair_sway[0].abs() < 0.4);
        for offset in 1..=14 {
            state.advance(0.016, Vec3::ZERO, -179.0 + offset as f32 * 6.0);
        }
        let pose = state.pose(0.0);
        assert!(pose.hair_sway[0].abs() > 0.04 || pose.hair_sway[1].abs() > 0.04);
        assert!(
            pose.garment_sway
                .into_iter()
                .all(|sample| (-1.0..=1.0).contains(&sample))
        );
    }

    #[test]
    fn steady_wind_moves_idle_hair_and_damps_after_the_actor_leaves_the_zone() {
        let mut state = State::default();
        for _ in 0..120 {
            state.advance_in_air(0.016, Vec3::ZERO, 0.0, Vec3::X * 6.0);
        }
        let windy = state.pose(0.0);
        assert!(windy.hair_sway[0] > 0.30);
        assert!(windy.garment_sway[0] > 0.18);
        for _ in 0..120 {
            state.advance(0.016, Vec3::ZERO, 0.0);
        }
        let calm = state.pose(0.0);
        assert!(calm.hair_sway[0].abs() < 0.01);
        assert!(calm.garment_sway[0].abs() < 0.01);
    }
}
