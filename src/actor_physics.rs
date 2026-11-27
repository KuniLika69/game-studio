use super::*;
use glam::{Mat4, Vec2};

const MAX_PAW_STEP: f32 = 0.34;

#[derive(Default)]
pub(super) struct ActorSecondary {
    position: Option<Vec3>,
    spring: secondary_motion::State,
    breathing: actor_motion::Breathing,
    pub transfer: actor_motion::WeightTransfer,
    heading: actor_motion::HeadingTurn,
}

impl ActorSecondary {
    pub fn advance(&mut self, dt: f32, position: Vec3, yaw: f32, wind: Vec3, tuning: avatar::ActorTuning, speaking: bool) {
        if dt <= 0.0 {
            return;
        }
        let velocity = self.position.replace(position).map_or(Vec3::ZERO, |old| (position - old) / dt);
        if velocity.length_squared() > 49.0_f32.powi(2) {
            self.spring = secondary_motion::State::default();
        } else {
            self.spring.advance_tuned(dt, velocity, yaw, wind, tuning);
            self.breathing.advance(dt, velocity.x.hypot(velocity.z), speaking, tuning);
        }
    }

    pub fn pose(&self, seconds: f32) -> model::SecondaryPose {
        self.spring.pose(seconds)
    }

    pub fn breath(&self, tuning: avatar::ActorTuning) -> actor_motion::BreathPose {
        self.breathing.pose(tuning)
    }

    pub fn preview(&mut self, dt: f32, time: f32, motion: Motion, wind: Vec3, tuning: avatar::ActorTuning) {
        let velocity = match motion {
            Motion::Walk => Vec3::new((time * 6.0).sin() * 0.22, (time * 13.0).cos() * 0.25, 1.4),
            Motion::Run => Vec3::new((time * 8.0).sin() * 0.50, (time * 17.0).cos() * 0.62, 3.9),
            Motion::Jump => Vec3::Y * (time * 7.3).cos() * 4.0,
            Motion::Idle => Vec3::ZERO,
        };
        self.spring.advance_tuned(dt, velocity, 0.0, wind, tuning);
        self.breathing.advance(dt, velocity.x.hypot(velocity.z), false, tuning);
    }
}

#[derive(Clone, Copy)]
pub(super) struct PlayerStride {
    pub movement: play_settings::Movement3d,
    pub enter_run: f32,
    pub keep_running: f32,
    walk_speed: f32,
    run_speed: f32,
}

impl PlayerStride {
    pub fn rate(self, motion: Motion, speed: f32) -> f32 {
        match motion {
            Motion::Walk => (speed / self.walk_speed).clamp(0.05, 1.64),
            Motion::Run => (speed / self.run_speed).clamp(0.05, 1.64),
            _ => 1.0,
        }
    }
}

impl Studio {
    pub(super) fn measured_player_stride(&self) -> Option<PlayerStride> {
        let avatar = self.scene.avatar.as_ref()?;
        if self.scene.map_2d.is_some() || !avatar.actor_tuning.match_stride {
            return None;
        }
        let (model, token) = self.avatar_asset()?;
        let model_height = model.bounds_max[1] - model.bounds_min[1];
        if model_height <= 0.001 {
            return None;
        }
        let scale = avatar.height / model_height * avatar.animation_speed;
        let mut actors = self.foot_locks.borrow_mut();
        let (_, feet) = actors.entry(0).or_default();
        let id = token | (1_u64 << 63);
        let walk_speed = feet.clip_stride_speed(model, id, avatar.walk?)? * scale;
        let run_speed = feet.clip_stride_speed(model, id, avatar.run?)? * scale;
        if !walk_speed.is_finite()
            || !run_speed.is_finite()
            || walk_speed < 0.25
            || run_speed < walk_speed * 1.18
        {
            return None;
        }
        let mut movement = self.scene.play_settings.movement_3d;
        movement.walk_speed = movement.walk_speed.min(walk_speed * 1.12);
        movement.sprint_speed = movement
            .sprint_speed
            .min(run_speed * 1.14)
            .max(movement.walk_speed);
        movement.acceleration = movement.acceleration.min(movement.walk_speed * 7.0);
        movement.sprint_acceleration = movement.sprint_acceleration.min(movement.sprint_speed * 6.3);
        movement.braking = movement.braking.min(movement.walk_speed * 11.0);
        let room_to_run = movement.sprint_speed - movement.walk_speed;
        Some(PlayerStride {
            movement,
            enter_run: movement.walk_speed + room_to_run * 0.54,
            keep_running: movement.walk_speed + room_to_run * 0.35,
            walk_speed,
            run_speed,
        })
    }

