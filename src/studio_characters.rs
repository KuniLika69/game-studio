use super::*;
use crate::assets::Library;

impl Studio {
    pub(super) fn character_library_tools(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Библиотека персонажей", |ui| {
            if ui
                .add_enabled(
                    self.scene.avatar.is_some(),
                    egui::Button::new("Сохранить персонажа в библиотеку…"),
                )
                .clicked()
            {
                self.save_character_to_library();
            }
            ui.label("Роль в этом проекте");
            egui::ComboBox::from_id_salt("character_library_role")
                .selected_text(self.character_role.label())
                .show_ui(ui, |ui| {
                    for role in character_library::Role::ALL {
                        ui.selectable_value(&mut self.character_role, role, role.label());
                    }
                });
            if self.character_role != character_library::Role::Player {
                ui.horizontal_wrapped(|ui| {
                    for (axis, value) in ["X ", "Y ", "Z "]
                        .into_iter()
                        .zip(&mut self.character_position)
                    {
                        ui.add(
                            egui::DragValue::new(value)
                                .range(-100.0..=100.0)
                                .prefix(axis)
                                .speed(0.05),
                        );
                    }
                });
            }
            if ui.button("Разместить из библиотеки…").clicked() {
                self.place_from_character_library();
            }
        });
        if self.scene.avatar.is_some() {
            ui.collapsing("Сменные детали GLB", |ui| {
                let occupied = self.scene.avatar.as_ref().is_some_and(|avatar| {
                    avatar
                        .modular_parts
                        .iter()
                        .any(|part| part.slot == self.character_slot)
                });
                egui::ComboBox::from_id_salt("character_part_slot")
                    .selected_text(self.character_slot.label())
                    .show_ui(ui, |ui| {
                        for slot in model::PartSlot::ALL {
                            ui.selectable_value(&mut self.character_slot, slot, slot.label());
                        }
                    });
                if ui.button("Импорт совместимой детали…").clicked() {
                    self.import_character_part();
                }
                if ui
                    .add_enabled(occupied, egui::Button::new("Снять деталь"))
                    .clicked()
                {
                    self.scene
                        .avatar
                        .as_mut()
                        .unwrap()
                        .modular_parts
                        .retain(|part| part.slot != self.character_slot);
                    self.sync_edited_human();
                    self.changed();
                }
                ui.weak("Для замены используй взвешенный GLB с костями исходной модели.");
            });
        }
    }

    fn character_path(&self, save: bool) -> Option<PathBuf> {
        if let Some(proof) = &self.proof {
            return proof.character_file.clone();
        }
        let dialog =
            rfd::FileDialog::new().add_filter("Персонаж Студии игр", &["character.json", "json"]);
        if save {
            dialog.set_file_name("hero.character.json").save_file()
        } else {
            dialog.pick_file()
        }
    }

    fn save_character_to_library(&mut self) {
        let Some(path) = self.character_path(true) else {
            return;
        };
        self.status = match character_library::save(&self.scene, &self.assets, &path) {
            Ok(()) => format!("Персонаж сохранён в библиотеку. {}", path.display()),
            Err(error) => error,
        };
    }

    fn place_from_character_library(&mut self) {
        let Some(path) = self.character_path(false) else {
            return;
        };
        let mut assets = self.assets.clone();
        if self.character_role == character_library::Role::Player {
            // Charge the scene for the incoming player and for any assets still referenced elsewhere.
            let previous = self.scene.avatar.take();
            assets.retain(asset_sources(&self.scene).map(|(source, _)| source));
            self.scene.avatar = previous;
        }
        let mut preset = match character_library::load(&mut assets, &path) {
            Ok(preset) => preset,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let avatar = preset.avatar.take().expect("library file has avatar");
        if let Err(error) = Self::model_from_library(&assets, &avatar) {
            self.status = error;
            return;
        }
        let name = avatar.name.clone();
        match self.character_role {
            character_library::Role::Player => {
                self.assets = assets;
                self.scene.avatar = Some(avatar);
                self.scene.sprite_art = preset.sprite_art;
                self.scene.sprite_pixels = preset.sprite_pixels;
                self.sync_edited_human();
            }
            role => {
                let Some(id) = npcs::next_id(&self.scene.npcs) else {
                    self.status = "Достигнут предел персонажей сцены.".into();
                    return;
                };
                self.assets = assets;
                let mut npc = npcs::Npc::new(
                    id,
                    avatar,
                    vec![npcs::Waypoint::at(self.character_position)],
                );
                npc.decorative = role == character_library::Role::Decoration;
                self.scene.npcs.push(npc);
                self.sync_npc_assets();
                self.selected_npc = Some(id);
            }
        }
        self.changed();
        self.status = format!("«{name}» размещён. Роль · {}.", self.character_role.label());
    }

    fn import_character_part(&mut self) {
        let Some(path) = self.glb_path() else {
            return;
        };
        let result = (|| {
            let source = self.import_asset(&path)?;
            let mut avatar = self
                .scene
                .avatar
                .clone()
                .ok_or("Сначала добавь персонажа.")?;
            avatar
                .modular_parts
                .retain(|part| part.slot != self.character_slot);
            avatar.modular_parts.push(avatar::ModularPart {
                slot: self.character_slot,
                source,
            });
            self.model_for_avatar(&avatar)?;
            self.scene.avatar = Some(avatar);
            self.sync_edited_human();
            self.changed();
            Ok::<_, String>(())
        })();
        self.status = match result {
            Ok(()) => format!(
                "Деталь «{}» привязана к скелету.",
                self.character_slot.label()
            ),
            Err(error) => error,
        };
    }

    pub(super) fn model_for_avatar(&self, avatar: &Avatar) -> Result<Arc<model::Model>, String> {
        Self::model_from_library(&self.assets, avatar)
    }

    pub(super) fn model_from_library(assets: &Library, avatar: &Avatar) -> Result<Arc<model::Model>, String> {
        let asset = assets.get(&avatar.source)
            .ok_or("Исходная модель персонажа отсутствует.")?;
        let style = avatar.human_style.filter(|style| {
            avatar.source.editable_human() && *style != model::HumanStyle::default()
        });
        let mut model = style.map_or_else(
            || Arc::clone(&asset.model),
            |style| Arc::new(asset.model.customize_human(&style)),
        );
        for part in &avatar.modular_parts {
            let loaded = assets.get(&part.source)
                .ok_or("Файл сменной детали отсутствует.")?;
            model = Arc::new(model.with_character_part(&loaded.model, part.slot)?);
        }
        for clip in &avatar.imported_clips {
            let donor = assets.get(&clip.source)
                .ok_or("Файл импортированного клипа отсутствует.")?;
            model = Arc::new(model.with_compatible_clip(&donor.model, clip.index)?);
        }
        if !avatar.look.is_empty() {
            model = Arc::new(model.with_character_look(&avatar.look)?);
        }
        for attachment in &avatar.bone_attachments {
            let detail = assets.get(&attachment.source).ok_or("Файл детали на кости отсутствует.")?;
            model = Arc::new(model.with_bone_attachment(
                &detail.model, &attachment.bone, attachment.offset, attachment.rotation, attachment.scale,
            )?);
        }
        Ok(model)
    }
}
