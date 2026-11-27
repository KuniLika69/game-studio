//! Authoring controls for WAV cues and footstep materials.
use eframe::egui::{self, DragValue};

use crate::{
    audio::Surface,
    audio_project::{
        FootstepZone, Library, MAX_FOOTSTEP_ZONES, MAX_STEP_VARIANTS, MarkerSound, SoundId,
    },
};

#[derive(Default)]
pub struct Actions {
    pub changed: bool,
    pub import_wav: bool,
    pub import_steps: Option<Surface>,
    pub preview: Option<MarkerSound>,
    pub preview_step: Option<Surface>,
}

#[derive(Default)]
pub struct Editor {
    selected: Option<SoundId>,
}

impl Editor {
    pub fn select(&mut self, sound: SoundId) {
        self.selected = Some(sound);
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        library: &mut Library,
        marker_time: f32,
        zones: &[(u32, &str)],
    ) -> Actions {
        ui.heading("Звук сцены");
        let mut actions = Actions {
            import_wav: ui.button("Импорт WAV…").clicked(),
            ..Actions::default()
        };
        if self.selected.is_none_or(|id| library.sound(id).is_none()) {
            self.selected = library.sounds.first().map(|sound| sound.id);
        }
        let display = self
            .selected
            .and_then(|id| library.sound(id))
            .map_or("Выбрать звук", |sound| sound.name.as_str());
        egui::ComboBox::from_id_salt("sound_library")
            .selected_text(display)
            .show_ui(ui, |ui| {
                for sound in &library.sounds {
                    ui.selectable_value(&mut self.selected, Some(sound.id), &sound.name);
                }
            });
        if let Some(id) = self.selected {
            self.sound(ui, library, id, marker_time, &mut actions);
        }
        ui.separator();
        footsteps(ui, library, zones, &mut actions);
        ui.collapsing("Атмосфера и фоли", |ui| {
            let settings = &mut library.ambience;
            actions.changed |= ui.checkbox(&mut settings.enabled, "Дождь, помещение и звуки тела").changed();
            actions.changed |= number(ui, "Дождь ", &mut settings.rain, 0.0..=0.45);
            actions.changed |= number(ui, "Помещение ", &mut settings.room, 0.0..=0.32);
            actions.changed |= number(ui, "Шорох одежды ", &mut settings.cloth, 0.0..=1.0);
            actions.changed |= number(ui, "Дыхание после нагрузки ", &mut settings.breath, 0.0..=1.0);
        });
        actions
    }

    fn sound(
        &mut self,
        ui: &mut egui::Ui,
        library: &mut Library,
        id: SoundId,
        marker_time: f32,
        actions: &mut Actions,
    ) {
        let Some(sound) = library.sounds.iter_mut().find(|sound| sound.id == id) else {
            return;
        };
        ui.label("Имя звука");
        let before = sound.name.clone();
        if ui.text_edit_singleline(&mut sound.name).changed() {
            sound.name = sound
                .name
                .chars()
                .filter(|ch| !ch.is_control())
                .take(64)
                .collect();
            if sound.name.trim().is_empty() {
                sound.name = before.clone();
            }
        }
        actions.changed |= sound.name != before;
        let mut attached = library.marker.is_some_and(|marker| marker.sound == id);
        if ui
            .checkbox(
                &mut attached,
                format!("Звук по метке анимации · {marker_time:.2} с"),
            )
            .changed()
        {
            library.marker = attached.then(|| MarkerSound::new(id));
            actions.changed = true;
        }
        if let Some(marker) = &mut library.marker
            && marker.sound == id
        {
            actions.changed |= ui
                .checkbox(&mut marker.spatial, "Пространственный звук")
                .changed();
            actions.changed |= number(ui, "Громкость звука ", &mut marker.gain, 0.0..=2.0);
            if marker.spatial {
                actions.changed |= number(ui, "Дальность звука ", &mut marker.reach, 1.0..=256.0);
                ui.label("Смещение источника от персонажа");
                ui.horizontal_wrapped(|ui| {
                    for (axis, offset) in ["X ", "Y ", "Z "].into_iter().zip(&mut marker.offset) {
                        actions.changed |= number(ui, axis, offset, -64.0..=64.0);
                    }
                });
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Проверить звук").clicked() {
                actions.preview = Some(
                    library
                        .marker
                        .filter(|marker| marker.sound == id)
                        .unwrap_or_else(|| MarkerSound::new(id)),
                );
            }
            if ui.small_button("Удалить звук").clicked() && library.remove(id) {
                self.selected = None;
                actions.changed = true;
            }
        });
    }
}

