use crate::{
    actor_motion::{self, Motion},
    model::{Model, MorphWeight, SecondaryPose},
};

#[derive(Clone, Copy, Debug, Default)]
struct Shapes {
    combined: Option<usize>,
    sides: [Option<usize>; 2],
}

impl Shapes {
    fn any(self) -> bool {
        self.combined.is_some() || self.sides.iter().any(Option::is_some)
    }

    fn push(self, names: &[String], output: &mut Vec<MorphWeight>, weight: f32) -> bool {
        if weight <= 0.001 {
            return false;
        }
        let indices = if self.combined.is_some() {
            [self.combined, None]
        } else {
            self.sides
        };
        for index in indices.into_iter().flatten() {
            let target = &names[index];
            if !output.iter().any(|value| &value.target == target) {
                output.push(MorphWeight {
                    target: target.clone(),
                    weight,
                });
            }
        }
        self.any()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Channels {
    blink: Shapes,
    smile: Shapes,
    left: Shapes,
    right: Shapes,
    up: Shapes,
    down: Shapes,
}

const BLINK: [&str; 3] = ["blink", "eyesClosed", "Fcl_EYE_Close"];
const BLINK_LEFT: [&str; 4] = ["eyeBlinkLeft", "blinkLeft", "blink_L", "Fcl_EYE_Close_L"];
const BLINK_RIGHT: [&str; 4] = ["eyeBlinkRight", "blinkRight", "blink_R", "Fcl_EYE_Close_R"];
const SMILE: [&str; 2] = ["smile", "mouthSmile"];
const SMILE_LEFT: [&str; 2] = ["mouthSmileLeft", "smileLeft"];
const SMILE_RIGHT: [&str; 2] = ["mouthSmileRight", "smileRight"];

impl Channels {
    pub(crate) fn resolve(names: &[String]) -> Self {
        let index = |aliases: &[&str]| {
            aliases.iter().find_map(|alias| {
                names
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(alias))
            })
        };
        let shapes = |combined: &[&str], left: &[&str], right: &[&str]| Shapes {
            combined: index(combined),
            sides: [index(left), index(right)],
        };
        Self {
            blink: shapes(&BLINK, &BLINK_LEFT, &BLINK_RIGHT),
            smile: shapes(&SMILE, &SMILE_LEFT, &SMILE_RIGHT),
            left: shapes(
                &["lookLeft", "eyeLookLeft"],
                &["eyeLookOutLeft"],
                &["eyeLookInRight"],
            ),
            right: shapes(
                &["lookRight", "eyeLookRight"],
                &["eyeLookInLeft"],
                &["eyeLookOutRight"],
            ),
            up: shapes(
                &["lookUp", "eyeLookUp"],
                &["eyeLookUpLeft"],
                &["eyeLookUpRight"],
            ),
            down: shapes(
                &["lookDown", "eyeLookDown"],
                &["eyeLookDownLeft"],
                &["eyeLookDownRight"],
            ),
        }
    }
}

pub(crate) fn owns_speech(weights: &[MorphWeight]) -> bool {
    weights.iter().any(|weight| {
        let target = &weight.target;
        starts_with(target, "jaw")
            || starts_with(target, "mouth")
                && !["mouthSmile", "mouthFrown", "mouthDimple"]
                    .iter()
                    .any(|prefix| starts_with(target, prefix))
            || starts_with(target, "viseme")
            || matches_one(
                target,
                &[
                    "aa",
                    "ih",
                    "ou",
                    "ee",
                    "oh",
                    "Fcl_MTH_A",
                    "Fcl_MTH_I",
                    "Fcl_MTH_U",
                    "Fcl_MTH_E",
                    "Fcl_MTH_O",
                ],
            )
    })
}

/// Returns whether named shapes already own the eye direction for this frame.
pub(crate) fn animate(
    model: &Model,
    weights: &mut Vec<MorphWeight>,
    secondary: &mut SecondaryPose,
    motion: &Motion,
    strength: f32,
    actor: u64,
    in_dialogue: bool,
) -> bool {
    let channels = model.expression_channels;
    let names = model.morph_names();
    let authored_dialogue = in_dialogue && !weights.is_empty();
    let has_authored_blink = weights.iter().any(|weight| {
        [&BLINK[..], &BLINK_LEFT[..], &BLINK_RIGHT[..]]
            .iter()
            .any(|names| matches_one(&weight.target, names))
    });
    if channels.blink.any() || has_authored_blink {
        // A model that authored eyelid shapes uses those instead of the older mesh blink.
        secondary.blink = 0.0;
        if !has_authored_blink {
            channels
                .blink
                .push(names, weights, blink(motion.seconds, actor) * strength);
        }
    } else {
        secondary.blink *= strength;
    }

    if !in_dialogue
        && !motion.talking
        && motion.grounded
        && motion.stride < 0.16
        && !weights.iter().any(|weight| {
            [&SMILE[..], &SMILE_LEFT[..], &SMILE_RIGHT[..]]
                .iter()
                .any(|names| matches_one(&weight.target, names))
        })
    {
        // A restrained idle shift, with room for an authored held expression.
        let phase = (motion.seconds + (actor % 17) as f32 * 0.47).rem_euclid(9.4);
        let rise = smooth((phase - 3.1) / 0.85);
        let settle = 1.0 - smooth((phase - 4.45) / 1.20);
        channels
            .smile
            .push(names, weights, 0.24 * rise * settle * strength);
    }

    if weights.iter().any(|weight| {
        starts_with(&weight.target, "eyeLook")
            || matches_one(
                &weight.target,
                &["lookLeft", "lookRight", "lookUp", "lookDown"],
            )
    }) {
        return true;
    }
    if authored_dialogue {
        return false;
    }
    let Some((pitch, yaw)) = actor_motion::gaze(motion) else {
        return false;
    };
    let horizontal = if yaw < 0.0 {
        channels.left
    } else {
        channels.right
    };
    let vertical = if pitch < 0.0 {
        channels.up
    } else {
        channels.down
    };
    let turns = horizontal.push(names, weights, yaw.abs() / 37.0 * 0.42 * strength);
    let lifts = vertical.push(names, weights, pitch.abs() / 22.0 * 0.34 * strength);
    turns || lifts
}

fn starts_with(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|beginning| beginning.eq_ignore_ascii_case(prefix))
}

