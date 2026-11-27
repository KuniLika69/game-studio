//! Inspector for the portable particle library. The host supplies paths and world preview.
use eframe::egui::{self, DragValue};

use crate::effects::{Kind, Library, MAX_PRESETS, PresetId};

#[derive(Default)]
pub struct Actions {
    pub changed: bool,
    pub preview: Option<PresetId>,
    pub import: bool,
    pub export: Option<PresetId>,
}

#[derive(Default)]
pub struct Editor {
    selected: Option<PresetId>,
}

impl Editor {
    pub fn select(&mut self, id: PresetId) {
        self.selected = Some(id);
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        library: &mut Library,
        animation_marker: f32,
    ) -> Actions {
        let mut actions = Actions::default();
        ui.heading("Библиотека эффектов");
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(library.presets.len() < MAX_PRESETS, |ui| {
                ui.menu_button("Добавить эффект", |ui| {
                    for kind in Kind::ALL {
                        if ui.button(kind.label()).clicked() {
                            if let Some(id) = library.create(kind) {
                                self.select(id);
                                actions.changed = true;
                            }
                            ui.close();
                        }
                    }
                });
            });
            actions.import = ui.button("Импорт эффекта…").clicked();
        });
        if self.selected.is_none_or(|id| library.preset(id).is_none()) {
            self.selected = library.presets.first().map(|preset| preset.id);
        }
        let label = self
            .selected
            .and_then(|id| library.preset(id))
            .map_or("Выбрать эффект", |preset| {
                preset.settings.name.as_str()
            });
        egui::ComboBox::from_id_salt("effect_library")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for preset in &library.presets {
                    ui.selectable_value(&mut self.selected, Some(preset.id), &preset.settings.name);
                }
            });
        let Some(id) = self.selected else {
            return actions;
        };
        let Some(preset) = library.presets.iter_mut().find(|preset| preset.id == id) else {
            return actions;
        };
        let before = preset.settings.clone();
        let settings = &mut preset.settings;
        ui.label("Имя эффекта");
        if ui.text_edit_singleline(&mut settings.name).changed() {
            settings.name = settings
                .name
                .chars()
                .filter(|ch| !ch.is_control())
                .take(64)
                .collect();
            if settings.name.trim().is_empty() {
                settings.name = before.name.clone();
            }
        }
        ui.label("Вид частиц");
        egui::ComboBox::from_id_salt("effect_kind")
            .selected_text(settings.kind.label())
            .show_ui(ui, |ui| {
                for kind in Kind::ALL {
                    ui.selectable_value(&mut settings.kind, kind, kind.label());
                }
            });
        ui.horizontal_wrapped(|ui| {
            ui.label("Цвет частиц");
            let mut srgb = settings.color.map(|value| (value * 255.0).round() as u8);
            if ui.color_edit_button_srgb(&mut srgb).changed() {
                settings.color = srgb.map(|value| f32::from(value) / 255.0);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                DragValue::new(&mut settings.intensity)
                    .prefix("Интенсивность ")
                    .range(0.0..=3.0)
                    .speed(0.025)
                    .max_decimals(2),
            );
            ui.add(
                DragValue::new(&mut settings.duration)
                    .prefix("Длительность ")
                    .suffix(" с")
                    .range(0.05..=30.0)
                    .speed(0.05)
                    .max_decimals(2),
            );
        });
        if !settings.intensity.is_finite() {
            settings.intensity = before.intensity;
        }
        if !settings.duration.is_finite() {
            settings.duration = before.duration;
        }
        actions.changed |= *settings != before;
        let mut at_marker = library.marker_effect == Some(id);
        let caption = format!("Запуск по метке анимации · {animation_marker:.2} с");
        if ui.checkbox(&mut at_marker, caption).changed() {
            library.marker_effect = at_marker.then_some(id);
            actions.changed = true;
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Воспроизвести эффект").clicked() {
                actions.preview = Some(id);
            }
            if ui.button("Сохранить эффект…").clicked() {
                actions.export = Some(id);
            }
        });
        if ui.small_button("Удалить эффект").clicked() && library.remove(id) {
            self.selected = None;
            actions.changed = true;
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_input;

    #[test]
    fn inspector_authors_effect_and_emits_preview_and_file_commands() {
        let mut app = Harness::default();
        app.frame(Vec::new());
        app.click("Добавить эффект", false);
        app.click("Дым", false);
        let id = app.library.presets[0].id;
        app.edit("Интенсивность ", "1.6");
        app.edit("Длительность ", "2.8");
        app.click("Запуск по метке анимации", true);
        app.click("Воспроизвести эффект", false);
        assert_eq!(app.preview, Some(id));
        app.click("Сохранить эффект…", false);
        assert_eq!(app.export, Some(id));
        app.click("Импорт эффекта…", false);
        assert!(app.import);
        let settings = &app.library.preset(id).unwrap().settings;
        assert_eq!(settings.kind, Kind::Smoke);
        assert!((settings.intensity - 1.6).abs() < 1e-6);
        assert!((settings.duration - 2.8).abs() < 1e-6);
        assert_eq!(app.library.marker_effect, Some(id));
        app.library.validate().unwrap();
    }

    struct Harness {
        ctx: egui::Context,
        driver: proof_input::Driver,
        editor: Editor,
        library: Library,
        output: Option<egui::FullOutput>,
        preview: Option<PresetId>,
        export: Option<PresetId>,
        import: bool,
    }

    impl Default for Harness {
        fn default() -> Self {
            Self {
                ctx: egui::Context::default(),
                driver: proof_input::Driver::new([420, 720]),
                editor: Editor::default(),
                library: Library::default(),
                output: None,
                preview: None,
                export: None,
                import: false,
            }
        }
    }

    impl Harness {
        fn frame(&mut self, events: Vec<egui::Event>) {
            let input = self.driver.raw(events);
            self.output = Some(self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let actions = self.editor.show(ui, &mut self.library, 0.72);
                    self.preview = self.preview.or(actions.preview);
                    self.export = self.export.or(actions.export);
                    self.import |= actions.import;
                });
            }));
        }

        fn click(&mut self, name: &str, prefix: bool) {
            self.driver.skip_frames(42);
            let labels = proof_input::labels(self.output.as_ref().unwrap());
            let point = labels
                .iter()
                .find(|label| {
                    if prefix {
                        label.text.starts_with(name)
                    } else {
                        label.text == name
                    }
                })
                .unwrap_or_else(|| panic!("label {name:?} missing in {labels:?}"))
                .center();
            self.frame(proof_input::hover(point));
            self.frame(proof_input::click(point));
            for _ in 0..4 {
                self.frame(Vec::new());
            }
        }

        fn edit(&mut self, name: &str, text: &str) {
            self.click(name, true);
            self.frame(proof_input::replace_text(text));
            self.frame(proof_input::key(egui::Key::Enter, egui::Modifiers::NONE));
            self.frame(Vec::new());
        }
    }
}
