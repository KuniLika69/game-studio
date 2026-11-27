use std::{collections::HashMap, path::Path};

use eframe::egui::{self, Color32, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};

use super::{
    Atlas, Collision, Layer, Map, Mode, PERSON_PNG, Player, Tile, add_marker, decode_png,
    object_bounds,
    terrain_atlas::Style as TerrainStyle,
    tooling::{CellRect, MapPatch},
};
use crate::{
    editor_theme::{ACCENT, BACKDROP},
    effects::Particle,
    effects_2d, environment, environment_2d, rules,
    scene::Scene,
    world::InteractionState,
};

#[derive(Clone, Copy)]
pub struct ViewTransform {
    pub rect: Rect,
    pub origin: Pos2,
    pub cell_points: f32,
    size: [u16; 2],
}

impl ViewTransform {
    pub fn point_at(self, [x, row]: [f32; 2]) -> Pos2 {
        self.origin + egui::vec2(x, row) * self.cell_points
    }

    pub fn at_cell(self, [x, row]: [u16; 2]) -> Pos2 {
        self.point_at([f32::from(x) + 0.5, f32::from(row) + 0.5])
    }

    pub fn tile_at(self, position: Pos2) -> Option<[u16; 2]> {
        let point = (position - self.origin) / self.cell_points;
        (self.rect.contains(position)
            && point.x >= 0.0
            && point.y >= 0.0
            && point.x < f32::from(self.size[0])
            && point.y < f32::from(self.size[1]))
        .then_some([point.x.floor() as u16, point.y.floor() as u16])
    }

    fn cell_clamped(self, position: Pos2) -> [u16; 2] {
        let point = (self.rect.clamp(position) - self.origin) / self.cell_points;
        [
            (point.x.floor() as u16).min(self.size[0] - 1),
            (point.y.floor() as u16).min(self.size[1] - 1),
        ]
    }

    fn box_at(self, [left, top, right, bottom]: [f32; 4]) -> Rect {
        Rect::from_min_max(self.point_at([left, top]), self.point_at([right, bottom]))
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tool {
    #[default]
    Tiles,
    Collision,
    Spawn,
    Respawn,
    Zone,
    Object,
    Patch,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Brush {
    #[default]
    Pencil,
    Rectangle,
    Fill,
}

#[derive(Default)]
pub struct Tools {
    pub changed: bool,
    pub import_png: bool,
}

pub struct Editor {
    tool: Tool,
    brush: Brush,
    layer: Layer,
    collision: Collision,
    erase: bool,
    show_collisions: bool,
    atlas_id: u32,
    tile: u16,
    import_cell: [u16; 2],
    whole_png: bool,
    terrain_style: TerrainStyle,
    terrain_error: Option<String>,
    zone: Option<u32>,
    object: Option<u64>,
    zoom: f32,
    pan: Vec2,
    last_cell: Option<[u16; 2]>,
    area_start: Option<[u16; 2]>,
    patch: Option<MapPatch>,
    copy_tiles: bool,
    copy_collisions: bool,
    paste: bool,
    view: View,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            tool: Tool::Tiles,
            brush: Brush::default(),
            layer: Layer::Ground,
            collision: Collision::Solid,
            erase: false,
            show_collisions: true,
            atlas_id: 1,
            tile: 0,
            import_cell: [32, 32],
            whole_png: false,
            terrain_style: TerrainStyle::default(),
            terrain_error: None,
            zone: None,
            object: None,
            zoom: 1.0,
            pan: Vec2::ZERO,
            last_cell: None,
            area_start: None,
            patch: None,
            copy_tiles: true,
            copy_collisions: true,
            paste: false,
            view: View::default(),
        }
    }
}

impl Editor {
    pub fn import_png(&mut self, scene: &mut Scene, path: &Path) -> Result<(), String> {
        let map = scene.map_2d.as_mut().ok_or("Сначала создай 2D-карту.")?;
        self.atlas_id = map.import_png(path, (!self.whole_png).then_some(self.import_cell))?;
        self.tile = 0;
        self.tool = Tool::Tiles;
        self.erase = false;
        Ok(())
    }

    pub fn transform(&self) -> Option<ViewTransform> {
        self.view.transform
    }

