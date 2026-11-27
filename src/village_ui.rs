use eframe::egui::{self, Align2, Color32, DragValue, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::{
    Studio, View,
    assets::Placement,
    editor_theme::{ACCENT, BACKDROP, EDGE, MUTED},
    village::{
        self, MAX_ROUTE_POINTS, MAX_X, MAX_Z, MAX_ZONES, MIN_X, MIN_Z, Recipe, Scope, Style, Zone,
        ZoneKind,
    },
};

#[derive(Clone)]
struct Draft {
    saved: Option<Recipe>,
    recipe: Recipe,
    active: usize,
    drag: Option<usize>,
    review_scope: Scope,
    review: Option<Review>,
}

#[derive(Clone)]
struct Review {
    recipe: Recipe,
    scope: Scope,
    source: Vec<Placement>,
    changes: Vec<(String, &'static str)>,
    preserved: usize,
}

impl Draft {
    fn new(saved: Option<Recipe>) -> Self {
        Self {
            recipe: saved.clone().unwrap_or_default(),
            saved,
            active: 1,
            drag: None,
            review_scope: Scope::All,
            review: None,
        }
    }
}

impl Studio {
    pub(crate) fn village_tools(&mut self, ui: &mut egui::Ui) {
        if self.game_running || self.scene.map_2d.is_some() {
            return;
        }
        ui.collapsing("Прибрежная деревня", |ui| {
            let id = ui.id().with("village_draft");
            let saved = self
                .scene
                .village
                .as_ref()
                .map(|state| state.recipe.clone());
            let mut draft = ui
                .data(|data| data.get_temp::<Draft>(id))
                .filter(|draft| draft.saved == saved)
                .unwrap_or_else(|| Draft::new(saved));
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    DragValue::new(&mut draft.recipe.seed)
                        .range(0..=u32::MAX)
                        .prefix("Зерно "),
                );
                ui.selectable_value(
                    &mut draft.recipe.style,
                    Style::Natural,
                    Style::Natural.label(),
                );
                ui.selectable_value(
                    &mut draft.recipe.style,
                    Style::Stylized,
                    Style::Stylized.label(),
                );
            });
            ui.add(
                DragValue::new(&mut draft.recipe.ridge_height)
                    .range(0.0..=9.0)
                    .fixed_decimals(1)
                    .speed(0.1)
                    .prefix("Высота холмов ")
                    .suffix(" м"),
            ).on_hover_text("Меняет дальний рельеф при полной сборке деревни. В режиме «Натура» дорога повторяет форму земли.");
            mini_plan(ui, &mut draft);
            route_controls(ui, &mut draft);
            zone_controls(ui, &mut draft.recipe);
            if self.scene.village.is_some() {
                self.village_house_locks(ui);
            }

            let mut action = None;
            if ui
                .button(RichText::new("Собрать деревню").strong())
                .clicked()
            {
                action = Some(Scope::All);
            }
            if self.scene.village.is_some() {
                ui.horizontal_wrapped(|ui| {
                    for (label, scope) in [
                        ("Пересоздать дорогу", Scope::Route),
                        ("Пересоздать дома", Scope::Houses),
                        ("Пересоздать растения", Scope::Vegetation),
                    ] {
                        if ui.button(label).clicked() {
                            action = Some(scope);
                        }
                    }
                });
                ui.collapsing("Предпросмотр пересборки", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (scope, name) in [(Scope::All, "Вся деревня"), (Scope::Route, "Дорога"), (Scope::Houses, "Дома"), (Scope::Vegetation, "Растения")] {
                            ui.selectable_value(&mut draft.review_scope, scope, name);
                        }
                    });
                    if ui.button("Показать изменения").clicked() {
                        let mut scene = self.scene.clone();
                        let mut assets = self.assets.clone();
                        match village::generate(&mut scene, &mut assets, draft.recipe.clone(), draft.review_scope) {
                            Ok(report) => {
                                let source = self.scene.world.models.clone();
                                let mut changes = Vec::new();
                                for current in &scene.world.models {
                                    match source.iter().find(|old| old.id == current.id) {
                                        Some(old) if old != current => changes.push((current.label.clone(), "Обновится")),
                                        None => changes.push((current.label.clone(), "Добавится")),
                                        _ => {}
                                    }
                                }
                                for prior in &source {
                                    if !scene.world.models.iter().any(|current| current.id == prior.id) {
                                        changes.push((prior.label.clone(), "Исчезнет"));
                                    }
                                }
                                draft.review = Some(Review { recipe: draft.recipe.clone(), scope: draft.review_scope, source, changes, preserved: report.preserved });
                            }
                            Err(error) => { draft.review = None; self.status = error; }
                        }
                    }
                    if let Some(review) = &draft.review {
                        if review.recipe != draft.recipe || review.scope != draft.review_scope || review.source != self.scene.world.models {
                            ui.weak("Сцена или параметры изменились. Обнови список.");
                        } else {
                            ui.label(format!("Изменений {} · сохранённых правок {}", review.changes.len(), review.preserved));
                            egui::ScrollArea::vertical().id_salt("village_review").max_height(126.0).show(ui, |ui| {
                                for (name, change) in &review.changes {
                                    ui.label(format!("{change} · {name}"));
                                }
                            });
                            if ui.button("Применить пересборку").clicked() { action = Some(review.scope); }
                        }
                    }
                });
            }
            if let Some(scope) = action {
                let first = self.scene.village.is_none();
                match village::generate(
                    &mut self.scene,
                    &mut self.assets,
                    draft.recipe.clone(),
                    scope,
                ) {
                    Ok(report) => {
                        draft.review = None;
                        draft.saved = Some(draft.recipe.clone());
                        self.view = View::Three;
                        if first {
                            self.camera.target = [-1.8, 0.9, -11.2];
                            self.camera.pitch = 0.47;
                            self.camera.yaw = 0.64;
                            self.camera.distance = 31.0;
                        }
                        self.status = format!(
                            "Деревня · размещено {} · сохранено {} · пропущено {}{}",
                            report.created,
                            report.preserved,
                            report.omitted,
                            if report.route_clear {
                                ""
                            } else {
                                " · на маршруте есть препятствие"
                            },
                        );
                        self.changed();
                    }
                    Err(error) => self.status = format!("Не удалось собрать деревню. {error}"),
                }
            }
            ui.data_mut(|data| data.insert_temp(id, draft));
        });
    }

    fn village_house_locks(&mut self, ui: &mut egui::Ui) {
        let Some(state) = &self.scene.village else {
            return;
        };
        let houses: Vec<_> = state
            .houses()
            .filter_map(|(id, locked)| {
                self.scene
                    .world
                    .models
                    .iter()
                    .find(|model| model.id == id)
                    .map(|model| (id, locked, model.label.clone()))
            })
            .collect();
        ui.collapsing("Защита домов", |ui| {
            if houses.is_empty() {
                ui.weak("Собери дома в зоне «Дома».");
            }
            for (id, mut locked, label) in houses {
                ui.push_id(id, |ui| {
                    if village::manually_changed(&self.scene, id) {
                        ui.label(format!("{label} · изменён вручную"));
                    } else if ui
                        .checkbox(&mut locked, &label)
                        .on_hover_text("Сохранять этот дом при пересборке деревни")
                        .changed()
                        && village::lock_house(&mut self.scene, id, locked)
                    {
                        self.changed();
                    }
                });
            }
            ui.weak("Дома из сцены и ручные правки генератор сохраняет.");
        });
    }
}

