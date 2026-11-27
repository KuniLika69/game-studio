//! Editor controls for time, weather, and environment volumes.
use eframe::egui::{self, DragValue, RichText};
use glam::Vec3;
use std::ops::RangeInclusive;

use crate::environment::{DayCycle, Environment, WeatherPreset, WeatherTrack, ZoneId};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Day,
    Weather,
    Wind,
    Water,
}

#[derive(Default)]
pub struct Editor {
    page: Page,
    cue: Option<u32>,
    wind: Option<ZoneId>,
    water: Option<ZoneId>,
    preview_seconds: f64,
    preview_playing: bool,
    preview_loop: bool,
}

impl Editor {
    pub fn preview_seconds(&self) -> f64 {
        self.preview_seconds
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut Option<Environment>,
        position: [f32; 3],
    ) -> bool {
        ui.heading("Среда сцены");
        let Some(config) = settings else {
            if ui.button("Настроить время суток и погоду").clicked() {
                *settings = Some(Environment::default());
                return true;
            }
            return false;
        };
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.page, Page::Day, "Время суток");
            ui.selectable_value(&mut self.page, Page::Weather, "Погода");
            ui.selectable_value(&mut self.page, Page::Wind, "Ветер");
            ui.selectable_value(&mut self.page, Page::Water, "Вода");
        });
        ui.separator();
        let changed = match self.page {
            Page::Day => day(ui, &mut config.time),
            Page::Weather => {
                let mist = ui.add(
                    DragValue::new(&mut config.ground_mist_height)
                        .range(0.0..=8.0)
                        .speed(0.05)
                        .max_decimals(2)
                        .prefix("Высота приземного тумана, м "),
                ).on_hover_text("Высота на которой плотность приземного слоя уменьшается вдвое. 0 м оставляет равномерный туман.");
                weather(ui, &mut config.weather, &mut self.cue) | mist.changed()
            }
            Page::Wind => self.wind(ui, config, position),
            Page::Water => self.water(ui, config, position),
        };
        ui.separator();
        if self.page == Page::Weather {
            self.timeline(ui, &config.weather);
        }
        let previous_preview = self.preview_seconds;
        ui.horizontal_wrapped(|ui| {
            ui.add(
                DragValue::new(&mut self.preview_seconds)
                    .range(0.0..=86_400.0)
                    .speed(0.1)
                    .prefix("Секунда просмотра ")
                    .max_decimals(2),
            );
            if ui.button("В начало просмотра").clicked() {
                self.preview_seconds = 0.0;
            }
            let caption = if self.preview_playing { "Пауза погоды" } else { "▶ Показать погоду в движении" };
            if ui.button(caption).clicked() {
                self.preview_playing = !self.preview_playing;
            }
            if self.page == Page::Weather {
                ui.checkbox(&mut self.preview_loop, "Повторять шкалу");
            }
        });
        if !self.preview_seconds.is_finite() {
            self.preview_seconds = previous_preview;
        }
        if self.preview_playing && ui.is_enabled() {
            let next = self.preview_seconds + f64::from(ui.input(|input| input.stable_dt.clamp(0.0, 0.1)));
            self.preview_seconds = if self.preview_loop && self.page == Page::Weather { next.rem_euclid(timeline_span(&config.weather)) } else { next.min(86_400.0) };
            self.preview_playing = self.preview_seconds < 86_400.0;
            ui.ctx().request_repaint();
        }
        let sample = config.sample(self.preview_seconds);
        let minutes = (sample.hour * 60.0).round() as u32 % 1440;
        ui.label(
            RichText::new(format!(
                "Время в сцене {:02}:{:02}",
                minutes / 60,
                minutes % 60
            ))
            .strong(),
        );
        changed
    }

    fn timeline(&mut self, ui: &mut egui::Ui, track: &WeatherTrack) {
        let span = timeline_span(track);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(100.0), 62.0), egui::Sense::click_and_drag());
        let rail = rect.shrink2(egui::vec2(12.0, 18.0));
        let at = |seconds: f64| rail.left() + (seconds / span).clamp(0.0, 1.0) as f32 * rail.width();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 7.0, crate::editor_theme::BACKDROP);
        painter.line_segment([rail.left_center(), rail.right_center()], (2.0, crate::editor_theme::EDGE));
        let font = egui::TextStyle::Small.resolve(ui.style());
        for cue in &track.cues {
            let start = at(cue.start_seconds.into());
            let end = at(f64::from(cue.start_seconds) + f64::from(cue.blend_seconds));
            let accent = match cue.preset {
                WeatherPreset::Clear => egui::Color32::from_rgb(242, 197, 113),
                WeatherPreset::Rain => egui::Color32::from_rgb(134, 185, 252),
                WeatherPreset::Fog => egui::Color32::from_rgb(194, 174, 212),
                WeatherPreset::Snow => egui::Color32::from_rgb(233, 236, 255),
            };
            let marker = egui::pos2(start, rail.center().y);
            painter.line_segment([marker, egui::pos2(end.max(start), marker.y)], (6.0, accent.gamma_multiply(0.53)));
            painter.circle_filled(marker, if self.cue == Some(cue.id) { 5.0 } else { 3.6 }, accent);
        }
        if (response.clicked() || response.dragged()) && let Some(cursor) = response.interact_pointer_pos() {
            self.preview_playing = false;
            self.preview_seconds = f64::from(((cursor.x - rail.left()) / rail.width()).clamp(0.0, 1.0)) * span;
            if response.clicked() && let Some(cue) = track.cues.iter().min_by(|a,b| {
                (at(a.start_seconds.into()) - cursor.x).abs().total_cmp(&(at(b.start_seconds.into()) - cursor.x).abs())
            }).filter(|cue| (at(cue.start_seconds.into()) - cursor.x).abs() < 9.0) {
                self.cue = Some(cue.id);
                self.preview_seconds = f64::from(cue.start_seconds) + f64::from(cue.blend_seconds);
            }
        }
        let playhead = at(self.preview_seconds);
        painter.line_segment([egui::pos2(playhead, rail.top()-3.0), egui::pos2(playhead, rail.bottom()+3.0)], (2.0, crate::editor_theme::ACCENT));
        painter.text(rect.left_bottom() + egui::vec2(10.0,-4.0), egui::Align2::LEFT_BOTTOM, "0 с", font.clone(), crate::editor_theme::MUTED);
        painter.text(rect.right_bottom() + egui::vec2(-10.0,-4.0), egui::Align2::RIGHT_BOTTOM, format!("{span:.0} с"), font, crate::editor_theme::MUTED);
        response.on_hover_text("Клик по метке — к концу перехода. Перетаскивай деление, чтобы просмотреть время.");
    }

    fn wind(&mut self, ui: &mut egui::Ui, config: &mut Environment, position: [f32; 3]) -> bool {
        let mut changed = direction(
            ui,
            "Направление общего ветра",
            &mut config.prevailing_direction,
        );
        if ui.button("+ Зона ветра").clicked()
            && let Some(id) = config.add_wind(position)
        {
            self.wind = Some(id);
            changed = true;
        }
        let choices = config
            .winds
            .iter()
            .map(|zone| (zone.id, zone.name.as_str()))
            .collect::<Vec<_>>();
        if let Some(id) = select(
            ui,
            "environment_wind",
            "Выбрать зону ветра",
            &choices,
            &mut self.wind,
        ) && let Some(index) = config.winds.iter().position(|zone| zone.id == id)
        {
            let zone = &mut config.winds[index];
            let before = zone.clone();
            name(ui, &mut zone.name);
            vector(
                ui,
                "Положение зоны, м",
                ["X ", "Y ", "Z "],
                &mut zone.center,
                -10_000.0..=10_000.0,
            );
            vector(
                ui,
                "Размер зоны, м",
                ["X ", "Y ", "Z "],
                &mut zone.size,
                0.05..=1024.0,
            );
            direction(ui, "Направление ветра", &mut zone.direction);
            value(ui, "Сила ветра ", &mut zone.speed, 0.0..=50.0, " м/с");
            value(ui, "Сила порывов ", &mut zone.gust, 0.0..=35.0, " м/с");
            value(
                ui,
                "Частота порывов ",
                &mut zone.gust_hz,
                0.01..=10.0,
                " Гц",
            );
            value(
                ui,
                "Затухание у границы ",
                &mut zone.feather,
                0.0..=512.0,
                " м",
            );
            changed |= *zone != before;
            if ui.small_button("Удалить зону ветра").clicked() {
                config.winds.remove(index);
                self.wind = None;
                return true;
            }
        }
        changed
    }

    fn water(&mut self, ui: &mut egui::Ui, config: &mut Environment, position: [f32; 3]) -> bool {
        let mut changed = false;
        if ui.button("+ Водный объём").clicked()
            && let Some(id) = config.add_water([position[0], position[2]], position[1])
        {
            self.water = Some(id);
            changed = true;
        }
        let choices = config
            .waters
            .iter()
            .map(|zone| (zone.id, zone.name.as_str()))
            .collect::<Vec<_>>();
        if let Some(id) = select(
            ui,
            "environment_water",
            "Выбрать водный объём",
            &choices,
            &mut self.water,
        ) && let Some(index) = config.waters.iter().position(|zone| zone.id == id)
        {
            let zone = &mut config.waters[index];
            let before = zone.clone();
            name(ui, &mut zone.name);
            vector(
                ui,
                "Центр воды, м",
                ["X ", "Z "],
                &mut zone.center,
                -10_000.0..=10_000.0,
            );
            vector(
                ui,
                "Размер воды, м",
                ["X ", "Z "],
                &mut zone.size,
                0.05..=1024.0,
            );
            ui.checkbox(&mut zone.rounded_edge, "Мягкая кромка лужи")
                .on_hover_text("Овальная поверхность без созданных движком стенок и дна.");
            let mut shore = zone.shore.is_some();
            if ui.checkbox(&mut shore, "Мелководье у берега").changed() {
                zone.shore = shore.then_some(crate::environment::WaterShore {
                    origin: zone.center,
                    seaward: [1.0, 0.0],
                    slope: 0.12,
                });
            }
            if let Some(shore) = &mut zone.shore {
                ui.collapsing("Линия берега", |ui| {
                    vector(ui, "Точка берега, м", ["X ", "Z "], &mut shore.origin, -10_000.0..=10_000.0);
                    let old = shore.seaward;
                    vector(ui, "Направление в воду", ["X ", "Z "], &mut shore.seaward, -1.0..=1.0);
                    if Vec3::new(shore.seaward[0], 0.0, shore.seaward[1]).length_squared() < 0.0001 {
                        shore.seaward = old;
                    }
                    value(ui, "Уклон дна ", &mut shore.slope, 0.01..=3.0, " м/м");
                });
            }
            let floor_cap = (10_000.0_f32 - 0.05).next_down();
            value(ui, "Дно ", &mut zone.floor_y, -10_000.0..=floor_cap, " м");
            let guess = zone.floor_y + 0.05;
            let min_level = if guess - zone.floor_y < 0.05 {
                guess.next_up()
            } else {
                guess
            };
            let max_level = (zone.floor_y + 256.0).min(10_000.0);
            zone.max_level = zone.max_level.clamp(min_level, max_level);
            value(
                ui,
                "Верхняя граница ",
                &mut zone.max_level,
                min_level..=max_level,
                " м",
            );
            zone.level = zone.level.clamp(zone.floor_y, zone.max_level);
            value(
                ui,
                "Уровень воды ",
                &mut zone.level,
                zone.floor_y..=zone.max_level,
                " м",
            );
            vector(
                ui,
                "Течение, м/с",
                ["X ", "Z "],
                &mut zone.current,
                -25.0..=25.0,
            );
            value(ui, "Плавучесть ", &mut zone.buoyancy, 0.0..=6.0, "");
            value(
                ui,
                "Сопротивление воды ",
                &mut zone.linear_drag,
                0.0..=25.0,
                " 1/с",
            );
            value(ui, "Высота волн ", &mut zone.wave_height, 0.0..=0.8, " м");
            value(ui, "Частота волн ", &mut zone.wave_hz, 0.02..=10.0, " Гц");
            ui.horizontal(|ui| {
                ui.label("Цвет воды");
                ui.color_edit_button_rgb(&mut zone.tint);
            });
            crate::assets::materials::blend_selector(ui, &mut zone.blend_mode);
            changed |= *zone != before;
            if ui.small_button("Удалить водный объём").clicked() {
                config.waters.remove(index);
                self.water = None;
                return true;
            }
        }
        changed
    }
}

