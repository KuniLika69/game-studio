use std::collections::{BTreeMap, HashSet};
#[cfg(test)]
use std::fs;
use std::path::Path;

use glam::{EulerRot, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use studio_core::render_settings::PostProcess;

use crate::ImportError;
use crate::world::{Collider, InteractionSnapshot, World};
use crate::{avatar::Avatar, map2d, npcs::Npc, rules, story::Story};

pub use studio_core::project::write_atomic;
pub use studio_core::scene_parts::{
    Animation, Effect, Keyframe, MAX_KEYS, MAX_PARTS, MAX_SPRITE_FRAMES, MAX_SPRITE_PIXELS,
    Part, PartMotion, Shape, SpriteArt, SpritePixel,
};

const PROJECT_VERSION: u32 = 21;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub position: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotor_angle: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tactics: Option<crate::tactics::Snapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<crate::environment::Snapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physics: Option<crate::body_physics::Snapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_respawn: Option<[u16; 2]>,
    pub story: crate::story::ProgressSnapshot,
    #[serde(default)]
    pub interactions: InteractionSnapshot,
    #[serde(default)]
    pub rules: rules::Snapshot,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) visited: BTreeMap<u32, Checkpoint>,
}

impl Checkpoint {
    pub(crate) fn validate_for(&self, scenes: &crate::linked_scenes::LinkedScenes) -> Result<(), String> {
        self.validate_location()?;
        let active = self.scene_id.unwrap_or(crate::linked_scenes::ROOT_SCENE_ID);
        if !scenes.contains(active)
            || self.visited.len() > scenes.entries.len()
            || self.visited.iter().any(|(&id, state)| {
                id == active || !scenes.contains(id) || state.scene_id != Some(id) || !state.visited.is_empty()
            })
        {
            return Err("Сохранение ссылается на отсутствующую или повторную локацию.".into());
        }
        for location in self.visited.values() {
            location.validate_location()?;
        }
        Ok(())
    }

    fn validate_location(&self) -> Result<(), ImportError> {
        if !self.position.iter().all(|value| value.is_finite() && value.abs() <= 100_000.0)
            || self.position[1] < 0.0
        {
            return Err("Некорректная позиция сохранённой игры.".into());
        }
        if self.rotor_angle.is_some_and(|angle| !(0.0..std::f32::consts::TAU).contains(&angle)) {
            return Err("Некорректное состояние ветряной мельницы.".into());
        }
        if let Some(snapshot) = &self.environment {
            snapshot.validate()?;
        }
        if let Some(snapshot) = &self.tactics {
            snapshot.validate()?;
        }
        if let Some(snapshot) = &self.physics {
            snapshot.validate().map_err(|error| format!("Некорректное сохранение физики. {error:?}"))?;
        }
        self.story.validate()?;
        self.interactions.validate()?;
        self.rules.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    version: u32,
    pub parts: Vec<Part>,
    pub animation: Animation,
    pub effect: Effect,
    #[serde(default, skip_serializing_if = "crate::effects::Library::is_default")]
    pub effects: crate::effects::Library,
    #[serde(default, skip_serializing_if = "crate::audio_project::Library::is_default")]
    pub audio: crate::audio_project::Library,
    #[serde(default)]
    pub sprite_pixels: Vec<SpritePixel>,
    #[serde(default)]
    pub sprite_art: SpriteArt,
    #[serde(default)]
    pub world: World,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub village: Option<crate::village::State>,
    #[serde(default)]
    pub avatar: Option<Avatar>,
    #[serde(default)]
    pub npcs: Vec<Npc>,
    #[serde(default)]
    pub story: Story,
    #[serde(default)]
    pub rules: rules::RuleSet,
    #[serde(
        default,
        skip_serializing_if = "crate::physics_project::PhysicsScene::is_default"
    )]
    pub physics: crate::physics_project::PhysicsScene,
    #[serde(
        default,
        skip_serializing_if = "crate::linked_scenes::LinkedScenes::is_default"
    )]
    pub linked_scenes: crate::linked_scenes::LinkedScenes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) wind_rotor: Option<crate::wind_rotor::Rotor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_2d: Option<map2d::Map>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tactics: Option<crate::tactics::Scenario>,
    #[serde(
        default,
        skip_serializing_if = "crate::assets::materials::Lighting::is_default"
    )]
    pub lighting: crate::assets::materials::Lighting,
    #[serde(default, skip_serializing_if = "PostProcess::is_default")]
    pub post_process: PostProcess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<crate::environment::Environment>,
    #[serde(
        default,
        skip_serializing_if = "crate::play_settings::PlaySettings::is_default"
    )]
    pub play_settings: crate::play_settings::PlaySettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<Checkpoint>,
}

impl Scene {
    pub fn empty() -> Self {
        Self {
            version: PROJECT_VERSION,
            parts: Vec::new(),
            animation: Animation::default(),
            effect: Effect::default(),
            effects: crate::effects::Library::default(),
            audio: crate::audio_project::Library::default(),
            sprite_pixels: Vec::new(),
            sprite_art: SpriteArt::default(),
            world: World::default(),
            village: None,
            avatar: None,
            npcs: Vec::new(),
            story: Story::default(),
            rules: rules::RuleSet::default(),
            physics: crate::physics_project::PhysicsScene::default(),
            linked_scenes: crate::linked_scenes::LinkedScenes::default(),
            wind_rotor: None,
            map_2d: None,
            tactics: None,
            lighting: crate::assets::materials::Lighting::default(),
            post_process: PostProcess::default(),
            environment: None,
            play_settings: crate::play_settings::PlaySettings::default(),
            checkpoint: None,
        }
    }

    pub fn add_part(&mut self, shape: Shape) -> u64 {
        let max_id = self.parts.iter().map(|part| part.id).max().unwrap_or(0);
        let id = max_id.checked_add(1).unwrap_or_else(|| {
            (1..)
                .find(|id| self.parts.iter().all(|part| part.id != *id))
                .expect("the scene has finitely many parts")
        });
        let name = match shape {
            Shape::Box => "Блок",
            Shape::Sphere => "Сфера",
            Shape::Cylinder => "Цилиндр",
        };
        self.parts.push(Part {
            id,
            name: name.to_owned(),
            shape,
            position: [0.0, 1.0, 0.45],
            rotation: [0.0; 3],
            size: [0.32; 3],
            color: [0.37, 0.74, 0.86],
            motion: PartMotion::Static,
        });
        id
    }