fn footsteps(
    ui: &mut egui::Ui,
    library: &mut Library,
    zones: &[(u32, &str)],
    actions: &mut Actions,
) {
    let steps = &mut library.footsteps;
    actions.changed |= ui.checkbox(&mut steps.enabled, "Звуки шагов").changed();
    actions.changed |= ui.checkbox(&mut steps.gait_contacts, "Шаг по контакту стопы")
        .on_hover_text("Использует постановку стопы 3D-скелета. Для 2D и остальных моделей используется пройденное расстояние.")
        .changed();
    if !steps.enabled {
        return;
    }
    ui.label("Материал по умолчанию");
    actions.changed |= material(
        ui,
        egui::Id::new("footsteps_default"),
        &mut steps.default_surface,
    );
    actions.changed |= number(ui, "Длина шага ", &mut steps.stride, 0.1..=8.0);
    actions.changed |= number(ui, "Громкость шагов ", &mut steps.gain, 0.0..=2.0);
    if ui.button("Проверить шаг").clicked() {
        actions.preview_step = Some(steps.default_surface);
    }
    ui.collapsing("Записи шагов по материалам", |ui| {
        for surface in Surface::ALL {
            ui.push_id(surface as usize, |ui| {
                let mut variants = steps.samples.get(&surface).cloned().unwrap_or_default();
                ui.collapsing(format!("{} · {}", surface.label(), variants.len()), |ui| {
                    if ui.button("Разобрать запись шагов…")
                        .on_hover_text("Один PCM WAV с паузами. Новые варианты назначаются выбранному материалу.")
                        .clicked()
                    {
                        actions.import_steps = Some(surface);
                    }
                    if library.sounds.is_empty() {
                        ui.weak("Импортируй WAV, чтобы назначить запись.");
                    }
                    let mut remove = None;
                    for index in 0..variants.len() {
                        ui.horizontal(|ui| {
                            let current = variants[index];
                            let mut selected = current;
                            let name = library
                                .sounds
                                .iter()
                                .find(|sound| sound.id == current)
                                .map_or("Выбрать WAV", |sound| sound.name.as_str());
                            egui::ComboBox::from_id_salt(("step_sample", index))
                                .selected_text(name)
                                .show_ui(ui, |ui| {
                                    for sound in &library.sounds {
                                        if sound.id == current || !variants.contains(&sound.id) {
                                            ui.selectable_value(
                                                &mut selected,
                                                sound.id,
                                                &sound.name,
                                            );
                                        }
                                    }
                                });
                            variants[index] = selected;
                            if ui.small_button("Убрать").clicked() {
                                remove = Some(index);
                            }
                        });
                    }
                    if let Some(index) = remove {
                        variants.remove(index);
                    }
                    ui.horizontal_wrapped(|ui| {
                        let next = library
                            .sounds
                            .iter()
                            .find(|sound| !variants.contains(&sound.id));
                        if ui
                            .add_enabled(
                                next.is_some() && variants.len() < MAX_STEP_VARIANTS,
                                egui::Button::new("+ Вариант WAV"),
                            )
                            .clicked()
                            && let Some(sound) = next
                        {
                            variants.push(sound.id);
                        }
                        if ui.small_button("Прослушать шаг").clicked() {
                            actions.preview_step = Some(surface);
                        }
                    });
                });
                if steps
                    .samples
                    .get(&surface)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    != variants
                {
                    if variants.is_empty() {
                        steps.samples.remove(&surface);
                    } else {
                        steps.samples.insert(surface, variants);
                    }
                    actions.changed = true;
                }
            });
        }
    });
    let available = zones
        .iter()
        .find(|&(id, _)| !steps.zones.iter().any(|entry| entry.zone == *id));
    if ui
        .add_enabled(
            available.is_some() && steps.zones.len() < MAX_FOOTSTEP_ZONES,
            egui::Button::new("+ Материал зоны"),
        )
        .clicked()
        && let Some(&(zone, _)) = available
    {
        steps.zones.push(FootstepZone {
            zone,
            surface: Surface::Wood,
        });
        actions.changed = true;
    }
    if zones.is_empty() {
        ui.weak("Добавь игровую зону, чтобы назначить частный материал.");
    }
    let mut remove = None;
    for index in 0..steps.zones.len() {
        let occupied = steps
            .zones
            .iter()
            .enumerate()
            .filter_map(|(other, value)| (index != other).then_some(value.zone))
            .collect::<Vec<_>>();
        let entry = &mut steps.zones[index];
        let before = *entry;
        ui.push_id(("footstep_zone", index), |ui| {
            let label = zones
                .iter()
                .find(|(id, _)| *id == entry.zone)
                .map_or("Зона удалена", |(_, label)| *label);
            egui::ComboBox::from_id_salt("zone")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for &(id, name) in zones {
                        if !occupied.contains(&id) {
                            ui.selectable_value(&mut entry.zone, id, name);
                        }
                    }
                });
            material(ui, ui.make_persistent_id("surface"), &mut entry.surface);
            if ui.small_button("Убрать материал зоны").clicked() {
                remove = Some(index);
            }
        });
        actions.changed |= *entry != before;
    }
    if let Some(index) = remove {
        steps.zones.remove(index);
        actions.changed = true;
    }
}