    pub fn tools(&mut self, ui: &mut egui::Ui, scene: &mut Scene) -> Tools {
        let Some(map) = &mut scene.map_2d else {
            return Tools::default();
        };
        let mut result = Tools::default();
        ui.heading(map.mode.title());
        let mut size = map.size;
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::DragValue::new(&mut size[0])
                    .range(4..=96)
                    .prefix("Столбцы "),
            );
            ui.add(
                egui::DragValue::new(&mut size[1])
                    .range(4..=96)
                    .prefix("Строки "),
            );
        });
        result.changed |= map.resize(size);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Tiles, "Тайлы");
            ui.selectable_value(&mut self.tool, Tool::Collision, "Столкновения");
            ui.selectable_value(&mut self.tool, Tool::Spawn, "Старт");
            ui.selectable_value(&mut self.tool, Tool::Zone, "Зоны");
            ui.selectable_value(&mut self.tool, Tool::Object, "Метки");
            ui.selectable_value(&mut self.tool, Tool::Patch, "Участок карты");
        });
        if map.mode == Mode::Side {
            ui.selectable_value(&mut self.tool, Tool::Respawn, "Точка возрождения");
        }
        ui.separator();
        if matches!(self.tool, Tool::Tiles | Tool::Collision) {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.brush, Brush::Pencil, "Карандаш");
                ui.selectable_value(&mut self.brush, Brush::Rectangle, "Прямоугольник");
                ui.selectable_value(&mut self.brush, Brush::Fill, "Заливка");
            });
        }
        match self.tool {
            Tool::Tiles => {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.layer, Layer::Ground, "Покрытие");
                    ui.selectable_value(&mut self.layer, Layer::Detail, "Декор");
                    ui.selectable_value(&mut self.layer, Layer::Tall, "Высокие объекты");
                });
                ui.checkbox(&mut self.erase, "Ластик");
                result.changed |= self.terrain_sets(ui, map);
                ui.checkbox(&mut self.whole_png, "Весь PNG как один тайл");
                if !self.whole_png {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.import_cell[0])
                                .range(1..=2048)
                                .prefix("Ширина тайла "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.import_cell[1])
                                .range(1..=2048)
                                .prefix("Высота тайла "),
                        );
                    });
                }
                result.import_png = ui.button("Импорт PNG…").clicked();
                result.changed |= self.palette(ui, map);
            }
            Tool::Collision => {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.collision, Collision::Solid, "Стена");
                    if map.mode == Mode::Side {
                        ui.selectable_value(
                            &mut self.collision,
                            Collision::Platform,
                            "Односторонняя платформа",
                        );
                    }
                    ui.selectable_value(&mut self.collision, Collision::Empty, "Без столкновения");
                });
                ui.label("Отметь клетки на карте. Оформление тайлов задаётся в другом слое.");
            }
            Tool::Spawn => {
                ui.label("Щёлкни по карте, чтобы поставить героя.");
            }
            Tool::Patch => self.patch_tools(ui),
            Tool::Respawn => {
                ui.label(
                    "Поставь метку в свободной клетке над землёй. Герой активирует её при проходе.",
                );
                if map
                    .respawn
                    .is_some_and(|cell| map.respawn_position(cell).is_none())
                {
                    ui.label("Под меткой нужна поверхность для приземления.");
                }
                if ui
                    .add_enabled(
                        map.respawn.is_some(),
                        egui::Button::new("Убрать точку возрождения"),
                    )
                    .clicked()
                {
                    result.changed |= map.set_respawn(None);
                }
            }
            Tool::Zone => {
                if ui.button("+ Зона").clicked()
                    && let Some(id) = scene.rules.add_zone(map.to_world(map.spawn))
                {
                    self.zone = Some(id);
                    result.changed = true;
                }
                if !self.zone.is_some_and(|id| scene.rules.zone(id).is_some()) {
                    self.zone = scene.rules.zones.first().map(|zone| zone.id);
                }
                let name = self
                    .zone
                    .and_then(|id| scene.rules.zone(id))
                    .map_or("Выбрать зону", |zone| zone.name.as_str());
                egui::ComboBox::from_id_salt("map2d_zone")
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for zone in &scene.rules.zones {
                            ui.selectable_value(&mut self.zone, Some(zone.id), &zone.name);
                        }
                    });
                if let Some(zone) = scene
                    .rules
                    .zones
                    .iter_mut()
                    .find(|zone| Some(zone.id) == self.zone)
                {
                    let title = ui.text_edit_singleline(&mut zone.name);
                    if title.changed() {
                        zone.name = zone
                            .name
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(64)
                            .collect();
                    }
                    if title.lost_focus() && zone.name.trim().is_empty() {
                        zone.name = format!("Зона {}", zone.id);
                    }
                    result.changed |= title.changed() || title.lost_focus();
                    let axis = if map.mode == Mode::Top { 2 } else { 1 };
                    ui.horizontal(|ui| {
                        result.changed |= ui
                            .add(
                                egui::DragValue::new(&mut zone.size[0])
                                    .range(0.2..=96.0)
                                    .prefix("Ширина "),
                            )
                            .changed();
                        result.changed |= ui
                            .add(
                                egui::DragValue::new(&mut zone.size[axis])
                                    .range(0.2..=96.0)
                                    .prefix("Высота "),
                            )
                            .changed();
                    });
                    ui.label("Щёлкни по карте, чтобы перенести зону.");
                }
            }
            Tool::Object => {
                let position = map.to_world(map.spawn);
                if ui.button("+ Метка").clicked()
                    && let Some(id) = add_marker(scene, position)
                {
                    self.object = Some(id);
                    result.changed = true;
                }
                if !self
                    .object
                    .is_some_and(|id| scene.world.objects.iter().any(|object| object.id == id))
                {
                    self.object = scene.world.objects.first().map(|object| object.id);
                }
                let name = scene
                    .world
                    .objects
                    .iter()
                    .find(|object| Some(object.id) == self.object)
                    .map_or("Выбрать метку", |object| object.name.as_str());
                egui::ComboBox::from_id_salt("map2d_object")
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for object in &scene.world.objects {
                            ui.selectable_value(&mut self.object, Some(object.id), &object.name);
                        }
                    });
                if let Some(object) = scene
                    .world
                    .objects
                    .iter_mut()
                    .find(|object| Some(object.id) == self.object)
                {
                    let name = ui.add(egui::TextEdit::singleline(&mut object.name).char_limit(64));
                    if name.changed() {
                        object.name.retain(|c| !c.is_control());
                    }
                    if name.lost_focus() && object.name.trim().is_empty() {
                        object.name = "Метка".into();
                    }
                    result.changed |= name.changed() || name.lost_focus();
                    result.changed |= ui.checkbox(&mut object.solid, "Твёрдая метка").changed();
                    ui.label("Щёлкни по карте, чтобы перенести метку.");
                    if ui.button("Удалить метку").clicked() {
                        scene
                            .world
                            .objects
                            .retain(|object| Some(object.id) != self.object);
                        self.object = None;
                        result.changed = true;
                    }
                } else {
                    ui.weak("На этой карте нет меток окружения.");
                }
            }
        }
        ui.separator();
        ui.checkbox(&mut self.show_collisions, "Показать столкновения");
        ui.add(egui::Slider::new(&mut self.zoom, 0.7..=3.0).text("Масштаб карты"));
        if ui.button("Вписать карту").clicked() {
            self.zoom = 1.0;
            self.pan = Vec2::ZERO;
        }
        result
    }

    fn patch_tools(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.paste, false, "Копировать участок");
            ui.add_enabled_ui(self.patch.is_some(), |ui| {
                ui.selectable_value(&mut self.paste, true, "Вставить участок");
            });
        });
        if let Some(patch) = &self.patch {
            let [width, height] = patch.size;
            let contents = match (patch.tiles, patch.collisions) {
                (true, true) => "тайлы и столкновения",
                (true, false) => "тайлы",
                _ => "столкновения",
            };
            ui.label(format!("Скопировано {width} × {height} · {contents}"));
        }
        if self.paste {
            ui.label("Щёлкни в клетку, где должен оказаться левый верхний угол. Пустые клетки тоже копируются.");
        } else {
            ui.checkbox(&mut self.copy_tiles, "Слои тайлов");
            ui.checkbox(&mut self.copy_collisions, "Коллизии участка");
            if self.copy_tiles || self.copy_collisions {
                ui.label("Выдели участок мышью, чтобы скопировать.");
            } else {
                ui.label("Выбери данные для копирования.");
            }
        }
    }

    fn terrain_sets(&mut self, ui: &mut egui::Ui, map: &mut Map) -> bool {
        let mut added = false;
        ui.collapsing("Наборы тайлов", |ui| {
            egui::ComboBox::from_id_salt("terrain_style")
                .selected_text(self.terrain_style.name())
                .show_ui(ui, |ui| {
                    for style in TerrainStyle::ALL {
                        ui.selectable_value(&mut self.terrain_style, style, style.name());
                    }
                });
            if ui.button("Добавить набор").clicked() {
                match map.add_terrain_atlas(self.terrain_style) {
                    Ok((id, inserted)) => {
                        self.atlas_id = id;
                        self.tile = 0;
                        self.erase = false;
                        self.terrain_error = None;
                        added = inserted;
                    }
                    Err(error) => self.terrain_error = Some(error),
                }
            }
            ui.weak("Покрытия, вода и декор. Столкновения рисуются отдельным инструментом.");
            if let Some(error) = &self.terrain_error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
        added
    }

    fn palette(&mut self, ui: &mut egui::Ui, map: &mut Map) -> bool {
        if map.atlas(self.atlas_id).is_none() {
            self.atlas_id = map.atlases.first().map_or(0, |atlas| atlas.id);
        }
        let current = map
            .atlas(self.atlas_id)
            .map_or("Выбрать атлас", |atlas| atlas.name.as_str());
        let previous = self.atlas_id;
        egui::ComboBox::from_id_salt("map2d_atlas")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for atlas in &map.atlases {
                    ui.selectable_value(&mut self.atlas_id, atlas.id, &atlas.name);
                }
            });
        if self.atlas_id != previous {
            self.tile = 0;
        }
        let Some(atlas) = map.atlas(self.atlas_id) else {
            return false;
        };
        let count = atlas.count() as usize;
        if count == 0 {
            return false;
        }
        let terrain_style = TerrainStyle::ALL
            .into_iter()
            .find(|style| atlas.name == style.name() && atlas.cell == TerrainStyle::CELL);
        ui.add(
            egui::DragValue::new(&mut self.tile)
                .range(0..=count as u16 - 1)
                .prefix("Тайл "),
        );
        let texture = self.view.texture(ui.ctx(), atlas).cloned();
        let cols = ((ui.available_width() / 47.0) as usize).clamp(2, 8);
        let rows = count.div_ceil(cols);
        egui::ScrollArea::vertical()
            .id_salt("map2d_palette")
            .max_height(200.0)
            .show_rows(ui, 45.0, rows, |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        for index in row * cols..((row + 1) * cols).min(count) {
                            let (rect, response) =
                                ui.allocate_exact_size(egui::vec2(43.0, 41.0), Sense::click());
                            let label = format!("Тайл {index}");
                            let response = response.on_hover_text(
                                terrain_style
                                    .and_then(|style| style.tile_name(index as u16))
                                    .unwrap_or(&label),
                            );
                            if response.clicked() {
                                self.tile = index as u16;
                                self.erase = false;
                            }
                            let painter = ui.painter();
                            painter.rect_filled(rect, 3.0, BACKDROP);
                            if let Some(texture) = &texture {
                                painter.image(
                                    texture.id(),
                                    rect.shrink(3.0),
                                    uv(atlas, index as u16),
                                    Color32::WHITE,
                                );
                            }
                            if usize::from(self.tile) == index {
                                painter.rect_stroke(
                                    rect,
                                    3.0,
                                    Stroke::new(2.0_f32, ACCENT),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }
                    });
                }
            });
        if let Some(name) = terrain_style.and_then(|style| style.tile_name(self.tile)) {
            ui.weak(name);
        }
        let tile = Tile {
            atlas: atlas.id,
            index: self.tile,
        };
        if ui.button("Назначить тайл героем").clicked() && map.hero != Some(tile)
        {
            map.hero = Some(tile);
            return true;
        }
        if map.hero.is_some() && ui.small_button("Вернуть героя по умолчанию").clicked()
        {
            map.hero = None;
            return true;
        }
        false
    }

    pub fn board(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        editing: bool,
        natural: Option<&environment::Frame>,
        particles: &[Particle],
    ) -> Option<(ViewTransform, Option<[u16; 2]>)> {
        let (transform, response) = self.canvas(
            ui,
            scene,
            editing && self.show_collisions,
            false,
            natural,
            particles,
        )?;
        let clicked = response
            .interact_pointer_pos()
            .and_then(|point| transform.tile_at(point))
            .filter(|_| response.clicked_by(PointerButton::Primary));
        Some((transform, clicked))
    }

    fn canvas(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        debug: bool,
        editor_marks: bool,
        natural: Option<&environment::Frame>,
        particles: &[Particle],
    ) -> Option<(ViewTransform, egui::Response)> {
        let map = scene.map_2d.as_ref()?;
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::splat(64.0)),
            Sense::click_and_drag(),
        );
        if response.dragged_by(egui::PointerButton::Secondary) {
            self.pan += ui.input(|input| input.pointer.delta());
        }
        if response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            self.zoom = (self.zoom * (0.002 * scroll).exp()).clamp(0.7, 3.0);
        }
        let transform = editor_transform(rect, map, self.zoom, self.pan);
        self.view.draw(
            ui,
            transform,
            scene,
            None,
            None,
            None,
            debug,
            editor_marks,
            natural,
            particles,
        );
        Some((transform, response))
    }

    pub fn view(
        &mut self,
        ui: &mut egui::Ui,
        scene: &mut Scene,
        natural: Option<&environment::Frame>,
        particles: &[Particle],
    ) -> bool {
        let debug = self.show_collisions || self.tool == Tool::Collision;
        let Some((transform, response)) = self.canvas(ui, scene, debug, true, natural, particles)
        else {
            return false;
        };
        let map_size = scene.map_2d.as_ref().expect("canvas requires a map").size;
        let rect = transform.rect;
        let hover = response.hover_pos().and_then(|pos| transform.tile_at(pos));
        if let Some(position) = hover {
            let [x, row] = position.map(f32::from);
            ui.painter().with_clip_rect(rect).rect_stroke(
                transform.box_at([x, row, x + 1.0, row + 1.0]),
                0.0,
                Stroke::new(1.5_f32, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }
        let painting = matches!(self.tool, Tool::Tiles | Tool::Collision);
        let cell = response
            .interact_pointer_pos()
            .and_then(|pos| transform.tile_at(pos));
        let clicked = cell.filter(|_| response.clicked_by(PointerButton::Primary));
        if painting && self.brush == Brush::Rectangle || self.tool == Tool::Patch && !self.paste {
            let Some(area) = self.rectangle(ui, &response, transform, clicked) else {
                return false;
            };
            if self.tool == Tool::Patch {
                if let Some(patch) = scene
                    .map_2d
                    .as_ref()
                    .and_then(|map| map.copy_patch(area, self.copy_tiles, self.copy_collisions))
                {
                    self.patch = Some(patch);
                }
                return false;
            }
            return area.cells().fold(false, |changed, cell| {
                self.apply_cell(scene, cell) | changed
            });
        }
        if self.tool == Tool::Patch {
            let Some(patch) = &self.patch else {
                return false;
            };
            if let Some(area) = hover.and_then(|cell| patch.destination(cell, map_size)) {
                preview_area(ui, transform, area);
            }
            return clicked
                .is_some_and(|cell| scene.map_2d.as_mut().unwrap().paste_patch(cell, patch));
        }
        if painting && self.brush == Brush::Fill {
            let map = scene.map_2d.as_mut().unwrap();
            return clicked.is_some_and(|cell| match self.tool {
                Tool::Tiles => map.fill_tiles(cell, self.layer, self.current_tile()),
                _ => map.fill_collisions(cell, self.collision),
            });
        }
        let drag = painting && response.dragged_by(PointerButton::Primary);
        let Some(cell) = cell.filter(|_| clicked.is_some() || drag) else {
            self.last_cell = None;
            return false;
        };
        let previous = self.last_cell.replace(cell).filter(|_| drag);
        let line = if response.drag_started_by(PointerButton::Primary) {
            ui.input(|input| input.pointer.press_origin())
                .and_then(|position| transform.tile_at(position))
                .unwrap_or(cell)
        } else {
            previous.unwrap_or(cell)
        };
        let [from_x, from_y] = line.map(i32::from);
        let [x, y] = cell.map(i32::from);
        let steps = (x - from_x).abs().max((y - from_y).abs()).max(1);
        let mut changed = false;
        for step in 0..=steps {
            let x = from_x + (x - from_x) * step / steps;
            let y = from_y + (y - from_y) * step / steps;
            changed |= self.apply_cell(scene, [x as u16, y as u16]);
        }
        changed
    }

    fn rectangle(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        transform: ViewTransform,
        clicked: Option<[u16; 2]>,
    ) -> Option<CellRect> {
        if response.drag_started_by(PointerButton::Primary) {
            self.area_start = ui
                .input(|input| input.pointer.press_origin())
                .and_then(|position| transform.tile_at(position));
        }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.area_start = None;
        }
        if let Some(cell) = clicked {
            return Some(CellRect::new(cell, cell));
        }
        let area = self
            .area_start
            .zip(ui.input(|input| input.pointer.latest_pos()))
            .map(|(from, to)| CellRect::new(from, transform.cell_clamped(to)));
        if response.drag_stopped_by(PointerButton::Primary) {
            self.area_start = None;
            return area;
        }
        if response.dragged_by(PointerButton::Primary)
            && let Some(area) = area
        {
            preview_area(ui, transform, area);
        }
        None
    }

    fn current_tile(&self) -> Option<Tile> {
        (!self.erase).then_some(Tile {
            atlas: self.atlas_id,
            index: self.tile,
        })
    }

    fn apply_cell(&mut self, scene: &mut Scene, cell: [u16; 2]) -> bool {
        let Some(map) = &mut scene.map_2d else {
            return false;
        };
        let point = [f32::from(cell[0]) + 0.5, f32::from(cell[1]) + 0.5];
        match self.tool {
            Tool::Tiles => map.paint(cell, self.layer, self.current_tile()),
            Tool::Collision => map.paint_collision(cell, self.collision),
            Tool::Spawn => map.set_spawn(point),
            Tool::Respawn => map.set_respawn(Some(cell)),
            Tool::Zone => {
                let Some(zone) = scene
                    .rules
                    .zones
                    .iter_mut()
                    .find(|zone| Some(zone.id) == self.zone)
                else {
                    return false;
                };
                let position = map.to_world(point);
                if zone.position == position {
                    return false;
                }
                zone.position = position;
                true
            }
            Tool::Object => {
                let Some(object) = scene
                    .world
                    .objects
                    .iter_mut()
                    .find(|object| Some(object.id) == self.object)
                else {
                    return false;
                };
                let position = map.to_world(point);
                if object.position == position {
                    return false;
                }
                object.position = position;
                true
            }
            Tool::Patch => false,
        }
    }
}