    pub fn remove_part(&mut self, id: u64) -> bool {
        let Some(index) = self.parts.iter().position(|part| part.id == id) else {
            return false;
        };
        self.parts.remove(index);
        self.animation.keys.retain(|key| key.part_id != id);
        true
    }

    pub fn sample(&self, time: f32) -> Vec<Part> {
        self.parts
            .iter()
            .map(|part| {
                let pose = self.animation.sample_part(part.id, time);
                Part {
                    position: std::array::from_fn(|i| part.position[i] + pose.position[i]),
                    rotation: std::array::from_fn(|i| part.rotation[i] + pose.rotation[i]),
                    ..part.clone()
                }
            })
            .collect()
    }

    pub fn sample_game(&self, time: f32, game: &Game) -> Vec<Part> {
        let mut parts = self.sample(time);
        let swing = game.phase.sin() * game.move_blend;
        if swing.abs() < 0.001 {
            return parts;
        }

        let top = |part: &Part| Vec3::from_array(part.position) + Vec3::Y * (part.size[1] * 0.5);
        let pivot_for = |motion| {
            parts
                .iter()
                .filter(|part| part.motion == motion)
                .map(top)
                .max_by(|a, b| a.y.total_cmp(&b.y))
        };
        let left_shoulder = pivot_for(PartMotion::LeftArm);
        let right_shoulder = pivot_for(PartMotion::RightArm);
        let left_hip = pivot_for(PartMotion::LeftLeg);
        let right_hip = pivot_for(PartMotion::RightLeg);

        for part in &mut parts {
            let (angle, pivot) = match part.motion {
                PartMotion::Static => continue,
                PartMotion::LeftArm => (0.65 * swing, left_shoulder.unwrap_or_else(|| top(part))),
                PartMotion::RightArm => {
                    (-0.65 * swing, right_shoulder.unwrap_or_else(|| top(part)))
                }
                PartMotion::LeftLeg => (-0.48 * swing, left_hip.unwrap_or_else(|| top(part))),
                PartMotion::RightLeg => (0.48 * swing, right_hip.unwrap_or_else(|| top(part))),
                PartMotion::LeftFoot => (-0.48 * swing, left_hip.unwrap_or_else(|| top(part))),
                PartMotion::RightFoot => (0.48 * swing, right_hip.unwrap_or_else(|| top(part))),
            };
            let rotation = Quat::from_rotation_x(angle);
            let position = Vec3::from_array(part.position);
            part.position = (pivot + rotation * (position - pivot)).to_array();
            let [x, y, z] = part.rotation.map(f32::to_radians);
            let base = Quat::from_euler(EulerRot::XYZ, x, y, z);
            let (x, y, z) = (rotation * base).to_euler(EulerRot::XYZ);
            part.rotation = [x, y, z].map(f32::to_degrees);
        }
        parts
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        studio_core::project::save(path, self)
    }

    pub fn load(path: &Path) -> Result<Self, ImportError> {
        let mut scene: Self = studio_core::project::load(path)?;
        scene.migrate();
        scene
            .validate_import()
            .map_err(|error| error.with_file(path))?;
        Ok(scene)
    }

    fn migrate(&mut self) {
        if self.version == 1 {
            self.assign_demo_motion();
        }
        if matches!(self.version, 1..=9) {
            self.sprite_art.use_on_plan =
                self.sprite_art.drawn_only || !self.sprite_pixels.is_empty();
        }
        if matches!(self.version, 1..=11) {
            self.story.migrate_answer_ids();
        }
        if matches!(self.version, 1..=20) {
            self.version = PROJECT_VERSION;
        }
        for link in &mut self.linked_scenes.entries {
            link.scene.migrate();
        }
    }

