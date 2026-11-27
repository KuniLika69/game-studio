use eframe::egui::{self, Color32, DragValue, Pos2};

use crate::{
    Avatar, Studio, View, asset_sources,
    avatar::{BoneAttachment, Motion},
    model::{Model, MorphWeight},
    play_settings::Movement3d,
    viewport3d::Camera,
};

const LUMI: &[u8] = include_bytes!("../demo_assets/chibi/lumi.glb");
const YUNA: &[u8] = include_bytes!("../demo_assets/chibi/yuna.glb");
const CAPE: &[u8] = include_bytes!("../demo_assets/chibi/cape.glb");
const CLOAK_NAME: &str = "Пелерина Луми";
const HOOD: &str = "Lumi | Hood";
const FABRIC: &str = "Lumi | Fabric";
const TRIM: &str = "Lumi | Trim";
const HAIR: &str = "Lumi | Hair";
const IRIS: &str = "Lumi | Irises";
const YUNA_HOOD: &str = "Yuna | Hood";
const YUNA_COAT: &str = "Yuna | Coat";
const YUNA_CUFFS: &str = "Yuna | Fleece";
const YUNA_CAPE: &str = "Yuna | Panel velvet";
const YUNA_LINING: &str = "Yuna | Hood lining";
const YUNA_HAIR: &str = "Yuna | Hair";
const YUNA_HAIR_GLINTS: &str = "Yuna | Hair glints";
const YUNA_IRIS: &str = "Yuna | Irises";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Kind {
    #[default]
    Lumi,
    Yuna,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Lumi => "Луми",
            Self::Yuna => "Юна",
        }
    }

    fn model(self) -> &'static [u8] {
        match self {
            Self::Lumi => LUMI,
            Self::Yuna => YUNA,
        }
    }

    fn color_slots(self) -> [&'static str; 2] {
        match self {
            Self::Lumi => [HAIR, IRIS],
            Self::Yuna => [YUNA_HAIR, YUNA_IRIS],
        }
    }

    fn identify(model: &crate::model::Model) -> Option<Self> {
        if model.has_named_material(YUNA_HOOD) {
            Some(Self::Yuna)
        } else {
            model.has_named_material(HOOD).then_some(Self::Lumi)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Outfit {
    #[default]
    Owl,
    Cape,
    Moth,
    Dusk,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Expression {
    #[default]
    Neutral,
    Smile,
    Joy,
    Surprise,
    Blink,
    Speech,
}

impl Expression {
    const OPTIONS: [(Self, &'static str); 6] = [
        (Self::Neutral, "Спокойно"),
        (Self::Smile, "Улыбка"),
        (Self::Joy, "Радость"),
        (Self::Surprise, "Удивление"),
        (Self::Blink, "Моргание"),
        (Self::Speech, "Артикуляция"),
    ];

    fn sample(self, elapsed: f32, amount: f32) -> Vec<MorphWeight> {
        let weight = |target: &str, weight| MorphWeight {
            target: target.into(),
            weight,
        };
        let amount = amount.clamp(0.0, 1.0);
        match self {
            Self::Neutral => Vec::new(),
            Self::Smile => vec![weight("Smile", amount)],
            Self::Joy => vec![weight("Joy", amount), weight("Smile", 0.0)],
            Self::Surprise => vec![weight("Surprise", amount), weight("Smile", 0.0)],
            Self::Blink => {
                let phase = elapsed.rem_euclid(1.13);
                let close = (1.0 - ((phase - 0.25) / 0.09).abs()).clamp(0.0, 1.0);
                let close = close * close * (3.0 - 2.0 * close) * amount;
                vec![
                    weight("EyeBlinkLeft", close),
                    weight("EyeBlinkRight", close),
                ]
            }
            Self::Speech => {
                const VOWELS: [&str; 4] = ["aa", "ih", "ou", "oh"];
                let beat = (elapsed * 3.5).rem_euclid(VOWELS.len() as f32);
                let open = (std::f32::consts::PI * beat.fract())
                    .sin()
                    .max(0.0)
                    .powf(0.68);
                vec![
                    weight(VOWELS[beat as usize], amount * open),
                    weight("Smile", 0.0),
                ]
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Draft {
    kind: Kind,
    name: String,
    height: f32,
    head_width: f32,
    torso_width: f32,
    outfit: Outfit,
    hair: Option<[f32; 3]>,
    iris: Option<[f32; 3]>,
    expression: Expression,
    expression_strength: f32,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            kind: Kind::Lumi,
            name: "Луми".into(),
            height: 1.62,
            head_width: 1.0,
            torso_width: 1.0,
            outfit: Outfit::Owl,
            hair: None,
            iris: None,
            expression: Expression::Neutral,
            expression_strength: 0.86,
        }
    }
}

impl Draft {
    fn from_avatar(avatar: &Avatar, kind: Kind) -> Self {
        let [hair, iris] = kind.color_slots();
        let outfit = match kind {
            Kind::Lumi
                if avatar
                    .bone_attachments
                    .iter()
                    .any(|part| part.name == CLOAK_NAME) =>
            {
                Outfit::Cape
            }
            Kind::Lumi => Outfit::Owl,
            Kind::Yuna if avatar.look.colors.contains_key(YUNA_COAT) => Outfit::Dusk,
            Kind::Yuna => Outfit::Moth,
        };
        Self {
            kind,
            name: avatar.name.clone(),
            height: avatar.height,
            head_width: avatar.look.scales.get("Head").map_or(1.0, |scale| scale[0]),
            torso_width: avatar
                .look
                .scales
                .get("Torso")
                .map_or(1.0, |scale| scale[0]),
            outfit,
            hair: avatar.look.colors.get(hair).copied(),
            iris: avatar.look.colors.get(iris).copied(),
            expression: Expression::Neutral,
            expression_strength: 0.86,
        }
    }

    fn select(&mut self, kind: Kind) {
        if self.kind == kind {
            return;
        }
        if self.name.trim().is_empty() || self.name == self.kind.name() {
            self.name = kind.name().into();
        }
        self.kind = kind;
        self.hair = None;
        self.iris = None;
        self.expression = Expression::Neutral;
        self.outfit = match kind {
            Kind::Lumi => Outfit::Owl,
            Kind::Yuna => Outfit::Moth,
        };
    }
}

impl Studio {
    fn chibi_kind(&self) -> Option<Kind> {
        self.avatar_asset()
            .and_then(|(model, _)| Kind::identify(model))
    }

    pub(super) fn chibi_button(&mut self, ui: &mut egui::Ui) {
        let existing = self.chibi_kind();
        if ui
            .button(if existing.is_some() {
                "Настроить чиби…"
            } else {
                "Собрать чиби…"
            })
            .clicked()
        {
            self.chibi_draft = Some(
                if let (Some(kind), Some(avatar)) = (existing, &self.scene.avatar) {
                    Draft::from_avatar(avatar, kind)
                } else {
                    Draft::default()
                },
            );
        }
    }

    pub(super) fn chibi_window(&mut self, ctx: &egui::Context) {
        if self.game_running {
            return;
        }
        let Some(mut draft) = self.chibi_draft.take() else {
            return;
        };
        let existing = self.chibi_kind();
        let mut open = true;
        let mut apply = false;
        let mut zoom = None;
        let mut playback = None;
        let mut face_selected = false;
        let mut dismiss = false;
        egui::Window::new("Мастерская чиби")
            .default_pos(Pos2::new(28.0, 157.0))
            .default_width(325.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for kind in [Kind::Lumi, Kind::Yuna] {
                        if ui
                            .selectable_label(draft.kind == kind, kind.name())
                            .clicked()
                        {
                            draft.select(kind);
                        }
                    }
                });
                ui.label("Имя персонажа");
                ui.add(egui::TextEdit::singleline(&mut draft.name).char_limit(100));
                ui.separator();
                ui.add(
                    DragValue::new(&mut draft.height)
                        .range(1.05..=2.0)
                        .speed(0.01)
                        .fixed_decimals(2)
                        .prefix("Рост чиби ")
                        .suffix(" м"),
                );
                ui.add(
                    DragValue::new(&mut draft.head_width)
                        .range(0.86..=1.18)
                        .speed(0.005)
                        .fixed_decimals(2)
                        .prefix("Ширина головы "),
                );
                ui.add(
                    DragValue::new(&mut draft.torso_width)
                        .range(0.90..=1.12)
                        .speed(0.005)
                        .fixed_decimals(2)
                        .prefix("Ширина корпуса "),
                );
                ui.separator();
                ui.label("Костюм");
                ui.horizontal_wrapped(|ui| {
                    let outfits = match draft.kind {
                        Kind::Lumi => [
                            (Outfit::Owl, "Лавандовый костюм"),
                            (Outfit::Cape, "Морская пелерина"),
                        ],
                        Kind::Yuna => [
                            (Outfit::Moth, "Лунный мотылёк"),
                            (Outfit::Dusk, "Лиловые сумерки"),
                        ],
                    };
                    for (value, label) in outfits {
                        ui.selectable_value(&mut draft.outfit, value, label);
                    }
                });
                tint_control(ui, "Тон волос", &mut draft.hair);
                tint_control(ui, "Тон радужек", &mut draft.iris);
                ui.separator();
                let active = existing == Some(draft.kind);
                let button = if active {
                    "Применить внешность"
                } else {
                    "Собрать персонажа"
                };
                apply = ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(button).strong().color(crate::BACKDROP),
                        )
                        .fill(crate::ACCENT),
                    )
                    .clicked();
                if active {
                    ui.horizontal(|ui| {
                        if ui.button("Лицо крупно").clicked() {
                            zoom = Some(true);
                        }
                        if ui.button("В полный рост").clicked() {
                            zoom = Some(false);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (label, motion) in [
                            ("Покой", Motion::Idle),
                            ("Ходьба", Motion::Walk),
                            ("Бег", Motion::Run),
                        ] {
                            if ui
                                .selectable_label(
                                    self.preview_avatar && self.preview_motion == motion,
                                    label,
                                )
                                .clicked()
                            {
                                playback = Some(motion);
                            }
                        }
                    });
                    if draft.kind == Kind::Yuna {
                        ui.separator();
                        ui.label("Мимика");
                        ui.horizontal_wrapped(|ui| {
                            for (expression, label) in Expression::OPTIONS {
                                if ui
                                    .selectable_label(draft.expression == expression, label)
                                    .clicked()
                                {
                                    draft.expression = expression;
                                    face_selected = true;
                                }
                            }
                        });
                        ui.add(
                            egui::Slider::new(&mut draft.expression_strength, 0.0..=1.0)
                                .text("Сила выражения"),
                        );
                    }
                    dismiss = ui.small_button("Закрыть мастерскую").clicked();
                }
            });
        if apply {
            self.status = self
                .apply_chibi(&draft)
                .map(|()| "Силуэт и костюм сохранены в проекте.".into())
                .unwrap_or_else(|message| message);
        }
        if let Some(portrait) = zoom {
            self.chibi_camera(portrait);
        }
        if let Some(motion) = playback {
            self.preview_avatar = true;
            self.preview_motion = motion;
            self.preview_elapsed = 0.0;
            self.preview_external_clip = None;
        }
        if face_selected {
            self.preview_avatar = true;
            self.preview_motion = Motion::Idle;
            self.preview_elapsed = 0.0;
            self.preview_external_clip = None;
        }
        if open && !dismiss {
            self.chibi_draft = Some(draft);
        }
    }

    pub(super) fn chibi_preview_face(&self, model: &Model) -> Vec<MorphWeight> {
        if self.game_running || Kind::identify(model) != Some(Kind::Yuna) {
            return Vec::new();
        }
        let Some(draft) = self
            .chibi_draft
            .as_ref()
            .filter(|draft| draft.kind == Kind::Yuna)
        else {
            return Vec::new();
        };
        draft
            .expression
            .sample(self.preview_elapsed, draft.expression_strength)
    }

    pub(super) fn chibi_preview_light(
        &self,
        model: &crate::model::Model,
    ) -> crate::material_graph::PreviewLight {
        if !self.game_running && self.chibi_draft.is_some() && Kind::identify(model).is_some() {
            crate::material_graph::PreviewLight::Outdoor
        } else {
            crate::material_graph::PreviewLight::Scene
        }
    }

    fn apply_chibi(&mut self, draft: &Draft) -> Result<(), String> {
        let creating = self.chibi_kind() != Some(draft.kind);
        let mut assets = self.assets.clone();
        let source = if !creating && let Some(avatar) = self.scene.avatar.as_ref() {
            avatar.source.clone()
        } else {
            let old = self.scene.avatar.take();
            assets.retain(asset_sources(&self.scene).map(|(source, _)| source));
            self.scene.avatar = old;
            assets.import_bytes(draft.kind.model().to_vec())?
        };
        let model = &assets
            .get(&source)
            .ok_or("Не найден исходный GLB чиби.")?
            .model;
        let mut avatar = self
            .scene
            .avatar
            .as_ref()
            .filter(|actor| actor.source == source)
            .cloned()
            .unwrap_or_else(|| Avatar::new(source, draft.kind.name().into(), model));
        if creating {
            let clip = |name: &str| model.animations.iter().position(|clip| clip.name == name);
            avatar.motion_graph.start = clip("Start");
            avatar.motion_graph.stop = clip("Stop");
            avatar.motion_graph.turn_left = clip("Turn_Left");
            avatar.motion_graph.turn_right = clip("Turn_Right");
        }
        let name = draft.name.trim();
        avatar.name = if name.is_empty() {
            draft.kind.name().into()
        } else {
            name.to_owned()
        };
        avatar.height = draft.height;
        for (node, width) in [("Head", draft.head_width), ("Torso", draft.torso_width)] {
            if (width - 1.0).abs() > 0.0001 {
                avatar.look.scales.insert(node.into(), [width, 1.0, 1.0]);
            } else {
                avatar.look.scales.remove(node);
            }
        }
        let [hair, iris] = draft.kind.color_slots();
        for (tag, tone) in [(hair, draft.hair), (iris, draft.iris)] {
            if let Some(color) = tone {
                avatar.look.colors.insert(tag.into(), color);
            } else {
                avatar.look.colors.remove(tag);
            }
        }
        for tag in [
            HOOD,
            FABRIC,
            TRIM,
            YUNA_HOOD,
            YUNA_COAT,
            YUNA_CUFFS,
            YUNA_CAPE,
            YUNA_LINING,
            YUNA_HAIR_GLINTS,
        ] {
            avatar.look.colors.remove(tag);
        }
        if draft.kind == Kind::Yuna
            && let Some(hair) = draft.hair
        {
            avatar.look.colors.insert(
                YUNA_HAIR_GLINTS.into(),
                hair.map(|channel| 0.42 + channel * 0.58),
            );
        }
        avatar
            .bone_attachments
            .retain(|part| part.name != CLOAK_NAME);
        if draft.outfit == Outfit::Cape {
            avatar.look.colors.extend([
                (HOOD.into(), [0.40, 0.66, 0.58]),
                (FABRIC.into(), [0.28, 0.52, 0.49]),
                (TRIM.into(), [0.91, 0.77, 0.49]),
            ]);
            avatar.bone_attachments.push(BoneAttachment {
                source: assets.import_bytes(CAPE.to_vec())?,
                name: CLOAK_NAME.into(),
                bone: "Torso".into(),
                offset: [0.0; 3],
                rotation: [0.0; 3],
                scale: 1.0,
            });
        }
        if draft.outfit == Outfit::Dusk {
            avatar.look.colors.extend([
                (YUNA_HOOD.into(), [1.0, 0.41, 0.67]),
                (YUNA_COAT.into(), [0.99, 0.40, 0.66]),
                (YUNA_CUFFS.into(), [0.91, 0.64, 0.56]),
                (YUNA_CAPE.into(), [0.22, 0.14, 0.34]),
                (YUNA_LINING.into(), [0.32, 0.17, 0.32]),
            ]);
        }
        avatar.validate()?;
        Self::model_from_library(&assets, &avatar)?;
        self.scene.avatar = Some(avatar);
        self.assets = assets;
        self.assets
            .retain(asset_sources(&self.scene).map(|(source, _)| source));
        if creating && self.scene.play_settings.movement_3d == Movement3d::default() {
            self.scene.play_settings.movement_3d.walk_speed = 1.55;
            self.scene.play_settings.movement_3d.sprint_speed = 3.15;
            self.scene.play_settings.movement_3d.acceleration = 12.0;
            self.scene.play_settings.movement_3d.braking = 19.0;
            self.scene.play_settings.camera.distance = Some(3.25);
        }
        self.sync_edited_human();
        self.chibi_camera(false);
        self.changed();
        Ok(())
    }

    fn chibi_camera(&mut self, portrait: bool) {
        let height = self
            .scene
            .avatar
            .as_ref()
            .map_or(1.62, |avatar| avatar.height);
        self.view = View::Three;
        self.overhead = false;
        self.camera = Camera {
            target: [0.0, height * if portrait { 0.71 } else { 0.49 }, 0.0],
            yaw: if portrait { 0.025 } else { 0.17 },
            pitch: if portrait { 0.03 } else { 0.07 },
            distance: if portrait {
                height * 0.89
            } else {
                height * 1.90
            },
        };
    }
}

fn tint_control(ui: &mut egui::Ui, label: &str, tone: &mut Option<[f32; 3]>) {
    let id = ui.id().with(label);
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            let mut enabled = tone.is_some();
            if ui.checkbox(&mut enabled, label).changed() {
                *tone = enabled.then(|| {
                    egui::Rgba::from(Color32::from_rgb(236, 216, 255)).to_array()[..3]
                        .try_into()
                        .unwrap()
                });
            }
            if let Some(color) = tone {
                crate::inspector::edit_rgb(ui, color);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuna_saves_her_style_and_plays_the_chibi_motion_set() {
        let mut studio = Studio::default();
        let mut draft = Draft::default();
        draft.select(Kind::Yuna);
        draft.head_width = 1.06;
        draft.outfit = Outfit::Dusk;
        draft.hair = Some([0.4, 0.6, 0.8]);
        studio.apply_chibi(&draft).unwrap();

        let actor = studio.scene.avatar.as_ref().unwrap();
        let source = actor.source.clone();
        assert_eq!(studio.assets.export_glb(&source), Some(YUNA));
        assert_eq!(actor.name, "Юна");
        let (model, _) = studio.avatar_asset().unwrap();
        assert_eq!(Kind::identify(model), Some(Kind::Yuna));
        assert!(
            model
                .morph_names()
                .iter()
                .any(|name| name == "EyeBlinkLeft")
        );
        for name in [
            "EyeBlinkRight",
            "Smile",
            "Joy",
            "Surprise",
            "aa",
            "ih",
            "ou",
            "oh",
        ] {
            assert!(
                model.morph_names().iter().any(|target| target == name),
                "{name}"
            );
            assert_eq!(model.morph_rest_weight(name), 0.0, "{name}");
        }
        let neutral = model.pose(None, 0.0).unwrap();
        for (expression, time) in [
            (Expression::Smile, 0.0),
            (Expression::Surprise, 0.0),
            (Expression::Blink, 0.25),
            (Expression::Speech, 0.145),
        ] {
            let weights = expression.sample(time, 0.9);
            let posed = model
                .pose_blended_with_morphs(None, 0.0, None, &[], Default::default(), &[], &weights)
                .unwrap();
            assert!(
                neutral
                    .iter()
                    .zip(&posed)
                    .any(|(a, b)| a[0].position != b[0].position),
                "{expression:?}"
            );
        }
        for motion in [Motion::Idle, Motion::Walk, Motion::Run] {
            let clip = actor.clip(motion).unwrap();
            let first = model.pose(Some(clip), 0.0).unwrap();
            let moved = model.pose(Some(clip), 0.155).unwrap();
            assert!(
                first
                    .iter()
                    .zip(&moved)
                    .any(|(a, b)| a[0].position != b[0].position)
            );
        }
        assert!(
            [
                actor.motion_graph.start,
                actor.motion_graph.stop,
                actor.motion_graph.turn_left,
                actor.motion_graph.turn_right,
            ]
            .into_iter()
            .all(|clip| clip.is_some())
        );
        let json = serde_json::to_vec(actor).unwrap();
        let restored = serde_json::from_slice(&json).unwrap();
        let reloaded = Draft::from_avatar(&restored, Kind::Yuna);
        assert_eq!(reloaded.hair, draft.hair);
        assert_eq!(reloaded.outfit, draft.outfit);
        assert_eq!(reloaded.head_width, draft.head_width);

        draft.select(Kind::Lumi);
        studio.apply_chibi(&draft).unwrap();
        assert_eq!(studio.chibi_kind(), Some(Kind::Lumi));
        assert_ne!(studio.scene.avatar.as_ref().unwrap().source, source);
    }
}
