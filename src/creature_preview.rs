use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use eframe::egui::{self, Color32};
use glam::{Mat4, Quat, Vec2, Vec3};

use crate::{
    assets,
    creature_mesh::{self, Draft, Kind},
    editor_theme,
    material_graph::PreviewLight,
    model::{self, GroundHit, Model, PawLock},
    scene::{Part, PartMotion, Shape},
    viewport3d::{self, Camera, ModelDraw, ViewportScene},
};

type ResultFrame = (Draft, Result<Arc<Model>, String>);

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Motion {
    #[default]
    Idle,
    Walk,
    Run,
    Turn,
}

impl Motion {
    fn name(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Walk | Self::Turn => "Walk",
            Self::Run => "Run",
        }
    }
}

pub(super) struct Preview {
    enabled: bool,
    playing: bool,
    slope: bool,
    motion: Motion,
    time: f32,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    changed_at: f64,
    requested: Option<Draft>,
    complete: Option<Draft>,
    worker: Option<mpsc::Receiver<ResultFrame>>,
    model: Option<(u64, Arc<Model>)>,
    message: Option<String>,
    feet: PawLock,
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            enabled: true,
            playing: true,
            slope: false,
            motion: Motion::Idle,
            time: 0.0,
            yaw: 0.46,
            pitch: 0.16,
            zoom: 1.0,
            changed_at: 0.0,
            requested: None,
            complete: None,
            worker: None,
            model: None,
            message: None,
            feet: PawLock::default(),
        }
    }
}

