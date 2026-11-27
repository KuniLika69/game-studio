use crate::{Part, Shape, Studio, scene::PartMotion};
use eframe::egui;
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Rotor {
    pub position: [f32; 3],
    pub radius: f32,
    pub response: f32,
    pub enabled_by: Option<u32>,
}

impl Default for Rotor {
    fn default() -> Self {
        Self {
            position: [2.7, 1.65, 0.9],
            radius: 0.87,
            response: 0.8,
            enabled_by: None,
        }
    }
}

impl Rotor {
    pub fn valid(&self) -> bool {
        self.position.iter().all(|v| (-256.0..=256.0).contains(v))
            && (0.1..=12.0).contains(&self.radius)
            && (0.1..=8.0).contains(&self.response)
    }

    pub fn parts(&self, radians: f32) -> impl Iterator<Item = Part> {
        let center = Vec3::from_array(self.position);
        let blade = self.radius;
        let part = move |id, position, size, rotation, color| Part {
            id,
            name: "Мельница".into(),
            shape: Shape::Box,
            position,
            size,
            rotation,
            color,
            motion: PartMotion::Static,
        };
        let stand = part(
            u64::MAX - 20,
            [center.x, center.y * 0.5, center.z - 0.12],
            [0.22, center.y.max(0.15), 0.24],
            [0.0; 3],
            [0.22, 0.15, 0.11],
        );
        let hub = part(
            u64::MAX - 21,
            self.position,
            [0.23; 3],
            [0.0; 3],
            [0.74, 0.53, 0.26],
        );
        [stand, hub].into_iter().chain((0..4).map(move |index| {
            let angle = radians + index as f32 * FRAC_PI_2;
            let position = center + Vec3::new(angle.cos(), angle.sin(), 0.0) * (blade * 0.55);
            part(
                u64::MAX - 22 - index,
                position.to_array(),
                [blade * 0.85, blade * 0.22, 0.07],
                [0.0, 0.0, angle.to_degrees()],
                [0.80, 0.64, 0.36],
            )
        }))
    }
}

impl Studio {
    pub(super) fn rotor_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.collapsing("Ветряная мельница", |ui| {
            let mut enabled = self.scene.wind_rotor.is_some();
            if ui.checkbox(&mut enabled, "Мельница от ветра").changed() {
                self.scene.wind_rotor = enabled.then(Rotor::default);
                changed = true;
            }
            let Some(rotor) = &mut self.scene.wind_rotor else {
                return;
            };
            ui.horizontal_wrapped(|ui| {
                for (axis, value) in ["X ", "Y ", "Z "].into_iter().zip(&mut rotor.position) {
                    changed |= ui
                        .add(
                            egui::DragValue::new(value)
                                .prefix(axis)
                                .speed(0.05)
                                .range(-256.0..=256.0),
                        )
                        .changed();
                }
            });
            changed |= ui
                .add(
                    egui::DragValue::new(&mut rotor.radius)
                        .prefix("Радиус ")
                        .speed(0.05)
                        .range(0.1..=12.0),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut rotor.response)
                        .prefix("Отклик на ветер ")
                        .speed(0.025)
                        .range(0.1..=8.0),
                )
                .changed();
            let title = rotor
                .enabled_by
                .and_then(|id| self.scene.rules.flag(id))
                .map_or("Любой ветер", |flag| flag.name.as_str());
            egui::ComboBox::from_id_salt("wind_rotor_gate")
                .selected_text(title)
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut rotor.enabled_by, None, "Любой ветер")
                        .changed();
                    for flag in &self.scene.rules.flags {
                        changed |= ui
                            .selectable_value(&mut rotor.enabled_by, Some(flag.id), &flag.name)
                            .changed();
                    }
                });
        });
        changed
    }

    pub(super) fn rotor_step(&mut self, dt: f32) -> bool {
        let Some(rotor) = &self.scene.wind_rotor else {
            return false;
        };
        if rotor
            .enabled_by
            .is_some_and(|id| self.rule_session.flag(&self.scene.rules, id) != Some(true))
        {
            return false;
        }
        let wind = self.environment_wind(Vec3::from_array(rotor.position));
        let next = self.rotor_angle + (wind.length() * rotor.response * dt).max(0.0);
        self.rotor_angle = next.rem_euclid(TAU);
        next >= TAU
    }
}
