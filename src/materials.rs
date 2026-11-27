use eframe::egui;
use serde::{Deserialize, Serialize};
use studio_core::render_settings::BlendMode;

/// A point light belongs to the placed object. Replacing its GLB preserves the light.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PointLight {
    pub offset: [f32; 3],
    pub color: [f32; 3],
    pub radius: f32,
    pub intensity: f32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            offset: [0.0, 0.35, 0.0],
            color: [1.0, 0.79, 0.55],
            radius: 5.0,
            intensity: 1.0,
        }
    }
}

impl PointLight {
    pub fn valid(self) -> bool {
        self.offset
            .iter()
            .all(|value| (-48.0..=48.0).contains(value))
            && self.color.iter().all(|value| (0.0..=1.0).contains(value))
            && (0.4..=32.0).contains(&self.radius)
            && (0.0..=6.0).contains(&self.intensity)
    }
}

pub fn light_inspector(ui: &mut egui::Ui, light: &mut Option<PointLight>) {
    ui.collapsing("Точечный свет", |ui| {
        let mut enabled = light.is_some();
        if ui.checkbox(&mut enabled, "Светильник").changed() {
            *light = enabled.then(PointLight::default);
        }
        if let Some(light) = light {
            ui.add(
                egui::DragValue::new(&mut light.intensity)
                    .prefix("Яркость ")
                    .range(0.0..=6.0)
                    .speed(0.035),
            );
            ui.add(
                egui::DragValue::new(&mut light.radius)
                    .prefix("Радиус света ")
                    .suffix(" м")
                    .range(0.4..=32.0)
                    .speed(0.05),
            );
            ui.label("Цвет света");
            crate::inspector::edit_rgb(ui, &mut light.color);
            ui.label("Смещение от позиции объекта");
            ui.horizontal(|ui| {
                for (axis, value) in ["X ", "Y ", "Z "].into_iter().zip(&mut light.offset) {
                    ui.add(
                        egui::DragValue::new(value)
                            .prefix(axis)
                            .range(-48.0..=48.0)
                            .speed(0.03),
                    );
                }
            });
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tuning {
    pub uv_scale: [f32; 2],
    pub roughness: Option<f32>,
    pub normal_strength: f32,
    #[serde(skip_serializing_if = "still")]
    pub wind_response: f32,
    pub emissive_strength: f32,
    pub emissive_color: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "BlendMode::is_default")]
    pub blend_mode: BlendMode,
}

fn still(value: &f32) -> bool {
    *value == 0.0
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            uv_scale: [1.0; 2],
            roughness: None,
            normal_strength: 1.0,
            wind_response: 0.0,
            emissive_strength: 1.0,
            emissive_color: None,
            blend_mode: BlendMode::Normal,
        }
    }
}

impl Tuning {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn valid(self) -> bool {
        self.uv_scale
            .into_iter()
            .all(|value| (0.05..=64.0).contains(&value))
            && self
                .roughness
                .is_none_or(|value| (0.05..=1.0).contains(&value))
            && (0.0..=3.0).contains(&self.normal_strength)
            && (0.0..=2.0).contains(&self.wind_response)
            && (0.0..=12.0).contains(&self.emissive_strength)
            && self
                .emissive_color
                .is_none_or(|color| color.into_iter().all(|value| (0.0..=1.0).contains(&value)))
    }
}

