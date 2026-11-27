use eframe::egui;

use crate::{
    Studio, asset_sources,
    avatar::Avatar,
    creature_mesh::{self, Draft, Kind},
    creature_preview::Preview,
    npc_ui, npcs,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Target {
    Player,
    NewNpc,
    Npc(u64),
}

pub(super) struct Workshop {
    target: Target,
    recipe: Draft,
    name: String,
    height: f32,
    editing: bool,
    preview: Preview,
}

impl Workshop {
    pub(super) fn paint_preview(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        response: &egui::Response,
        diagnostics: bool,
    ) -> bool {
        self.preview
            .paint(ui, rect, response, self.recipe, self.height, diagnostics)
    }

    fn new(target: Target, kind: Kind) -> Self {
        let mut workshop = Self {
            target,
            recipe: Draft::for_kind(kind),
            name: String::new(),
            height: 0.0,
            editing: false,
            preview: Preview::default(),
        };
        workshop.set_kind(kind);
        workshop
    }

    fn set_kind(&mut self, kind: Kind) {
        self.recipe = Draft::for_kind(kind);
        let (name, height) = match kind {
            Kind::Robot => ("Маяк", 1.64),
            Kind::Quadruped => ("Ива", 0.90),
        };
        self.name = name.into();
        self.height = height;
    }
}

impl Studio {
    pub(super) fn creature_button(&mut self, ui: &mut egui::Ui, target: Target) {
        let recipe = self
            .creature_avatar(target)
            .and_then(|avatar| avatar.generated.as_ref());
        if matches!(target, Target::Npc(_)) && recipe.is_none() {
            return;
        }
        let caption = if recipe.is_some() {
            "Изменить конструкцию…"
        } else {
            "Собрать робота или зверя…"
        };
        if ui
            .add_enabled(!self.game_running, egui::Button::new(caption))
            .clicked()
        {
            self.open_creature_workshop(target);
        }
    }

    fn creature_avatar(&self, target: Target) -> Option<&Avatar> {
        match target {
            Target::Player => self.scene.avatar.as_ref(),
            Target::Npc(id) => self
                .scene
                .npcs
                .iter()
                .find(|npc| npc.id == id)
                .map(|npc| &npc.avatar),
            Target::NewNpc => None,
        }
    }

    fn open_creature_workshop(&mut self, target: Target) {
        self.creature_workshop = Some(
            self.creature_avatar(target)
                .and_then(|avatar| {
                    avatar.generated.clone().map(|recipe| Workshop {
                        target,
                        recipe,
                        name: avatar.name.clone(),
                        height: avatar.height,
                        editing: true,
                        preview: Preview::default(),
                    })
                })
                .unwrap_or_else(|| Workshop::new(target, Kind::Robot)),
        );
    }

    pub(super) fn creature_window(&mut self, ctx: &egui::Context) {
        if self.game_running {
            return;
        }
        let Some(mut workshop) = self.creature_workshop.take() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        egui::Window::new("Мастерская персонажей")
            .default_pos(egui::pos2(275.0, 108.0))
            .default_width(340.0)
            .open(&mut open)
            .show(ctx, |ui| {
                if workshop.editing {
                    ui.heading(workshop.recipe.kind.label());
                } else {
                    ui.horizontal_wrapped(|ui| {
                        for kind in [Kind::Robot, Kind::Quadruped] {
                            if ui
                                .selectable_label(workshop.recipe.kind == kind, kind.label())
                                .clicked()
                                && workshop.recipe.kind != kind
                            {
                                workshop.set_kind(kind);
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Имя");
                    ui.add(egui::TextEdit::singleline(&mut workshop.name).char_limit(80));
                });
                let draft = &mut workshop.recipe;
                for (label, value) in [
                    ("Голова", &mut draft.head),
                    ("Корпус", &mut draft.body),
                    ("Ноги", &mut draft.legs),
                    ("Хвост", &mut draft.tail),
                ] {
                    if label == "Хвост" && draft.kind != Kind::Quadruped {
                        continue;
                    }
                    ui.add(
                        egui::Slider::new(value, 0.55..=1.55)
                            .text(label)
                            .fixed_decimals(2),
                    );
                }
                ui.add(
                    egui::Slider::new(&mut workshop.height, 0.5..=3.5)
                        .text("Рост, м")
                        .fixed_decimals(2),
                );
                for (label, color) in [
                    ("Основной цвет", &mut draft.primary),
                    ("Дополнительный цвет", &mut draft.secondary),
                    ("Акцент", &mut draft.accent),
                ] {
                    ui.horizontal(|ui| {
                        ui.color_edit_button_srgb(color);
                        ui.label(label);
                    });
                }
                ui.separator();
                workshop.preview.controls(ui, workshop.recipe.kind);
                ui.separator();
                let caption = if workshop.editing {
                    "Обновить персонажа"
                } else {
                    "Собрать персонажа"
                };
                apply = ui
                    .add_enabled(!workshop.name.trim().is_empty(), egui::Button::new(caption))
                    .clicked();
            });
        if apply {
            match self.apply_creature(
                workshop.target,
                workshop.recipe.clone(),
                workshop.name.trim(),
                workshop.height,
                workshop.editing,
            ) {
                Ok(_) => return,
                Err(error) => self.status = error,
            }
        }
        self.creature_workshop = open.then_some(workshop);
    }

    pub(super) fn apply_creature(
        &mut self,
        target: Target,
        recipe: Draft,
        name: &str,
        height: f32,
        editing: bool,
    ) -> Result<Option<u64>, String> {
        let previous = self.creature_avatar(target);
        if editing
            && previous
                .and_then(|avatar| avatar.generated.as_ref())
                .is_none_or(|old| old.kind != recipe.kind)
        {
            return Err(
                "Исходная конструкция персонажа изменилась. Открой мастерскую снова.".into(),
            );
        }
        let mut library = self.assets.clone();
        let source = library.import_bytes(creature_mesh::build(&recipe)?)?;
        let model = &library
            .get(&source)
            .ok_or("Созданный GLB отсутствует.")?
            .model;
        let mut avatar = if editing {
            previous.cloned().ok_or("Персонаж удалён.")?
        } else {
            Avatar::new(source.clone(), name.to_owned(), model)
        };
        avatar.source = source;
        avatar.generated = Some(recipe);
        avatar.name = name.into();
        avatar.height = height;
        avatar.validate()?;
        Self::model_from_library(&library, &avatar)?;

        let npc_id = match target {
            Target::Player => {
                self.scene.avatar = Some(avatar);
                None
            }
            Target::Npc(id) => {
                let npc = self
                    .scene
                    .npcs
                    .iter_mut()
                    .find(|npc| npc.id == id)
                    .ok_or("Персонаж удалён.")?;
                npc.avatar = avatar;
                Some(id)
            }
            Target::NewNpc => {
                let id = npcs::next_id(&self.scene.npcs).ok_or("Достигнут предел NPC в сцене.")?;
                let [x, _, z] = self.game_position().to_array();
                let offset = if self.scene.npcs.len().is_multiple_of(2) {
                    2.0
                } else {
                    -2.0
                };
                let z = z - 1.7 * (self.scene.npcs.len() / 2) as f32;
                self.scene.npcs.push(npcs::Npc::new(
                    id,
                    avatar,
                    vec![
                        npcs::Waypoint::at([x + offset, 0.0, z]),
                        npcs::Waypoint::at([x + offset, 0.0, z - 2.5]),
                    ],
                ));
                self.npc_panel = npc_ui::Panel::Route;
                self.npc_waypoint = 0;
                Some(id)
            }
        };
        library.retain(asset_sources(&self.scene).map(|(source, _)| source));
        self.assets = library;
        if let Some(id) = npc_id {
            self.selected_npc = Some(id);
            self.sync_npc_assets();
        } else {
            self.sync_edited_human();
            self.preview_avatar = true;
        }
        self.focus_camera();
        self.changed();
        self.status = format!("«{name}» готов. Доступны покой, ходьба и бег.");
        Ok(npc_id)
    }
}
