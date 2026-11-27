use std::{fs::File, io::Read, path::Path};

use eframe::egui::{self, Color32};

use crate::{
    face_timeline::{Group, Key, Lane, MAX_MS, MIN_MS, valid_name},
    story::Story,
};

#[derive(Default)]
pub struct Preview {
    target: Option<(u64, u32)>,
    seconds: f32,
    playing: bool,
    morph: String,
    key: Option<u16>,
    filter: Option<Group>,
    import_error: Option<String>,
}

impl Preview {
    pub fn select_node(&mut self, npc: u64, node: u32) {
        if self.target != Some((npc, node)) {
            *self = Self {
                target: Some((npc, node)),
                ..Self::default()
            };
        }
    }

    pub fn timeline_time(&self, npc: u64) -> Option<(u32, f32)> {
        let (owner, node) = self.target?;
        (owner == npc).then_some((node, self.seconds))
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn advance(&mut self, dt: f32, story: &Story) {
        if !self.playing || !dt.is_finite() || dt <= 0.0 {
            return;
        }
        if let Some((npc, node)) = self.target
            && let Some(lane) = story.facial_lane(npc, node)
        {
            let end = seconds(lane.duration_ms);
            self.seconds = (self.seconds + dt).min(end);
            self.playing = self.seconds < end;
        } else {
            self.playing = false;
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        lane: &mut Option<Lane>,
        model_targets: &[String],
    ) -> bool {
        let mut changed = false;
        if ui.button("Импорт лицевой дорожки…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Лицевая дорожка", &["face.json", "json"])
                .pick_file()
        {
            match read_lane(&path, model_targets) {
                Ok(imported) => {
                    *lane = Some(imported);
                    self.seconds = 0.0;
                    self.playing = false;
                    self.key = None;
                    self.import_error = None;
                    changed = true;
                }
                Err(error) => self.import_error = Some(error),
            }
        }
        if let Some(error) = &self.import_error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        if lane.is_none() {
            let available = model_targets.iter().any(|target| valid_name(target));
            if !available {
                ui.weak("У модели нет именованных мимических морфов.");
                return false;
            }
            if ui.button("Создать лицевую дорожку").clicked() {
                *lane = Some(Lane::default());
                changed = true;
            }
        }
        let Some(current) = lane.as_mut() else {
            return changed;
        };
        ui.horizontal_wrapped(|ui| {
            let mut duration = seconds(current.duration_ms);
            if ui
                .add(
                    egui::DragValue::new(&mut duration)
                        .prefix("Длительность ")
                        .suffix(" с")
                        .range(seconds(MIN_MS)..=seconds(MAX_MS))
                        .speed(0.01),
                )
                .changed()
            {
                changed |= current.resize(millis(duration));
                self.seconds = self.seconds.min(seconds(current.duration_ms));
            }
            let play = if self.playing {
                "Пауза лица"
            } else {
                "Сыграть лицевую дорожку"
            };
            if ui.button(play).clicked() {
                if !self.playing && self.seconds >= seconds(current.duration_ms) {
                    self.seconds = 0.0;
                }
                self.playing = !self.playing;
            }
        });
        if ui
            .add(
                egui::Slider::new(&mut self.seconds, 0.0..=seconds(current.duration_ms))
                    .text("Время реплики")
                    .suffix(" с")
                    .fixed_decimals(3),
            )
            .changed()
        {
            self.playing = false;
        }
        timeline(ui, self, current);
        self.choose_morph(ui, current, model_targets);
        let allowed = model_targets.iter().any(|name| name == &self.morph);
        if !self.morph.is_empty() {
            let ms = millis(self.seconds).min(current.duration_ms);
            if ui
                .add_enabled(allowed, egui::Button::new("+ Ключ лица"))
                .clicked()
            {
                changed |= current.add_key(&self.morph, ms);
                self.key = current
                    .channel(&self.morph)
                    .filter(|channel| channel.keys.iter().any(|key| key.ms == ms))
                    .map(|_| ms);
                self.playing = false;
            }
            if let Some(channel) = current.channel(&self.morph) {
                ui.horizontal_wrapped(|ui| {
                    for key in &channel.keys {
                        if ui
                            .selectable_label(self.key == Some(key.ms), timestamp(key.ms))
                            .clicked()
                        {
                            self.set_key(key.ms);
                        }
                    }
                });
            }
            changed |= self.edit_key(ui, current);
        }
        if ui.small_button("Удалить лицевую дорожку").clicked() {
            *lane = None;
            self.key = None;
            self.seconds = 0.0;
            self.playing = false;
            changed = true;
        }
        changed
    }

    fn choose_morph(&mut self, ui: &mut egui::Ui, lane: &Lane, model_targets: &[String]) {
        let mut targets: Vec<_> = model_targets
            .iter()
            .filter(|name| valid_name(name))
            .map(String::as_str)
            .collect();
        for channel in &lane.channels {
            if !targets.contains(&channel.target.as_str()) {
                targets.push(&channel.target);
            }
        }
        targets.sort_unstable();
        targets.dedup();
        let before = self.filter;
        egui::ComboBox::from_id_salt("face_group")
            .selected_text(self.filter.map_or("Все каналы лица", Group::label))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.filter, None, "Все каналы лица");
                for group in Group::ALL {
                    if targets.iter().any(|target| Group::of(target) == group) {
                        ui.selectable_value(&mut self.filter, Some(group), group.label());
                    }
                }
            });
        if before != self.filter || !targets.contains(&self.morph.as_str()) {
            let next = targets
                .iter()
                .copied()
                .find(|target| self.filter.is_none_or(|group| Group::of(target) == group))
                .unwrap_or("");
            self.morph = next.into();
            self.key = None;
        }
        let before = self.morph.clone();
        ui.label("Морф модели");
        egui::ComboBox::from_id_salt("face_morph")
            .selected_text(if self.morph.is_empty() {
                "Выбрать морф"
            } else {
                &self.morph
            })
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for target in targets
                    .into_iter()
                    .filter(|target| self.filter.is_none_or(|group| Group::of(target) == group))
                {
                    let label = if model_targets.iter().any(|name| name == target) {
                        target.to_owned()
                    } else {
                        format!("{target} · нет у модели")
                    };
                    ui.selectable_value(&mut self.morph, target.to_owned(), label);
                }
            });
        if before != self.morph {
            self.key = None;
        }
    }

    fn set_key(&mut self, ms: u16) {
        self.key = Some(ms);
        self.seconds = seconds(ms);
        self.playing = false;
    }

    fn edit_key(&mut self, ui: &mut egui::Ui, lane: &mut Lane) -> bool {
        let Some(old_ms) = self.key else {
            return false;
        };
        let Some(key) = lane
            .channel(&self.morph)
            .and_then(|channel| channel.keys.iter().find(|key| key.ms == old_ms))
            .copied()
        else {
            return false;
        };
        let mut at = seconds(key.ms);
        let mut weight = key.weight;
        let (edited, remove) = ui
            .horizontal_wrapped(|ui| {
                let moved = ui
                    .add(
                        egui::DragValue::new(&mut at)
                            .prefix("Время ключа ")
                            .suffix(" с")
                            .fixed_decimals(3)
                            .range(0.0..=seconds(lane.duration_ms))
                            .speed(0.005),
                    )
                    .changed();
                let valued = ui
                    .add(
                        egui::DragValue::new(&mut weight)
                            .prefix("Сила ")
                            .fixed_decimals(3)
                            .range(0.0..=1.0)
                            .speed(0.01),
                    )
                    .changed();
                (
                    moved || valued,
                    ui.small_button("Удалить ключ лица").clicked(),
                )
            })
            .inner;
        if remove {
            self.key = None;
            return lane.remove_key(&self.morph, old_ms);
        }
        let ms = millis(at);
        if edited && lane.update_key(&self.morph, old_ms, Key { ms, weight }) {
            self.set_key(ms);
            return true;
        }
        false
    }
}