impl Preview {
    pub(super) fn controls(&mut self, ui: &mut egui::Ui, kind: Kind) {
        ui.checkbox(&mut self.enabled, "Живой предпросмотр");
        if !self.enabled {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (motion, text) in [
                (Motion::Idle, "Покой"),
                (Motion::Walk, "Ходьба"),
                (Motion::Run, "Бег"),
                (Motion::Turn, "Поворот"),
            ] {
                if ui
                    .selectable_value(&mut self.motion, motion, text)
                    .changed()
                {
                    self.time = 0.0;
                    self.feet.reset();
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            let text = if self.playing {
                "Пауза предпросмотра"
            } else {
                "Продолжить предпросмотр"
            };
            if ui.button(text).clicked() {
                self.playing = !self.playing;
            }
            if kind == Kind::Quadruped && ui.checkbox(&mut self.slope, "Тестовый склон").changed()
            {
                self.feet.reset();
            }
        });
        if ui
            .add(egui::Slider::new(&mut self.time, 0.0..=4.0).text("Кадр, с"))
            .changed()
        {
            self.playing = false;
            self.feet.reset();
        }
    }

    pub(super) fn paint(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        response: &egui::Response,
        recipe: Draft,
        height: f32,
        debug: bool,
    ) -> bool {
        if !self.enabled {
            return false;
        }
        self.prepare(ui.ctx(), recipe);
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = ui.input(|i| i.pointer.delta());
            self.yaw -= delta.x * 0.009;
            self.pitch = (self.pitch + delta.y * 0.008).clamp(-0.09, 1.1);
        }
        if response.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta.y);
            self.zoom = (self.zoom * (-delta * 0.004).exp()).clamp(0.54, 2.8);
        }
        let dt = if self.playing {
            ui.input(|i| i.stable_dt.clamp(0.0, 0.05))
        } else {
            0.0
        };
        if self.playing {
            self.time = (self.time + dt).rem_euclid(4.0);
            ui.ctx().request_repaint();
        }
        let Some((token, model)) = &self.model else {
            ui.painter().rect_filled(rect, 0.0, editor_theme::BACKDROP);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.message.as_deref().unwrap_or("Готовлю предпросмотр…"),
                egui::TextStyle::Body.resolve(ui.style()),
                editor_theme::MUTED,
            );
            return true;
        };
        let scale = height / (model.bounds_max[1] - model.bounds_min[1]).max(0.05);
        let angle = if self.motion == Motion::Turn {
            (self.time * 1.57).to_degrees()
        } else {
            0.0
        };
        let dx = if self.motion == Motion::Walk || self.motion == Motion::Run {
            (self.time * 1.57).sin() * 0.27
        } else {
            0.0
        };
        let slope = if self.slope && recipe.kind == Kind::Quadruped {
            0.17_f32
        } else {
            0.0
        };
        let floor_at = |p: Vec2| (0.29 + slope * p.x).max(0.0);
        let origin = Vec3::new(
            dx,
            floor_at(Vec2::new(dx, 0.0)) - model.bounds_min[1] * scale,
            0.0,
        );
        let target = [0.0, 0.29 + height * 0.52, 0.0];
        let camera = Camera {
            yaw: self.yaw,
            pitch: self.pitch,
            distance: (height * 2.65).max(1.55) * self.zoom,
            target,
        };
        let clip = model
            .animations
            .iter()
            .position(|clip| clip.name == self.motion.name());
        let mut bones = Vec::new();
        let mut translations = Vec::new();
        if recipe.kind == Kind::Quadruped {
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::splat(scale),
                Quat::from_rotation_y(angle.to_radians()),
                origin,
            );
            let (corrections, pelvis) = self.feet.solve(
                model::FootLockInput {
                    model,
                    identity: *token,
                    clip,
                    elapsed: self.time,
                    previous: None,
                    offsets: &[],
                    world_from_model: transform,
                    ground_height: floor_at(Vec2::new(dx, 0.0)),
                    dt,
                    enabled: true,
                },
                |p| GroundHit {
                    height: floor_at(p),
                    normal: Vec3::new(-slope, 1.0, 0.0).normalize(),
                },
            );
            bones = corrections;
            translations.extend(pelvis);
        }
        let draw = ModelDraw {
            token: *token,
            model,
            position: origin.to_array(),
            rotation: [0.0, angle, 0.0],
            scale: [scale; 3],
            selected: false,
            tuning: Default::default(),
            vegetation: [0.0; 4],
            receives_weather: false,
            graph: None,
            material_preview: PreviewLight::Outdoor,
            clip,
            elapsed: self.time,
            blend: None,
            bone_offsets: bones,
            bone_translations: translations,
            morph_weights: Vec::new(),
            secondary: Default::default(),
        };
        let ramp = Part {
            id: 1,
            name: String::new(),
            shape: Shape::Box,
            position: [0.0, 0.13, 0.0],
            rotation: [0.0, 0.0, slope.atan().to_degrees()],
            size: [3.2, 0.32, 3.2],
            color: [0.27, 0.25, 0.37],
            motion: PartMotion::Static,
        };
        viewport3d::paint_preview(
            ui,
            rect,
            ViewportScene {
                robot: &[],
                environment: &[ramp],
                architecture: &[],
                models: &[draw],
                camera,
                selected: None,
                sparks: &[],
                glow: 0.0,
                playing: true,
                lighting: Default::default(),
                post_process: Default::default(),
                lights: &[],
                natural: None,
                effects: &[],
            },
        );
        let overlay = ui.painter_at(rect);
        let font = egui::TextStyle::Small.resolve(ui.style());
        overlay.text(
            rect.right_top() + egui::vec2(-18.0, 14.0),
            egui::Align2::RIGHT_TOP,
            self.message
                .as_deref()
                .unwrap_or(if self.requested == self.complete {
                    "ЛКМ — вращать · колесо — приблизить"
                } else {
                    "Обновляю модель…"
                }),
            font.clone(),
            Color32::from_rgb(226, 219, 242),
        );
        if debug && recipe.kind == Kind::Quadruped {
            for contact in self.feet.reports().into_iter().flatten() {
                let p = Vec3::from_array(contact.solved_position_m);
                let n = Vec3::from_array(contact.ground_normal);
                if let Some(point) = camera.screen_point(rect, p.to_array()) {
                    let color = if contact.stance > 0.88 {
                        Color32::from_rgb(124, 240, 190)
                    } else {
                        editor_theme::ACCENT
                    };
                    overlay.circle_filled(point, 4.0, color);
                    if let Some(tip) = camera.screen_point(rect, (p + n * 0.19).to_array()) {
                        overlay.line_segment([point, tip], (1.6, color));
                    }
                }
            }
        }
        true
    }

    fn prepare(&mut self, ctx: &egui::Context, draft: Draft) {
        let now = ctx.input(|input| input.time);
        if self.requested != Some(draft) {
            self.changed_at = if self.requested.is_none() {
                now - 1.0
            } else {
                now
            };
            self.requested = Some(draft);
        }
        if let Some(worker) = &self.worker {
            match worker.try_recv() {
                Ok((built, result)) => {
                    self.worker = None;
                    if self.requested == Some(built) {
                        self.complete = Some(built);
                        match result {
                            Ok(model) => {
                                self.model = Some((assets::render_token(), model));
                                self.message = None;
                            }
                            Err(message) => self.message = Some(message),
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.complete = Some(draft);
                    self.message = Some("Предпросмотр не собран.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.worker.is_none() && self.complete != Some(draft) && now - self.changed_at >= 0.18 {
            let (tx, rx) = mpsc::sync_channel(1);
            let signal = ctx.clone();
            match thread::Builder::new()
                .name("asset-preview".into())
                .spawn(move || {
                    let result = creature_mesh::build(&draft)
                        .and_then(|bytes| model::decode_glb(&bytes).map_err(String::from))
                        .map(Arc::new);
                    let _ = tx.send((draft, result));
                    signal.request_repaint();
                }) {
                Ok(_) => self.worker = Some(rx),
                Err(error) => {
                    self.complete = Some(draft);
                    self.message = Some(error.to_string());
                }
            }
        }
        if self.complete != Some(draft) {
            ctx.request_repaint_after(Duration::from_millis(45));
        }
    }
}