fn preview_area(ui: &egui::Ui, transform: ViewTransform, area: CellRect) {
    let [left, top] = area.min.map(f32::from);
    let [right, bottom] = area.max.map(f32::from);
    let bounds = transform.box_at([left, top, right + 1.0, bottom + 1.0]);
    let painter = ui.painter().with_clip_rect(transform.rect);
    painter.rect_filled(
        bounds,
        0.0,
        Color32::from_rgba_unmultiplied(78, 214, 195, 66),
    );
    painter.rect_stroke(
        bounds,
        0.0,
        Stroke::new(2.0_f32, Color32::from_rgb(111, 235, 214)),
        egui::StrokeKind::Inside,
    );
}

impl Player {
    pub fn transform(&self) -> Option<ViewTransform> {
        self.view.transform
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        session: &rules::Session,
        interactions: &InteractionState,
        player_texture: Option<egui::TextureId>,
        natural: Option<&environment::Frame>,
        particles: &[Particle],
    ) {
        let Some(map) = &scene.map_2d else {
            return;
        };
        let (rect, _) =
            ui.allocate_exact_size(ui.available_size().max(Vec2::splat(64.0)), Sense::hover());
        let transform = game_transform(rect, map, self.position);
        let pose = Pose {
            position: self.position,
            direction: self.direction,
            active_respawn: self.respawn_cell(),
            texture: player_texture,
            frame: if self.moving {
                1 + self.stride.floor() as u16
            } else {
                0
            },
        };
        self.view.draw(
            ui,
            transform,
            scene,
            Some(pose),
            Some(session),
            Some(interactions),
            false,
            false,
            natural,
            particles,
        );
    }
}

