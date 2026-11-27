use crate::{ACCENT, MUTED, Studio, asset_sources, editor_history::Replace, scene::Scene};
use eframe::egui::{self, Event, Key, KeyboardShortcut, Modifiers, RichText};

impl Studio {
    pub(super) fn editor_begin(&mut self, ctx: &egui::Context) {
        if !self.desk.pending && !ctx.input(|input| input.pointer.any_down()) {
            // The default Studio can also be constructed without opening a project.
            self.desk.initialize(&self.scene, &self.assets);
        }
        if self.desk.replace.is_some() || self.desk.recovery_open {
            return;
        }
        let shortcut = |modifiers, key| {
            ctx.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(modifiers, key)))
        };
        if shortcut(Modifiers::COMMAND, Key::S) {
            self.save(false);
        }
        if !self.game_running && shortcut(Modifiers::COMMAND, Key::F) {
            self.search.open();
        }
        if self.game_running
            || ctx.wants_keyboard_input()
            || ctx.input(|input| input.pointer.any_down())
        {
            return;
        }
        if shortcut(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z)
            || shortcut(Modifiers::COMMAND, Key::Y)
        {
            self.history_step(true);
        } else if shortcut(Modifiers::COMMAND, Key::Z) {
            self.history_step(false);
        }
    }

    pub(super) fn editor_end(&mut self, ctx: &egui::Context) {
        let (hold, input_changed, close) = ctx.input(|input| {
            let changed = input.events.iter().any(|event| {
                matches!(
                    event,
                    Event::PointerButton { .. }
                        | Event::Key { .. }
                        | Event::Text(_)
                        | Event::Paste(_)
                        | Event::Cut
                        | Event::Ime(_)
                        | Event::MouseWheel { .. }
                )
            });
            (
                input.pointer.any_down(),
                changed,
                input.viewport().close_requested(),
            )
        });
        self.desk.pending |= input_changed || self.revision != self.desk.watch_revision;
        if self.desk.pending && !hold {
            self.commit_editor_change();
        }
        if close && !self.desk.closing {
            self.commit_editor_change();
            if self.desk.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.desk.replace = Some(Replace::Close);
            }
        }
        if !hold {
            if let Some(Err(error)) = self
                .desk
                .maybe_autosave(&self.assets, self.project.as_deref())
            {
                self.status = format!("Не удалось записать автосохранение. {error}");
            }
            if let Some(after) = self.desk.remaining() {
                ctx.request_repaint_after(after);
            }
        }
        self.search_window(ctx);
        self.game_settings(ctx);
        self.recovery_dialog(ctx);
        self.unsaved_dialog(ctx);
    }

    pub(super) fn editor_commands(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                !self.game_running && self.desk.can_undo(),
                egui::Button::new("Отменить"),
            )
            .on_hover_text("Ctrl+Z")
            .clicked()
        {
            self.history_step(false);
        }
        if ui
            .add_enabled(
                !self.game_running && self.desk.can_redo(),
                egui::Button::new("Повторить"),
            )
            .on_hover_text("Ctrl+Y")
            .clicked()
        {
            self.history_step(true);
        }
        self.search_button(ui);
        ui.menu_button("Проект", |ui| {
            if ui
                .add_enabled(
                    !self.game_running,
                    egui::Button::new("Управление и движение…"),
                )
                .clicked()
            {
                self.game_settings_open = true;
                ui.close();
            }
            if ui.button("Графика и производительность…").clicked() {
                self.quality.open = true;
                ui.close();
            }
            if ui.button("Восстановить автосохранение…").clicked() {
                self.desk.show_recoveries();
                ui.close();
            }
        });
        let name = self
            .project
            .as_ref()
            .and_then(|path| path.file_name())
            .map_or_else(|| "Без имени".into(), |name| name.to_string_lossy());
        if self.desk.dirty() {
            ui.label(RichText::new(name).color(ACCENT))
                .on_hover_text("Сцена содержит несохранённые изменения");
            ui.label(RichText::new("● Изменено").color(ACCENT));
        } else {
            ui.label(RichText::new(name).color(MUTED));
        }
    }

    fn game_settings(&mut self, ctx: &egui::Context) {
        if !self.game_settings_open {
            return;
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.game_settings_open = false;
            return;
        }
        let mut done = false;
        let mut changed = false;
        egui::Window::new("Управление и движение")
            .open(&mut self.game_settings_open)
            .default_width(440.0)
            .default_pos(egui::pos2(340.0, 75.0))
            .show(ctx, |ui| {
                done = ui.button("Готово").clicked();
                egui::ScrollArea::vertical()
                    .max_height(485.0)
                    .show(ui, |ui| {
                        changed =
                            crate::play_settings::inspector(ui, &mut self.scene.play_settings);
                    });
            });
        if changed {
            self.changed();
        }
        if done {
            self.game_settings_open = false;
        }
    }

    pub(super) fn commit_editor_change(&mut self) {
        self.desk.watch_revision = self.revision;
        if self.link_playthrough.is_some() {
            self.desk.pending = false;
            return;
        }
        self.desk.observe(&self.scene, &self.assets);
    }

    fn history_step(&mut self, forward: bool) {
        self.commit_editor_change();
        let Some(snapshot) = self.desk.travel(forward) else {
            return;
        };
        let mut scene = (*snapshot.scene).clone();
        scene.checkpoint = self.scene.checkpoint.take();
        self.scene = scene;
        self.assets
            .retain(asset_sources(&self.scene).map(|(source, _)| source));
        self.assets.restore_cached(&snapshot.imports);
        for (source, label) in asset_sources(&self.scene) {
            if let Err(error) = self.assets.ensure(source, self.project.as_deref()) {
                self.status = format!("Модель «{label}» не загружена. {error}");
                self.import_problems.record(label, error);
            }
        }
        self.selected = self
            .selected
            .filter(|id| self.scene.parts.iter().any(|part| part.id == *id));
        self.world_selection = self.world_selection.filter(|id| {
            self.scene.world.models.iter().any(|item| item.id == *id)
                || self.scene.world.objects.iter().any(|item| item.id == *id)
                || self.scene.world.buildings.iter().any(|item| item.id == *id)
                || self
                    .scene
                    .world
                    .keys
                    .iter()
                    .any(|item| crate::plan2d::key_marker_id(item.id) == *id)
        });
        self.selected_npc = self
            .selected_npc
            .filter(|id| self.scene.npcs.iter().any(|npc| npc.id == *id));
        self.sync_edited_human();
        self.sync_npc_assets();
        self.changed();
        self.desk.watch_revision = self.revision;
        self.status = if forward {
            "Правка повторена."
        } else {
            "Правка отменена."
        }
        .into();
    }

    pub(super) fn request_replace(&mut self, ctx: &egui::Context, action: Replace) {
        self.commit_editor_change();
        if self.desk.dirty() {
            self.desk.replace = Some(action);
        } else {
            self.replace(ctx, action);
        }
    }

    fn replace(&mut self, ctx: &egui::Context, action: Replace) {
        match action {
            Replace::New => self.restart_open = true,
            Replace::Open => self.open(),
            Replace::Close => {
                self.desk.discard_recovery();
                self.desk.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Replace::Recover(index) => {
                let Some(copy) = self.desk.recoveries.get(index).cloned() else {
                    return;
                };
                match Scene::load(&copy.scene) {
                    Ok(scene) => {
                        self.set_scene(
                            scene,
                            Some(copy.scene.clone()),
                            "Восстановлена отдельная копия сцены.",
                        );
                        self.project = copy.original.clone();
                        self.desk.recovered(&copy);
                        self.desk.recovery_open = false;
                    }
                    Err(error) => self.record_import_problem("Копия сцены", error),
                }
            }
        }
    }

    fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.desk.replace else {
            return;
        };
        let modal = egui::Modal::new(egui::Id::new("studio_unsaved")).show(ctx, |ui| {
            ui.heading("Сохранить изменения?");
            ui.label("В сцене есть несохранённые правки.");
            ui.horizontal(|ui| {
                if ui.button("Сохранить и продолжить").clicked() {
                    Some(true)
                } else if ui.button("Продолжить без сохранения").clicked() {
                    Some(false)
                } else {
                    if ui.button("Отмена").clicked() {
                        self.desk.replace = None;
                    }
                    None
                }
            })
            .inner
        });
        if modal.should_close() {
            self.desk.replace = None;
        }
        if let Some(save) = modal.inner {
            if save {
                self.save(false);
                if self.desk.dirty() {
                    return;
                }
            }
            self.desk.replace = None;
            self.replace(ctx, action);
        }
    }

    fn recovery_dialog(&mut self, ctx: &egui::Context) {
        if !self.desk.recovery_open {
            return;
        }
        let mut selected = None;
        egui::Window::new("Восстановление сцены")
            .open(&mut self.desk.recovery_open)
            .default_width(420.0)
            .show(ctx, |ui| {
                if self.desk.recoveries.is_empty() {
                    ui.label("Отдельных копий пока нет.");
                }
                egui::ScrollArea::vertical()
                    .max_height(290.0)
                    .show(ui, |ui| {
                        for (index, copy) in self.desk.recoveries.iter().enumerate() {
                            let name = copy
                                .original
                                .as_deref()
                                .and_then(|path| path.file_name())
                                .map_or_else(
                                    || "Новая сцена".into(),
                                    |name| name.to_string_lossy(),
                                );
                            ui.horizontal(|ui| {
                                if ui.button("Восстановить").clicked() {
                                    selected = Some(index);
                                }
                                ui.label(name)
                                    .on_hover_text(copy.scene.display().to_string());
                            });
                        }
                    });
            });
        if let Some(index) = selected {
            self.request_replace(ctx, Replace::Recover(index));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_the_editor_does_not_create_an_edit() {
        let ctx = egui::Context::default();
        let mut input = crate::proof_input::Driver::new([1440, 810]);
        let mut studio = Studio::default();
        let before = serde_json::to_value(&studio.scene).unwrap();
        for _ in 0..4 {
            let _ = ctx.run(input.raw(vec![]), |ctx| studio.run_frame(ctx));
        }
        let after = serde_json::to_value(&studio.scene).unwrap();
        for (field, old) in before.as_object().unwrap() {
            assert_eq!(&after[field], old, "changed field: {field}");
        }
        assert!(!studio.desk.dirty());
    }
}
