use super::*;
use crate::{model::ArmPose, world::Interaction};

const REACH_SECONDS: f32 = 0.34;
const GRIP_SECONDS: f32 = 0.080;
const PUSH_SECONDS: f32 = 0.10;
const RELEASE_SECONDS: f32 = 0.22;
const CONTACT_TOLERANCE: f32 = 0.020;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Approach,
    Reach,
    Grip,
    Push,
    Release,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DoorHand {
    id: u64,
    side: f32,
    stand: Vec3,
    heading: f32,
    phase: Phase,
    elapsed: f32,
    total: f32,
    release_anchor: Vec3,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) struct ContactMeasure {
    pub door: u64,
    pub seconds_from_action: f32,
    pub gap_m: f32,
    pub leaf_fraction_at_contact: f32,
    pub palm: [f32; 3],
    pub lever: [f32; 3],
    pub wrist: [f32; 3],
    pub index_tip: Option<[f32; 3]>,
    pub thumb_tip: Option<[f32; 3]>,
    pub pressure_seconds: Option<f32>,
}

impl Studio {
    pub(super) fn start_door_hand(&mut self, position: [f32; 3]) -> bool {
        if self.view != View::Three
            || self.scene.map_2d.is_some()
            || self
                .avatar_asset()
                .is_none_or(|(model, _)| !model.has_door_hand())
        {
            return false;
        }
        let Some(Interaction::Door { id, opening: true }) = self
            .scene
            .world
            .interaction_near(&self.interactions, position)
        else {
            return false;
        };
        let Some(door) = self.scene.world.door(id) else {
            return false;
        };
        if !door.settings.hand_contact {
            return false;
        }
        let fraction = self.interactions.fraction(&door);
        let side = door.approach_side(Vec3::from_array(position), fraction);
        let normal = door.lever_normal(fraction, side);
        let lever = door.lever(fraction, side);
        let forward = -normal;
        let Some((model, _)) = self.avatar_asset() else {
            return false;
        };
        let Some(avatar) = &self.scene.avatar else {
            return false;
        };
        let heading = forward.x.atan2(forward.z).to_degrees() - avatar.yaw;
        let (position, rotation, scale) = avatar.transform(model, Vec3::ZERO, heading);
        let shoulder = model
            .door_shoulder(ArmPose {
                clip: avatar.idle,
                time: 0.0,
                blend: None,
                translations: &[],
                world: world_matrix(position, rotation, scale),
            })
            .unwrap_or(Vec3::ZERO);
        let tangent = normal.cross(Vec3::Y);
        let stand = lever + normal * 0.35 - tangent * shoulder.dot(tangent);
        self.door_hand = Some(DoorHand {
            id,
            side,
            stand,
            heading,
            phase: Phase::Approach,
            elapsed: 0.0,
            total: 0.0,
            release_anchor: lever,
        });
        self.door_contact = None;
        self.walk_target = None;
        self.interaction_message = None;
        true
    }

    pub(super) fn door_hand_controls(&mut self, dt: f32) -> Option<playback::Intent> {
        let mut hand = self.door_hand.take()?;
        if self.view != View::Three || self.free_camera {
            return None;
        }
        hand.total += dt;
        hand.elapsed += dt;
        let mut movement = glam::Vec2::ZERO;
        if hand.phase == Phase::Approach {
            let delta = hand.stand - self.actor_position();
            let delta = glam::Vec2::new(delta.x, delta.z);
            if delta.length() <= 0.065 && self.game.horizontal_speed() < 0.24 {
                hand.phase = Phase::Reach;
                hand.elapsed = 0.0;
            } else if hand.elapsed >= 3.5 {
                self.interaction_message = Some(("Подойди прямо к дверной ручке.".into(), 2.8));
                return None;
            } else {
                movement = (delta / 0.30).clamp_length_max(1.0);
            }
        }
        self.door_hand = Some(hand);
        Some(playback::Intent {
            dt,
            movement: movement.to_array(),
            ..Default::default()
        })
    }

    pub(super) fn door_hand_heading(&self) -> Option<f32> {
        self.door_hand
            .filter(|hand| hand.phase != Phase::Approach)
            .map(|hand| hand.heading)
    }

    pub(super) fn advance_door_palm(&mut self) {
        let Some(mut hand) = self.door_hand else {
            return;
        };
        match hand.phase {
            Phase::Approach => {}
            Phase::Reach if hand.elapsed >= REACH_SECONDS => {
                let contact = (|| {
                    let door = self.scene.world.door(hand.id)?;
                    let fraction = self.interactions.fraction(&door);
                    let lever = door.lever(fraction, hand.side);
                    let draw = self.avatar_draw_without_foot()?;
                    let markers = draw.model.hand_markers(arm_pose(&draw), &draw.bone_offsets)?;
                    Some(ContactMeasure {
                        door: hand.id,
                        seconds_from_action: hand.total,
                        gap_m: markers.palm.distance(lever),
                        leaf_fraction_at_contact: fraction,
                        palm: markers.palm.to_array(),
                        lever: lever.to_array(),
                        wrist: markers.wrist.to_array(),
                        index_tip: markers.index_tip.map(|p| p.to_array()),
                        thumb_tip: markers.thumb_tip.map(|p| p.to_array()),
                        pressure_seconds: None,
                    })
                })();
                if let Some(contact) = contact.filter(|sample| sample.gap_m <= CONTACT_TOLERANCE) {
                    self.door_contact = Some(contact);
                    hand.phase = Phase::Grip;
                    hand.elapsed = 0.0;
                } else if hand.elapsed > REACH_SECONDS + 0.25 {
                    self.interaction_message =
                        Some(("Не дотянулся до ручки. Подойди ближе.".into(), 2.8));
                    self.door_hand = None;
                    return;
                }
            }
            Phase::Grip if hand.elapsed >= GRIP_SECONDS => {
                if let Some(door) = self.scene.world.door(hand.id) {
                    self.interactions.toggle_player(&door);
                }
                if let Some(contact) = &mut self.door_contact {
                    contact.pressure_seconds = Some(hand.total - contact.seconds_from_action);
                    let lever = Vec3::from_array(contact.lever);
                    self.play_door_latch(lever);
                }
                hand.phase = Phase::Push;
                hand.elapsed = 0.0;
            }
            Phase::Push if hand.elapsed >= PUSH_SECONDS => {
                if let Some(door) = self.scene.world.door(hand.id) {
                    hand.release_anchor = door.lever(self.interactions.fraction(&door), hand.side);
                }
                hand.phase = Phase::Release;
                hand.elapsed = 0.0;
            }
            Phase::Release if hand.elapsed >= RELEASE_SECONDS => {
                self.door_hand = None;
                return;
            }
            _ => {}
        }
        self.door_hand = Some(hand);
    }

