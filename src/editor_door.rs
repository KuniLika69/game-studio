use eframe::egui;

use crate::{EditorTab, Studio, buildings::Surface, interior, rules, world};

impl Studio {
    pub(super) fn select_door(&mut self, id: u64) {
        if let Some(house) = self
            .scene
            .world
            .buildings
            .iter()
            .find(|house| house.doors().any(|door| door.id == id))
        {
            self.editor_tab = EditorTab::World;
            self.world_selection = Some(house.id);
            self.selected_door = Some(id);
            self.focus_camera();
        }
    }

    pub(super) fn door_choices(&mut self, ui: &mut egui::Ui) {
        let mut selected = None;
        if self
            .scene
            .world
            .buildings
            .iter()
            .any(|house| house.doors().next().is_some())
        {
            ui.collapsing("Двери сцены", |ui| {
                for house in &self.scene.world.buildings {
                    for door in house.doors() {
                        if ui
                            .selectable_label(
                                self.selected_door == Some(door.id)
                                    && self.world_selection == Some(house.id),
                                format!("{} · {}", house.name, door.name),
                            )
                            .clicked()
                        {
                            selected = Some(door.id);
                        }
                    }
                }
            });
        }
        if let Some(id) = selected {
            self.select_door(id);
        }
    }

    pub(super) fn door_properties(&mut self, ui: &mut egui::Ui) -> bool {
        let (Some(id), Some(house_id)) = (self.selected_door, self.world_selection) else {
            return false;
        };
        let Some(house) = self
            .scene
            .world
            .buildings
            .iter()
            .find(|house| house.id == house_id)
        else {
            return false;
        };
        let Some(door) = house.doors().find(|door| door.id == id) else {
            return false;
        };
        ui.heading(door.name);
        if ui.small_button("К настройкам проёма").clicked() {
            self.selected_door = None;
            return false;
        }
        ui.label(format!(
            "Створка · {:.2} × {:.2} м",
            door.width, door.height
        ));
        let cells = door.colliders(0.0).count();
        ui.label(format!("Коллизия створки · {cells} объёмов"));
        let [x, y, z] = door.interaction_position();
        ui.label(format!("Точка взаимодействия · {x:.2}, {y:.2}, {z:.2} м"));
        let lever = door.lever(0.0, 1.0);
        ui.label(format!(
            "Ручка · {:.2}, {:.2}, {:.2} м",
            lever.x, lever.y, lever.z
        ));
        let front = house.front_door().is_some_and(|front| front.id == id);
        let keys = &self.scene.world.keys;
        let house = self
            .scene
            .world
            .buildings
            .iter_mut()
            .find(|house| house.id == house_id)
            .expect("selected building");
        let settings = if front {
            Some(&mut house.door)
        } else {
            house
                .interior
                .partitions
                .iter_mut()
                .find(|wall| interior::part_id(house_id, wall.id, 12, Surface::Wood) == id)
                .and_then(|wall| wall.opening.as_mut())
                .map(|opening| &mut opening.door)
        };
        if let Some(settings) = settings
            && ui
                .add_enabled_ui(!self.game_running, |ui| {
                    world::configure_door(ui, settings, keys)
                })
                .inner
        {
            self.changed();
        }
        for rule in &self.scene.rules.rules {
            if (rule.event == Some(rules::Event::Door { door_id: id })
                || rule
                    .actions
                    .contains(&rules::Action::UnlockDoor { door_id: id }))
                && ui.button(format!("Правило · {}", rule.name)).clicked()
            {
                self.editor_tab = EditorTab::Rules;
                self.rule_editor.select_rule(rule.id);
            }
        }
        true
    }
}