fn read_lane(path: &Path, targets: &[String]) -> Result<Lane, String> {
    const BYTE_LIMIT: usize = 128 * 1024;
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(BYTE_LIMIT as u64 + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("Не удалось открыть лицевую дорожку. {error}"))?;
    if bytes.len() > BYTE_LIMIT {
        return Err("Файл лицевой дорожки больше 128 КиБ.".into());
    }
    let lane: Lane = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Некорректный JSON лицевой дорожки. {error}"))?;
    lane.validate()?;
    if let Some(channel) = lane.channels.iter().find(|channel| !targets.contains(&channel.target)) {
        return Err(format!("У выбранной модели нет морфа «{}».", channel.target));
    }
    Ok(lane)
}

fn timeline(ui: &mut egui::Ui, preview: &mut Preview, lane: &Lane) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(80.0), 51.0),
        egui::Sense::click_and_drag(),
    );
    let track = rect.shrink(6.0);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
    let duration = f32::from(lane.duration_ms);
    let x_at = |ms: u16| track.left() + track.width() * f32::from(ms) / duration;
    let mut closest = None;
    for (index, group) in Group::ALL.into_iter().enumerate() {
        let y = track.top() + track.height() * (index as f32 + 0.5) / Group::ALL.len() as f32;
        let color = match group {
            Group::Blink => Color32::from_rgb(113, 193, 253),
            Group::Mouth => Color32::from_rgb(255, 153, 132),
            Group::Brow => Color32::from_rgb(230, 188, 111),
            Group::Gaze => Color32::from_rgb(139, 210, 151),
            Group::Other => Color32::from_rgb(196, 156, 234),
        };
        painter.hline(
            track.x_range(),
            y,
            egui::Stroke::new(1.0_f32, color.gamma_multiply(0.3)),
        );
        for channel in lane
            .channels
            .iter()
            .filter(|channel| Group::of(&channel.target) == group)
        {
            for key in &channel.keys {
                let at = egui::pos2(x_at(key.ms), y);
                let chosen = preview.morph == channel.target && preview.key == Some(key.ms);
                painter.circle_filled(at, if chosen { 4.0 } else { 2.7 }, color);
                if let Some(pointer) = response.interact_pointer_pos() {
                    let distance = (at - pointer).length_sq();
                    if distance <= 31.0 && closest.is_none_or(|(old, _, _)| old > distance) {
                        closest = Some((distance, &channel.target, key.ms));
                    }
                }
            }
        }
    }
    let cursor_x = x_at(millis(preview.seconds).min(lane.duration_ms));
    painter.vline(
        cursor_x,
        track.y_range(),
        egui::Stroke::new(1.3_f32, Color32::WHITE),
    );
    if response.clicked() || response.dragged() {
        if response.clicked()
            && let Some((_distance, morph, ms)) = closest
        {
            preview.morph.clone_from(morph);
            preview.filter = None;
            preview.set_key(ms);
        } else if let Some(position) = response.interact_pointer_pos() {
            let fraction = ((position.x - track.left()) / track.width()).clamp(0.0, 1.0);
            preview.seconds = fraction * seconds(lane.duration_ms);
            preview.playing = false;
            preview.key = None;
        }
    }
}