    pub(super) fn step_stride_heading(&mut self, target: f32, dt: f32) {
        self.avatar_heading = self.player_secondary.heading.step(self.avatar_heading, target, dt);
    }

    pub(super) fn stride_turn_input(&self, movement: [f32; 2]) -> ([f32; 2], Option<f32>) {
        let direction = Vec2::from_array(movement);
        if !self.game.grounded || direction.length_squared() <= 0.0025 {
            return (movement, None);
        }
        let target = direction.x.atan2(direction.y).to_degrees();
        let delta = (target - self.avatar_heading + 180.0).rem_euclid(360.0) - 180.0;
        // Forward gait can bank into a turn. Brake towards a pivot on reversals
        // and keep the movement command close to the direction of the walk cycle.
        let angle = (self.avatar_heading + delta.clamp(-19.0, 19.0)).to_radians();
        let (sin, cos) = angle.sin_cos();
        let pivot = ((delta.abs() - 28.0) / 127.0).clamp(0.0, 1.0);
        let scale = 1.0 - 0.91 * pivot * pivot * (3.0 - 2.0 * pivot);
        let command = Vec2::new(sin, cos) * direction.length().min(1.0) * scale;
        (command.to_array(), Some(target))
    }

    pub(super) fn player_gait_transition(
        &self,
        avatar: &Avatar,
        model: &model::Model,
        token: u64,
        from: Motion,
        to: Motion,
        clock: f32,
    ) -> f32 {
        let fallback = avatar.match_gait_phase(model, from, to, clock);
        // Keep the calibrated start/stop phases. Resampling those phases can put a
        // foot in stance across a quick change of heading as the actor accelerates.
        if self.scene.map_2d.is_some()
            || self.avatar_elapsed < self.player_blend_seconds()
            || !matches!((from, to), (Motion::Walk, Motion::Run) | (Motion::Run, Motion::Walk))
        {
            return fallback;
        }
        let Some(next_clip) = avatar.clip(to) else {
            return fallback;
        };
        let previous_clip = avatar.clip(from);
        if previous_clip == Some(next_clip) {
            return clock;
        }
        let mut actors = self.foot_locks.borrow_mut();
        let (_, feet) = actors.entry(0).or_default();
        let scale = avatar.animation_speed;
        feet.transition_time(model, token | (1_u64 << 63), previous_clip, clock * scale, next_clip, fallback * scale)
            .map_or(fallback, |time| time / scale)
    }

    pub(super) fn advance_player_secondary(&mut self, dt: f32, motion: [f32; 2]) {
        let position = self.actor_position();
        let heading = self
            .seating
            .as_ref()
            .map_or(self.avatar_heading, Seating::heading);
        let yaw = heading + self.scene.avatar.as_ref().map_or(0.0, |avatar| avatar.yaw);
        let wind = self.player_hair_wind();
        let tuning = self.scene.avatar.as_ref().map_or_else(avatar::ActorTuning::default, |actor| actor.actor_tuning);
        let speaking = self.active_speaker() == Some(story::Speaker::Player);
        let rise = self.player_secondary.position.map_or(0.0, |before| position.y - before.y);
        let desired = Vec2::from_array(motion);
        // The feet follow the physical turn. Let the chest anticipate the input
        // while the avatar is still braking into the new direction.
        let intended = self.door_hand_heading().unwrap_or_else(|| {
            if desired.length_squared() > 0.0025 {
                desired.x.atan2(desired.y).to_degrees()
            } else {
                self.game.yaw
            }
        });
        let heading_delta = (intended - heading + 180.0).rem_euclid(360.0) - 180.0;
        self.player_secondary.transfer.advance(dt, self.game.horizontal_speed(), heading_delta, rise, self.game.grounded);
        self.player_secondary.advance(dt, position, yaw, wind, tuning, speaking);
    }

    pub(super) fn player_hair_wind(&self) -> Vec3 {
        self.scene.environment.as_ref().map_or(Vec3::ZERO, |_| {
            let height = self
                .scene
                .avatar
                .as_ref()
                .map_or(1.6, |avatar| avatar.height);
            self.environment_wind(self.actor_position() + Vec3::Y * (0.87 * height))
        })
    }

    pub(super) fn advance_npc_secondary(&mut self, dt: f32) {
        let windy = self.scene.environment.is_some();
        let speaking = (self.active_speaker() == Some(story::Speaker::Npc)).then(|| self.story_session.active_npc()).flatten();
        for npc in &self.scene.npcs {
            if let Some(pose) = self.npc_simulation.state(npc.id) {
                let position = Vec3::from_array(pose.position());
                let wind = if windy {
                    self.environment_wind(position + Vec3::Y * (0.87 * npc.avatar.height))
                } else {
                    Vec3::ZERO
                };
                let secondary = self.npc_secondary.entry(npc.id).or_insert_with(|| ActorSecondary {
                    breathing: actor_motion::Breathing::for_character(npc.id),
                    ..ActorSecondary::default()
                });
                secondary.advance(
                    dt,
                    position,
                    pose.yaw + npc.avatar.yaw,
                    wind,
                    npc.avatar.actor_tuning,
                    speaking == Some(npc.id),
                );
            }
        }
    }

