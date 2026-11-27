use glam::{EulerRot, Quat};

use crate::{avatar::ActorTuning, model::{BoneRotation, Model}};

pub struct Motion {
    pub seconds: f32,
    /// Zero at idle and one at a full run.
    pub stride: f32,
    pub grounded: bool,
    pub seated: bool,
    /// Actor heading around world Y, facing +Z at zero degrees.
    pub yaw: f32,
    /// World-space vector from eye level to the current point of interest.
    pub look_delta: Option<[f32; 3]>,
    pub talking: bool,
    pub breathing: Option<BreathPose>,
    pub upper_body_tilt: [f32; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct BreathPose {
    pub phase: f32,
    pub exertion: f32,
    pub depth: f32,
}

#[derive(Default)]
pub struct Breathing {
    phase: f32,
    exertion: f32,
    voicing: f32,
}

impl Breathing {
    pub fn for_character(id: u64) -> Self {
        // Spread NPCs with adjacent project IDs across the full breathing cycle.
        let phase = (id.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / 16_777_216.0;
        Self {
            phase,
            ..Self::default()
        }
    }

    pub fn advance(&mut self, dt: f32, speed: f32, voiced: bool, tuning: ActorTuning) {
        let dt = dt.clamp(0.0, 0.12);
        let target = ((speed - 0.45) / 2.75).clamp(0.0, 1.0);
        let recovery = if target > self.exertion { 1.7 } else { 0.58 };
        self.exertion += (target - self.exertion) * (1.0 - (-dt * recovery).exp());
        self.voicing += (f32::from(u8::from(voiced)) - self.voicing) * (1.0 - (-dt * 7.5).exp());
        self.phase = (self.phase + dt * (tuning.breaths_per_minute + 17.0 * self.exertion) / 60.0)
            .rem_euclid(1.0);
    }

    pub fn pose(&self, tuning: ActorTuning) -> BreathPose {
        BreathPose {
            phase: self.phase,
            exertion: self.exertion,
            depth: tuning.breath_depth * (1.0 + 0.63 * self.exertion) * (1.0 - 0.64 * self.voicing),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub enum TransferKind {
    #[default]
    None,
    Start,
    Brake,
    QuarterTurn,
    HalfTurn,
    StepUp,
}

#[derive(Default)]
pub struct HeadingTurn {
    degrees_per_second: f32,
}

impl HeadingTurn {
    pub fn step(&mut self, from: f32, target: f32, dt: f32) -> f32 {
        if !dt.is_finite() || dt <= 0.0 {
            return from;
        }
        let delta = (target - from + 180.0).rem_euclid(360.0) - 180.0;
        if delta.abs() < 0.025 {
            self.degrees_per_second = 0.0;
            return target.rem_euclid(360.0);
        }
        let dt = dt.min(0.07);
        let acceleration = 2_650.0;
        // Limit speed by the remaining stopping distance. The direction settles
        // before the next stride even when the input flips to the opposite side.
        let requested = delta.signum() * (2.0 * acceleration * delta.abs()).sqrt().min(490.0);
        let previous = self.degrees_per_second;
        self.degrees_per_second +=
            (requested - previous).clamp(-acceleration * dt, acceleration * dt);
        let step = (previous + self.degrees_per_second) * (0.5 * dt);
        if step.signum() == delta.signum() && step.abs() >= delta.abs() {
            self.degrees_per_second = 0.0;
            target.rem_euclid(360.0)
        } else {
            (from + step).rem_euclid(360.0)
        }
    }
}

#[derive(Default)]
pub struct WeightTransfer {
    speed: f32,
    active: TransferKind,
    seconds: f32,
    turn: f32,
    tilt: [f32; 3],
}

impl WeightTransfer {
    pub fn advance(&mut self, dt: f32, speed: f32, heading_delta: f32, rise: f32, grounded: bool) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.seconds += dt;
        let previous = std::mem::replace(&mut self.speed, speed);
        if !grounded {
            self.active = TransferKind::None;
            self.settle_tilt(dt);
            return;
        }
        let turn = heading_delta.abs();
        let candidate = if rise >= 0.022 {
            TransferKind::StepUp
        } else if speed > 0.18 && turn >= 136.0 {
            TransferKind::HalfTurn
        } else if speed > 0.18 && turn >= 56.0 {
            TransferKind::QuarterTurn
        } else if previous > 0.55 && speed < previous - 0.095 {
            TransferKind::Brake
        } else if previous < 0.19 && speed >= 0.19 {
            TransferKind::Start
        } else {
            TransferKind::None
        };
        let supersede = matches!(candidate, TransferKind::StepUp | TransferKind::HalfTurn | TransferKind::QuarterTurn)
            && matches!(self.active, TransferKind::None | TransferKind::Start | TransferKind::Brake);
        if candidate != TransferKind::None && (self.seconds >= self.duration() || supersede) {
            self.active = candidate;
            self.seconds = 0.0;
            self.turn = heading_delta.signum();
        } else if self.seconds >= self.duration() {
            self.active = TransferKind::None;
        }
        self.settle_tilt(dt);
    }

    fn duration(&self) -> f32 {
        match self.active {
            TransferKind::None => 0.0,
            TransferKind::Start => 0.24,
            TransferKind::Brake => 0.29,
            TransferKind::QuarterTurn => 0.22,
            TransferKind::HalfTurn => 0.36,
            TransferKind::StepUp => 0.23,
        }
    }

    pub fn torso_tilt(&self) -> [f32; 3] {
        self.tilt
    }

    fn settle_tilt(&mut self, dt: f32) {
        let target = self.target_tilt();
        let blend = 1.0 - (-34.0 * dt).exp();
        for (tilt, target) in self.tilt.iter_mut().zip(target) {
            *tilt += (target - *tilt) * blend;
        }
    }

    fn target_tilt(&self) -> [f32; 3] {
        let tilt = match self.active {
            TransferKind::Start => [3.6, 0.0, 0.0],
            TransferKind::Brake => [-3.7, 0.0, 0.0],
            TransferKind::QuarterTurn => [1.9, 3.5 * self.turn, -4.5 * self.turn],
            TransferKind::HalfTurn => [2.7, 5.8 * self.turn, -6.5 * self.turn],
            TransferKind::StepUp => [3.0, 0.0, 0.0],
            TransferKind::None => return [0.0; 3],
        };
        let phase = self.seconds / self.duration();
        let pulse = ease(phase / 0.38) * (1.0 - ease((phase - 0.38) / 0.62));
        tilt.map(|value| value * pulse)
    }

    pub(crate) fn kind(&self) -> TransferKind {
        self.active
    }
}

#[cfg(test)]
pub fn append(model: &Model, offsets: &mut Vec<BoneRotation>, motion: Motion) {
    append_with_eye_tracking(model, offsets, motion, 1.0);
}

pub fn append_with_eye_tracking(
    model: &Model,
    offsets: &mut Vec<BoneRotation>,
    motion: Motion,
    eye_strength: f32,
) {
    let bones = model.bone_names();
    if bones.is_empty() {
        return;
    }
    let time = if motion.seconds.is_finite() {
        f64::from(motion.seconds)
    } else {
        0.0
    };
    let wave = |frequency: f64, phase: f64| {
        (std::f64::consts::TAU * time * frequency + phase).sin() as f32
    };
    let stride = if motion.stride.is_finite() {
        motion.stride.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let inactive_arms = if motion.seated {
        0.42
    } else if motion.grounded {
        1.0 - stride * 0.86
    } else {
        0.21
    };
    let (phase, breath_depth) = motion.breathing.map_or(
        ((time / 3.6).rem_euclid(1.0) as f32, 1.0 + 0.35 * stride),
        |sample| (sample.phase, sample.depth),
    );
    let breath = breath_cycle(phase);
    let abdomen = breath_cycle(phase + 0.035);
    let shoulder_breath = breath_cycle(phase - 0.04);
    let mut turn = |aliases: &[&str], degrees| {
        append_rotation(model, &bones, offsets, aliases, degrees);
    };
    turn(
        &["lowerchest", "spine_02", "mixamorig:Spine1"],
        [-0.52 * breath_depth * abdomen, 0.0, 0.0],
    );
    // Both imported rigs parent their thighs to the lower spine. Layer the motion on
    // the chest so acceleration and stopping accents don't pull on planted feet.
    let [pitch_tilt, yaw_tilt, roll_tilt] = motion.upper_body_tilt;
    turn(
        &["chest", "spine_03", "spine2", "mixamorig:Spine2", "torso"],
        [1.85 * breath_depth * breath + pitch_tilt, yaw_tilt, wave(0.18, 1.3) * 0.29 + roll_tilt],
    );

    // Small motions at the shoulders and wrists complement the imported gait clips.
    for (sign, phase, shoulder, wrist) in [
        (
            1.0,
            0.0,
            &[
                "shoulderL",
                "clavicle_l",
                "clavicle.l",
                "mixamorig:LeftShoulder",
            ][..],
            &["handL", "hand_l", "hand.l", "mixamorig:LeftHand"][..],
        ),
        (
            -1.0,
            1.38,
            &[
                "shoulderR",
                "clavicle_r",
                "clavicle.r",
                "mixamorig:RightShoulder",
            ][..],
            &["handR", "hand_r", "hand.r", "mixamorig:RightHand"][..],
        ),
    ] {
        let drift = wave(0.43, phase) * inactive_arms;
        turn(
            shoulder,
            [
                drift * 1.28 + shoulder_breath * breath_depth * 0.34,
                0.0,
                sign * (shoulder_breath * breath_depth * 0.64 + drift * 0.9 * inactive_arms),
            ],
        );
        turn(
            wrist,
            [
                drift * 2.2,
                wave(0.31, phase) * inactive_arms * sign * 1.7,
                0.0,
            ],
        );
    }

    let (pitch, yaw) = gaze(&motion).unwrap_or_else(|| {
        // The eyes and head lead an impending step while the chest follows its shift.
        let lead = (yaw_tilt * 1.55).clamp(-10.0, 10.0);
        (breath * -0.57, wave(0.12, 1.8) * 1.35 + lead)
    });
    let (pitch, yaw) = (pitch * (1.0 - stride * 0.26), yaw * (1.0 - stride * 0.26));
    let neck = ["neck", "neck_01", "neck1", "mixamorig:Neck", "J_Bip_C_Neck"];
    let head = ["head", "head_01", "mixamorig:Head", "J_Bip_C_Head"];
    let neck_weight = if find_bone(&bones, &neck).is_some() && find_bone(&bones, &head).is_some() {
        0.29
    } else {
        0.0
    };
    if find_bone(&bones, &head).is_some() {
        turn(&neck, [pitch * neck_weight, yaw * neck_weight, 0.0]);
        turn(
            &head,
            [pitch * (1.0 - neck_weight), yaw * (1.0 - neck_weight), 0.0],
        );
    } else {
        turn(&neck, [pitch, yaw, 0.0]);
    }
    if eye_strength > 0.0 {
        for eye in [
            &[
                "eye_l",
                "eye.l",
                "leftEye",
                "left_eye",
                "CC_Base_L_Eye",
                "mixamorig:LeftEye",
                "J_Bip_L_Eye",
            ][..],
            &[
                "eye_r",
                "eye.r",
                "rightEye",
                "right_eye",
                "CC_Base_R_Eye",
                "mixamorig:RightEye",
                "J_Bip_R_Eye",
            ][..],
        ] {
            turn(
                eye,
                [
                    (pitch * 0.20).clamp(-5.0, 5.0) * eye_strength,
                    (yaw * 0.20).clamp(-7.0, 7.0) * eye_strength,
                    0.0,
                ],
            );
        }
    }

    if motion.talking {
        let syllable = (0.56 * wave(3.63, 0.09) + 0.31 * wave(5.14, 0.90) + 0.36).clamp(0.0, 1.0);
        turn(
            &[
                "jaw",
                "jaw_01",
                "CC_Base_JawRoot",
                "mixamorig:Jaw",
                "Bip01 MJaw",
            ],
            [1.0 + syllable * 5.2, 0.0, 0.0],
        );
    }
}

fn breath_cycle(phase: f32) -> f32 {
    const INHALE_FRACTION: f32 = 0.38;
    let phase = phase.rem_euclid(1.0);
    let expansion = if phase < INHALE_FRACTION {
        ease(phase / INHALE_FRACTION)
    } else {
        1.0 - ease((phase - INHALE_FRACTION) / (1.0 - INHALE_FRACTION))
    };
    2.0 * expansion - 1.0
}

pub fn balance(model: &Model, offsets: &mut Vec<BoneRotation>, pace: f32, turn_degrees: f32) {
    if pace < 0.05 {
        return;
    }
    let bones = model.bone_names();
    append_rotation(
        model,
        &bones,
        offsets,
        &["spine_01", "spine0", "mixamorig:Spine"],
        [
            pace * 5.5,
            0.0,
            -turn_degrees.clamp(-90.0, 90.0) * 0.081 * pace,
        ],
    );
}

pub fn listen(model: &Model, offsets: &mut Vec<BoneRotation>, seconds: f32) {
    let pulse = |center: f32, width: f32| {
        let phase = ((seconds - center).abs() / width).clamp(0.0, 1.0);
        (phase * std::f32::consts::PI).cos().mul_add(0.5, 0.5)
    };
    let nod = pulse(2.80, 0.31) * 6.8 + pulse(9.85, 0.36) * 8.5
        + pulse(10.40, 0.27) * 3.5;
    if nod > 0.001 {
        let bones = model.bone_names();
        append_rotation(model, &bones, offsets, &["head", "head_01", "mixamorig:Head", "J_Bip_C_Head"], [nod, 0.0, 0.0]);
    }
}

pub fn jump(model: &Model, offsets: &mut Vec<BoneRotation>, phase: f32, speed: f32) {
    let airborne = (std::f32::consts::PI * phase).sin().max(0.0);
    let landing = ease((phase - 0.62) / 0.38);
    let bend = airborne * (1.0 - landing);
    let bones = model.bone_names();
    legs(
        model,
        offsets,
        &bones,
        [-29.0 * bend - 10.0 * landing, -18.0 * bend - 9.0 * landing],
        [40.0 * bend + 15.0 * landing, 43.0 * bend + 13.0 * landing],
        -8.5 * bend - 6.0 * landing,
    );
    for (side, arm, forearm, drive) in [
        (
            1.0,
            ["upperarm_l", "mixamorig:LeftArm"],
            ["lowerarm_l", "mixamorig:LeftForeArm"],
            -18.0,
        ),
        (
            -1.0,
            ["upperarm_r", "mixamorig:RightArm"],
            ["lowerarm_r", "mixamorig:RightForeArm"],
            -9.0,
        ),
    ] {
        append_rotation(
            model,
            &bones,
            offsets,
            &arm,
            [
                bend * (drive - speed * 10.0) + 12.0 * landing,
                0.0,
                side * (bend * 13.0 + landing * 6.0),
            ],
        );
        append_rotation(model, &bones, offsets, &forearm, [-24.0 * bend, 0.0, 0.0]);
    }
    append_rotation(
        model,
        &bones,
        offsets,
        &["spine_01", "mixamorig:Spine"],
        [bend * 6.0 + landing * 7.0, 0.0, 0.0],
    );
}

pub fn sit(model: &Model, offsets: &mut Vec<BoneRotation>, weight: f32) {
    let bones = model.bone_names();
    legs(
        model,
        offsets,
        &bones,
        [-85.0 * weight; 2],
        [83.0 * weight; 2],
        -4.0 * weight,
    );
    for arms in [
        &["upperarm_l", "mixamorig:LeftArm"][..],
        &["upperarm_r", "mixamorig:RightArm"][..],
    ] {
        append_rotation(model, &bones, offsets, arms, [-14.0 * weight, 0.0, 0.0]);
    }
    for arms in [
        &["lowerarm_l", "mixamorig:LeftForeArm"][..],
        &["lowerarm_r", "mixamorig:RightForeArm"][..],
    ] {
        append_rotation(model, &bones, offsets, arms, [-33.0 * weight, 0.0, 0.0]);
    }
}

pub fn impact_weight(phase: f32) -> f32 {
    ease(phase / 0.22) * (1.0 - ease((phase - 0.27) / 0.73))
}

pub fn land(model: &Model, offsets: &mut Vec<BoneRotation>, phase: f32, speed: f32) {
    let impact = impact_weight(phase) * (1.0 - speed * 0.25);
    let bones = model.bone_names();
    legs(
        model,
        offsets,
        &bones,
        [-12.0 * impact; 2],
        [22.0 * impact; 2],
        -8.0 * impact,
    );
    append_rotation(
        model,
        &bones,
        offsets,
        &["spine_01", "spine0", "mixamorig:Spine"],
        [impact * 9.0, 0.0, 0.0],
    );
}

fn ease(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn legs(
    model: &Model,
    offsets: &mut Vec<BoneRotation>,
    bones: &[&str],
    hip: [f32; 2],
    knee: [f32; 2],
    ankle: f32,
) {
    for (side, (thigh, calf, foot)) in [
        (
            &["thigh_l", "thighL", "mixamorig:LeftUpLeg"][..],
            &["calf_l", "shinL", "mixamorig:LeftLeg"][..],
            &["foot_l", "footL", "mixamorig:LeftFoot"][..],
        ),
        (
            &["thigh_r", "thighR", "mixamorig:RightUpLeg"][..],
            &["calf_r", "shinR", "mixamorig:RightLeg"][..],
            &["foot_r", "footR", "mixamorig:RightFoot"][..],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        append_rotation(model, bones, offsets, thigh, [hip[side], 0.0, 0.0]);
        append_rotation(model, bones, offsets, calf, [knee[side], 0.0, 0.0]);
        append_rotation(model, bones, offsets, foot, [ankle, 0.0, 0.0]);
    }
}

pub(crate) fn gaze(motion: &Motion) -> Option<(f32, f32)> {
    let [x, y, z] = motion.look_delta?;
    if ![x, y, z, motion.yaw].into_iter().all(f32::is_finite) {
        return None;
    }
    let flat = x.hypot(z);
    if flat < 0.15 {
        return None;
    }
    let world_heading = x.atan2(z).to_degrees();
    let relative = (world_heading - motion.yaw + 180.0).rem_euclid(360.0) - 180.0;
    // Blend out a target behind the shoulder instead of snapping as it crosses ±180 degrees.
    let forward = (1.0 - (relative.abs() - 80.0).max(0.0) / 55.0).clamp(0.0, 1.0);
    Some((
        -y.atan2(flat).to_degrees().clamp(-22.0, 21.0) * forward,
        relative.clamp(-37.0, 37.0) * forward,
    ))
}

fn find_bone<'a>(bones: &[&'a str], aliases: &[&str]) -> Option<&'a str> {
    aliases.iter().find_map(|alias| {
        bones
            .iter()
            .copied()
            .find(|name| name.eq_ignore_ascii_case(alias))
    })
}

fn append_rotation(
    model: &Model,
    bones: &[&str],
    offsets: &mut Vec<BoneRotation>,
    aliases: &[&str],
    degrees: [f32; 3],
) {
    let Some(name) = find_bone(bones, aliases) else {
        return;
    };
    if degrees.into_iter().all(|angle| angle.abs() < 0.001)
        || offsets.iter().any(|offset| offset.bone == name)
    {
        return;
    }
    let Some(basis) = model.bone_rest_rotation(name) else {
        return;
    };
    let [x, y, z] = degrees.map(f32::to_radians);
    let rotation = basis.conjugate() * Quat::from_euler(EulerRot::XYZ, x, y, z) * basis;
    let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
    offsets.push(BoneRotation {
        bone: name.into(),
        degrees: [x, y, z].map(f32::to_degrees),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_eases_across_wrap_and_settles_without_overshoot() {
        let mut state = HeadingTurn::default();
        let mut yaw = 340.0;
        let dt = 1.0 / 60.0;
        let mut speed: f32 = 0.0;
        for _ in 0..60 {
            let next = state.step(yaw, 71.0, dt);
            let displacement = (next - yaw + 180.0).rem_euclid(360.0) - 180.0;
            assert!(displacement >= -0.001 && displacement <= 490.0 * dt + 0.001);
            speed = speed.max(displacement / dt);
            yaw = next;
        }
        assert!(speed > 200.0);
        assert_eq!(yaw, 71.0);
    }

    #[test]
    fn idle_characters_breathe_out_of_sync_and_replay_reproducibly() {
        let tuning = ActorTuning::default();
        let mut first = Breathing::for_character(1);
        let mut second = Breathing::for_character(2);
        let mut replay = Breathing::for_character(1);
        for _ in 0..120 {
            first.advance(1.0 / 60.0, 0.0, false, tuning);
            second.advance(1.0 / 60.0, 0.0, false, tuning);
            replay.advance(1.0 / 60.0, 0.0, false, tuning);
        }
        assert_eq!(first.pose(tuning).phase, replay.pose(tuning).phase);
        assert!((first.pose(tuning).phase - second.pose(tuning).phase).abs() > 0.3);
    }

    #[test]
    fn breath_clock_stays_continuous_on_start_stop_and_during_speech() {
        let tuning = ActorTuning::default();
        let mut state = Breathing::default();
        let dt = 1.0 / 60.0;
        for (speed, voiced, steps) in [(0.0, false, 120), (3.55, false, 120), (0.0, false, 30), (0.0, true, 60), (0.0, false, 180)] {
            for _ in 0..steps {
                let before = state.pose(tuning);
                state.advance(dt, speed, voiced, tuning);
                let after = state.pose(tuning);
                let continuous = (after.phase - before.phase).rem_euclid(1.0);
                assert!((0.004..0.011).contains(&continuous));
                assert!((after.depth - before.depth).abs() < 0.13);
            }
            if speed > 3.0 {
                assert!(state.pose(tuning).exertion > 0.92);
            }
            if voiced {
                assert!(state.pose(tuning).depth < 0.7);
            }
        }
    }

    #[test]
    fn weight_transfer_distinguishes_stop_quarter_half_and_step() {
        let mut state = WeightTransfer::default();
        let step = 1.0 / 60.0;
        state.advance(step, 0.24, 0.0, 0.0, true);
        assert_eq!(state.kind(), TransferKind::Start);
        state.advance(step, 1.1, 91.0, 0.0, true);
        assert_eq!(state.kind(), TransferKind::QuarterTurn);
        for _ in 0..20 { state.advance(step, 1.1, 0.0, 0.0, true); }
        state.advance(step, 1.1, -176.0, 0.0, true);
        assert_eq!(state.kind(), TransferKind::HalfTurn);
        for _ in 0..30 { state.advance(step, 1.1, 0.0, 0.0, true); }
        state.advance(step, 0.93, 0.0, 0.0, true);
        assert_eq!(state.kind(), TransferKind::Brake);
        state.advance(step, 0.93, 0.0, 0.071, true);
        assert_eq!(state.kind(), TransferKind::StepUp);
    }

    #[test]
    fn both_rigs_move_torso_and_clavicles_in_breath_cycle() {
        for bytes in [include_bytes!("../demo_assets/emma.glb").as_slice(), include_bytes!("../demo_assets/ryan.glb").as_slice()] {
            let model = crate::model::decode_glb(bytes).unwrap();
            let pose = |phase| {
                let mut offsets = Vec::new();
                append_with_eye_tracking(&model, &mut offsets, Motion {
                    seconds: 1.0, stride: 0.0, grounded: true, seated: false,
                    yaw: 0.0, look_delta: Some([0.0, 0.0, 1.0]), talking: false,
                    breathing: Some(BreathPose { phase, depth: 1.3, exertion: 0.0 }),
                    upper_body_tilt: [0.0; 3],
                }, 1.0);
                offsets
            };
            let inhale = pose(0.38);
            let exhale = pose(0.0);
            for bone in ["mixamorig:Spine1", "mixamorig:Spine2", "mixamorig:LeftShoulder", "mixamorig:RightShoulder"] {
                let value = |pose: &[BoneRotation]| pose.iter().find(|offset| offset.bone == bone).unwrap().degrees;
                let distance = glam::Vec3::from_array(value(&inhale)).distance(glam::Vec3::from_array(value(&exhale)));
                // Euler terms may wrap at 180 degrees; compare the physical rotation instead in the existing mesh test.
                assert!(distance > 0.1, "{bone}");
            }
        }
    }

    #[test]
    fn breathing_blends_through_effort_and_respects_authored_bones() {
        let model = crate::model::decode_glb(include_bytes!("../demo_assets/emma.glb")).unwrap();
        let motion = |seconds, stride| Motion {
            seconds,
            stride,
            grounded: true,
            seated: false,
            yaw: 0.0,
            look_delta: Some([0.0, 0.0, 1.0]),
            talking: false,
            breathing: None,
            upper_body_tilt: [0.0; 3],
        };
        let chest = "mixamorig:Spine2";
        let torso = "mixamorig:Spine1";
        let shoulder = "mixamorig:LeftShoulder";
        let authored_shoulder = BoneRotation {
            bone: shoulder.into(),
            degrees: [0.0, 0.0, 10.0],
        };
        let mut inhale = vec![authored_shoulder.clone()];
        append(&model, &mut inhale, motion(361.368, 0.0));
        assert_eq!(
            inhale
                .iter()
                .filter(|offset| offset.bone == shoulder)
                .count(),
            1
        );
        assert_eq!(inhale[0], authored_shoulder);
        assert!(inhale.iter().any(|offset| offset.bone == torso));

        let mut exhale = vec![authored_shoulder.clone()];
        append(&model, &mut exhale, motion(360.0, 0.0));
        let expanded_mesh = model.pose_with_overrides(None, 0.0, &inhale).unwrap();
        let relaxed_mesh = model.pose_with_overrides(None, 0.0, &exhale).unwrap();
        let vertex_delta = expanded_mesh
            .iter()
            .flatten()
            .zip(relaxed_mesh.iter().flatten())
            .map(|(a, b)| {
                glam::Vec3::from_array(a.position).distance(glam::Vec3::from_array(b.position))
            })
            .fold(0.0_f32, f32::max);
        assert!(
            vertex_delta > 0.005,
            "peak skin displacement {vertex_delta}"
        );

        let mut under_effort = vec![authored_shoulder];
        append(&model, &mut under_effort, motion(361.368, 1.0));
        let angle = |pose: &[BoneRotation]| {
            let [x, y, z] = pose
                .iter()
                .find(|offset| offset.bone == chest)
                .unwrap()
                .degrees
                .map(f32::to_radians);
            Quat::from_euler(EulerRot::XYZ, x, y, z)
        };
        let (inhale_axis, inhale_angle) = angle(&inhale).to_axis_angle();
        let (effort_axis, effort_angle) = angle(&under_effort).to_axis_angle();
        assert!(inhale_axis.dot(effort_axis) > 0.99);
        assert!(effort_angle > inhale_angle * 1.25);
    }
}