fn editor_transform(rect: Rect, map: &Map, zoom: f32, pan: Vec2) -> ViewTransform {
    let size = egui::vec2(f32::from(map.size[0]), f32::from(map.size[1]));
    let cell = ((rect.width() - 20.0) / size.x)
        .min((rect.height() - 20.0) / size.y)
        .max(1.0)
        * zoom;
    ViewTransform {
        rect,
        origin: (rect.center() - size * cell * 0.5 + pan).round(),
        cell_points: cell,
        size: map.size,
    }
}

fn game_transform(rect: Rect, map: &Map, position: [f32; 2]) -> ViewTransform {
    let size = egui::vec2(f32::from(map.size[0]), f32::from(map.size[1]));
    let fit = (rect.width() / size.x).min(rect.height() / size.y);
    let cell = fit.max((rect.height() / 15.0).max(14.0));
    let viewport_half = rect.size() / (cell * 2.0);
    let center = egui::vec2(
        follow_axis(size.x, viewport_half.x, position[0]),
        follow_axis(size.y, viewport_half.y, position[1]),
    );
    ViewTransform {
        rect,
        origin: (rect.center() - center * cell).round(),
        cell_points: cell,
        size: map.size,
    }
}

fn follow_axis(length: f32, half: f32, value: f32) -> f32 {
    if length <= 2.0 * half {
        length * 0.5
    } else {
        value.clamp(half, length - half)
    }
}