    pub(super) fn apply_foot_locks(
        &self,
        actor_id: u64,
        draw: &mut ModelDraw<'_>,
        floor: f32,
        grounded: bool,
        motion: Motion,
    ) {
        if !self.game_running || self.view != View::Three {
            return;
        }
        let mut locks = self.foot_locks.borrow_mut();
        let (updated, lock) = locks.entry(actor_id).or_default();
        let elapsed = self.live_seconds - *updated;
        if elapsed > 0.14 || elapsed < 0.0 {
            lock.reset();
        }
        *updated = self.live_seconds;
        let [rx, ry, rz] = draw.rotation.map(f32::to_radians);
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::from_array(draw.scale).max(Vec3::splat(0.001)),
            Quat::from_euler(EulerRot::XYZ, rx, ry, rz),
            Vec3::from_array(draw.position),
        );
        let mut input = model::FootLockInput {
            model: draw.model,
            identity: draw.token,
            clip: draw.clip,
            elapsed: draw.elapsed,
            previous: draw
                .blend
                .map(|blend| (blend.clip, blend.elapsed, blend.weight)),
            offsets: &draw.bone_offsets,
            world_from_model: transform,
            ground_height: floor,
            // Multiple viewport or capture passes in one simulation step reuse the same contact.
            dt: elapsed.clamp(0.0, self.play_dt),
            enabled: grounded
                && (matches!(motion, Motion::Walk | Motion::Run)
                    || (motion == Motion::Idle && draw.blend.is_some())),
        };
        let avatar = if actor_id == 0 {
            self.scene.avatar.as_ref()
        } else {
            self.scene
                .npcs
                .iter()
                .find(|actor| actor.id == actor_id)
                .map(|actor| &actor.avatar)
        };
        if avatar
            .and_then(|actor| actor.generated)
            .is_some_and(|recipe| recipe.kind == creature_mesh::Kind::Quadruped)
        {
            input.enabled = grounded
                && motion != Motion::Jump
                && (actor_id == 0
                    || self
                        .npc_simulation
                        .state(actor_id)
                        .is_some_and(npcs::NpcState::grounded));
            if !input.enabled {
                lock.quadruped.reset();
                return;
            }
            let center = Vec2::new(draw.position[0], draw.position[2]);
            let extent =
                Vec3::from_array(draw.model.bounds_max) - Vec3::from_array(draw.model.bounds_min);
            let radius = (extent * Vec3::from_array(draw.scale)).max_element() * 1.10 + 0.15;
            let terrain = PawTerrain::new(self, floor, center, radius);
            let (rotations, pelvis) = lock.quadruped.solve(input, |xz| terrain.at(xz));
            draw.bone_offsets.extend(rotations);
            draw.bone_translations.extend(pelvis);
            return;
        }
        let corrections = lock.corrections(input);
        draw.bone_offsets.extend(corrections);
        draw.bone_translations
            .extend_from_slice(lock.translations());
    }

    pub(crate) fn quadruped_contact_report(&self, actor_id: u64) -> [Option<model::PawContact>; 4] {
        self.foot_locks
            .borrow()
            .get(&actor_id)
            .map_or([None; 4], |(_, lock)| lock.quadruped.reports())
    }
}

impl Studio {
    pub(super) fn route_surface_height(&self, point: Vec2, height: f32) -> f32 {
        PawTerrain::new(self, height, point, 0.35).at(point).height
    }
}

struct PawTerrain {
    floor: f32,
    solids: Vec<world::Collider>,
    faces: Vec<TerrainFace>,
}