    pub(super) fn pose_door_hand(&self, draw: &mut ModelDraw<'_>) {
        let Some(hand) = self.door_hand else {
            return;
        };
        let Some(door) = self.scene.world.door(hand.id) else {
            return;
        };
        let fraction = self.interactions.fraction(&door);
        let target = door.lever(fraction, hand.side);
        let normal = door.lever_normal(fraction, hand.side);
        let (target, weight) = match hand.phase {
            Phase::Approach => return,
            Phase::Reach => (target, ease(hand.elapsed / REACH_SECONDS)),
            Phase::Grip => (target, 1.0),
            Phase::Push => (target, 1.0),
            Phase::Release => (
                hand.release_anchor,
                1.0 - ease(hand.elapsed / RELEASE_SECONDS),
            ),
        };
        let pose = ArmPose {
            clip: draw.clip,
            time: draw.elapsed,
            blend: draw
                .blend
                .map(|blend| (blend.clip, blend.elapsed, blend.weight)),
            translations: &draw.bone_translations,
            world: world_transform(draw),
        };
        draw.model
            .reach_door_handle(pose, &mut draw.bone_offsets, target, -normal, weight);
    }
}

fn ease(fraction: f32) -> f32 {
    let x = fraction.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn world_transform(draw: &ModelDraw<'_>) -> glam::Mat4 {
    world_matrix(draw.position, draw.rotation, draw.scale)
}

fn world_matrix(position: [f32; 3], rotation: [f32; 3], scale: [f32; 3]) -> glam::Mat4 {
    let [x, y, z] = rotation.map(f32::to_radians);
    glam::Mat4::from_scale_rotation_translation(
        Vec3::from_array(scale),
        Quat::from_euler(EulerRot::XYZ, x, y, z),
        Vec3::from_array(position),
    )
}

fn arm_pose<'a>(draw: &'a ModelDraw<'_>) -> ArmPose<'a> {
    ArmPose {
        clip: draw.clip,
        time: draw.elapsed,
        blend: draw
            .blend
            .map(|blend| (blend.clip, blend.elapsed, blend.weight)),
        translations: &draw.bone_translations,
        world: world_transform(draw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_hand_touches_stationary_leaf_before_it_swings() {
        for source in [assets::Source::Ryan, assets::Source::Emma] {
            let mut studio = Studio::default();
            studio.assets.ensure(&source, None).unwrap();
            let model = &studio.assets.get(&source).unwrap().model;
            let mut scene = Scene::empty();
            scene.avatar = Some(Avatar::new(source, "Герой".into(), model));
            scene.world.add_building_at([0.0, 0.0, -5.0]).unwrap();
            let building = &mut scene.world.buildings[0];
            building.door = Some(world::DoorSettings {
                hand_contact: true,
                ..Default::default()
            });
            let door = building.doors().next().unwrap();
            let id = door.id;
            let at = door.interaction_position();
            let ctx = egui::Context::default();
            let mut inputs = crate::proof_input::Driver::new([940, 640]);
            studio.standalone = true;
            studio.set_scene(scene, None, "");
            studio.start_play(&ctx, false);
            studio.game.x = at[0];
            studio.game.z = at[2] + 1.12;
            let events = crate::proof_input::key(Key::E, egui::Modifiers::NONE);
            let _ = ctx.run(inputs.raw(events), |ctx| studio.run_frame(ctx));
            assert!(studio.door_hand.is_some());
            let mut moved = false;
            for _ in 0..240 {
                let _ = ctx.run(inputs.raw(Vec::new()), |ctx| studio.run_frame(ctx));
                let door = studio.scene.world.door(id).unwrap();
                let fraction = studio.interactions.fraction(&door);
                if studio.door_contact.is_none() {
                    assert_eq!(fraction, 0.0);
                }
                if fraction > 0.99 {
                    moved = true;
                    break;
                }
            }
            let contact = studio.door_contact.unwrap_or_else(|| {
                panic!(
                    "Не было касания. {} {:?}",
                    studio
                        .interaction_message
                        .as_ref()
                        .map_or("", |(text, _)| text),
                    studio.game_position()
                )
            });
            assert_eq!(contact.leaf_fraction_at_contact, 0.0);
            assert!(contact.gap_m < CONTACT_TOLERANCE);
            assert!(moved);
            eprintln!(
                "{:?} contact {contact:?}",
                studio.scene.avatar.as_ref().unwrap().source
            );
        }
    }
}