fn seconds(ms: u16) -> f32 {
    f32::from(ms) / 1_000.0
}

fn millis(seconds: f32) -> u16 {
    (seconds * 1_000.0).round() as u16
}

fn timestamp(ms: u16) -> String {
    format!("{:.3} с", seconds(ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_input::{self, Driver, click, find_label, replace_text};

    #[test]
    fn facial_editor_authors_morph_key_and_previews_without_saving_clock() {
        let ctx = egui::Context::default();
        let mut driver = Driver::new([850, 820]);
        let mut preview = Preview::default();
        preview.select_node(17, 2);
        let mut lane = None;
        let model = ["eyeBlinkLeft", "jawOpen", "browInnerUp", "eyeLookUpLeft"].map(str::to_owned);
        let mut show = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| preview.show(ui, &mut lane, &model));
        };
        let mut output = ctx.run(driver.raw(Vec::new()), &mut show);
        for (label, text) in [
            ("Создать лицевую дорожку", None),
            ("Все каналы лица", None),
            ("Моргание", None),
            ("+ Ключ лица", None),
            ("Сила 1.000", Some("0")),
            ("0.000 с", Some("0.180")),
            ("+ Ключ лица", None),
            ("Сила 0.000", Some("0.850")),
            ("Сыграть лицевую дорожку", None),
        ] {
            let point = find_label(&output, label, 0)
                .unwrap_or_else(|| panic!("missing face authoring control {label}"));
            driver.skip_frames(45);
            let _ = ctx.run(driver.raw(click(point)), &mut show);
            if let Some(text) = text {
                let _ = ctx.run(driver.raw(replace_text(text)), &mut show);
                let _ = ctx.run(
                    driver.raw(proof_input::key(egui::Key::Enter, egui::Modifiers::NONE)),
                    &mut show,
                );
            }
            output = ctx.run(driver.raw(Vec::new()), &mut show);
        }
        assert!(preview.playing());
        assert_eq!(preview.timeline_time(17), Some((2, 0.180)));
        assert_eq!(preview.timeline_time(16), None);
        let lane = lane.unwrap();
        assert_eq!(lane.weight("eyeBlinkLeft", 0.0), Some(0.0));
        assert_eq!(lane.weight("eyeBlinkLeft", 0.180), Some(0.850));
        assert_eq!(lane.weight("jawOpen", 0.0), None);
        lane.validate().unwrap();
        preview.select_node(17, 3);
        assert!(!preview.playing());
        assert_eq!(preview.timeline_time(17), Some((3, 0.0)));
    }
}