#[derive(Clone, Copy)]
struct Pose {
    position: [f32; 2],
    direction: u16,
    active_respawn: Option<[u16; 2]>,
    texture: Option<egui::TextureId>,
    frame: u16,
}

#[derive(Default)]
pub(super) struct View {
    transform: Option<ViewTransform>,
    atlases: HashMap<u32, (usize, egui::TextureHandle)>,
    hero: Option<egui::TextureHandle>,
}

impl View {
    fn texture(&mut self, ctx: &egui::Context, atlas: &Atlas) -> Option<&egui::TextureHandle> {
        let identity = atlas.png.as_ptr() as usize;
        if !self
            .atlases
            .get(&atlas.id)
            .is_some_and(|(_, texture)| texture.size() == atlas.size.map(usize::from))
            || self
                .atlases
                .get(&atlas.id)
                .is_none_or(|entry| entry.0 != identity)
        {
            let rgba = decode_png(&atlas.png).ok()?;
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            );
            self.atlases.insert(
                atlas.id,
                (
                    identity,
                    ctx.load_texture(
                        format!("2D atlas {}", atlas.id),
                        image,
                        egui::TextureOptions::NEAREST,
                    ),
                ),
            );
        }
        self.atlases.get(&atlas.id).map(|entry| &entry.1)
    }

    fn draw(
        &mut self,
        ui: &egui::Ui,
        at: ViewTransform,
        scene: &Scene,
        pose: Option<Pose>,
        session: Option<&rules::Session>,
        interactions: Option<&InteractionState>,
        debug: bool,
        editor_marks: bool,
        natural: Option<&environment::Frame>,
        particles: &[Particle],
    ) {
        let Some(map) = &scene.map_2d else {
            return;
        };
        self.transform = Some(at);
        let painter = ui.painter().with_clip_rect(at.rect);
        painter.rect_filled(at.rect, 0.0, Color32::from_rgb(14, 27, 36));
        let area = at.box_at([0.0, 0.0, f32::from(map.size[0]), f32::from(map.size[1])]);
        if map.mode == Mode::Side {
            hillside(&painter, at);
        } else {
            painter.rect_filled(area, 0.0, Color32::from_rgb(31, 49, 55));
        }
        self.atlases.retain(|id, _| map.atlas(*id).is_some());
        for atlas in &map.atlases {
            self.texture(ui.ctx(), atlas);
        }
        let width = usize::from(map.size[0]);
        let rows = f32::from(map.size[1]);
        for (index, _) in map.colliders.iter().enumerate() {
            let [x, row] = [(index % width) as f32, (index / width) as f32];
            let cell = at.box_at([x, row, x + 1.0, row + 1.0]);
            if !cell.intersects(at.rect) {
                continue;
            }
            for layer in &map.layers[..2] {
                if let Some(tile) = layer[index] {
                    self.tile(&painter, map, tile, cell);
                }
            }
            if pose.is_none() {
                painter.rect_stroke(
                    cell,
                    0.0,
                    Stroke::new(0.5_f32, Color32::from_rgba_unmultiplied(180, 208, 194, 62)),
                    egui::StrokeKind::Inside,
                );
            }
            if debug {
                match map.colliders[index] {
                    Collision::Empty => {}
                    Collision::Solid => {
                        painter.rect_filled(
                            cell.shrink(2.0),
                            1.0,
                            Color32::from_rgba_unmultiplied(235, 95, 90, 70),
                        );
                    }
                    Collision::Platform => {
                        painter.line_segment(
                            [cell.left_top(), cell.right_top()],
                            Stroke::new(3.0_f32, Color32::from_rgb(107, 226, 208)),
                        );
                    }
                }
            }
        }
        if let Some(frame) = natural {
            let projection = match map.mode {
                Mode::Top => environment_2d::Projection::Xz,
                Mode::Side => environment_2d::Projection::Xy { z: 0.0 },
            };
            environment_2d::paint_frame(&painter, frame, projection, |world| {
                at.point_at(map.from_world(world.to_array()))
            });
        }
        for zone in &scene.rules.zones {
            let [x, row] = map.from_world(zone.position);
            let half = zone.size[0] * 0.5;
            let [top, bottom] = if map.mode == Mode::Top {
                [row - zone.size[2] * 0.5, row + zone.size[2] * 0.5]
            } else {
                [row - zone.size[1], row]
            };
            let bounds = at.box_at([x - half, top, x + half, bottom]);
            if !bounds.intersects(at.rect) {
                continue;
            }
            painter.rect_filled(
                bounds,
                2.0,
                Color32::from_rgba_unmultiplied(109, 226, 202, 38),
            );
            painter.rect_stroke(
                bounds,
                2.0,
                Stroke::new(1.0_f32, Color32::from_rgb(102, 194, 183)),
                egui::StrokeKind::Inside,
            );
            painter.text(
                bounds.center_top() + egui::vec2(0.0, 5.0),
                egui::Align2::CENTER_TOP,
                &zone.name,
                egui::FontId::proportional(12.0),
                Color32::from_rgb(186, 232, 213),
            );
        }
        for object in &scene.world.objects {
            if session.is_some_and(|state| !state.object_enabled(object.id)) {
                continue;
            }
            let bounds = at.box_at(object_bounds(map, object));
            if !bounds.intersects(at.rect) {
                continue;
            }
            let color = object.color.map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8);
            let front = Color32::from_rgb(color[0], color[1], color[2]);
            painter.rect_filled(
                bounds.shrink(2.0),
                3.0,
                Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 135),
            );
            painter.rect_stroke(
                bounds.shrink(2.0),
                3.0,
                Stroke::new(2.0_f32, front),
                egui::StrokeKind::Inside,
            );
            painter.circle_filled(bounds.center(), (bounds.width() * 0.19).min(8.0), front);
        }
        let mut draw_actor = pose;
        for (index, tile) in map.layers[2].iter().enumerate() {
            let Some(tile) = *tile else {
                continue;
            };
            let [x, row] = [(index % width) as f32, (index / width) as f32];
            let down = (row + 1.0).min(rows);
            if map.mode == Mode::Top
                && draw_actor.is_some_and(|pose| pose.position[1] < down - 0.08)
            {
                self.actor(
                    ui,
                    &painter,
                    at,
                    map,
                    draw_actor.take().expect("checked pose"),
                );
            }
            let Some(atlas) = map.atlas(tile.atlas) else {
                continue;
            };
            let tall = f32::from(atlas.cell[1]) / f32::from(atlas.cell[0]);
            let bounds = at.box_at([x, down - tall, x + 1.0, down]);
            if bounds.intersects(at.rect) {
                self.tile(&painter, map, tile, bounds);
            }
        }
        for key in &scene.world.keys {
            if interactions.is_some_and(|state| state.has_key(key.id)) {
                continue;
            }
            let point = at.point_at(map.from_world(key.position));
            if !at.rect.contains(point) {
                continue;
            }
            let radius = at.cell_points * 0.125;
            let gold = Color32::from_rgb(252, 218, 142);
            let anchor = point - egui::vec2(radius, radius);
            painter.circle_stroke(anchor, radius * 0.55, Stroke::new(2.0_f32, gold));
            painter.line_segment(
                [
                    anchor + egui::vec2(radius * 0.38, radius * 0.38),
                    point + egui::vec2(radius, radius),
                ],
                Stroke::new(2.6_f32, gold),
            );
            painter.line_segment(
                [
                    point + egui::vec2(radius * 0.32, radius * 0.32),
                    point + egui::vec2(radius * 0.32, radius),
                ],
                Stroke::new(2.0_f32, gold),
            );
        }
        if let Some(cell) = map.respawn {
            let point = at.at_cell(cell);
            let active = pose.is_some_and(|pose| pose.active_respawn == Some(cell));
            let color = if active {
                Color32::from_rgb(145, 254, 192)
            } else {
                Color32::from_rgb(124, 184, 212)
            };
            let size = (at.cell_points * 0.24).max(5.0);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    point - egui::vec2(0.0, size),
                    point + egui::vec2(size * 0.7, 0.0),
                    point + egui::vec2(0.0, size),
                    point - egui::vec2(size * 0.7, 0.0),
                ],
                if active {
                    color
                } else {
                    color.gamma_multiply(0.14)
                },
                Stroke::new(1.7_f32, color),
            ));
            if pose.is_none() || active {
                painter.text(
                    point + egui::vec2(size, -size),
                    egui::Align2::LEFT_BOTTOM,
                    "Точка возрождения",
                    egui::FontId::proportional(13.0),
                    color,
                );
            }
        }
        if let Some(actor) = draw_actor {
            self.actor(ui, &painter, at, map, actor);
        }
        effects_2d::paint(
            &painter,
            particles,
            effects_2d::Style::Pixel,
            at.cell_points,
            |point| at.point_at(map.from_world(point.to_array())),
        );
        if let Some(frame) = natural {
            environment_2d::paint_atmosphere(&painter, frame);
        }
        if editor_marks && pose.is_none() {
            let point = at.point_at(map.spawn);
            painter.circle_filled(point, 5.0, Color32::from_rgb(251, 237, 171));
            painter.text(
                point + egui::vec2(7.0, -4.0),
                egui::Align2::LEFT_BOTTOM,
                "Старт",
                egui::FontId::proportional(13.0),
                Color32::from_rgb(251, 237, 171),
            );
        }
    }

    fn tile(&self, painter: &egui::Painter, map: &Map, tile: Tile, dest: Rect) {
        if let Some(atlas) = map.atlas(tile.atlas)
            && let Some((_, texture)) = self.atlases.get(&atlas.id)
        {
            painter.image(texture.id(), dest, uv(atlas, tile.index), Color32::WHITE);
        }
    }

    fn actor(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        at: ViewTransform,
        map: &Map,
        pose: Pose,
    ) {
        let [x, row] = pose.position;
        let bottom = if map.mode == Mode::Top {
            row + 0.18
        } else {
            row + 0.12
        };
        if let Some(tile) = map.hero {
            let aspect = map.atlas(tile.atlas).map_or(1.0, |atlas| {
                f32::from(atlas.cell[0]) / f32::from(atlas.cell[1])
            });
            let half = aspect.clamp(0.3, 1.6) * 0.52;
            self.tile(
                painter,
                map,
                tile,
                at.box_at([x - half, bottom - 1.12, x + half, bottom]),
            );
            return;
        }
        if let Some(texture) = pose.texture {
            let uv = if map.mode == Mode::Side && pose.direction == 2 {
                Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0))
            } else {
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0))
            };
            painter.image(
                texture,
                at.box_at([x - 0.64, bottom - 1.28, x + 0.64, bottom]),
                uv,
                Color32::WHITE,
            );
            return;
        }
        if self.hero.is_none()
            && let Ok(rgba) = decode_png(PERSON_PNG)
        {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            );
            self.hero = Some(ui.ctx().load_texture(
                "2D герой",
                image,
                egui::TextureOptions::NEAREST,
            ));
        }
        if let Some(hero) = &self.hero {
            let u = f32::from(pose.frame.min(3)) / 4.0;
            let v = f32::from(pose.direction.min(3)) / 4.0;
            painter.image(
                hero.id(),
                at.box_at([x - 0.51, bottom - 1.25, x + 0.51, bottom]),
                Rect::from_min_max(egui::pos2(u, v), egui::pos2(u + 0.25, v + 0.25)),
                Color32::WHITE,
            );
        }
    }
}

