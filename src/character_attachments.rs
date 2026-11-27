use std::path::Path;

use eframe::egui;
use glam::Vec3;

use crate::{
    Studio, asset_sources,
    assets::Library,
    avatar::{Avatar, BoneAttachment, MAX_BONE_ATTACHMENTS},
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Target {
    Player,
    Npc(u64),
}

impl Studio {
    pub(super) fn attachment_button(&mut self, ui: &mut egui::Ui, target: Target) {
        if ui
            .add_enabled(!self.game_running, egui::Button::new("Детали на костях…"))
            .clicked()
        {
            self.attachment_target = Some(target);
        }
    }

    pub(super) fn attachment_window(&mut self, ctx: &egui::Context) {
        let Some(target) = self.attachment_target.filter(|_| !self.game_running) else {
            return;
        };
        let Some(mut avatar) = self.attachment_avatar(target).cloned() else {
            self.attachment_target = None;
            return;
        };
        let bones: Vec<String> = self
            .assets
            .get(&avatar.source)
            .map_or_else(Vec::new, |asset| {
                asset
                    .model
                    .attachment_bones()
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            });
        let id = egui::Id::new(("attach_to_bone", target));
        let mut bone = ctx
            .data(|data| data.get_temp::<String>(id))
            .unwrap_or_default();
        if !bones.contains(&bone) {
            bone = bones
                .iter()
                .find(|name| name.to_ascii_lowercase().contains("head"))
                .or(bones.first())
                .cloned()
                .unwrap_or_default();
        }

        let mut open = true;
        let mut import = false;
        let mut changed = false;
        let mut remove = None;
        egui::Window::new(format!("Детали на костях · {}", avatar.name))
            .id(egui::Id::new("character_attachments"))
            .default_width(390.0)
            .default_pos(egui::pos2(225.0, 105.0))
            .open(&mut open)
            .show(ctx, |ui| {
                if bones.is_empty() {
                    ui.label("У этой модели нет доступных именованных костей.");
                    return;
                }
                ui.label("Точка крепления новой детали");
                bone_picker(ui, "new_attachment_bone", &bones, &mut bone);
                import = ui
                    .add_enabled(
                        avatar.bone_attachments.len() < MAX_BONE_ATTACHMENTS,
                        egui::Button::new("Добавить деталь GLB…"),
                    )
                    .clicked();
                ui.weak("Начало координат GLB ставится на кость. Деталь повторяет её движения.");
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(355.0)
                    .show(ui, |ui| {
                        for (index, part) in avatar.bone_attachments.iter_mut().enumerate() {
                            ui.push_id(index, |ui| {
                                ui.collapsing(&part.name, |ui| {
                                    changed |= bone_picker(ui, "anchor", &bones, &mut part.bone);
                                    changed |= axes(
                                        ui,
                                        "Смещение по осям кости",
                                        &mut part.offset,
                                        -30.0..=30.0,
                                        0.005,
                                    );
                                    changed |= axes(
                                        ui,
                                        "Поворот, °",
                                        &mut part.rotation,
                                        -3600.0..=3600.0,
                                        1.0,
                                    );
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut part.scale)
                                                .range(0.01..=50.0)
                                                .speed(0.01)
                                                .prefix("Масштаб "),
                                        )
                                        .changed();
                                    if ui.small_button("Снять деталь").clicked() {
                                        remove = Some(index);
                                    }
                                });
                            });
                        }
                    });
                ui.weak(&self.status);
            });
        ctx.data_mut(|data| data.insert_temp(id, bone.clone()));
        if !open {
            self.attachment_target = None;
        }
        if let Some(index) = remove {
            avatar.bone_attachments.remove(index);
            changed = true;
        }
        if import && let Some(path) = self.glb_path() {
            let mut library = self.assets.clone();
            self.status = prepare(&mut library, &mut avatar, &bone, &path)
                .and_then(|()| self.apply_bone_attachments(target, avatar, library))
                .map(|()| "Деталь закреплена на кости.".into())
                .unwrap_or_else(|err| err);
        } else if changed {
            self.status = self
                .apply_bone_attachments(target, avatar, self.assets.clone())
                .map(|()| "Детали сохранены в персонаже.".into())
                .unwrap_or_else(|err| err);
        }
    }

    fn attachment_avatar(&self, target: Target) -> Option<&Avatar> {
        match target {
            Target::Player => self.scene.avatar.as_ref(),
            Target::Npc(id) => self
                .scene
                .npcs
                .iter()
                .find(|npc| npc.id == id)
                .map(|npc| &npc.avatar),
        }
    }

    fn apply_bone_attachments(
        &mut self,
        target: Target,
        avatar: Avatar,
        library: Library,
    ) -> Result<(), String> {
        avatar.validate()?;
        Self::model_from_library(&library, &avatar)?;
        match target {
            Target::Player => self.scene.avatar = Some(avatar),
            Target::Npc(id) => {
                let npc = self
                    .scene
                    .npcs
                    .iter_mut()
                    .find(|npc| npc.id == id)
                    .ok_or("Персонаж удалён.")?;
                npc.avatar = avatar;
            }
        }
        self.assets = library;
        self.assets
            .retain(asset_sources(&self.scene).map(|(source, _)| source));
        match target {
            Target::Player => self.sync_edited_human(),
            Target::Npc(_) => self.sync_npc_assets(),
        }
        self.changed();
        Ok(())
    }
}