    fn assign_demo_motion(&mut self) {
        for part in &mut self.parts {
            part.motion = match part.name.as_str() {
                "Левая рука" => PartMotion::LeftArm,
                "Правая рука" => PartMotion::RightArm,
                "Левая нога" => PartMotion::LeftLeg,
                "Правая нога" => PartMotion::RightLeg,
                "Левая стопа" => PartMotion::LeftFoot,
                "Правая стопа" => PartMotion::RightFoot,
                _ => part.motion,
            };
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        self.validate_import().map_err(Into::into)
    }

    pub(crate) fn validate_import(&self) -> Result<(), ImportError> {
        self.validate_content()?;
        self.linked_scenes.validate_for(self).map_err(Into::into)
    }

    pub(crate) fn validate_content(&self) -> Result<(), ImportError> {
        if let Some(village) = &self.village {
            village.validate()?;
        }
        if self.wind_rotor.as_ref().is_some_and(|rotor| !rotor.valid()) {
            return Err("Некорректное состояние ветряной мельницы.".into());
        }
        if self.version != PROJECT_VERSION {
            return Err(ImportError::new(
                "Версия проекта",
                format!("Версия проекта {} пока не поддерживается", self.version),
            ));
        }
        if let Some(map) = &self.map_2d {
            map.validate()?;
        }
        if let Some(scenario) = &self.tactics {
            if !self
                .map_2d
                .as_ref()
                .is_some_and(|map| map.mode == map2d::Mode::Top)
            {
                return Err("Для пошаговой тактики требуется карта 2D сверху.".into());
            }
            scenario.validate()?;
        }
        if let Some(config) = &self.environment {
            config.validate()?;
        }
        self.physics.validate()?;
        self.effects.validate()?;
        self.audio.validate()?;
        self.story.validate_voices(&self.audio)?;
        if !self.lighting.valid() {
            return Err("Некорректные настройки света.".into());
        }
        if !self.post_process.valid() {
            return Err(ImportError::new(
                "Постобработка",
                "Экспозиция или порог свечения вне допустимого диапазона.",
            ));
        }
        crate::play_settings::validate(&self.play_settings)?;
        if let Some(avatar) = &self.avatar {
            avatar.validate()?;
        }
        if let Some(checkpoint) = &self.checkpoint {
            checkpoint.validate_for(&self.linked_scenes)?;
        }
        Npc::validate_all(&self.npcs)?;
        self.story.validate()?;
        self.rules.validate()?;
        self.story
            .validate_npcs(self.npcs.iter().map(|npc| npc.id))?;
        if self.parts.len() > MAX_PARTS || self.animation.keys.len() > MAX_KEYS {
            return Err("В проекте слишком много деталей или ключей анимации".into());
        }
        let mut ids = HashSet::with_capacity(self.parts.len());
        for part in &self.parts {
            if part.id == 0
                || !ids.insert(part.id)
                || !in_range(part.position, -100.0, 100.0)
                || !in_range(part.rotation, -3_600.0, 3_600.0)
                || !in_range(part.size, 0.01, 100.0)
                || !in_range(part.color, 0.0, 1.0)
            {
                return Err(ImportError::new(
                    format!("Деталь «{}»", part.name),
                    "Некорректные параметры детали.",
                ));
            }
        }

        let duration = self.animation.duration;
        if !(0.05..=60.0).contains(&duration)
            || !(0.0..=duration).contains(&self.animation.event_time)
        {
            return Err("Некорректная длина анимации или метка эффекта".into());
        }
        for key in &self.animation.keys {
            if !ids.contains(&key.part_id)
                || !(0.0..=duration).contains(&key.time)
                || !in_range(key.position, -100.0, 100.0)
                || !in_range(key.rotation, -3_600.0, 3_600.0)
            {
                return Err("Некорректные параметры ключа анимации".into());
            }
        }

        let effect = &self.effect;
        if !in_range(effect.color, 0.0, 1.0)
            || effect.count > 1_024
            || !(0.0..=10.0).contains(&effect.intensity)
            || !(0.05..=10.0).contains(&effect.duration)
        {
            return Err("Некорректные параметры эффекта".into());
        }
        if !(1..=MAX_SPRITE_FRAMES).contains(&self.sprite_art.frames)
            || !(1..=24).contains(&self.sprite_art.fps)
            || self.sprite_pixels.len() > MAX_SPRITE_PIXELS
            || self.sprite_pixels.iter().any(|pixel| {
                pixel.direction >= 4
                    || pixel.x >= 96
                    || pixel.y >= 96
                    || pixel
                        .frame
                        .is_some_and(|frame| frame >= self.sprite_art.frames)
            })
        {
            return Err("Некорректные координаты дорисовок спрайта".into());
        }
        self.world.validate()
    }
}

fn in_range(values: [f32; 3], min: f32, max: f32) -> bool {
    values.into_iter().all(|value| (min..=max).contains(&value))
}

impl Default for Scene {
    fn default() -> Self {
        let mut scene = Self::empty();
        scene.world = World::demo();
        let blue = [0.21, 0.67, 0.79];
        let dark = [0.12, 0.23, 0.30];
        let orange = [1.0, 0.55, 0.20];
        let light = [0.55, 0.96, 1.0];

        let mut add = |name: &str, shape, position, size, color| {
            let id = scene.add_part(shape);
            let part = scene.parts.last_mut().expect("part was just added");
            part.name = name.to_owned();
            part.position = position;
            part.size = size;
            part.color = color;
            id
        };

        let body = add(
            "Корпус",
            Shape::Box,
            [0.0, 1.26, 0.0],
            [0.86, 0.65, 0.51],
            blue,
        );
        let chest = add(
            "Панель",
            Shape::Box,
            [0.0, 1.29, 0.27],
            [0.43, 0.22, 0.07],
            dark,
        );
        let core = add(
            "Индикатор",
            Shape::Box,
            [0.0, 1.29, 0.32],
            [0.19, 0.065, 0.04],
            orange,
        );
        let head = add(
            "Голова",
            Shape::Box,
            [0.0, 1.82, 0.0],
            [0.73, 0.44, 0.54],
            blue,
        );
        let visor = add(
            "Визор",
            Shape::Box,
            [0.0, 1.84, 0.29],
            [0.63, 0.25, 0.09],
            dark,
        );
        let left_eye = add(
            "Левый глаз",
            Shape::Box,
            [-0.15, 1.85, 0.35],
            [0.11, 0.08, 0.04],
            light,
        );
        let right_eye = add(
            "Правый глаз",
            Shape::Box,
            [0.15, 1.85, 0.35],
            [0.11, 0.08, 0.04],
            light,
        );
        let antenna = add(
            "Антенна",
            Shape::Cylinder,
            [0.20, 2.11, 0.02],
            [0.05, 0.20, 0.05],
            dark,
        );
        let beacon = add(
            "Маячок",
            Shape::Sphere,
            [0.20, 2.23, 0.02],
            [0.11; 3],
            orange,
        );
        let left_arm = add(
            "Левая рука",
            Shape::Box,
            [-0.62, 1.10, 0.0],
            [0.25, 0.59, 0.27],
            blue,
        );
        let right_arm = add(
            "Правая рука",
            Shape::Box,
            [0.62, 1.10, 0.0],
            [0.25, 0.59, 0.27],
            blue,
        );
        add(
            "Левая нога",
            Shape::Box,
            [-0.23, 0.56, 0.0],
            [0.27, 0.63, 0.29],
            dark,
        );
        add(
            "Правая нога",
            Shape::Box,
            [0.23, 0.56, 0.0],
            [0.27, 0.63, 0.29],
            dark,
        );
        add(
            "Левая стопа",
            Shape::Box,
            [-0.23, 0.125, 0.10],
            [0.34, 0.25, 0.55],
            orange,
        );
        add(
            "Правая стопа",
            Shape::Box,
            [0.23, 0.125, 0.10],
            [0.34, 0.25, 0.55],
            orange,
        );

        for part_id in [
            body, chest, core, head, visor, left_eye, right_eye, antenna, beacon, left_arm,
            right_arm,
        ] {
            for (time, height) in [(0.22, -0.08), (0.40, 0.09), (0.72, -0.12), (0.98, 0.0)] {
                scene.animation.keys.push(Keyframe {
                    part_id,
                    time,
                    position: [0.0, height, 0.0],
                    rotation: [0.0; 3],
                });
            }
        }
        for (arm, direction) in [(left_arm, -1.0), (right_arm, 1.0)] {
            for (time, angle) in [(0.22, 12.0), (0.40, 28.0), (0.72, 6.0)] {
                scene.animation.keyframe_mut(arm, time).rotation[2] = direction * angle;
            }
        }

        scene.assign_demo_motion();
        scene
    }
}

#[derive(Clone, Debug)]
pub struct Game {
    pub x: f32,
    pub z: f32,
    pub height: f32,
    pub grounded: bool,
    pub yaw: f32,
    pub move_blend: f32,
    pub phase: f32,
    planar_velocity: Vec2,
    vertical_speed: f32,
    jump_buffer: f32,
    ground_grace: f32,
}

impl Default for Game {
    fn default() -> Self {
        Self {
            x: 0.0,
            z: 0.0,
            height: 0.0,
            grounded: true,
            yaw: 0.0,
            move_blend: 0.0,
            phase: 0.0,
            planar_velocity: Vec2::ZERO,
            vertical_speed: 0.0,
            jump_buffer: 0.0,
            ground_grace: 0.0,
        }
    }
}

pub(crate) const PLAYER_RADIUS: f32 = 0.36;
pub(crate) const PLAYER_HEIGHT: f32 = 2.25;
const CONTACT_TOLERANCE: f32 = 0.0001;
const GRAVITY: f32 = 16.0;
const JUMP_BUFFER_SECONDS: f32 = 0.13;
const GROUND_GRACE_SECONDS: f32 = 0.085;
pub(crate) const STEP_HEIGHT: f32 = 0.23;

impl Game {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn horizontal_speed(&self) -> f32 {
        self.planar_velocity.length()
    }