fn material(ui: &mut egui::Ui, id: egui::Id, selected: &mut Surface) -> bool {
    let before = *selected;
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected.label())
        .show_ui(ui, |ui| {
            for surface in Surface::ALL {
                ui.selectable_value(selected, surface, surface.label());
            }
        });
    before != *selected
}

fn number(
    ui: &mut egui::Ui,
    label: &str,
    number: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    let before = *number;
    ui.add(
        DragValue::new(number)
            .prefix(label)
            .range(range)
            .speed(0.025)
            .max_decimals(2),
    );
    if !number.is_finite() {
        *number = before;
    }
    before != *number
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audio::{OUTPUT_RATE, pcm16_stereo_wav},
        proof_input,
    };

    #[test]
    fn inspector_binds_a_wav_to_timeline_and_authors_material_zones() {
        let mut app = Harness::default();
        let wave = pcm16_stereo_wav(&[[0.2, 0.1]; 140], OUTPUT_RATE).unwrap();
        let sound = app.library.import_bytes("Звон".into(), wave).unwrap();
        app.frame(Vec::new());
        app.click("Звук по метке анимации", true);
        app.click("Проверить звук", false);
        assert_eq!(app.preview, Some(MarkerSound::new(sound)));
        app.click("Звуки шагов", false);
        app.click("+ Материал зоны", false);
        app.click("Проверить шаг", false);
        assert_eq!(app.step, Some(Surface::Stone));
        assert!(app.library.footsteps.enabled);
        assert_eq!(app.library.footsteps.surface(|id| id == 2), Surface::Wood);
        assert_eq!(app.library.footsteps.surface(|_| false), Surface::Stone);
        app.library.validate().unwrap();
    }

    struct Harness {
        ctx: egui::Context,
        driver: proof_input::Driver,
        editor: Editor,
        output: Option<egui::FullOutput>,
        library: Library,
        preview: Option<MarkerSound>,
        step: Option<Surface>,
    }

    impl Default for Harness {
        fn default() -> Self {
            Self {
                ctx: egui::Context::default(),
                driver: proof_input::Driver::new([460, 760]),
                editor: Editor::default(),
                output: None,
                library: Library::default(),
                preview: None,
                step: None,
            }
        }
    }

    impl Harness {
        fn frame(&mut self, events: Vec<egui::Event>) {
            let input = self.driver.raw(events);
            self.output = Some(self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let result = self
                        .editor
                        .show(ui, &mut self.library, 0.72, &[(2, "Паркет")]);
                    self.preview = self.preview.or(result.preview);
                    self.step = self.step.or(result.preview_step);
                });
            }));
        }

        fn click(&mut self, label: &str, prefix: bool) {
            let labels = proof_input::labels(self.output.as_ref().unwrap());
            let point = labels
                .iter()
                .find(|name| {
                    if prefix {
                        name.text.starts_with(label)
                    } else {
                        name.text == label
                    }
                })
                .unwrap_or_else(|| panic!("No {label} in {labels:?}"))
                .center();
            self.driver.skip_frames(42);
            self.frame(proof_input::hover(point));
            self.frame(proof_input::click(point));
            for _ in 0..4 {
                self.frame(Vec::new());
            }
        }
    }
}