fn prepare(
    library: &mut Library,
    avatar: &mut Avatar,
    bone: &str,
    path: &Path,
) -> Result<(), String> {
    let span = |model: &crate::model::Model| {
        (Vec3::from_array(model.bounds_max) - Vec3::from_array(model.bounds_min)).max_element()
    };
    let size = span(
        &library
            .get(&avatar.source)
            .ok_or("Загрузи модель персонажа.")?
            .model,
    );
    let source = library.import(path)?;
    let detail = library.get(&source).ok_or("Не удалось загрузить деталь.")?;
    let name = path
        .file_stem()
        .map(|name| {
            name.to_string_lossy()
                .trim()
                .chars()
                .take(128)
                .collect::<String>()
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Деталь".into());
    avatar.bone_attachments.push(BoneAttachment {
        source,
        name,
        bone: bone.into(),
        offset: [0.0; 3],
        rotation: [0.0; 3],
        scale: (size * 0.13 / span(&detail.model).max(0.0001)).clamp(0.01, 50.0),
    });
    Ok(())
}

fn bone_picker(
    ui: &mut egui::Ui,
    key: &'static str,
    choices: &[String],
    bone: &mut String,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(key)
        .width(320.0)
        .height(295.0)
        .selected_text(&*bone)
        .show_ui(ui, |ui| {
            for name in choices {
                if ui.selectable_label(bone == name, name).clicked() && bone != name {
                    bone.clone_from(name);
                    changed = true;
                }
            }
        });
    changed
}

fn axes(
    ui: &mut egui::Ui,
    name: &str,
    values: &mut [f32; 3],
    range: std::ops::RangeInclusive<f32>,
    speed: f32,
) -> bool {
    ui.label(name);
    ui.horizontal_wrapped(|ui| {
        let mut changed = false;
        for (axis, value) in ["X ", "Y ", "Z "].into_iter().zip(values) {
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .range(range.clone())
                        .speed(speed)
                        .prefix(axis),
                )
                .changed();
        }
        changed
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{assets::Source, proof, proof_input, scene::Scene};

    #[test]
    fn imported_detail_is_editable_in_ui_and_retained_as_a_project_asset() {
        let mut studio = Studio::default();
        let hero = studio
            .assets
            .import_bytes(include_bytes!("../demo_assets/world_palette/fox.glb").to_vec())
            .unwrap();
        studio.assign_avatar(hero, "Тестовый персонаж".into());
        let original = studio.avatar_asset().unwrap().0.primitives.len();
        let detail = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo_assets/rain/lamp.glb");
        studio.proof = Some(proof::Environment {
            scene: None,
            export: None,
            map_png: None,
            import_glb: Some(detail),
            character_file: None,
            effect_file: None,
            wav_file: None,
            sprite_canvas: None,
            projection: None,
        });
        let ctx = egui::Context::default();
        let mut input = proof_input::Driver::new([900, 650]);
        let mut frame = |events| {
            ctx.run(input.raw(events), |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| studio.attachment_button(ui, Target::Player));
                studio.attachment_window(ctx);
            })
        };
        let open = proof_input::find_label(&frame(vec![]), "Детали на костях…", 0).unwrap();
        frame(proof_input::click(open));
        for _ in 0..12 {
            frame(vec![]);
        }
        let add = proof_input::find_label(&frame(vec![]), "Добавить деталь GLB…", 0).unwrap();
        frame(proof_input::click(add));
        frame(vec![]);
        let avatar = studio.scene.avatar.as_ref().unwrap();
        assert_eq!(avatar.bone_attachments.len(), 1, "{}", studio.status);
        let part = &avatar.bone_attachments[0];
        assert_eq!(part.bone, "FoxHead");
        assert!(matches!(&part.source, Source::Imported(_)));
        assert!(studio.avatar_asset().unwrap().0.primitives.len() > original);
        let serialized = serde_json::to_vec(&studio.scene).unwrap();
        let reopened: Scene = serde_json::from_slice(&serialized).unwrap();
        reopened.validate().unwrap();
        assert_eq!(
            reopened.avatar.unwrap().bone_attachments,
            avatar.bone_attachments
        );
        assert!(asset_sources(&studio.scene).any(|(source, _)| source == &part.source));
    }
}
