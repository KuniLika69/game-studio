use std::collections::HashSet;

use eframe::egui;

use crate::model::{MAX_MORPH_TARGETS, MORPH_OVERRIDE_RANGE, Model, MorphWeight};

pub(crate) fn valid(shapes: &[MorphWeight]) -> bool {
    let mut names = HashSet::with_capacity(shapes.len().min(MAX_MORPH_TARGETS));
    shapes.len() <= MAX_MORPH_TARGETS
        && shapes.iter().all(|shape| {
            !shape.target.is_empty()
                && shape.target.trim() == shape.target
                && shape.target.len() <= 128
                && !shape.target.chars().any(char::is_control)
                && MORPH_OVERRIDE_RANGE.contains(&shape.weight)
                && names.insert(shape.target.as_str())
        })
}

pub(crate) fn compose(model: &Model, shapes: &[MorphWeight], animated: &mut Vec<MorphWeight>) {
    let names = model.morph_names();
    for shape in shapes {
        if names.binary_search(&shape.target).is_ok()
            && !animated.iter().any(|weight| weight.target == shape.target)
        {
            animated.push(shape.clone());
        }
    }
}

pub(crate) fn inspector(ui: &mut egui::Ui, model: &Model, shapes: &mut Vec<MorphWeight>) -> bool {
    let mut changed = false;
    ui.collapsing("Формы модели", |ui| {
        if model.morph_names().is_empty() {
            ui.weak("Загрузи модель GLB или VRM с формами, чтобы менять их здесь.");
            return;
        }
        ui.weak("Изменения видны в сцене и сохраняются с этим персонажем.");
        let filter_id = ui.id().with("morph_search");
        let mut filter = ui
            .data_mut(|data| data.get_temp::<String>(filter_id))
            .unwrap_or_default();
        ui.add(
            egui::TextEdit::singleline(&mut filter)
                .char_limit(128)
                .hint_text("Поиск формы"),
        );
        let query = filter.trim().to_lowercase();
        ui.data_mut(|data| data.insert_temp(filter_id, filter));

        if ui
            .add_enabled(
                !shapes.is_empty(),
                egui::Button::new("Восстановить формы модели"),
            )
            .clicked()
        {
            shapes.clear();
            changed = true;
        }
        egui::ScrollArea::vertical()
            .id_salt("model_shape_weights")
            .max_height(280.0)
            .show(ui, |ui| {
                let mut matches = false;
                for name in model
                    .morph_names()
                    .iter()
                    .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
                {
                    matches = true;
                    ui.push_id(name, |ui| {
                        ui.label(name);
                        ui.horizontal_wrapped(|ui| {
                            changed |= shape_row(ui, model, name, shapes);
                        });
                    });
                }
                if !matches {
                    ui.weak("Формы с таким именем не найдены.");
                }
            });
    });
    changed
}

fn shape_row(ui: &mut egui::Ui, model: &Model, name: &str, shapes: &mut Vec<MorphWeight>) -> bool {
    let existing = shapes.iter().position(|shape| shape.target == name);
    let mut value = existing.map_or_else(
        || model.morph_rest_weight(name),
        |index| shapes[index].weight,
    );
    let slider = egui::Slider::new(&mut value, MORPH_OVERRIDE_RANGE)
        .clamping(egui::SliderClamping::Edits)
        .fixed_decimals(2);
    let edited = ui.add(slider).on_hover_text(name).changed();
    let reset = ui
        .add_enabled(existing.is_some(), egui::Button::new("Сбросить"))
        .on_hover_text("Восстановить исходный вес этой формы в модели")
        .clicked();
    if let Some(index) = existing {
        if reset {
            shapes.remove(index);
        } else if edited {
            shapes[index].weight = value;
        }
    } else if edited {
        shapes.push(MorphWeight {
            target: name.into(),
            weight: value,
        });
    }
    edited || reset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Avatar, Studio,
        assets::Source,
        npcs::{FIRST_NPC_ID, Npc, Waypoint},
        scene::Scene,
    };

    #[test]
    fn player_and_npc_keep_independent_model_forms_in_the_project_and_playback() {
        let mut studio = Studio::default();
        studio.assets.ensure(&Source::Emma, None).unwrap();
        let model = &studio.assets.get(&Source::Emma).unwrap().model;
        let mut player = Avatar::new(Source::Emma, "Игрок".into(), model);
        let mut npc = Avatar::new(Source::Emma, "Собеседник".into(), model);
        player.shape_weights.push(MorphWeight {
            target: "jawOpen".into(),
            weight: 0.46,
        });
        npc.shape_weights.push(MorphWeight {
            target: "jawOpen".into(),
            weight: -0.25,
        });
        let mut scene = Scene::empty();
        scene.avatar = Some(player);
        scene.npcs.push(Npc::new(
            FIRST_NPC_ID,
            npc,
            vec![Waypoint::at([2.0, 0.0, 0.0])],
        ));
        scene.validate().unwrap();
        let json = serde_json::to_vec(&scene).unwrap();
        let restored: Scene = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored.avatar, scene.avatar);
        assert_eq!(restored.npcs[0].avatar, scene.npcs[0].avatar);
        studio.set_scene(restored, None, "");

        let views = |studio: &Studio| {
            assert_eq!(studio.avatar_draw().unwrap().morph_weights[0].weight, 0.46);
            assert_eq!(studio.npc_draws()[0].morph_weights[0].weight, -0.25);
        };
        views(&studio);
        studio.start_play(&egui::Context::default(), false);
        assert!(studio.game_running);
        views(&studio);

        let player = studio.scene.avatar.as_mut().unwrap();
        player.shape_weights.push(player.shape_weights[0].clone());
        assert!(player.validate().is_err());
    }
}
