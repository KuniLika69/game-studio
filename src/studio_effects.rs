use super::*;
use crate::effects::{self, EmitterId, Particle, PresetId, Session};

#[derive(Default)]
pub(super) struct Runtime {
    session: Session,
    seconds: f64,
    preview_trail: Option<PreviewTrail>,
    followers: Vec<EmitterId>,
}

impl Runtime {
    pub(super) fn is_active(&self) -> bool {
        self.session.is_active()
    }
}

#[derive(Clone, Copy)]
struct PreviewTrail {
    id: EmitterId,
    origin: Vec3,
    started: f64,
}

impl Studio {
    pub(super) fn effect_properties(&mut self, ctx: &egui::Context) -> bool {
        if self.editor_tab != EditorTab::Effects {
            return false;
        }
        egui::SidePanel::right("effects_libraries")
            .default_width(310.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let actions = ui
                        .add_enabled_ui(!self.game_running, |ui| {
                            self.effects_editor.show(
                                ui,
                                &mut self.scene.effects,
                                self.scene.animation.event_time,
                            )
                        })
                        .inner;
                    if actions.changed {
                        self.changed();
                    }
                    if let Some(id) = actions.preview {
                        self.preview_effect(id);
                    }
                    if actions.import
                        && let Some(path) = self.effect_path(false)
                    {
                        self.status = match self.scene.effects.import(&path) {
                            Ok(id) => {
                                self.effects_editor.select(id);
                                self.changed();
                                "Эффект добавлен в библиотеку.".into()
                            }
                            Err(error) => error,
                        };
                    }
                    if let Some(id) = actions.export
                        && let Some(path) = self.effect_path(true)
                    {
                        self.status = match self.scene.effects.export(id, &path) {
                            Ok(()) => format!("Эффект сохранён. {}", path.display()),
                            Err(error) => error,
                        };
                    }
                    ui.separator();
                    ui.weak(&self.status);
                });
            });
        true
    }

    fn effect_path(&self, save: bool) -> Option<PathBuf> {
        self.proof.as_ref().map_or_else(
            || {
                let dialog = rfd::FileDialog::new().add_filter("Эффект Студии игр", &["json"]);
                if save {
                    dialog.set_file_name("scene.effect.json").save_file()
                } else {
                    dialog.pick_file()
                }
            },
            |proof| proof.effect_file.clone(),
        )
    }

    fn preview_effect(&mut self, id: PresetId) {
        let origin = self.scene.map_2d.as_ref().map_or_else(
            || {
                if self.view == View::Two && self.overhead {
                    [self.plan_offset.x, 0.0, self.plan_offset.y]
                } else {
                    [self.camera.target[0], 0.0, self.camera.target[2]]
                }
            },
            |map| map.to_world(map.spawn),
        );
        let kind = self
            .scene
            .effects
            .preset(id)
            .map(|preset| preset.settings.kind);
        let Some(emitter) = self.play_effect(id, origin) else {
            self.status = "Проверь интенсивность выбранного эффекта.".into();
            return;
        };
        if kind == Some(effects::Kind::Trail) {
            self.effects_runtime.preview_trail = Some(PreviewTrail {
                id: emitter,
                origin: Vec3::from_array(origin),
                started: self.effects_runtime.seconds,
            });
        }
        self.status = "Предпросмотр эффекта.".into();
    }

    pub(super) fn play_effect(&mut self, id: PresetId, origin: [f32; 3]) -> Option<EmitterId> {
        let runtime = &mut self.effects_runtime;
        runtime.session.play(
            &self.scene.effects,
            id,
            Vec3::from_array(origin),
            runtime.seconds,
        )
    }

    pub(super) fn move_effect(&mut self, id: EmitterId, origin: [f32; 3]) -> bool {
        let runtime = &mut self.effects_runtime;
        runtime
            .session
            .move_emitter(id, Vec3::from_array(origin), runtime.seconds)
    }

    pub(super) fn follow_effect(&mut self, id: EmitterId) {
        if !self.effects_runtime.followers.contains(&id)
            && self.move_effect(id, self.animation_origin().to_array())
        {
            self.effects_runtime.followers.push(id);
        }
    }

    pub(super) fn play_marked_effect(&mut self, before: f32, after: f32) -> bool {
        let Some(id) =
            self.scene
                .effects
                .animation_effect(before, after, self.scene.animation.event_time)
        else {
            return false;
        };
        if let Some(emitter) = self.play_effect(id, self.animation_origin().to_array()) {
            self.follow_effect(emitter);
        }
        true
    }

    pub(super) fn advance_effects(&mut self, dt: f32) {
        if !self.effects_runtime.is_active() || !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.effects_runtime.seconds += f64::from(dt);
        if !self.effects_runtime.followers.is_empty() {
            let position = self.animation_origin();
            let runtime = &mut self.effects_runtime;
            let seconds = runtime.seconds;
            let session = &mut runtime.session;
            runtime
                .followers
                .retain(|&id| session.move_emitter(id, position, seconds));
        }
        if let Some(preview) = self.effects_runtime.preview_trail {
            let age = (self.effects_runtime.seconds - preview.started) as f32;
            let sweep = Vec3::new(age.min(1.5) * 2.1, 0.0, (age * 2.4).sin() * 0.38);
            if !self.move_effect(preview.id, (preview.origin + sweep).to_array()) {
                self.effects_runtime.preview_trail = None;
            }
        }
        self.effects_runtime
            .session
            .expire(self.effects_runtime.seconds);
    }

    pub(super) fn effects_active(&self) -> bool {
        self.effects_runtime.is_active()
    }

    pub(super) fn effect_particles(&self) -> Vec<Particle> {
        let runtime = &self.effects_runtime;
        if !runtime.is_active() {
            return Vec::new();
        }
        let budget = if self.low_fx { 128 } else { 512 };
        if self.scene.environment.is_some() {
            runtime.session.samples(runtime.seconds, budget, |point| {
                self.environment_wind(point)
            })
        } else {
            runtime
                .session
                .samples(runtime.seconds, budget, |_| Vec3::ZERO)
        }
    }
}