fn uv(atlas: &Atlas, index: u16) -> Rect {
    let columns = atlas.size[0] / atlas.cell[0];
    let row = index / columns;
    let col = index % columns;
    let step = egui::vec2(
        f32::from(atlas.cell[0]) / f32::from(atlas.size[0]),
        f32::from(atlas.cell[1]) / f32::from(atlas.size[1]),
    );
    Rect::from_min_size(
        egui::pos2(f32::from(col) * step.x, f32::from(row) * step.y),
        step,
    )
}

fn hillside(painter: &egui::Painter, at: ViewTransform) {
    for i in 0..24 {
        let p = i as f32 / 24.0;
        let band = Rect::from_min_max(
            egui::pos2(at.rect.left(), at.rect.top() + p * at.rect.height()),
            egui::pos2(
                at.rect.right(),
                at.rect.top() + ((i + 1) as f32 / 24.0) * at.rect.height() + 0.5,
            ),
        );
        painter.rect_filled(
            band,
            0.0,
            Color32::from_rgb(
                (31.0 + p * 72.0) as u8,
                (49.0 + p * 52.0) as u8,
                (79.0 + p * 34.0) as u8,
            ),
        );
    }
    for (i, color) in [
        Color32::from_rgb(69, 77, 112),
        Color32::from_rgb(49, 65, 84),
        Color32::from_rgb(36, 62, 72),
    ]
    .into_iter()
    .enumerate()
    {
        let base = at.rect.top() + at.rect.height() * (0.45 + i as f32 * 0.10);
        for peak in 0..7 {
            let w = at.rect.width() / 5.0;
            let x = at.rect.left() + (peak as f32 - 1.0) * w
                - (at.origin.x * (i as f32 + 1.0) * 0.08).rem_euclid(w);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x, at.rect.bottom()),
                    egui::pos2(x, base),
                    egui::pos2(x + w * 0.5, base - w * (0.21 + 0.09 * (peak % 3) as f32)),
                    egui::pos2(x + w, base + 18.0),
                    egui::pos2(x + w, at.rect.bottom()),
                ],
                color,
                Stroke::NONE,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof_input;

    #[test]
    fn adding_a_terrain_set_selects_it_for_the_tile_brush() {
        let ctx = egui::Context::default();
        crate::configure_context(&ctx);
        let mut input = proof_input::Driver::new([420, 760]);
        let mut scene = crate::map2d::scene(Mode::Top, false);
        let mut editor = Editor {
            erase: true,
            ..Editor::default()
        };
        let mut frame = |raw| {
            let mut changed = false;
            let output = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    changed = editor.tools(ui, &mut scene).changed;
                });
            });
            (output, changed)
        };

        frame(input.raw(Vec::new()));
        let (output, _) = frame(input.raw(Vec::new()));
        let header = proof_input::find_label(&output, "Наборы тайлов", 0).unwrap();
        frame(input.raw(proof_input::hover(header)));
        frame(input.raw(proof_input::click(header)));
        for _ in 0..25 {
            frame(input.raw(Vec::new()));
        }
        let (output, _) = frame(input.raw(Vec::new()));
        let add = proof_input::find_label(&output, "Добавить набор", 0).unwrap();
        frame(input.raw(proof_input::hover(add)));
        assert!(frame(input.raw(proof_input::click(add))).1);
        input.skip_frames(25);
        assert!(!frame(input.raw(proof_input::click(add))).1);
        assert_eq!(editor.current_tile(), Some(Tile { atlas: 3, index: 0 }));
        assert!(editor.apply_cell(&mut scene, [4, 4]));
        let map = scene.map_2d.as_ref().unwrap();
        assert_eq!(map.atlases.len(), 3);
        assert_eq!(
            map.layers[0][4 * usize::from(map.size[0]) + 4],
            editor.current_tile()
        );
        assert_eq!(map.collision_at([4, 4]), Collision::Empty);
    }
}
