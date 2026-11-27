//! Studio controls for the procedural floor-plan experiment.
use super::*;
use crate::{
    dungeon::{self, Layout, Settings},
    dungeon_map,
};

#[derive(Default)]
pub(super) struct Editor {
    open: bool,
    settings: Settings,
    seed: String,
    preview: Option<(Scene, Layout)>,
    feedback: String,
    zone: Option<u32>,
    checked: Option<(u64, u32, Option<usize>)>,
}

impl Editor {
    pub(super) fn open(&mut self) {
        if self.seed.is_empty() {
            self.seed = self.settings.seed.to_string();
        }
        self.open = true;
    }

    fn window(&mut self, ctx: &egui::Context) -> Option<Scene> {
        if !self.open {
            return None;
        }
        let mut open = self.open;
        let mut use_preview = false;
        egui::Window::new("Закрытая карта · эксперимент")
            .resizable(false)
            .collapsible(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Seed");
                    if ui
                        .add(egui::TextEdit::singleline(&mut self.seed).desired_width(153.0))
                        .changed()
                    {
                        self.preview = None;
                    }
                });
                let old = self.settings;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.settings.size[0])
                            .prefix("Ширина ")
                            .range(dungeon::MIN_SIDE..=dungeon::MAX_SIDE),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.settings.size[1])
                            .prefix("Высота ")
                            .range(dungeon::MIN_SIDE..=dungeon::MAX_SIDE),
                    );
                });
                let max_rooms = self.settings.max_rooms();
                self.settings.rooms = self.settings.rooms.clamp(dungeon::MIN_ROOMS, max_rooms);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.settings.rooms)
                            .prefix("Комнаты ")
                            .range(dungeon::MIN_ROOMS..=max_rooms),
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.settings.extra_passages)
                            .prefix("Доп. проходы ")
                            .suffix(" %")
                            .range(0..=100),
                    );
                });
                if self.settings != old {
                    self.preview = None;
                }
                if ui.button("Построить карту").clicked() {
                    let result = self
                        .seed
                        .trim()
                        .parse::<u64>()
                        .map_err(|_| {
                            "Seed должен быть целым числом от 0 до 18446744073709551615.".to_owned()
                        })
                        .and_then(|seed| {
                            self.settings.seed = seed;
                            dungeon_map::scene(self.settings)
                        });
                    match result {
                        Ok(generated) => {
                            self.feedback.clear();
                            self.preview = Some(generated);
                        }
                        Err(error) => {
                            self.preview = None;
                            self.feedback = error;
                        }
                    }
                }
                if !self.feedback.is_empty() {
                    ui.colored_label(Color32::LIGHT_RED, &self.feedback);
                }
                if let Some((_, layout)) = &self.preview {
                    map_preview(ui, layout);
                    ui.label(format!(
                        "Комнаты {} · Шаги до выхода {} · Открытые клетки {}",
                        layout.room_count(),
                        layout.route.len().saturating_sub(1),
                        layout.floor_cells()
                    ));
                    if ui.button("Создать сцену").clicked() {
                        use_preview = true;
                    }
                }
            });
        self.open = open && !use_preview;
        if !use_preview {
            return None;
        }
        self.zone = None;
        self.checked = None;
        self.preview.take().map(|(scene, _)| scene)
    }

    pub(super) fn check(&mut self, ui: &mut egui::Ui, scene: &Scene, revision: u64) {
        let Some(map) = &scene.map_2d else { return };
        if map.mode != map2d::Mode::Top || scene.rules.zones.is_empty() {
            return;
        }
        ui.collapsing("Проверка маршрута", |ui| {
            if self.zone.is_none_or(|id| scene.rules.zone(id).is_none()) {
                self.zone = scene
                    .rules
                    .zones
                    .iter()
                    .find(|zone| zone.name == dungeon_map::EXIT_NAME)
                    .or_else(|| scene.rules.zones.first())
                    .map(|zone| zone.id);
            }
            let Some(id) = self.zone else { return };
            let name = scene.rules.zone(id).map_or("", |zone| zone.name.as_str());
            egui::ComboBox::from_id_salt("route_exit")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for zone in &scene.rules.zones {
                        ui.selectable_value(&mut self.zone, Some(zone.id), &zone.name);
                    }
                });
            let zone = self.zone.unwrap_or(id);
            if ui.button("Проверить путь").clicked() {
                let path = dungeon_map::exit_cell(scene, zone)
                    .and_then(|exit| dungeon_map::route(map, exit));
                self.checked = Some((
                    revision,
                    zone,
                    path.map(|cells| cells.len().saturating_sub(1)),
                ));
            }
            ui.weak("По клеточным коллизиям от старта до центра выбранной зоны.");
            if let Some((edit, target, steps)) = self.checked
                && edit == revision
                && target == zone
            {
                if let Some(steps) = steps {
                    ui.colored_label(ACCENT, format!("Путь найден · {steps} шагов по клеткам"));
                } else {
                    ui.colored_label(Color32::LIGHT_RED, "Выход недостижим по клеткам");
                }
            }
        });
    }
}

fn map_preview(ui: &mut egui::Ui, layout: &Layout) {
    let [width, height] = layout.settings.size.map(f32::from);
    let cell = (368.0 / width).min(238.0 / height);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width * cell, height * cell),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(41, 33, 49));
    let position = |[x, y]: dungeon::Cell| rect.min + egui::vec2(f32::from(x), f32::from(y)) * cell;
    for y in 0..layout.settings.size[1] {
        for x in 0..layout.settings.size[0] {
            let color = match layout.tile([x, y]) {
                Some(dungeon::Tile::Room) => Color32::from_rgb(104, 126, 147),
                Some(dungeon::Tile::Passage) => Color32::from_rgb(76, 101, 119),
                _ => continue,
            };
            let tile = Rect::from_min_size(position([x, y]), egui::vec2(cell, cell));
            painter.rect_filled(tile, 0.0, color);
        }
    }
    let center = |tile| position(tile) + egui::vec2(cell, cell) * 0.5;
    let path = layout
        .route
        .iter()
        .map(|&tile| center(tile))
        .collect::<Vec<_>>();
    painter.add(egui::Shape::line(
        path,
        egui::Stroke::new((cell * 0.24).max(1.0), ACCENT),
    ));
    painter.circle_filled(
        center(layout.entrance),
        cell * 0.42,
        Color32::from_rgb(255, 200, 116),
    );
    painter.circle_filled(center(layout.exit), cell * 0.42, ACCENT);
}

impl Studio {
    pub(super) fn dungeon_window(&mut self, ctx: &egui::Context) {
        if let Some(scene) = self.dungeon_editor.window(ctx) {
            self.set_scene(
                scene,
                None,
                "Закрытая карта создана. Сохрани её как проект.",
            );
            self.view = View::Two;
        }
    }
}