impl PawTerrain {
    fn new(studio: &Studio, floor: f32, center: Vec2, radius: f32) -> Self {
        let min = center - Vec2::splat(radius);
        let max = center + Vec2::splat(radius);
        let overlaps = |a: Vec2, b: Vec2| a.cmple(max).all() && min.cmple(b).all();
        let nearby = |bounds: &world::Collider| {
            overlaps(
                Vec2::new(bounds.min[0], bounds.min[2]),
                Vec2::new(bounds.max[0], bounds.max[2]),
            )
        };
        let mut faces = Vec::new();
        let mut coarse_terrain_bounds = Vec::new();
        for item in studio.scene.world.models.iter().filter(|item| {
            (item.paw_ground
                || studio
                    .scene
                    .village
                    .as_ref()
                    .is_some_and(|village| village.supports_surface(item.id)))
                && studio.interactions.object_active(item.id)
        }) {
            let Some(asset) = studio.assets.get(&item.source) else {
                continue;
            };
            let model = asset.model.as_ref();
            let bounds = item.bounds(model);
            if !nearby(&bounds) {
                continue;
            }
            if item.solid && item.collider.is_none() {
                coarse_terrain_bounds.push(bounds);
            }
            let [x, y, z] = item.rotation.map(f32::to_radians);
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::from_array(item.scale),
                Quat::from_euler(EulerRot::XYZ, x, y, z),
                Vec3::from_array(item.visual_position()),
            );
            for primitive in &model.primitives {
                for index in primitive.indices.chunks_exact(3) {
                    let [a, b, c] = std::array::from_fn(|i| {
                        transform.transform_point3(Vec3::from_array(
                            primitive.vertices[index[i] as usize].position,
                        ))
                    });
                    let low = a.min(b).min(c);
                    let high = a.max(b).max(c);
                    if low.y <= floor + MAX_PAW_STEP
                        && high.y >= 0.0
                        && overlaps(Vec2::new(low.x, low.z), Vec2::new(high.x, high.z))
                        && let Some(face) = TerrainFace::new(a, b, c)
                    {
                        faces.push(face);
                    }
                }
            }
        }
        let solids = studio.colliders().into_iter().filter(|bounds| {
            nearby(bounds) && !coarse_terrain_bounds.contains(bounds)
        }).collect();
        Self {
            floor,
            solids,
            faces,
        }
    }

    fn at(&self, xz: Vec2) -> model::GroundHit {
        // An overhead platform is not a ground contact for an actor underneath it.
        let ceiling = self.floor + MAX_PAW_STEP;
        let under = |bounds: &world::Collider| {
            xz.x >= bounds.min[0]
                && xz.x <= bounds.max[0]
                && xz.y >= bounds.min[2]
                && xz.y <= bounds.max[2]
        };
        let mut hit = model::GroundHit {
            height: 0.0,
            normal: Vec3::Y,
        };
        for platform in self.solids.iter().filter(|bounds| under(bounds)) {
            if (hit.height..=ceiling).contains(&platform.max[1]) {
                hit.height = platform.max[1];
            }
        }
        for face in &self.faces {
            if let Some(surface) = face.at(xz)
                && (hit.height..=ceiling).contains(&surface.height)
            {
                hit = surface;
            }
        }
        hit
    }
}

struct TerrainFace {
    origin: Vec3,
    edges: [Vec3; 2],
    area_inv: f32,
    normal: Vec3,
}

impl TerrainFace {
    fn new(a: Vec3, b: Vec3, c: Vec3) -> Option<Self> {
        let ab = b - a;
        let ac = c - a;
        let cross = ac.cross(ab);
        let area = cross.y;
        if area.abs() <= 1e-8 {
            return None;
        }
        let normal = cross.normalize() * area.signum();
        (normal.y > 0.48).then_some(Self {
            origin: a,
            edges: [ab, ac],
            area_inv: area.recip(),
            normal,
        })
    }

    fn at(&self, xz: Vec2) -> Option<model::GroundHit> {
        let [ab, ac] = self.edges;
        let from = xz - Vec2::new(self.origin.x, self.origin.z);
        let u = from.perp_dot(Vec2::new(ac.x, ac.z)) * self.area_inv;
        let v = Vec2::new(ab.x, ab.z).perp_dot(from) * self.area_inv;
        if u < -0.0001 || v < -0.0001 || u + v > 1.0001 {
            return None;
        }
        Some(model::GroundHit {
            height: self.origin.y + u * ab.y + v * ac.y,
            normal: self.normal,
        })
    }
}

#[cfg(test)]
mod paw_terrain_tests {
    use super::*;

    #[test]
    fn terrain_sample_matches_the_actual_triangular_surface() {
        let triangle = [
            Vec3::new(0.0, 0.06, 0.0),
            Vec3::new(4.0, 0.42, 0.0),
            Vec3::new(0.0, 0.18, 4.0),
        ];
        for [a, b, c] in [triangle, [triangle[2], triangle[1], triangle[0]]] {
            let surface = TerrainFace::new(a, b, c).unwrap();
            let hit = surface.at(Vec2::new(1.0, 2.0)).unwrap();
            assert!((hit.height - 0.21).abs() < 1e-6);
            assert!(hit.normal.dot(Vec3::new(-0.09, 1.0, -0.03).normalize()) > 0.9999);
            assert!(surface.at(Vec2::new(3.0, 3.0)).is_none());
        }
    }
}