fn matches_one(value: &str, alternatives: &[&str]) -> bool {
    alternatives
        .iter()
        .any(|name| value.eq_ignore_ascii_case(name))
}

fn smooth(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn blink(seconds: f32, actor: u64) -> f32 {
    if !seconds.is_finite() || seconds < 0.0 {
        return 0.0;
    }
    // Use simulation time and stable actor IDs so replay retains the same blinking.
    let window = (seconds / 4.0).floor() as u64;
    let variant =
        actor.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ window.wrapping_mul(0x94d0_49bb_1331_11eb);
    let variant = variant ^ (variant >> 27);
    let start = 1.03 + ((variant >> 22) & 255) as f32 * (2.06 / 255.0);
    let clock = seconds.rem_euclid(4.0);
    let pulse = |start, rate| {
        let age = (clock - start) * rate;
        smooth(age / 0.055) * (1.0 - smooth((age - 0.074) / 0.108))
    };
    let second = if variant % 5 == 2 {
        pulse(start + 0.29, 1.12) * 0.84
    } else {
        0.0
    };
    pulse(start, 1.0).max(second)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn at(seconds: f32) -> Motion {
        Motion {
            seconds,
            stride: 0.0,
            grounded: true,
            seated: false,
            yaw: 0.0,
            look_delta: None,
            talking: false,
            breathing: None,
            upper_body_tilt: [0.0; 3],
        }
    }

    #[test]
    fn runtime_blink_moves_mira_and_lumi_eyelids_on_distinct_clocks() {
        let actor = 5;
        let peaks = |seed| {
            (0..3840)
                .filter(|&tick| blink(tick as f32 / 240.0, seed) > 0.95)
                .collect::<Vec<_>>()
        };
        let first = peaks(actor);
        assert!(first.len() >= 16);
        assert_eq!(first, peaks(actor));
        assert_ne!(first, peaks(actor + 1));
        let motion = at(first[0] as f32 / 240.0);

        for (bytes, left) in [
            (
                include_bytes!("../demo_assets/character-showcase/mira.glb").as_slice(),
                "eyeBlinkLeft",
            ),
            (
                include_bytes!("../demo_assets/chibi/lumi.glb").as_slice(),
                "EyeBlinkLeft",
            ),
        ] {
            let model = crate::model::decode_glb(bytes).unwrap();
            let mut secondary = SecondaryPose {
                blink: 1.0,
                ..SecondaryPose::default()
            };
            let mut weights = Vec::new();
            animate(
                &model,
                &mut weights,
                &mut secondary,
                &motion,
                1.0,
                actor,
                false,
            );
            assert_eq!(secondary.blink, 0.0);
            assert!(
                weights
                    .iter()
                    .any(|weight| weight.target == left && weight.weight > 0.95)
            );
            let face_pose = model
                .pose_blended_with_morphs(None, 0.0, None, &[], secondary, &[], &weights)
                .unwrap();
            let neutral = model.pose(None, 0.0).unwrap();
            assert!(
                face_pose
                    .iter()
                    .flatten()
                    .zip(neutral.iter().flatten())
                    .any(|(a, b)| {
                        Vec3::from_array(a.position).distance(Vec3::from_array(b.position)) > 0.003
                    })
            );

            let mut authored = vec![MorphWeight {
                target: left.into(),
                weight: 0.21,
            }];
            animate(
                &model,
                &mut authored,
                &mut secondary,
                &motion,
                1.0,
                actor,
                true,
            );
            assert_eq!(authored.len(), 1);
            assert_eq!(authored[0].weight, 0.21);
        }
    }

    #[test]
    fn gaze_uses_captured_shapes_or_an_articulated_head_and_skips_unknown_rigs() {
        let mira =
            crate::model::decode_glb(include_bytes!("../demo_assets/character-showcase/mira.glb"))
                .unwrap();
        let lumi =
            crate::model::decode_glb(include_bytes!("../demo_assets/chibi/lumi.glb")).unwrap();
        let mut weights = Vec::new();
        let mut secondary = SecondaryPose::default();
        let look = || Motion {
            look_delta: Some([1.1, 0.7, 2.0]),
            ..at(0.2)
        };
        assert!(animate(
            &mira,
            &mut weights,
            &mut secondary,
            &look(),
            1.0,
            1,
            true
        ));
        assert!(
            weights
                .iter()
                .any(|weight| weight.target == "eyeLookInLeft")
        );
        assert!(
            weights
                .iter()
                .any(|weight| weight.target == "eyeLookUpRight")
        );

        weights.clear();
        assert!(!animate(
            &lumi,
            &mut weights,
            &mut secondary,
            &look(),
            1.0,
            2,
            true
        ));
        let mut offsets = Vec::new();
        actor_motion::append_with_eye_tracking(&lumi, &mut offsets, look(), 1.0);
        assert!(
            offsets
                .iter()
                .any(|offset| offset.bone == "Head" && offset.degrees[1].abs() > 1.0)
        );
        assert!(offsets.iter().any(|offset| offset.bone == "Eye.L"));
        let posed = lumi.pose_with_overrides(None, 0.0, &offsets).unwrap();
        let rest = lumi.pose(None, 0.0).unwrap();
        assert!(
            posed
                .iter()
                .flatten()
                .zip(rest.iter().flatten())
                .any(|(a, b)| {
                    Vec3::from_array(a.position).distance(Vec3::from_array(b.position)) > 0.01
                })
        );

        use crate::creature_mesh::{self, Draft, Kind};
        for kind in [Kind::Robot, Kind::Quadruped] {
            let generated = creature_mesh::build(&Draft::for_kind(kind)).unwrap();
            let creature = crate::model::decode_glb(&generated).unwrap();
            assert!(!animate(
                &creature,
                &mut weights,
                &mut secondary,
                &look(),
                1.0,
                3,
                true
            ));
            assert!(weights.is_empty());
        }
        animate(&lumi, &mut weights, &mut secondary, &at(4.1), 1.0, 0, false);
        assert!(
            weights
                .iter()
                .any(|weight| weight.target == "Smile" && (0.2..=0.24).contains(&weight.weight))
        );
    }
}