    pub fn vertical_velocity(&self) -> f32 {
        self.vertical_speed
    }

    pub fn flight_phase_with(&self, jump_speed: f32) -> f32 {
        (0.5 - self.vertical_speed / (2.0 * jump_speed)).clamp(0.0, 1.0)
    }

    pub fn clear_motion(&mut self) {
        self.stop_horizontal();
        self.vertical_speed = 0.0;
        self.jump_buffer = 0.0;
        self.ground_grace = 0.0;
    }

    pub(crate) fn stop_horizontal(&mut self) {
        self.planar_velocity = Vec2::ZERO;
        self.move_blend = 0.0;
    }

    /// Motion uses world X and Z. Returns true on the first ground contact after falling.
    pub fn update(
        &mut self,
        dt_secs: f32,
        motion: [f32; 2],
        sprint: bool,
        jump_pressed: bool,
        colliders: &[Collider],
    ) -> bool {
        self.update_configured(
            dt_secs,
            motion,
            sprint,
            jump_pressed,
            colliders,
            crate::play_settings::Movement3d::default(),
        )
    }

    pub fn update_configured(
        &mut self,
        dt_secs: f32,
        motion: [f32; 2],
        sprint: bool,
        jump_pressed: bool,
        colliders: &[Collider],
        settings: crate::play_settings::Movement3d,
    ) -> bool {
        if jump_pressed {
            self.jump_buffer = JUMP_BUFFER_SECONDS;
        }
        if !dt_secs.is_finite() || dt_secs <= 0.0 {
            return false;
        }
        let dt = dt_secs.min(0.1);
        let motion = Vec2::from_array(motion);
        let motion = if motion.is_finite() {
            motion.clamp_length_max(1.0)
        } else {
            Vec2::ZERO
        };

        let speed = if sprint {
            settings.sprint_speed
        } else {
            settings.walk_speed
        };
        let target_velocity = motion * speed;
        let substeps = ((dt * 120.0).ceil() as u32).max(1);
        let step = dt / substeps as f32;
        let start = Vec2::new(self.x, self.z);
        let mut landed = false;

        for _ in 0..substeps {
            let previous_velocity = self.planar_velocity;
            self.steer(target_velocity, sprint, step, settings);
            self.try_jump(settings.jump_speed);
            let displacement = (previous_velocity + self.planar_velocity) * (step * 0.5);
            self.move_axis(0, displacement.x, colliders);
            self.move_axis(2, displacement.y, colliders);
            landed |= self.advance_vertical(step, colliders);
            if self.grounded {
                self.ground_grace = GROUND_GRACE_SECONDS;
            } else {
                self.ground_grace = (self.ground_grace - step).max(0.0);
            }
            self.jump_buffer = (self.jump_buffer - step).max(0.0);
        }

        let movement = Vec2::new(self.x, self.z) - start;
        let distance = movement.length();
        if motion.length_squared() > 0.0025 {
            let facing = if distance > dt * 0.1 {
                movement
            } else {
                motion
            };
            self.yaw = facing.x.atan2(facing.y).to_degrees();
        }
        let amplitude = if !self.grounded {
            0.40
        } else if sprint {
            1.0
        } else {
            0.76
        };
        let blend = (distance / (speed * dt)).min(1.0) * amplitude;
        self.move_blend += (blend - self.move_blend) * (1.0 - (-15.0 * dt).exp());
        self.phase = (self.phase + 3.4 * distance).rem_euclid(std::f32::consts::TAU);
        landed
    }

    fn steer(
        &mut self,
        target: Vec2,
        sprint: bool,
        dt: f32,
        settings: crate::play_settings::Movement3d,
    ) {
        if !self.grounded && target == Vec2::ZERO {
            self.planar_velocity *= (-0.40 * dt).exp();
            return;
        }
        let acceleration = if !self.grounded {
            settings.air_acceleration
        } else if target.length_squared() < self.planar_velocity.length_squared()
            || target.dot(self.planar_velocity) < 0.0
        {
            settings.braking
        } else if sprint {
            settings.sprint_acceleration
        } else {
            settings.acceleration
        };
        let change = target - self.planar_velocity;
        self.planar_velocity += change.clamp_length_max(acceleration * dt);
    }

    fn try_jump(&mut self, jump_speed: f32) {
        if self.jump_buffer > 0.0 && (self.grounded || self.ground_grace > 0.0) {
            self.vertical_speed = jump_speed;
            self.grounded = false;
            self.ground_grace = 0.0;
            self.jump_buffer = 0.0;
        }
    }

