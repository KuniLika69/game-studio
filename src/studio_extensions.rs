use std::path::Path;

use eframe::egui;

use crate::{
    Studio,
    assets::materials::Lighting,
    extensions::{Capability, Manifest},
};

impl Studio {
    pub(super) fn extension_tools(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Расширения проекта", |ui| {
            let id = egui::Id::new("studio-extension-file");
            let mut path = ui
                .data(|data| data.get_temp::<String>(id))
                .unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut path)
                        .hint_text("Путь к файлу .studio-extension.json"),
                )
                .changed()
            {
                ui.data_mut(|data| data.insert_temp(id, path.clone()));
            }
            let path = Path::new(path.trim());
            if ui
                .add_enabled(
                    !path.as_os_str().is_empty(),
                    egui::Button::new("Импортировать расширение"),
                )
                .clicked()
            {
                let result = Manifest::read(path).and_then(|manifest| {
                    let title = manifest.title().to_owned();
                    self.scene
                        .rules
                        .extensions
                        .install(manifest)
                        .map(|changed| (title, changed))
                });
                self.status = match result {
                    Ok((title, true)) => {
                        self.changed();
                        format!("«{title}» добавлено в проект.")
                    }
                    Ok((title, false)) => format!("«{title}» уже есть в проекте."),
                    Err(error) => error,
                };
            }
            let mut lighting = None;
            for extension in self.scene.rules.extensions.manifests() {
                ui.label(extension.title());
                match extension.capability() {
                    Capability::MultiplyCounter { label, factor } => {
                        ui.weak(format!("Действие в правилах · {label} × {factor}"));
                    }
                    Capability::SetLighting {
                        label,
                        daylight,
                        interior_gain,
                    } => {
                        let target = Lighting {
                            daylight: *daylight,
                            interior_gain: *interior_gain,
                            ground_tint: self.scene.lighting.ground_tint,
                        };
                        if ui
                            .add_enabled(
                                self.scene.lighting != target,
                                egui::Button::new(label.as_str()),
                            )
                            .clicked()
                        {
                            lighting = Some((target, label.as_str()));
                        }
                    }
                }
            }
            if let Some((lighting, label)) = lighting {
                self.status = format!("«{label}» применено.");
                self.scene.lighting = lighting;
                self.changed();
            }
        });
    }
}
