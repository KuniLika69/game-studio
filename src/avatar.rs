use crate::{
    actor_transitions, assets::Source, character_shapes,
    model::{HumanStyle, Model, MorphWeight, PartSlot},
};
use eframe::egui;
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const MAX_BONE_KEYS: usize = 128;
const KEY_EPSILON: f32 = 0.002;
pub(crate) const MAX_BONE_ATTACHMENTS: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Motion {
    #[default]
    Idle,
    Walk,
    Run,
    Jump,
}

impl Motion {
    pub const ALL: [Self; 4] = [Self::Idle, Self::Walk, Self::Run, Self::Jump];

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Покой",
            Self::Walk => "Ходьба",
            Self::Run => "Бег",
            Self::Jump => "Прыжок",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Avatar {
    pub source: Source,
    pub name: String,
    pub height: f32,
    pub yaw: f32,
    pub idle: Option<usize>,
    pub walk: Option<usize>,
    pub run: Option<usize>,
    pub jump: Option<usize>,
    #[serde(default)]
    pub sit: Option<usize>,
    #[serde(default)]
    pub human_style: Option<HumanStyle>,
    #[serde(default = "default_animation_speed")]
    pub animation_speed: f32,
    #[serde(default)]
    pub bone_keys: Vec<BoneKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modular_parts: Vec<ModularPart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bone_attachments: Vec<BoneAttachment>,
    #[serde(default, skip_serializing_if = "crate::model::CharacterLook::is_empty")]
    pub look: crate::model::CharacterLook,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imported_clips: Vec<ImportedClip>,
    #[serde(default, skip_serializing_if = "ActorTuning::is_default")]
    pub actor_tuning: ActorTuning,
    #[serde(default, skip_serializing_if = "actor_transitions::Graph::is_default")]
    pub motion_graph: actor_transitions::Graph,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shape_weights: Vec<MorphWeight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) generated: Option<crate::creature_mesh::Draft>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActorTuning {
    pub breath_depth: f32,
    pub breaths_per_minute: f32,
    pub hair_stiffness: f32,
    pub hair_damping: f32,
    pub hair_contacts: bool,
    pub match_stride: bool,
    pub face_strength: f32,
}

impl Default for ActorTuning {
    fn default() -> Self {
        Self {
            breath_depth: 1.0,
            breaths_per_minute: 16.7,
            hair_stiffness: 1.0,
            hair_damping: 1.0,
            hair_contacts: true,
            match_stride: false,
            face_strength: 1.0,
        }
    }
}

impl ActorTuning {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    fn valid(self) -> bool {
        (0.0..=2.5).contains(&self.breath_depth)
            && (6.0..=42.0).contains(&self.breaths_per_minute)
            && (0.35..=2.6).contains(&self.hair_stiffness)
            && (0.35..=2.6).contains(&self.hair_damping)
            && (0.0..=1.0).contains(&self.face_strength)
    }

    pub(super) fn inspector(&mut self, ui: &mut egui::Ui) {
        use egui::DragValue;
        ui.checkbox(&mut self.match_stride, "Согласовать скорость с длиной шага")
            .on_hover_text("Для совместимой 3D-модели движок измеряет походку и бег в клипах, а повороты выполняет с разгоном и торможением.");
        ui.add(egui::Slider::new(&mut self.face_strength, 0.0..=1.0).text("Сила автомимики"))
            .on_hover_text("Моргание, взгляд и лёгкая улыбка в покое используют подходящие формы этой модели.");
        ui.collapsing("Дыхание и волосы", |ui| {
            ui.add(DragValue::new(&mut self.breath_depth).range(0.0..=2.5).speed(0.02).fixed_decimals(2).prefix("Глубина дыхания "));
            ui.add(DragValue::new(&mut self.breaths_per_minute).range(6.0..=42.0).speed(0.2).fixed_decimals(1).prefix("Вдохов в минуту "));
            ui.add(DragValue::new(&mut self.hair_stiffness).range(0.35..=2.6).speed(0.02).fixed_decimals(2).prefix("Жёсткость волос "));
            ui.add(DragValue::new(&mut self.hair_damping).range(0.35..=2.6).speed(0.02).fixed_decimals(2).prefix("Затухание волос "));
            ui.checkbox(&mut self.hair_contacts, "Ограничить прохождение волос сквозь тело")
                .on_hover_text("Высокий профиль проверяет голову и плечи. Низкий профиль оставляет ограничитель головы.");
        });
    }