pub fn inspector(ui: &mut egui::Ui, tuning: &mut Tuning) {
    ui.collapsing("Материалы GLB", |ui| {
        blend_selector(ui, &mut tuning.blend_mode);
        ui.label("Повтор текстур");
        ui.horizontal(|ui| {
            for (axis, value) in ["U ", "V "].into_iter().zip(&mut tuning.uv_scale) {
                ui.add(
                    egui::DragValue::new(value)
                        .prefix(axis)
                        .speed(0.05)
                        .range(0.05..=64.0),
                );
            }
        });
        let mut override_roughness = tuning.roughness.is_some();
        if ui
            .checkbox(&mut override_roughness, "Задать шероховатость")
            .changed()
        {
            tuning.roughness = override_roughness.then_some(0.55);
        }
        if let Some(roughness) = &mut tuning.roughness {
            ui.add(
                egui::DragValue::new(roughness)
                    .prefix("Шероховатость ")
                    .range(0.05..=1.0)
                    .speed(0.01),
            );
        }
        ui.add(
            egui::DragValue::new(&mut tuning.normal_strength)
                .prefix("Сила карты нормалей ")
                .range(0.0..=3.0)
                .speed(0.025),
        );
        ui.add(egui::Slider::new(&mut tuning.wind_response, 0.0..=2.0).text("Изгиб от ветра"))
            .on_hover_text("Для травы, кустов и деревьев. Нижняя часть модели закреплена. Столкновения сохраняют исходную форму.");
        ui.add(
            egui::DragValue::new(&mut tuning.emissive_strength)
                .prefix("Свечение ")
                .range(0.0..=12.0)
                .speed(0.04),
        );
        let mut override_emission = tuning.emissive_color.is_some();
        if ui
            .checkbox(&mut override_emission, "Задать цвет свечения")
            .changed()
        {
            tuning.emissive_color = override_emission.then_some([0.35, 0.24, 0.11]);
        }
        if let Some(color) = &mut tuning.emissive_color {
            crate::inspector::edit_rgb(ui, color);
        }
        if ui.button("Сбросить материал").clicked() {
            *tuning = Tuning::default();
        }
    });
}

pub(crate) fn blend_selector(ui: &mut egui::Ui, mode: &mut BlendMode) {
    fn label(mode: BlendMode) -> &'static str {
        match mode {
            BlendMode::Normal => "Обычный (Normal)",
            BlendMode::Add => "Сложение (Add)",
            BlendMode::Multiply => "Умножение (Multiply)",
            BlendMode::Screen => "Экран (Screen)",
        }
    }

    let response = egui::ComboBox::from_label("Смешивание прозрачности")
        .selected_text(label(*mode))
        .show_ui(ui, |ui| {
            for option in BlendMode::ALL {
                ui.selectable_value(mode, option, label(option));
            }
        })
        .response;
    response.on_hover_text("Режим наложения полупрозрачной поверхности в 3D.");
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lighting {
    pub daylight: f32,
    pub interior_gain: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground_tint: Option<[f32; 3]>,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            daylight: 1.0,
            interior_gain: 1.0,
            ground_tint: None,
        }
    }
}

impl Lighting {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn valid(self) -> bool {
        (0.15..=2.5).contains(&self.daylight) && (0.0..=3.0).contains(&self.interior_gain)
            && self.ground_tint.is_none_or(|rgb| rgb.into_iter().all(|channel| (0.0..=1.0).contains(&channel)))
    }
}

pub fn lighting_inspector(ui: &mut egui::Ui, lighting: &mut Lighting) -> bool {
    let before = *lighting;
    ui.collapsing("Освещение сцены", |ui| {
        ui.add(
            egui::DragValue::new(&mut lighting.daylight)
                .prefix("Дневной свет ")
                .range(0.15..=2.5)
                .speed(0.025),
        );
        ui.add(
            egui::DragValue::new(&mut lighting.interior_gain)
                .prefix("Свет внутри ")
                .range(0.0..=3.0)
                .speed(0.025),
        );
        let mut custom_ground = lighting.ground_tint.is_some();
        if ui.checkbox(&mut custom_ground, "Основной цвет земли").changed() {
            lighting.ground_tint = custom_ground.then_some([0.23, 0.25, 0.25]);
        }
        if let Some(ground) = &mut lighting.ground_tint {
            crate::inspector::edit_rgb(ui, ground);
        }
        if ui.button("Сбросить освещение").clicked() {
            *lighting = Lighting::default();
        }
    });
    before != *lighting
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_material_blends_preserve_old_settings_and_roundtrip() -> serde_json::Result<()> {
        let legacy = serde_json::json!({"roughness": 0.4});
        let mut tuning: Tuning = serde_json::from_value(legacy)?;
        assert_eq!(tuning.blend_mode, BlendMode::Normal);
        assert!(serde_json::to_value(tuning)?.get("blend_mode").is_none());

        for mode in BlendMode::ALL {
            tuning.blend_mode = mode;
            let reopened: Tuning = serde_json::from_value(serde_json::to_value(tuning)?)?;
            assert_eq!(reopened, tuning);
            assert!(reopened.valid());
        }
        Ok(())
    }
}