fn day(ui: &mut egui::Ui, day: &mut DayCycle) -> bool {
    let before = *day;
    value(
        ui,
        "Начальный час ",
        &mut day.start_hour,
        0.0..=24.0_f32.next_down(),
        "",
    );
    value(
        ui,
        "Часов за секунду ",
        &mut day.hours_per_second,
        0.0..=3.0,
        "",
    );
    value(
        ui,
        "Азимут в полдень ",
        &mut day.noon_bearing,
        -360.0..=360.0,
        "°",
    );
    *day != before
}

fn timeline_span(track: &WeatherTrack) -> f64 {
    track.cues.last().map_or(15.0, |cue| {
        let end = f64::from(cue.start_seconds) + f64::from(cue.blend_seconds);
        ((end + 3.0) / 5.0).ceil().mul_add(5.0, 0.0).clamp(15.0, 86_400.0)
    })
}

fn weather(ui: &mut egui::Ui, track: &mut WeatherTrack, selected: &mut Option<u32>) -> bool {
    ui.label("Начальная погода");
    let mut changed = preset(ui, "weather_initial", &mut track.initial);
    if ui.button("+ Переход погоды").clicked() {
        let next = match track.cues.last().map_or(track.initial, |cue| cue.preset) {
            WeatherPreset::Clear => WeatherPreset::Rain,
            WeatherPreset::Rain => WeatherPreset::Fog,
            WeatherPreset::Fog => WeatherPreset::Snow,
            WeatherPreset::Snow => WeatherPreset::Clear,
        };
        if let Some(id) = track.add_cue(next) {
            *selected = Some(id);
            changed = true;
        }
    }
    let choices = track
        .cues
        .iter()
        .map(|cue| {
            (
                cue.id,
                format!("{} · {:.2} с", cue.preset.label(), cue.start_seconds),
            )
        })
        .collect::<Vec<_>>();
    let entries = choices
        .iter()
        .map(|(id, label)| (*id, label.as_str()))
        .collect::<Vec<_>>();
    let Some(id) = select(
        ui,
        "weather_timeline",
        "Выбрать переход",
        &entries,
        selected,
    ) else {
        return changed;
    };
    let Some(index) = track.cues.iter().position(|cue| cue.id == id) else {
        return changed;
    };
    let low = index
        .checked_sub(1)
        .and_then(|earlier| track.cues.get(earlier))
        .map_or(0.0, |cue| {
            let end = cue.end() as f32;
            let end = if f64::from(end) < cue.end() {
                end.next_up()
            } else {
                end
            };
            end.max(cue.start_seconds.next_up())
        });
    let next = track.cues.get(index + 1).map(|cue| cue.start_seconds);
    let high = next.map_or(86_400.0, |time| time.next_down().max(low));
    let cue = &mut track.cues[index];
    let before = cue.clone();
    ui.label("Погода после перехода");
    preset(ui, "weather_target", &mut cue.preset);
    value(
        ui,
        "Секунда начала ",
        &mut cue.start_seconds,
        low..=high,
        " с",
    );
    let limit = next.map_or(600.0, |time| {
        let exact = (f64::from(time) - f64::from(cue.start_seconds)).clamp(0.0, 600.0);
        let bound = exact as f32;
        if f64::from(bound) > exact {
            bound.next_down()
        } else {
            bound
        }
    });
    value(
        ui,
        "Длительность перехода ",
        &mut cue.blend_seconds,
        0.0..=limit,
        " с",
    );
    changed |= *cue != before;
    if ui.small_button("Удалить переход").clicked() {
        track.cues.remove(index);
        *selected = None;
        return true;
    }
    changed
}

