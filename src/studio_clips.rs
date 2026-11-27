use super::*;

#[derive(Clone)]
struct Candidate {
    source: assets::Source,
    clip: usize,
}

impl Studio {
    pub(super) fn animation_clip_tools(&mut self, ui: &mut egui::Ui) {
        if self.game_running || self.scene.avatar.is_none() {
            return;
        }
        ui.collapsing(
            "Импорт и предпросмотр клипов GLB",
            |ui| {
                let id = egui::Id::new("compatible-animation-donor");
                let mut candidate: Option<Candidate> = ui.data(|data| data.get_temp(id));
                if ui.button("Выбрать GLB с анимацией…").clicked()
                    && let Some(path) = self.glb_path()
                {
                    match self.import_asset(&path) {
                        Ok(source)
                            if self
                                .assets
                                .get(&source)
                                .is_some_and(|loaded| !loaded.model.animations.is_empty()) =>
                        {
                            candidate = Some(Candidate { source, clip: 0 });
                            self.status =
                                "Выбери клип из файла. Скелет будет проверен при добавлении."
                                    .into();
                        }
                        Ok(_) => self.status = "В GLB нет анимации скелета.".into(),
                        Err(_) => {}
                    }
                }
                if let Some(donor) = &mut candidate
                    && let Some(loaded) = self.assets.get(&donor.source)
                {
                    let clips = &loaded.model.animations;
                    let selected =
                        clips.get(donor.clip).map_or("Выбрать", |clip| &clip.name);
                    egui::ComboBox::from_id_salt("donor-clip")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for (index, clip) in clips.iter().enumerate() {
                                ui.selectable_value(&mut donor.clip, index, &clip.name);
                            }
                        });
                    if ui.button("Добавить совместимый клип").clicked() {
                        let request = avatar::ImportedClip {
                            source: donor.source.clone(),
                            index: donor.clip,
                        };
                        self.status = match self.add_external_clip(request) {
                            Ok(label) => format!(
                                "Клип «{label}» добавлен. Его можно назначить в списках движения."
                            ),
                            Err(error) => error,
                        };
                    }
                }
                if let Some(candidate) = candidate {
                    ui.data_mut(|data| data.insert_temp(id, candidate));
                }
                let Some(avatar) = self.scene.avatar.as_ref() else {
                    return;
                };
                let count = avatar.imported_clips.len();
                let clips = self
                    .avatar_asset()
                    .into_iter()
                    .flat_map(|(model, _)| model.animations.iter().enumerate().rev().take(count))
                    .map(|(index, clip)| (index, clip.name.clone()))
                    .collect::<Vec<_>>();
                for (index, label) in clips.iter().rev() {
                    let selected =
                        self.preview_external_clip == Some(*index) && self.preview_avatar;
                    if ui
                        .selectable_label(selected, format!("Предпросмотр · {label}"))
                        .clicked()
                    {
                        if selected {
                            self.preview_external_clip = None;
                            self.preview_avatar = false;
                        } else {
                            self.begin_external_preview(*index);
                        }
                    }
                }
                if let Some((last, _)) = clips.first()
                    && ui.button("Убрать последний импортированный клип").clicked()
                {
                    let avatar = self.scene.avatar.as_mut().expect("avatar exists");
                    avatar.imported_clips.pop();
                    for slot in [
                        &mut avatar.idle,
                        &mut avatar.walk,
                        &mut avatar.run,
                        &mut avatar.jump,
                        &mut avatar.sit,
                    ] {
                        if *slot == Some(*last) {
                            *slot = None;
                        }
                    }
                    for (_, slot) in avatar.motion_graph.slots() {
                        if *slot == Some(*last) {
                            *slot = None;
                        }
                    }
                    if self.preview_external_clip == Some(*last) {
                        self.preview_external_clip = None;
                        self.preview_avatar = false;
                    }
                    self.sync_edited_human();
                    self.changed();
                }
            },
        );
    }

    fn add_external_clip(&mut self, clip: avatar::ImportedClip) -> Result<String, String> {
        let mut avatar = self
            .scene
            .avatar
            .clone()
            .ok_or("Сначала добавь персонажа.")?;
        if avatar.imported_clips.len() >= model::MAX_IMPORTED_CLIPS {
            return Err("У персонажа уже максимум импортированных клипов.".into());
        }
        if avatar.imported_clips.contains(&clip) {
            return Err("Этот клип уже добавлен.".into());
        }
        avatar.imported_clips.push(clip);
        let candidate = self.model_for_avatar(&avatar)?;
        let index = candidate.animations.len().saturating_sub(1);
        let label = candidate.animations[index].name.clone();
        self.scene.avatar = Some(avatar);
        self.sync_edited_human();
        self.begin_external_preview(index);
        self.changed();
        Ok(label)
    }

    fn begin_external_preview(&mut self, clip: usize) {
        self.preview_external_clip = Some(clip);
        self.preview_avatar = true;
        self.preview_elapsed = 0.0;
        self.view = View::Three;
        self.focus_camera();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_clip_is_previewed_and_reopened_with_asset_in_project() {
        let mut studio = Studio::default();
        studio.set_scene(Scene::empty(), None, "");
        studio.assets.ensure(&assets::Source::Emma, None).unwrap();
        studio.assign_avatar(assets::Source::Emma, "Спутница".into());
        let donor = studio
            .assets
            .import_bytes(include_bytes!("../demo_assets/emma.glb").to_vec())
            .unwrap();
        let original = studio.avatar_asset().unwrap().0.animations.len();
        let index = studio
            .assets
            .get(&donor)
            .unwrap()
            .model
            .animations
            .iter()
            .position(|clip| clip.name == "Wave")
            .unwrap();
        let imported = avatar::ImportedClip {
            source: donor.clone(),
            index,
        };
        let original_walk = studio.scene.avatar.as_ref().unwrap().walk;
        assert_eq!(
            studio.add_external_clip(imported.clone()).unwrap(),
            "Wave (2)"
        );
        assert_eq!(studio.preview_external_clip, Some(original));
        assert_eq!(studio.avatar_draw().unwrap().clip, Some(original));
        assert_eq!(studio.scene.avatar.as_ref().unwrap().walk, original_walk);
        assert!(studio.add_external_clip(imported.clone()).is_err());

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder =
            std::env::temp_dir().join(format!("studio-clip-{}-{unique}", std::process::id()));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("scene.robot.json");
        studio.scene.save(&path).unwrap();
        studio
            .assets
            .save_files(asset_sources(&studio.scene), &path)
            .unwrap();
        let scene = Scene::load(&path).unwrap();
        let mut reopened = Studio::default();
        reopened.set_scene(scene, Some(path), "");
        assert!(reopened.assets.get(&donor).is_some());
        assert_eq!(
            reopened.scene.avatar.as_ref().unwrap().imported_clips,
            vec![imported]
        );
        assert_eq!(
            reopened.avatar_asset().unwrap().0.animations[original].name,
            "Wave (2)"
        );

        let game = folder.join("standalone.exe");
        game_export::export(&game, &reopened.scene, &reopened.assets, false).unwrap();
        let game_bytes = std::fs::metadata(&game).unwrap().len();
        let editor_bytes = std::fs::metadata(std::env::current_exe().unwrap())
            .unwrap()
            .len();
        assert!(game_bytes > editor_bytes + 8_000_000);
        let root = std::fs::canonicalize(&folder).unwrap();
        assert!(root.starts_with(std::fs::canonicalize(std::env::temp_dir()).unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