fn route_controls(ui: &mut egui::Ui, draft: &mut Draft) {
    ui.collapsing("Точки маршрута", |ui| {
        ui.add(
            DragValue::new(&mut draft.recipe.road_width)
                .range(1.8..=4.2)
                .speed(0.05)
                .fixed_decimals(2)
                .prefix("Ширина дороги ")
                .suffix(" м"),
        );
        ui.horizontal_wrapped(|ui| {
            for index in 0..draft.recipe.route.len() {
                let label = if index == 0 {
                    "Старт".into()
                } else {
                    (index + 1).to_string()
                };
                ui.selectable_value(&mut draft.active, index, label);
            }
        });
        if draft.active == 0 {
            ui.weak("Старт игры находится в (0, 0).");
        } else {
            coords(ui, &mut draft.recipe.route[draft.active]);
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    draft.recipe.route.len() < MAX_ROUTE_POINTS,
                    egui::Button::new("Добавить точку"),
                )
                .clicked()
            {
                let after = draft.active + 1;
                let from = draft.recipe.route[draft.active];
                let point = if let Some(to) = draft.recipe.route.get(after) {
                    [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5]
                } else if from[1] > MIN_Z + 2.0 {
                    [from[0], from[1] - 2.0]
                } else {
                    [(from[0] - 2.0).max(MIN_X), (from[1] + 1.0).min(MAX_Z)]
                };
                draft.recipe.route.insert(after, point);
                draft.active = after;
            }
            if ui
                .add_enabled(
                    draft.active != 0 && draft.recipe.route.len() > 2,
                    egui::Button::new("Удалить точку"),
                )
                .clicked()
            {
                draft.recipe.route.remove(draft.active);
                draft.active -= 1;
            }
        });
    });
}

fn zone_controls(ui: &mut egui::Ui, recipe: &mut Recipe) {
    ui.collapsing("Зоны застройки и растений", |ui| {
        let mut delete = None;
        for (index, zone) in recipe.zones.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                let title = format!("{} · зона {}", zone.kind.label(), index + 1);
                ui.collapsing(title, |ui| {
                    egui::ComboBox::from_id_salt("kind")
                        .selected_text(zone.kind.label())
                        .show_ui(ui, |ui| {
                            for kind in ZoneKind::ALL {
                                ui.selectable_value(&mut zone.kind, kind, kind.label());
                            }
                        });
                    coords(ui, &mut zone.center);
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            DragValue::new(&mut zone.radius)
                                .range(1.0..=16.0)
                                .speed(0.1)
                                .fixed_decimals(1)
                                .prefix("Радиус ")
                                .suffix(" м"),
                        );
                        ui.add(
                            DragValue::new(&mut zone.count)
                                .range(0..=16)
                                .prefix("Объекты "),
                        );
                    });
                    if ui.button("Удалить зону").clicked() {
                        delete = Some(index);
                    }
                });
            });
        }
        if let Some(index) = delete {
            recipe.zones.remove(index);
        }
        if ui
            .add_enabled(
                recipe.zones.len() < MAX_ZONES,
                egui::Button::new("Добавить зону"),
            )
            .clicked()
        {
            recipe.zones.push(Zone {
                kind: ZoneKind::Flowers,
                center: [-4.0, -9.0],
                radius: 5.0,
                count: 3,
            });
        }
    });
}