    /// Translate a rider with its support, checking walls and ceiling without injecting run velocity.
    pub(crate) fn carry_motion(&mut self, [dx, dy, dz]: [f32; 3], colliders: &[Collider]) {
        self.x = self.sweep_horizontal(0, self.x, self.z, self.height, dx, colliders);
        self.z = self.sweep_horizontal(2, self.z, self.x, self.height, dz, colliders);
        let intended = (self.height + dy).max(0.0);
        let obstacles = colliders.iter().filter(|collider| {
            overlaps_axis(self.x, collider, 0) && overlaps_axis(self.z, collider, 2)
        });
        self.height = if dy > 0.0 {
            obstacles
                .map(|collider| collider.min[1] - PLAYER_HEIGHT)
                .filter(|ceiling| *ceiling >= self.height - CONTACT_TOLERANCE)
                .fold(intended, f32::min)
                .max(self.height)
        } else {
            obstacles
                .map(|collider| collider.max[1])
                .filter(|floor| *floor <= self.height + CONTACT_TOLERANCE)
                .fold(intended, f32::max)
                .min(self.height)
        };
    }

    fn move_axis(&mut self, axis: usize, delta: f32, colliders: &[Collider]) {
        if delta == 0.0 {
            return;
        }
        let (position, lateral) = if axis == 0 {
            (self.x, self.z)
        } else {
            (self.z, self.x)
        };
        let mut next =
            self.sweep_horizontal(axis, position, lateral, self.height, delta, colliders);
        if (next - (position + delta)).abs() > CONTACT_TOLERANCE {
            if self.grounded
                && let Some(height) = self.step_up(axis, position, lateral, delta, colliders)
            {
                self.height = height;
                next = position + delta;
            } else {
                self.planar_velocity[axis / 2] = 0.0;
            }
        }
        if axis == 0 {
            self.x = next;
        } else {
            self.z = next;
        }
    }

    fn step_up(
        &self,
        axis: usize,
        position: f32,
        lateral: f32,
        delta: f32,
        colliders: &[Collider],
    ) -> Option<f32> {
        let target = position + delta;
        let (next_x, next_z) = if axis == 0 {
            (target, lateral)
        } else {
            (lateral, target)
        };
        colliders
            .iter()
            .filter(|collider| {
                overlaps_axis(next_x, collider, 0) && overlaps_axis(next_z, collider, 2)
            })
            .map(|collider| collider.max[1])
            .filter(|&top| {
                top > self.height + CONTACT_TOLERANCE && top <= self.height + STEP_HEIGHT
            })
            .filter(|&top| {
                self.body_clear(self.x, self.z, top, colliders)
                    && self.body_clear(next_x, next_z, top, colliders)
                    && (self.sweep_horizontal(axis, position, lateral, top, delta, colliders)
                        - target)
                        .abs()
                        <= CONTACT_TOLERANCE
            })
            .min_by(f32::total_cmp)
    }

    fn body_clear(&self, x: f32, z: f32, floor: f32, colliders: &[Collider]) -> bool {
        !colliders.iter().any(|collider| {
            overlaps_axis(x, collider, 0)
                && overlaps_axis(z, collider, 2)
                && floor < collider.max[1] - CONTACT_TOLERANCE
                && floor + PLAYER_HEIGHT > collider.min[1] + CONTACT_TOLERANCE
        })
    }

    fn sweep_horizontal(
        &self,
        axis: usize,
        position: f32,
        other: f32,
        floor: f32,
        delta: f32,
        colliders: &[Collider],
    ) -> f32 {
        let mut next = position + delta;
        for collider in colliders {
            if floor + PLAYER_HEIGHT <= collider.min[1] + CONTACT_TOLERANCE
                || floor >= collider.max[1] - CONTACT_TOLERANCE
                || !overlaps_axis(other, collider, 2 - axis)
            {
                continue;
            }

            if delta > 0.0 {
                let boundary = collider.min[axis] - PLAYER_RADIUS;
                if position <= boundary + CONTACT_TOLERANCE {
                    next = next.min(boundary.max(position));
                }
            } else {
                let boundary = collider.max[axis] + PLAYER_RADIUS;
                if position >= boundary - CONTACT_TOLERANCE {
                    next = next.max(boundary.min(position));
                }
            }
        }

        next
    }