fn preset(ui: &mut egui::Ui, source: &'static str, value: &mut WeatherPreset) -> bool {
    let before = *value;
    egui::ComboBox::from_id_salt(source)
        .selected_text(value.label())
        .show_ui(ui, |ui| {
            for choice in WeatherPreset::ALL {
                ui.selectable_value(value, choice, choice.label());
            }
        });
    *value != before
}

fn select(
    ui: &mut egui::Ui,
    source: &'static str,
    placeholder: &str,
    choices: &[(u32, &str)],
    selected: &mut Option<u32>,
) -> Option<u32> {
    if !choices.iter().any(|&(id, _)| Some(id) == *selected) {
        *selected = choices.first().map(|&(id, _)| id);
    }
    let label = choices
        .iter()
        .find_map(|&(id, label)| (Some(id) == *selected).then_some(label))
        .unwrap_or(placeholder);
    egui::ComboBox::from_id_salt(source)
        .selected_text(label)
        .show_ui(ui, |ui| {
            for &(id, label) in choices {
                ui.selectable_value(selected, Some(id), label);
            }
        });
    *selected
}

fn name(ui: &mut egui::Ui, label: &mut String) {
    let previous = label.clone();
    if ui.text_edit_singleline(label).changed() {
        *label = label
            .chars()
            .filter(|ch| !ch.is_control())
            .take(64)
            .collect();
        if label.trim().is_empty() {
            *label = previous;
        }
    }
}