fn coords(ui: &mut egui::Ui, point: &mut [f32; 2]) {
    ui.horizontal_wrapped(|ui| {
        ui.add(
            DragValue::new(&mut point[0])
                .range(MIN_X..=MAX_X)
                .speed(0.1)
                .fixed_decimals(1)
                .prefix("X "),
        );
        ui.add(
            DragValue::new(&mut point[1])
                .range(MIN_Z..=MAX_Z)
                .speed(0.1)
                .fixed_decimals(1)
                .prefix("Z "),
        );
    });
}

fn mini_plan(ui: &mut egui::Ui, draft: &mut Draft) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 185.0),
        Sense::click_and_drag(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    let inner = rect.shrink(9.0);
    let left = MIN_X - 3.0;
    let right = MAX_X + 8.2;
    let center = |[x, z]: [f32; 2]| {
        Pos2::new(
            inner.left() + (x - left) / (right - left) * inner.width(),
            inner.top() + (z - MIN_Z) / (MAX_Z - MIN_Z) * inner.height(),
        )
    };
    painter.rect_filled(rect, 8, BACKDROP);
    painter.rect_stroke(
        rect,
        8,
        Stroke::new(1.0_f32, EDGE),
        egui::StrokeKind::Inside,
    );
    let sea = Rect::from_min_max(center([MAX_X + 0.9, MIN_Z]), inner.right_bottom());
    painter.rect_filled(sea, 4, Color32::from_rgb(35, 63, 83));
    painter.text(
        sea.center_top() + egui::vec2(0.0, 5.0),
        Align2::CENTER_TOP,
        "Море",
        egui::FontId::proportional(11.0),
        MUTED,
    );
    for zone in &draft.recipe.zones {
        let color = match zone.kind {
            ZoneKind::Houses => Color32::from_rgb(244, 184, 131),
            ZoneKind::Trees => Color32::from_rgb(117, 169, 133),
            ZoneKind::Flowers => Color32::from_rgb(215, 150, 220),
            ZoneKind::Grass => Color32::from_rgb(174, 201, 127),
        };
        let at = center(zone.center);
        let size = egui::vec2(
            zone.radius / (right - left) * inner.width(),
            zone.radius / (MAX_Z - MIN_Z) * inner.height(),
        );
        painter.add(egui::Shape::ellipse_filled(
            at,
            size,
            color.gamma_multiply(0.13),
        ));
        painter.add(egui::Shape::ellipse_stroke(
            at,
            size,
            Stroke::new(1.0_f32, color.gamma_multiply(0.67)),
        ));
    }
    let route = &mut draft.recipe.route;
    painter.add(egui::Shape::line(
        route.iter().copied().map(center).collect(),
        Stroke::new(2.5_f32, ACCENT),
    ));
    if response.drag_stopped() {
        draft.drag = None;
    }
    if let Some(pointer) = response.interact_pointer_pos() {
        if response.clicked() || response.drag_started() {
            let target = if response.drag_started() {
                ui.input(|input| input.pointer.press_origin())
                    .unwrap_or(pointer)
            } else {
                pointer
            };
            let selected = route
                .iter()
                .enumerate()
                .map(|(index, point)| (index, center(*point).distance(target)))
                .filter(|(_, distance)| *distance <= 16.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(index, _)| index);
            if let Some(index) = selected {
                draft.active = index;
            }
            if response.drag_started() {
                draft.drag = selected.filter(|&index| index != 0);
            }
        }
        if response.dragged()
            && let Some(index) = draft.drag
        {
            route[index] = [
                (left + (pointer.x - inner.left()) / inner.width() * (right - left))
                    .clamp(MIN_X, MAX_X),
                (MIN_Z + (pointer.y - inner.top()) / inner.height() * (MAX_Z - MIN_Z))
                    .clamp(MIN_Z, MAX_Z),
            ];
        }
    }
    for (index, &point) in route.iter().enumerate() {
        let at = center(point);
        let fill = if draft.active == index { ACCENT } else { MUTED };
        painter.circle_filled(at, 7.0, fill);
        painter.text(
            at,
            Align2::CENTER_CENTER,
            format!("{}", index + 1),
            egui::FontId::proportional(9.0),
            BACKDROP,
        );
    }
    response.on_hover_text("Выбери или перетащи точку маршрута.");
}
