use std::path::Path;

use eframe::egui;

use crate::{Studio, assets::MAX_PLACEMENTS, object_library};

impl Studio {
    pub(super) fn object_library_controls(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Библиотека объектов", |ui| {
            let id = egui::Id::new("studio_object_library_path");
            let mut path = ui
                .ctx()
                .data(|data| data.get_temp::<String>(id))
                .unwrap_or_default();
            if ui
                .add(egui::TextEdit::singleline(&mut path).hint_text("Путь к файлу .studio-object"))
                .changed()
            {
                ui.ctx().data_mut(|data| data.insert_temp(id, path.clone()));
            }
            let path = Path::new(path.trim());
            let supplied = !path.as_os_str().is_empty() && !self.game_running;
            let selected = self
                .scene
                .world
                .models
                .iter()
                .find(|item| self.world_selection == Some(item.id));
            if ui
                .add_enabled(
                    supplied && selected.is_some(),
                    egui::Button::new("Сохранить объект в библиотеку"),
                )
                .clicked()
                && let Some(item) = selected
            {
                self.status = match object_library::save(path, item, &self.assets) {
                    Ok(()) => format!(
                        "Объект «{}» сохранён вместе со своими настройками.",
                        item.label
                    ),
                    Err(error) => error,
                };
            }
            if ui
                .add_enabled(
                    supplied && self.scene.world.models.len() < MAX_PLACEMENTS,
                    egui::Button::new("Вставить объект из библиотеки"),
                )
                .clicked()
            {
                let index = self.scene.world.models.len();
                let foot = [
                    1.7 + (index % 5) as f32 * 1.45,
                    0.0,
                    -0.4 - (index / 5) as f32 * 2.0,
                ];
                match object_library::insert(
                    path,
                    &mut self.assets,
                    &mut self.scene.world.models,
                    foot,
                ) {
                    Ok(id) => {
                        self.world_selection = Some(id);
                        self.status =
                            "Объект вставлен. Материал, коллайдер и точка посадки сохранены."
                                .into();
                        self.changed();
                        self.focus_camera();
                    }
                    Err(error) => self.status = error,
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        assets::Placement,
        proof_input::{Driver, click, find_label, replace_text},
        scene::Scene,
    };

    fn frame(
        ctx: &egui::Context,
        input: &mut Driver,
        studio: &mut Studio,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(input.raw(events), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| studio.object_library_controls(ui));
        })
    }

    fn press(ctx: &egui::Context, input: &mut Driver, studio: &mut Studio, label: &str) {
        let drawn = frame(ctx, input, studio, Vec::new());
        let point = find_label(&drawn, label, 0).unwrap_or_else(|| panic!("missing {label}"));
        input.skip_frames(40);
        frame(ctx, input, studio, click(point));
        frame(ctx, input, studio, Vec::new());
    }

    #[test]
    fn object_library_buttons_save_and_insert_into_a_blank_project() {
        let path = std::env::temp_dir().join(format!(
            "studio-object-ui-{}.studio-object",
            std::process::id()
        ));
        let mut studio = Studio::default();
        studio.set_scene(Scene::empty(), None, "");
        let source = studio
            .assets
            .import_bytes(include_bytes!("../demo_assets/quarter/bench_alt.glb").to_vec())
            .unwrap();
        let placement = Placement::at_ground(
            &[],
            &studio.assets.get(&source).unwrap().model,
            source.clone(),
            "Лавка".into(),
            [0.0; 3],
            1.8,
            0.0,
        );
        studio.world_selection = Some(placement.id);
        studio.scene.world.models.push(placement);
        let ctx = egui::Context::default();
        let mut input = Driver::new([800, 600]);
        press(&ctx, &mut input, &mut studio, "Библиотека объектов");
        press(&ctx, &mut input, &mut studio, "Путь к файлу .studio-object");
        frame(
            &ctx,
            &mut input,
            &mut studio,
            replace_text(&path.to_string_lossy()),
        );
        press(
            &ctx,
            &mut input,
            &mut studio,
            "Сохранить объект в библиотеку",
        );
        assert!(path.is_file(), "{}", studio.status);
        studio.set_scene(Scene::empty(), None, "");
        assert!(studio.assets.get(&source).is_none());
        press(
            &ctx,
            &mut input,
            &mut studio,
            "Вставить объект из библиотеки",
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(studio.scene.world.models[0].source, source);
        assert_eq!(
            studio.world_selection,
            Some(studio.scene.world.models[0].id)
        );
        assert!(!studio.assets.get(&source).unwrap().model.images.is_empty());
    }
}