fn direction(ui: &mut egui::Ui, label: &str, axes: &mut [f32; 3]) -> bool {
    let previous = *axes;
    vector(ui, label, ["X ", "Y ", "Z "], axes, -1.0..=1.0);
    if Vec3::from_array(*axes).length_squared() <= 1e-8 {
        *axes = previous;
    }
    *axes != previous
}

fn vector<const N: usize>(
    ui: &mut egui::Ui,
    label: &str,
    axes: [&str; N],
    values: &mut [f32; N],
    range: RangeInclusive<f32>,
) {
    ui.label(label);
    ui.horizontal_wrapped(|ui| {
        for (axis, amount) in axes.into_iter().zip(values) {
            value(ui, axis, amount, range.clone(), "");
        }
    });
}

fn value(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    suffix: &str,
) {
    let before = *value;
    ui.add(
        DragValue::new(value)
            .range(range)
            .speed(0.04)
            .prefix(label)
            .suffix(suffix)
            .max_decimals(2),
    );
    if !value.is_finite() {
        *value = before;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_input;

    #[test]
    fn editor_authors_weather_wind_and_water_from_empty_environment() {
        let mut ui = Harness::default();
        ui.frame(Vec::new());
        ui.click("Настроить время суток и погоду");
        ui.edit("Начальный час ", "15.5");
        ui.edit("Часов за секунду ", "0.25");
        ui.click("Погода");
        ui.click("+ Переход погоды");
        ui.click("+ Переход погоды");
        ui.edit("Секунда просмотра ", "9");
        ui.click("Ветер");
        ui.click("+ Зона ветра");
        ui.edit("Сила ветра ", "6.5");
        ui.click("Вода");
        ui.click("+ Водный объём");
        ui.edit("Уровень воды ", "1.4");
        let edited = ui.config.as_ref().unwrap();
        edited.validate().unwrap();
        assert!((edited.time.start_hour - 15.5).abs() < 1e-6);
        assert!((edited.time.hours_per_second - 0.25).abs() < 1e-6);
        assert_eq!(edited.weather.cues.len(), 2);
        assert_eq!(edited.weather.cues[0].preset, WeatherPreset::Rain);
        assert_eq!(edited.weather.cues[1].preset, WeatherPreset::Fog);
        assert!(edited.weather.cues[0].end() <= f64::from(edited.weather.cues[1].start_seconds));
        assert!((edited.winds[0].speed - 6.5).abs() < 1e-6);
        assert!((edited.waters[0].level - 1.4).abs() < 1e-6);
        assert_eq!(ui.editor.preview_seconds(), 9.0);
        ui.edit("Дно ", "9999.95");
        ui.config.as_ref().unwrap().validate().unwrap();
    }

    struct Harness {
        context: egui::Context,
        driver: proof_input::Driver,
        output: Option<egui::FullOutput>,
        editor: Editor,
        config: Option<Environment>,
    }

    impl Default for Harness {
        fn default() -> Self {
            Self {
                context: egui::Context::default(),
                driver: proof_input::Driver::new([440, 900]),
                output: None,
                editor: Editor::default(),
                config: None,
            }
        }
    }

    impl Harness {
        fn frame(&mut self, events: Vec<egui::Event>) {
            let input = self.driver.raw(events);
            self.output = Some(self.context.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.editor.show(ui, &mut self.config, [0.0, 0.0, 3.2]);
                });
            }));
        }

        fn label(&self, text: &str, prefix: bool) -> egui::Pos2 {
            let labels = proof_input::labels(self.output.as_ref().unwrap());
            labels
                .iter()
                .find(|label| {
                    if prefix {
                        label.text.starts_with(text)
                    } else {
                        label.text == text
                    }
                })
                .unwrap_or_else(|| panic!("label {text:?} missing in {labels:?}"))
                .center()
        }

        fn click_at(&mut self, point: egui::Pos2) {
            self.frame(proof_input::hover(point));
            self.frame(proof_input::click(point));
            for _ in 0..4 {
                self.frame(Vec::new());
            }
        }

        fn click(&mut self, label: &str) {
            self.driver.skip_frames(42);
            self.click_at(self.label(label, false));
        }

        fn edit(&mut self, prefix: &str, text: &str) {
            self.driver.skip_frames(42);
            self.click_at(self.label(prefix, true));
            self.frame(proof_input::replace_text(text));
            self.frame(proof_input::key(egui::Key::Enter, egui::Modifiers::NONE));
            self.frame(Vec::new());
        }
    }
}
