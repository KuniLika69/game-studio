use eframe::egui::{self, RichText};

use crate::{
    EditorTab, Studio, View,
    editor_theme::{ACCENT, BACKDROP, BRIGHT, EDGE, MUTED, SELECTED, SURFACE},
    human_editor,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Workspace {
    Create,
    Develop,
}

impl Studio {
    pub(super) fn workspace_switcher(&mut self, ui: &mut egui::Ui) {
        let previous = self.workspace;
        egui::Frame::new()
            .fill(BACKDROP)
            .stroke(egui::Stroke::new(1.0_f32, EDGE))
            .inner_margin(3)
            .corner_radius(10)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.horizontal(|ui| {
                    for (workspace, title, hint) in [
                        (
                            Workspace::Create,
                            "Создание",
                            "Персонажи, сцена, правила и сборка игры",
                        ),
                        (
                            Workspace::Develop,
                            "Разработка",
                            "Инструменты проекта, проверка сценариев и правки ИИ",
                        ),
                    ] {
                        let selected = self.workspace == workspace;
                        let mut title = RichText::new(title);
                        if selected {
                            title = title.color(BRIGHT).strong();
                        }
                        if ui
                            .add(egui::Button::selectable(selected, title))
                            .on_hover_text(hint)
                            .clicked()
                        {
                            self.workspace = workspace;
                        }
                    }
                });
            });
        if previous != self.workspace {
            self.workspace_help_open = false;
        }
        let title = match self.workspace {
            Workspace::Create => "Первые шаги…",
            Workspace::Develop => "Справка разработчика…",
        };
        let help = egui::Button::new(title);
        let help = if self.workspace_help_open {
            help.fill(SELECTED)
        } else {
            help
        };
        if ui
            .add(help)
            .on_hover_text("Открыть или закрыть подсказки для этого режима · F1")
            .clicked()
        {
            self.workspace_help_open = !self.workspace_help_open;
        }
    }

    pub(super) fn workspace_tabs(&mut self, ui: &mut egui::Ui) {
        let map = self.scene.map_2d.is_some();
        let tactics = self.supports_tactics();
        let advanced = [
            (EditorTab::Environment, "Среда", true),
            (EditorTab::Effects, "Эффекты", true),
            (EditorTab::Audio, "Звук", true),
            (EditorTab::Lighting, "Свет", !map),
            (EditorTab::Tactics, "Тактика", map && tactics),
        ];
        egui::Frame::new()
            .fill(BACKDROP)
            .stroke(egui::Stroke::new(1.0_f32, EDGE))
            .corner_radius(10)
            .inner_margin(5)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(3.0, 4.0);
                ui.spacing_mut().button_padding.x = 6.0;
                ui.horizontal_wrapped(|ui| {
                    if map {
                        ui.selectable_value(&mut self.editor_tab, EditorTab::Map, "Карта");
                    }
                    ui.selectable_value(&mut self.editor_tab, EditorTab::Character, "Персонаж");
                    ui.selectable_value(&mut self.editor_tab, EditorTab::World, "Окружение");
                    ui.selectable_value(&mut self.editor_tab, EditorTab::Npcs, "NPC")
                        .on_hover_text("Персонажи сцены, их маршруты и диалоги");
                    ui.selectable_value(&mut self.editor_tab, EditorTab::Rules, "Правила");
                });
                ui.separator();
                if self.workspace == Workspace::Develop {
                    ui.horizontal_wrapped(|ui| {
                        for (tab, label, available) in advanced {
                            if available {
                                ui.selectable_value(&mut self.editor_tab, tab, label);
                            }
                        }
                    });
                } else {
                    let label = advanced
                        .iter()
                        .find(|(tab, _, available)| *available && *tab == self.editor_tab)
                        .map_or("Ещё инструменты", |(_, label, _)| *label);
                    ui.menu_button(label, |ui| {
                        for (tab, label, available) in advanced {
                            if available
                                && ui
                                    .selectable_value(&mut self.editor_tab, tab, label)
                                    .clicked()
                            {
                                ui.close();
                            }
                        }
                    });
                }
            });
        if map
            && matches!(
                self.editor_tab,
                EditorTab::Map
                    | EditorTab::Tactics
                    | EditorTab::Environment
                    | EditorTab::Effects
                    | EditorTab::Audio
            )
        {
            self.map_sprite_editor = false;
        }
    }

    pub(super) fn workspace_help(&mut self, ctx: &egui::Context) {
        if !self.game_running
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F1))
        {
            self.workspace_help_open = !self.workspace_help_open;
        }
        if !self.workspace_help_open {
            return;
        }
        let mut open = true;
        let title = match self.workspace {
            Workspace::Create => "Первые шаги",
            Workspace::Develop => "Справка разработчика",
        };
        let guide_height = (ctx.content_rect().height() - 178.0).clamp(170.0, 450.0);
        egui::Window::new(title)
            .id(egui::Id::new("studio_workspace_help"))
            .open(&mut open)
            .collapsible(false)
            .default_width(418.0)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                egui::ScrollArea::vertical()
                    .max_height(guide_height)
                    .show(ui, |ui| {
                        ui.add_enabled_ui(!self.game_running, |ui| match self.workspace {
                            Workspace::Create => self.creation_guide(ui),
                            Workspace::Develop => self.development_guide(ui),
                        });
                    });
            });
        self.workspace_help_open = open;
    }

    fn creation_guide(&mut self, ui: &mut egui::Ui) {
        let map = self.scene.map_2d.is_some();
        let (heading, steps, action, tab) = if map {
            (
                "Нарисовать уровень",
                "Положи плитки на карту, отметь препятствия и задай место появления игрока.",
                "Открыть карту",
                EditorTab::Map,
            )
        } else {
            (
                "Собрать сцену",
                "Добавь модель или здание. Выбери объект в сцене, чтобы настроить его внешний вид и коллайдер.",
                "Открыть окружение",
                EditorTab::World,
            )
        };
        if guide_entry(ui, heading, steps, action) {
            self.open_workspace_tab(tab);
        }
        if guide_entry(
            ui,
            "Настроить героя",
            "Выбери модель, движение и управление. Проверь результат клавишами WASD или стрелками.",
            "Открыть персонажа",
        ) {
            self.open_workspace_tab(EditorTab::Character);
        }
        if guide_entry(
            ui,
            "Добавить цель",
            "Выбери событие, условие и действие. Например, при входе в зону показать сообщение.",
            "Открыть правила",
        ) {
            self.open_workspace_tab(EditorTab::Rules);
        }
        if !map {
            ui.collapsing("Внешность, голос и библиотека", |ui| {
                if self.scene.avatar.as_ref().is_none_or(|avatar| avatar.source.editable_human())
                    && ui.button("Настроить внешность…").clicked()
                {
                    self.open_character_appearance();
                }
                ui.label("Для реплики добавь WAV на вкладке «Звук», затем выбери запись в диалоге персонажа.");
                if ui.button("Открыть звук").clicked() {
                    self.open_workspace_tab(EditorTab::Audio);
                }
                ui.weak("На вкладке «Персонаж» открой «Библиотека и клипы». Сохрани героя в библиотеку, чтобы разместить его в другом проекте.");
            });
        }
        ui.weak("Нажми «Играть», чтобы проверить сцену. «Собрать игру для Windows» создаст отдельный EXE.");
    }

    fn development_guide(&mut self, ui: &mut egui::Ui) {
        if guide_entry(
            ui,
            "Механики и расширения",
            "Создай правила из сигналов, условий и действий. Здесь же подключаются расширения и сохраняются шаблоны.",
            "Открыть правила",
        ) {
            self.open_workspace_tab(EditorTab::Rules);
        }
        if guide_entry(
            ui,
            "Проверки и сохранения",
            "Запиши прохождение и ожидаемые состояния. После правки повтори тот же сценарий.",
            "Открыть проверки",
        ) {
            self.open_playback_tools();
        }
        if guide_entry(
            ui,
            "Проверка правки в EXE",
            "Предложи правку через ИИ или JSON и сравни прохождения до её применения.",
            "Открыть правки ИИ",
        ) {
            self.open_scene_review();
        }
        if guide_entry(
            ui,
            "Время кадра и профиль",
            "Сравни время кадра при одинаковых настройках и ракурсе.",
            "Открыть производительность",
        ) {
            self.quality.open = true;
        }
    }

    fn open_workspace_tab(&mut self, tab: EditorTab) {
        self.close_game_wizard();
        self.editor_tab = tab;
    }

    pub(super) fn open_character_appearance(&mut self) {
        self.open_workspace_tab(EditorTab::Character);
        self.view = View::Three;
        self.human_target = None;
        self.human_draft = self
            .scene
            .avatar
            .as_ref()
            .filter(|avatar| avatar.source.editable_human())
            .map_or_else(
                human_editor::Draft::default,
                human_editor::Draft::from_avatar,
            );
        self.human_editor_open = true;
    }
}

fn guide_entry(ui: &mut egui::Ui, heading: &str, steps: &str, button: &str) -> bool {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0_f32, EDGE))
        .corner_radius(10)
        .inner_margin(11)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(RichText::new(heading).strong().size(15.0).color(ACCENT));
            ui.label(RichText::new(steps).color(MUTED));
            ui.add(egui::Button::new(button).fill(SELECTED)).clicked()
        })
        .inner
}