    fn advance_vertical(&mut self, dt: f32, colliders: &[Collider]) -> bool {
        let next = self.height + self.vertical_speed * dt - 0.5 * GRAVITY * dt * dt;
        self.vertical_speed -= GRAVITY * dt;

        let under_player = colliders.iter().filter(|collider| {
            overlaps_axis(self.x, collider, 0) && overlaps_axis(self.z, collider, 2)
        });

        if next > self.height {
            let previous_head = self.height + PLAYER_HEIGHT;
            let next_head = next + PLAYER_HEIGHT;
            let ceiling = under_player
                .map(|collider| collider.min[1])
                .filter(|bottom| {
                    *bottom >= previous_head - CONTACT_TOLERANCE && *bottom <= next_head
                })
                .min_by(f32::total_cmp);
            self.height = ceiling.map_or(next, |height| height - PLAYER_HEIGHT);
            if ceiling.is_some() {
                self.vertical_speed = 0.0;
            }
            self.grounded = false;
            return false;
        }

        // Stay on shallow steps and small seams without entering a falling animation.
        let lower = if self.grounded {
            next.min(self.height - STEP_HEIGHT)
        } else {
            next
        };
        let floor = (lower <= 0.0).then_some(0.0);
        let surface = under_player
            .map(|collider| collider.max[1])
            .filter(|top| *top >= lower && *top <= self.height + CONTACT_TOLERANCE)
            .chain(floor)
            .max_by(f32::total_cmp);

        let landed = !self.grounded && surface.is_some();
        self.height = surface.unwrap_or(next);
        self.grounded = surface.is_some();
        if self.grounded {
            self.vertical_speed = 0.0;
        }
        landed
    }
}

fn overlaps_axis(center: f32, collider: &Collider, axis: usize) -> bool {
    center + PLAYER_RADIUS > collider.min[axis] + CONTACT_TOLERANCE
        && center - PLAYER_RADIUS < collider.max[axis] - CONTACT_TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a3_post_process_preserves_legacy_defaults_and_roundtrips() -> Result<(), Box<dyn std::error::Error>> {
        let mut old_project = serde_json::to_value(Scene::empty())?;
        assert!(old_project.get("post_process").is_none());
        let unchanged: Scene = serde_json::from_value(old_project.clone())?;
        assert_eq!(unchanged.post_process, PostProcess::default());
        assert_eq!(serde_json::to_value(unchanged)?, old_project);

        old_project["post_process"] = serde_json::json!({"exposure_ev": -0.75});
        let partial: Scene = serde_json::from_value(old_project)?;
        assert_eq!(
            partial.post_process,
            PostProcess {
                exposure_ev: -0.75,
                ..PostProcess::default()
            }
        );

        let original = PostProcess {
            exposure_ev: 1.25,
            bloom: true,
            bloom_threshold: 2.5,
            ..PostProcess::default()
        };
        let mut scene = Scene::empty();
        scene.post_process = original;
        let reopened: Scene = serde_json::from_slice(&serde_json::to_vec(&scene)?)?;
        reopened.validate_import()?;
        assert_eq!(reopened.post_process, original);
        Ok(())
    }

    #[test]
    fn a3_post_process_rejects_invalid_exposure_and_threshold() {
        let mut scene = Scene::empty();
        for post_process in [
            PostProcess {
                exposure_ev: f32::NAN,
                ..PostProcess::default()
            },
            PostProcess {
                exposure_ev: 6.01,
                ..PostProcess::default()
            },
            PostProcess {
                bloom_threshold: 0.0,
                ..PostProcess::default()
            },
            PostProcess {
                bloom: true,
                bloom_threshold: f32::INFINITY,
                ..PostProcess::default()
            },
        ] {
            scene.post_process = post_process;
            assert!(matches!(
                scene.validate_import(),
                Err(ImportError::Invalid { context, .. }) if context == "Постобработка"
            ));
        }
    }

    #[test]
    fn a5_cascades_roundtrip_in_a_scene_and_single_restores_legacy_serialization()
    -> Result<(), Box<dyn std::error::Error>> {
        use studio_core::render_settings::ShadowQuality;

        let mut scene = Scene::empty();
        let legacy = serde_json::to_vec(&scene)?;
        scene.post_process.shadow_quality = ShadowQuality::Cascaded;
        let encoded = serde_json::to_value(&scene)?;
        assert_eq!(encoded["post_process"]["shadow_quality"], "Cascaded");
        let mut reopened: Scene = serde_json::from_value(encoded)?;
        reopened.validate_import()?;
        assert_eq!(reopened.post_process.shadow_quality, ShadowQuality::Cascaded);

        reopened.post_process.shadow_quality = ShadowQuality::Single;
        assert_eq!(serde_json::to_vec(&reopened)?, legacy);
        Ok(())
    }

    fn assert_bad_import(file: &str, reason: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/a1_scene")
            .join(file);
        let mut game = Game::default();
        let world = World::demo();
        let colliders = world.colliders();
        game.update(0.1, [0.0, 1.0], false, false, &colliders);
        let before = game.z;

        let error: ImportError = Scene::load(&path).expect_err("битая сцена принята");
        assert!(error.to_string().contains(file), "{error}");
        assert!(error.to_string().contains(reason), "{error}");

        game.update(0.1, [0.0, 1.0], false, false, &colliders);
        assert!(game.z > before);
    }

    #[test]
    fn a1_truncated_project_reports_file_and_json_position() {
        assert_bad_import("truncated.robot.json", "JSON, строка");
    }

    #[test]
    fn a1_unknown_project_version_is_rejected() {
        assert_bad_import("unknown-version.robot.json", "Версия проекта 999");
    }

    #[test]
    fn a1_foreign_world_id_reports_the_object() {
        assert_bad_import("foreign-world-id.robot.json", "Окружение, объект №1");
    }

    #[test]
    fn a1_invalid_weather_reports_the_setting() {
        assert_bad_import("invalid-weather.robot.json", "Погода, приземный туман");
    }

    #[test]
    fn archived_v014_through_v023_projects_still_open() -> Result<(), Box<dyn std::error::Error>> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../verification");
        let mut directories: Vec<_> = (14..=23).map(|version| root.join(format!("v0.{version}"))).collect();
        let mut opened = 0;
        while let Some(directory) = directories.pop() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    directories.push(entry.path());
                } else if kind.is_file() && entry.file_name().to_string_lossy().ends_with(".robot.json") {
                    Scene::load(&entry.path())?;
                    opened += 1;
                }
            }
        }
        assert!(opened >= 47, "Открыто только {opened} архивных проектов");
        Ok(())
    }

    #[test]
    fn previous_release_projects_save_the_same_bytes() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../verification");
        let saved = std::env::temp_dir().join(format!(
            "studio-previous-project-{}.robot.json",
            std::process::id()
        ));
        for path in [
            "v0.23/trial/manual/junction.robot.json",
            "yuna-detailed/yuna-glade.robot.json",
        ] {
            let source = directory.join(path);
            let original = fs::read_to_string(&source).unwrap().replace("\r\n", "\n");
            Scene::load(&source).unwrap().save(&saved).unwrap();
            let actual = fs::read(&saved).unwrap();
            fs::remove_file(&saved).unwrap();
            assert!(actual == original.as_bytes(), "serialization changed for {path}");
        }
    }

    #[test]
    fn keyframe_fills_in_current_pose_and_deleting_part_removes_its_keys() {
        let mut scene = Scene::default();
        let id = scene.add_part(Shape::Box);
        let duration = scene.animation.duration;
        scene.animation.keyframe_mut(id, duration * 0.5).position = [2.0, 0.0, 0.0];
        let start_x = scene.parts.last().unwrap().position[0];
        assert_eq!(
            scene.sample(duration * 0.25).last().unwrap().position[0],
            start_x + 1.0
        );
        assert_eq!(
            scene.animation.keyframe_mut(id, duration * 0.75).position,
            [1.0, 0.0, 0.0]
        );
        assert!(scene.remove_part(id));
        assert!(scene.animation.keys.iter().all(|key| key.part_id != id));
    }

    #[test]
    fn scene_round_trip_replaces_existing_project_and_rejects_dangling_keys() {
        let mut scene = Scene::default();
        let temp = std::env::temp_dir().join(format!(
            "robot-studio-scene-test-{}.json",
            std::process::id()
        ));
        scene.save(&temp).unwrap();
        scene.effect.count = 17;
        scene.save(&temp).unwrap();
        assert_eq!(Scene::load(&temp).unwrap(), scene);
        scene.animation.keyframe_mut(u64::MAX, 0.0);
        assert!(scene.save(&temp).is_err());
        assert_eq!(Scene::load(&temp).unwrap().effect.count, 17);
        fs::remove_file(temp).unwrap();
    }

    #[test]
    fn version_one_scene_loads_with_limb_roles_and_empty_world() {
        let mut data = serde_json::to_value(Scene::default()).unwrap();
        data["version"] = 1.into();
        data.as_object_mut().unwrap().remove("world");
        for part in data["parts"].as_array_mut().unwrap() {
            part.as_object_mut().unwrap().remove("motion");
        }
        let temp = std::env::temp_dir().join(format!(
            "robot-studio-migration-test-{}.json",
            std::process::id()
        ));
        fs::write(&temp, serde_json::to_vec(&data).unwrap()).unwrap();
        let scene = Scene::load(&temp).unwrap();
        fs::remove_file(temp).unwrap();
        assert_eq!(scene.version, PROJECT_VERSION);
        assert_eq!(scene.world, World::default());
        assert!(
            scene
                .parts
                .iter()
                .any(|part| part.motion == PartMotion::LeftArm)
        );
    }

    #[test]
    fn version_ten_checkpoint_migrates_without_changing_authored_doors() {
        use crate::world::{DoorSettings, InteractionState};

        let mut scene = Scene::default();
        scene.sprite_art.use_on_plan = true;
        scene.world.add_building_at([0.0, 0.0, -4.0]).unwrap();
        scene.world.buildings[0].door = Some(DoorSettings {
            initially_open: true,
            ..Default::default()
        });
        scene.checkpoint = Some(Checkpoint {
            position: [2.0, 0.0, -2.0],
            scene_id: None,
            rotor_angle: None,
            tactics: None,
            environment: None,
            physics: None,
            map_respawn: None,
            story: Default::default(),
            interactions: Default::default(),
            rules: Default::default(),
            visited: Default::default(),
        });
        let mut legacy = serde_json::to_value(&scene).unwrap();
        legacy["version"] = 10.into();
        legacy["world"].as_object_mut().unwrap().remove("keys");
        legacy["world"]
            .as_object_mut()
            .unwrap()
            .remove("next_key_id");
        legacy["checkpoint"]
            .as_object_mut()
            .unwrap()
            .remove("interactions");
        let temp = std::env::temp_dir().join(format!(
            "robot-studio-migration-10-{}.json",
            std::process::id()
        ));
        fs::write(&temp, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let loaded = Scene::load(&temp).unwrap();
        assert_eq!(loaded, scene);
        let mut state = InteractionState::default();
        state.restore(
            &loaded.world,
            &loaded.checkpoint.as_ref().unwrap().interactions,
        );
        let door = loaded.world.buildings[0].doors().next().unwrap();
        assert!(state.key_ids().next().is_none());
        assert_eq!(state.fraction(&door), 1.0);
        fs::remove_file(temp).unwrap();
    }

    #[test]
    fn version_eleven_project_gets_stable_dialogue_answers_and_empty_rules() {
        use crate::{
            assets::Source,
            model,
            npcs::{FIRST_NPC_ID, Waypoint},
        };

        let mut scene = Scene::empty();
        let model = model::decode_glb(include_bytes!("../demo_assets/emma.glb")).unwrap();
        let speaker = Avatar::new(Source::Emma, "Эмма".into(), &model);
        scene.npcs.push(Npc::new(
            FIRST_NPC_ID,
            speaker,
            vec![Waypoint::at([1.0, 0.0, 1.0])],
        ));
        scene.story = Story::demo(FIRST_NPC_ID, [2.0, -1.0]);
        scene.checkpoint = Some(Checkpoint {
            position: [2.0, 0.0, -1.0],
            scene_id: None,
            rotor_angle: None,
            tactics: None,
            environment: None,
            physics: None,
            map_respawn: None,
            story: Default::default(),
            interactions: Default::default(),
            rules: Default::default(),
            visited: Default::default(),
        });
        let mut legacy = serde_json::to_value(&scene).unwrap();
        legacy["version"] = 11.into();
        legacy.as_object_mut().unwrap().remove("rules");
        legacy["checkpoint"]
            .as_object_mut()
            .unwrap()
            .remove("rules");
        legacy["story"]
            .as_object_mut()
            .unwrap()
            .remove("next_choice_id");
        for conversation in legacy["story"]["conversations"].as_array_mut().unwrap() {
            for node in conversation["nodes"].as_array_mut().unwrap() {
                for choice in node["choices"].as_array_mut().unwrap() {
                    choice.as_object_mut().unwrap().remove("id");
                }
            }
        }
        let temp = std::env::temp_dir().join(format!(
            "robot-studio-migration-11-{}.json",
            std::process::id()
        ));
        fs::write(&temp, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let loaded = Scene::load(&temp).unwrap();
        assert_eq!(loaded, scene);
        let mut dialogue = crate::story::Session::default();
        assert!(dialogue.open(&loaded.story, FIRST_NPC_ID));
        assert_eq!(
            dialogue.choose_event(&loaded.story, 0),
            Some(rules::Event::Dialogue {
                npc_id: FIRST_NPC_ID,
                node_id: 1,
                choice_id: 1
            })
        );
        fs::remove_file(temp).unwrap();
    }

    #[test]
    fn walking_rotates_arms_in_opposite_directions() {
        let scene = Scene::default();
        let game = Game {
            phase: std::f32::consts::FRAC_PI_2,
            move_blend: 0.75,
            ..Game::default()
        };
        let parts = scene.sample_game(0.0, &game);
        let x_rotation = |role| {
            parts
                .iter()
                .find(|part| part.motion == role)
                .unwrap()
                .rotation[0]
        };
        assert!(x_rotation(PartMotion::LeftArm) > 15.0);
        assert!(x_rotation(PartMotion::RightArm) < -15.0);
        assert_eq!(parts[0], scene.parts[0]);
    }

    #[test]
    fn jump_reports_single_landing_and_rests_at_ground_level() {
        let mut game = Game::default();
        assert!(!game.update(1.0 / 60.0, [1.0, 0.0], false, true, &[]));
        assert!(game.height > 0.0);
        assert!(game.x > 0.0);
        let landings = (0..120)
            .filter(|_| game.update(1.0 / 60.0, [0.0; 2], false, false, &[]))
            .count();
        assert_eq!(landings, 1);
        assert_eq!(game.height, 0.0);
        assert!(game.grounded);
    }

    #[test]
    fn diagonal_is_normalized_and_character_slides_along_a_wall() {
        let wall = Collider {
            min: [1.0, 0.0, -10.0],
            max: [1.1, 4.0, 10.0],
        };
        let mut walk = Game::default();
        let mut diagonal = Game::default();
        walk.update(0.05, [1.0, 0.0], false, false, &[]);
        diagonal.update(0.05, [1.0, 1.0], false, false, &[]);
        assert!((Vec2::new(diagonal.x, diagonal.z).length() - walk.x).abs() < 0.0001);

        let mut run = Game::default();
        for _ in 0..15 {
            run.update(0.1, [1.0, 1.0], true, false, &[wall]);
        }
        assert!((run.x - (1.0 - PLAYER_RADIUS)).abs() < 0.0001);
        assert!(run.z > 5.5);
    }

    #[test]
    fn player_lands_on_platform_then_falls_off_its_edge() {
        let platform = Collider {
            min: [1.0, 0.0, -1.0],
            max: [3.0, 0.8, 1.0],
        };
        let mut game = Game::default();
        let mut landings = 0;
        for frame in 0..120 {
            let motion = if frame < 36 { [1.0, 0.0] } else { [0.0; 2] };
            if game.update(1.0 / 60.0, motion, false, frame == 0, &[platform]) {
                landings += 1;
            }
        }
        assert_eq!(landings, 1);
        assert_eq!(game.height, 0.8);
        assert!(game.grounded);

        for _ in 0..90 {
            game.update(1.0 / 60.0, [1.0, 0.0], true, false, &[platform]);
        }
        assert_eq!(game.height, 0.0);
        assert!(game.grounded);
    }

    #[test]
    fn walking_accelerates_then_brakes_without_overshoot() {
        let mut game = Game::default();
        game.update(1.0 / 60.0, [1.0, 0.0], false, false, &[]);
        assert!((0.20..0.45).contains(&game.horizontal_speed()));
        for _ in 0..12 {
            game.update(1.0 / 60.0, [1.0, 0.0], false, false, &[]);
        }
        assert!(
            (game.horizontal_speed() - crate::play_settings::Movement3d::default().walk_speed)
                .abs()
                < 0.001
        );
        let release_x = game.x;
        game.update(0.035, [0.0; 2], false, false, &[]);
        assert!((1.5..2.8).contains(&game.horizontal_speed()));
        assert!(game.x > release_x + 0.05);
        for _ in 0..15 {
            game.update(1.0 / 60.0, [0.0; 2], false, false, &[]);
        }
        assert!(game.horizontal_speed() < 0.001);
        let stop_x = game.x;
        game.update(0.10, [0.0; 2], false, false, &[]);
        assert_eq!(game.x, stop_x);
    }

    #[test]
    fn walks_over_a_curb_without_falling_but_respects_overhead_clearance() {
        let curb = Collider {
            min: [0.7, 0.0, -1.0],
            max: [2.0, 0.17, 1.0],
        };
        let low_ceiling = Collider {
            min: [0.65, 2.29, -1.0],
            max: [2.0, 2.5, 1.0],
        };
        let mut free = Game::default();
        let mut obstructed = Game::default();
        let mut mounted = false;
        for index in 0..70 {
            let dt = if index % 2 == 0 { 0.014 } else { 0.025 };
            assert!(!free.update(dt, [1.0, 0.0], false, false, &[curb]));
            assert!(free.grounded);
            mounted |= (free.height - curb.max[1]).abs() < 0.0001;
            assert!(!obstructed.update(dt, [1.0, 0.0], false, false, &[curb, low_ceiling]));
            assert_eq!(obstructed.height, 0.0);
        }
        assert!(mounted && free.x > curb.max[0]);
        assert_eq!(free.height, 0.0);
        assert!((obstructed.x - (curb.min[0] - PLAYER_RADIUS)).abs() < 0.001);
    }

    #[test]
    fn buffers_a_jump_before_landing_and_allows_a_short_edge_grace() {
        let mut falling = Game {
            height: 0.07,
            grounded: false,
            vertical_speed: -2.0,
            ..Game::default()
        };
        falling.update(0.015, [0.0; 2], false, true, &[]);
        let mut contacted = false;
        for _ in 0..5 {
            contacted |= falling.update(0.015, [0.0; 2], false, false, &[]);
        }
        assert!(contacted && falling.vertical_velocity() > 4.0);
        assert!(!falling.grounded);

        let ledge = Collider {
            min: [-1.0, 0.0, -1.0],
            max: [0.25, 0.7, 1.0],
        };
        let mut walker = Game {
            height: 0.7,
            ..Game::default()
        };
        for _ in 0..40 {
            walker.update(0.016, [1.0, 0.0], false, false, &[ledge]);
            if !walker.grounded {
                break;
            }
        }
        assert!(!walker.grounded && walker.vertical_velocity() < 0.0);
        walker.update(0.022, [1.0, 0.0], false, false, &[ledge]);
        walker.update(0.016, [1.0, 0.0], false, true, &[ledge]);
        assert!(walker.vertical_velocity() > 4.0);
    }

    #[test]
    fn a_sprint_cannot_tunnel_through_a_thin_wall_on_long_frames() {
        let wall = Collider {
            min: [1.08, 0.0, -3.0],
            max: [1.091, 4.0, 3.0],
        };
        let mut game = Game::default();
        for dt in [0.043, 0.097, 0.061, 0.1].into_iter().cycle().take(40) {
            game.update(dt, [1.0, 0.0], true, false, &[wall]);
            assert!(game.x <= wall.min[0] - PLAYER_RADIUS + 0.0001);
        }
        assert!(game.horizontal_speed() < 0.001);
        game.clear_motion();
        game.update(0.10, [-1.0, 0.0], false, false, &[wall]);
        assert!(game.x < wall.min[0] - PLAYER_RADIUS - 0.04);
    }
}
