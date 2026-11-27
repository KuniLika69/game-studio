use crate::{ACCENT, EditorTab, Studio, View, assets, plan2d::key_marker_id, rules_ui};
use eframe::egui::{self, RichText, Vec2};

#[derive(Default)]
pub(crate) struct Search {
    open: bool,
    focus: bool,
    query: String,
    diagnostics: bool,
}

impl Search {
    pub fn open(&mut self) {
        self.open = true;
        self.focus = true;
    }
}

#[derive(Clone, Copy)]
enum Target {
    World(u64),
    Door(u64),
    Key(u32, [f32; 3]),
    Npc(u64),
    Character,
    Part(u64),
    Rule(u32),
    Zone(u32, [f32; 3]),
    Atlas,
}

struct ResultRow {
    label: String,
    detail: String,
    target: Target,
    error: bool,
}

impl Studio {
    pub(super) fn search_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(!self.game_running, egui::Button::new("Поиск…"))
            .on_hover_text("Ctrl+F · объекты, ресурсы и ошибки привязок")
            .clicked()
        {
            self.search.open();
        }
    }

    pub(super) fn search_window(&mut self, ctx: &egui::Context) {
        if !self.search.open || self.game_running {
            return;
        }
        let rows = self.search_rows();
        let mut target = None;
        egui::Window::new("Поиск по сцене")
            .open(&mut self.search.open)
            .default_width(500.0)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.search.query)
                        .desired_width(f32::INFINITY)
                        .hint_text("Имя объекта, правила или ресурса"),
                );
                if std::mem::take(&mut self.search.focus) {
                    response.request_focus();
                }
                let errors = rows.iter().filter(|row| row.error).count();
                ui.checkbox(
                    &mut self.search.diagnostics,
                    format!("Только ошибки ({errors})"),
                );
                let query = self.search.query.trim().to_lowercase();
                let mut any = false;
                egui::ScrollArea::vertical()
                    .max_height(335.0)
                    .show(ui, |ui| {
                        for row in rows.into_iter().filter(|row| {
                            (!self.search.diagnostics || row.error)
                                && (query.is_empty()
                                    || row.label.to_lowercase().contains(&query)
                                    || row.detail.to_lowercase().contains(&query))
                        }) {
                            any = true;
                            let text = if row.error {
                                RichText::new(&row.label).color(ACCENT)
                            } else {
                                RichText::new(&row.label)
                            };
                            if ui
                                .selectable_label(false, text)
                                .on_hover_text(&row.detail)
                                .clicked()
                            {
                                target = Some(row.target);
                            }
                            ui.weak(row.detail);
                        }
                        if !any {
                            ui.weak("По этому запросу ничего не найдено.");
                        }
                    });
            });
        if let Some(target) = target {
            self.search.open = false;
            self.reveal_search_result(target);
        }
    }

    fn search_rows(&self) -> Vec<ResultRow> {
        let mut result = Vec::new();
        let mut add = |label: &str, detail: &str, target, error| {
            result.push(ResultRow {
                label: label.to_owned(),
                detail: detail.to_owned(),
                target,
                error,
            })
        };
        for part in &self.scene.parts {
            add(&part.name, "Деталь персонажа", Target::Part(part.id), false);
        }
        for object in &self.scene.world.objects {
            add(
                &object.name,
                "Объект сцены",
                Target::World(object.id),
                false,
            );
        }
        for building in &self.scene.world.buildings {
            add(&building.name, "Дом", Target::World(building.id), false);
            for door in building.doors() {
                add(
                    &door.name,
                    &format!("Дверь · {}", building.name),
                    Target::Door(door.id),
                    false,
                );
            }
        }
        for key in &self.scene.world.keys {
            add(&key.name, "Ключ", Target::Key(key.id, key.position), false);
        }
        for item in &self.scene.world.models {
            let unavailable = self.assets.get(&item.source).is_none();
            add(
                &item.label,
                &model_detail(&item.source, unavailable),
                Target::World(item.id),
                unavailable,
            );
        }
        for (actor, target) in self
            .scene
            .avatar
            .iter()
            .map(|avatar| (avatar, Target::Character))
            .chain(
                self.scene
                    .npcs
                    .iter()
                    .map(|npc| (&npc.avatar, Target::Npc(npc.id))),
            )
        {
            let unavailable = self.assets.get(&actor.source).is_none();
            add(
                &actor.name,
                &model_detail(&actor.source, unavailable),
                target,
                unavailable,
            );
            for part in &actor.modular_parts {
                let unavailable = self.assets.get(&part.source).is_none();
                add(
                    part.slot.label(),
                    &model_detail(&part.source, unavailable),
                    target,
                    unavailable,
                );
            }
        }
        if let Some(map) = &self.scene.map_2d {
            for atlas in &map.atlases {
                add(&atlas.name, "Атлас PNG", Target::Atlas, false);
            }
        }
        let broken = rules_ui::Editor::broken_rules(&self.scene);
        for rule in &self.scene.rules.rules {
            let error = broken.contains(&rule.id);
            add(
                &rule.name,
                if error {
                    "Ошибка · у правила отсутствует событие или ссылка на объект"
                } else {
                    "Правило"
                },
                Target::Rule(rule.id),
                error,
            );
        }
        for zone in &self.scene.rules.zones {
            add(
                &zone.name,
                "Зона",
                Target::Zone(zone.id, zone.position),
                false,
            );
        }
        result
    }

    fn reveal_search_result(&mut self, target: Target) {
        match target {
            Target::World(id) => {
                self.editor_tab = EditorTab::World;
                self.world_selection = Some(id);
                self.selected_door = None;
                self.focus_camera();
            }
            Target::Door(id) => self.select_door(id),
            Target::Key(id, position) => {
                self.editor_tab = EditorTab::World;
                self.world_selection = Some(key_marker_id(id));
                self.plan_offset = Vec2::new(position[0], position[2]);
                self.camera.target = position;
            }
            Target::Npc(id) => {
                self.editor_tab = EditorTab::Npcs;
                self.selected_npc = Some(id);
                self.focus_camera();
            }
            Target::Part(id) => {
                self.editor_tab = EditorTab::Character;
                self.selected = Some(id);
                self.focus_camera();
            }
            Target::Character => {
                self.editor_tab = EditorTab::Character;
                self.focus_camera();
            }
            Target::Rule(id) => {
                self.editor_tab = EditorTab::Rules;
                self.rule_editor.select_rule(id);
            }
            Target::Zone(id, position) => {
                self.editor_tab = EditorTab::Rules;
                self.rule_editor.select_zone(id);
                if self.scene.map_2d.is_none() {
                    self.plan_offset = Vec2::new(position[0], position[2]);
                    self.view = View::Two;
                    self.overhead = true;
                }
            }
            Target::Atlas => {
                self.editor_tab = EditorTab::Map;
                self.map_sprite_editor = false;
            }
        }
    }
}

fn model_detail(source: &assets::Source, unavailable: bool) -> String {
    let label = if unavailable {
        "Ошибка · недоступен ресурс"
    } else {
        "Ресурс"
    };
    match source {
        assets::Source::Imported(id) => format!("{label} GLB · {id}"),
        bundled => format!("{label} · {bundled:?}"),
    }
}