    pub fn hair_contact_quality(self, performance_profile: bool) -> crate::model::HairContacts {
        use crate::model::HairContacts;
        if !self.hair_contacts {
            HairContacts::Off
        } else if performance_profile {
            HairContacts::Head
        } else {
            HairContacts::HeadAndShoulders
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImportedClip {
    pub source: Source,
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModularPart {
    pub slot: PartSlot,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoneAttachment {
    pub source: Source,
    pub name: String,
    pub bone: String,
    pub offset: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: f32,
}

impl BoneAttachment {
    pub(crate) fn valid(&self) -> bool {
        let label = |value: &str| !value.trim().is_empty() && value.chars().count() <= 128;
        self.source.validate().is_ok() && label(&self.name) && label(&self.bone)
            && Model::valid_attachment_transform(self.offset, self.rotation, self.scale)
    }
}

fn default_animation_speed() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoneKey {
    pub bone: String,
    pub time: f32,
    pub degrees: [f32; 3],
}

impl Avatar {
    pub fn new(source: Source, name: String, model: &Model) -> Self {
        let find = |names: &[&str]| {
            names.iter().find_map(|name| {
                model
                    .animations
                    .iter()
                    .position(|clip| clip.name.eq_ignore_ascii_case(name))
            })
        };
        let height = source.height();
        Self {
            source,
            name,
            height,
            yaw: 0.0,
            idle: find(&["Idle", "Idle_A", "Standing", "Idle_Standing"]),
            walk: find(&["Walking_A", "Walk", "Walking", "Walk_Relaxed", "Walk_Brisk"])
                .or_else(|| (model.animations.len() == 1).then_some(0)),
            run: find(&["Running_A", "Run", "Running"]),
            jump: find(&["Jump_Full_Short", "Jump", "Jump_Start"]),
            sit: find(&["Sit_Stand", "Sit", "Seated", "Sitting"]),
            human_style: None,
            animation_speed: default_animation_speed(),
            bone_keys: Vec::new(),
            modular_parts: Vec::new(),
            bone_attachments: Vec::new(),
            look: crate::model::CharacterLook::default(),
            imported_clips: Vec::new(),
            actor_tuning: ActorTuning::default(),
            motion_graph: actor_transitions::Graph::default(),
            shape_weights: Vec::new(),
            generated: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.source.validate()?;
        if let Some(recipe) = &self.generated {
            recipe.validate()?;
        }
        if !self.motion_graph.valid() {
            return Err("Некорректные настройки переходов анимации.".into());
        }
        self.look.validate()?;
        if !character_shapes::valid(&self.shape_weights) {
            return Err("Некорректные веса форм 3D-модели.".into());
        }
        let mut clips = HashSet::new();
        if self.imported_clips.len() > crate::model::MAX_IMPORTED_CLIPS
            || self.imported_clips.iter().any(|clip| {
                clip.source.validate().is_err() || clip.index >= 128 || !clips.insert(clip)
            })
        {
            return Err("Некорректный список импортированных клипов GLB.".into());
        }
        let mut slots = Vec::new();
        for part in &self.modular_parts {
            part.source.validate()?;
            if slots.contains(&part.slot) {
                return Err("Слот персонажа занят несколькими деталями.".into());
            }
            slots.push(part.slot);
        }
        if self.bone_attachments.len() > MAX_BONE_ATTACHMENTS
            || self.bone_attachments.iter().any(|part| !part.valid())
        {
            return Err("Некорректные детали на костях персонажа.".into());
        }
        if !self.actor_tuning.valid()
            || self.name.trim().is_empty()
            || self.name.chars().count() > 100
            || !(0.25..=15.0).contains(&self.height)
            || !(-3_600.0..=3_600.0).contains(&self.yaw)
            || !(0.25..=2.5).contains(&self.animation_speed)
            || self.bone_keys.len() > MAX_BONE_KEYS
            || self.bone_keys.iter().any(|key| {
                key.bone.trim().is_empty()
                    || key.bone.chars().count() > 128
                    || !(0.0..=60.0).contains(&key.time)
                    || key.degrees.iter().any(|x| !(-180.0..=180.0).contains(x))
            })
            || [self.idle, self.walk, self.run, self.jump, self.sit]
                .into_iter()
                .flatten()
                .any(|index| index >= 4096)
        {
            return Err("Некорректные параметры 3D-персонажа.".into());
        }
        if let Some(style) = &self.human_style {
            if !self.source.editable_human() {
                return Err("Редактор внешности использует встроенную модель человека.".into());
            }
            style.validate()?;
        }
        Ok(())
    }

    pub fn clip(&self, motion: Motion) -> Option<usize> {
        match motion {
            Motion::Idle => self.idle,
            Motion::Walk => self.walk,
            Motion::Run => self.run.or(self.walk),
            Motion::Jump if self.procedural_jump() => self.idle,
            Motion::Jump => self.jump.or(self.walk),
        }
    }

    pub fn resources(&self) -> impl Iterator<Item = (&Source, &str)> {
        std::iter::once((&self.source, self.name.as_str()))
            .chain(
                self.modular_parts
                    .iter()
                    .map(|part| (&part.source, part.slot.label())),
            )
            .chain(self.bone_attachments.iter().map(|part| (&part.source, part.name.as_str())))
            .chain(
                self.imported_clips
                    .iter()
                    .map(|clip| (&clip.source, "Клип GLB")),
            )
    }

    pub fn procedural_jump(&self) -> bool {
        (self.source == Source::CityWoman || self.source.modern_human()) && self.jump.is_none()
    }

    pub fn gait_rate(&self, model: &Model, motion: Motion, actual_speed: f32) -> f32 {
        if let Some(recipe) = self.generated.filter(|recipe| recipe.kind == crate::creature_mesh::Kind::Quadruped) {
            let contact_stride = match motion {
                Motion::Walk => 0.54 / (0.94 * 0.64),
                Motion::Run => 0.92 / (0.60 * 0.51),
                _ => return 1.0,
            };
            let scale = self.height / (model.bounds_max[1] - model.bounds_min[1]).max(0.001);
            let calibrated_speed = contact_stride * 0.44 * recipe.legs * scale;
            return (actual_speed / calibrated_speed).clamp(0.12, 5.0);
        }
        let reference = match (motion, self.source == Source::CityWoman) {
            (Motion::Walk, _) if self.source == Source::Emma => 1.24,
            (Motion::Walk, _) if self.source == Source::Ryan => 1.00,
            (Motion::Run, _) if self.source.modern_human() => 2.86,
            (Motion::Walk, true) => match self.walk.and_then(|index| model.animations.get(index)) {
                Some(clip) if clip.name == "Walk_Brisk" => 1.69,
                _ => 1.32,
            },
            (Motion::Walk, false) => 1.51,
            (Motion::Run, true) => 3.74,
            (Motion::Run, false) => 3.68,
            _ => return 1.0,
        };
        let base_height = if self.source.modern_human() {
            self.source.height()
        } else {
            1.72
        };
        (actual_speed / (reference * (self.height / base_height).clamp(0.65, 1.55)))
            .clamp(0.05, 1.64)
    }

    pub fn match_gait_phase(&self, model: &Model, from: Motion, to: Motion, clock: f32) -> f32 {
        if !matches!(
            (from, to),
            (Motion::Walk, Motion::Run) | (Motion::Run, Motion::Walk)
        ) {
            return 0.0;
        }
        let duration = |motion| {
            self.clip(motion)
                .and_then(|index| model.animations.get(index))
                .map(|clip| clip.duration)
        };
        match (duration(from), duration(to)) {
            (Some(from), Some(to)) if from > 0.0 && to > 0.0 => {
                (clock * self.animation_speed).rem_euclid(from) * (to / from) / self.animation_speed
            }
            _ => 0.0,
        }
    }

    fn bone_rotation_at(&self, bone: &str, time: f32) -> [f32; 3] {
        let before = self
            .bone_keys
            .iter()
            .filter(|key| key.bone == bone && key.time <= time)
            .max_by(|a, b| a.time.total_cmp(&b.time));
        let after = self
            .bone_keys
            .iter()
            .filter(|key| key.bone == bone && key.time >= time)
            .min_by(|a, b| a.time.total_cmp(&b.time));
        match (before, after) {
            (Some(before), Some(after)) if (after.time - before.time).abs() > KEY_EPSILON => {
                let fraction = (time - before.time) / (after.time - before.time);
                std::array::from_fn(|i| {
                    before.degrees[i] + (after.degrees[i] - before.degrees[i]) * fraction
                })
            }
            (Some(key), _) | (_, Some(key)) => key.degrees,
            _ => [0.0; 3],
        }
    }

    pub fn bone_offsets(&self, time: f32) -> Vec<crate::model::BoneRotation> {
        let mut seen = HashSet::new();
        self.bone_keys
            .iter()
            .filter_map(|key| {
                seen.insert(key.bone.as_str())
                    .then(|| crate::model::BoneRotation {
                        bone: key.bone.clone(),
                        degrees: self.bone_rotation_at(&key.bone, time),
                    })
            })
            .collect()
    }

    pub fn retime_bones(&mut self, old: f32, new: f32) {
        for key in &mut self.bone_keys {
            key.time = (key.time * new / old).clamp(0.0, new);
        }
    }

    pub fn transform(
        &self,
        model: &Model,
        foot: Vec3,
        heading: f32,
    ) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let min = Vec3::from_array(model.bounds_min);
        let max = Vec3::from_array(model.bounds_max);
        let scale = (self.height / (max.y - min.y).max(0.001)).clamp(0.001, 100.0);
        let pivot = Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5);
        let yaw = (self.yaw + heading).rem_euclid(360.0);
        let rotation = Quat::from_rotation_y(yaw.to_radians());
        (
            (foot - rotation * (pivot * scale)).to_array(),
            [0.0, yaw, 0.0],
            [scale; 3],
        )
    }
}

pub fn inspector(
    ui: &mut egui::Ui,
    avatar: &mut Avatar,
    model: Option<&Model>,
    editing: bool,
    advanced: bool,
) {
    ui.heading(&avatar.name);
    ui.add_enabled_ui(editing, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::DragValue::new(&mut avatar.height)
                    .prefix("Рост ")
                    .range(0.25..=15.0)
                    .speed(0.04),
            );
            ui.add(
                egui::DragValue::new(&mut avatar.yaw)
                    .prefix("Поворот ")
                    .range(-3_600.0..=3_600.0)
                    .suffix("°"),
            );
        });
        if advanced {
            animation_inspector(ui, avatar, model);
        } else {
            ui.collapsing("Клипы, дыхание и волосы", |ui| {
                animation_inspector(ui, avatar, model);
            });
        }
    });
}

fn animation_inspector(ui: &mut egui::Ui, avatar: &mut Avatar, model: Option<&Model>) {
    avatar.actor_tuning.inspector(ui);
    let Some(model) = model else {
        ui.weak("Модель персонажа не загрузилась.");
        return;
    };
    if model.animations.is_empty() {
        ui.weak("В GLB нет анимационных клипов.");
        return;
    }
    ui.add(egui::Slider::new(&mut avatar.animation_speed, 0.25..=2.5).text("Скорость клипов"));
    for (label, value) in [
        ("Покой", &mut avatar.idle),
        ("Ходьба", &mut avatar.walk),
        ("Бег", &mut avatar.run),
        ("Прыжок", &mut avatar.jump),
        ("Посадка", &mut avatar.sit),
    ] {
        clip_picker(ui, model, label, value, "По умолчанию");
    }
    ui.collapsing("Переходы анимации 3D-игрока", |ui| {
        ui.add(egui::Slider::new(&mut avatar.motion_graph.fade_seconds, actor_transitions::BLEND_RANGE)
            .text("Смешивание, с"));
        for (label, value) in avatar.motion_graph.slots() {
            clip_picker(ui, model, label, value, "Обычное смешивание");
        }
    });
}

fn clip_picker(ui: &mut egui::Ui, model: &Model, label: &str, value: &mut Option<usize>, fallback: &str) {
    let selected = value.and_then(|index| model.animations.get(index))
        .map_or(fallback, |clip| clip.name.as_str());
    egui::ComboBox::from_label(label).selected_text(selected).show_ui(ui, |ui| {
        ui.selectable_value(value, None, fallback);
        for (index, clip) in model.animations.iter().enumerate() {
            ui.selectable_value(value, Some(index), &clip.name);
        }
    });
}

pub fn pose_editor(
    ui: &mut egui::Ui,
    avatar: &mut Avatar,
    model: &Model,
    bone: &mut String,
    time: &mut f32,
) {
    let available = model.bone_names();
    if available.is_empty() {
        return;
    }
    ui.collapsing("Ключи позы", |ui| {
        if !available.iter().any(|name| *name == bone.as_str()) {
            *bone = available
                .iter()
                .copied()
                .find(|name| *name == "head")
                .unwrap_or(available[0])
                .into();
        }
        egui::ComboBox::from_id_salt("pose_bone")
            .selected_text(bone_label(bone))
            .show_ui(ui, |ui| {
                for name in available
                    .iter()
                    .copied()
                    .filter(|name| *name != "motion_root")
                {
                    ui.selectable_value(bone, name.to_string(), bone_label(name));
                }
            });
        ui.label(format!("Текущий момент {:.2} с", *time));
        ui.horizontal_wrapped(|ui| {
            for key in avatar.bone_keys.iter().filter(|key| key.bone == *bone) {
                if ui.small_button(format!("{:.2} с", key.time)).clicked() {
                    *time = key.time;
                }
            }
        });
        let index = avatar
            .bone_keys
            .iter()
            .position(|key| key.bone == *bone && (key.time - *time).abs() <= KEY_EPSILON);
        if let Some(index) = index {
            for (axis, degrees) in ["Наклон X", "Поворот Y", "Наклон Z"]
                .into_iter()
                .zip(&mut avatar.bone_keys[index].degrees)
            {
                ui.add(egui::Slider::new(degrees, -180.0..=180.0).text(axis));
            }
            if ui.button("Удалить ключ").clicked() {
                avatar.bone_keys.remove(index);
            }
        } else if ui
            .add_enabled(
                avatar.bone_keys.len() < MAX_BONE_KEYS,
                egui::Button::new("+ Ключ позы"),
            )
            .clicked()
        {
            avatar.bone_keys.push(BoneKey {
                bone: bone.clone(),
                time: *time,
                degrees: avatar.bone_rotation_at(bone, *time),
            });
        }
    });
}

fn bone_label(name: &str) -> &str {
    match name {
        "hips" => "Таз",
        "spine0" => "Спина",
        "chest" => "Грудь",
        "neck" => "Шея",
        "head" => "Голова",
        "jaw" => "Челюсть",
        "upperArmL" => "Левая рука",
        "upperArmR" => "Правая рука",
        "forearmL" => "Левый локоть",
        "forearmR" => "Правый локоть",
        "handL" => "Левая кисть",
        "handR" => "Правая кисть",
        "thighL" => "Левое бедро",
        "thighR" => "Правое бедро",
        "shinL" => "Левая голень",
        "shinR" => "Правая голень",
        "footL" => "Левая стопа",
        "footR" => "Правая стопа",
        _ => name,
    }
}
