use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Scene, Studio, View, asset_sources,
    assets::{Library, MAX_PLACEMENTS, Placement},
    environment::Environment,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Kind {
    #[default]
    Cottage,
    Blossom,
    Stones,
    Mushroom,
    Honey,
    Block,
    GardenPath,
    Meadow,
}

impl Kind {
    const ALL: [Self; 8] = [
        Self::Cottage,
        Self::Blossom,
        Self::Stones,
        Self::Mushroom,
        Self::Honey,
        Self::Block,
        Self::GardenPath,
        Self::Meadow,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Cottage => "Домик с черепицей",
            Self::Blossom => "Цветущее дерево",
            Self::Stones => "Камни со мхом",
            Self::Mushroom => "Лесной гриб",
            Self::Honey => "Бочонок с мёдом",
            Self::Block => "Керамический кирпичик",
            Self::GardenPath => "Садовая дорожка",
            Self::Meadow => "Лесная полянка",
        }
    }

    fn bytes(self) -> &'static [u8] {
        macro_rules! glb {
            ($name:literal) => {
                include_bytes!(concat!("../demo_assets/storybook/", $name, ".glb"))
            };
        }
        match self {
            Self::Cottage => glb!("cottage"),
            Self::Blossom => glb!("blossom"),
            Self::Stones => glb!("stones"),
            Self::Mushroom => glb!("mushroom"),
            Self::Honey => glb!("honey"),
            Self::Block => glb!("block"),
            Self::GardenPath => glb!("path"),
            Self::Meadow => glb!("meadow"),
        }
    }

    fn extent(self) -> f32 {
        match self {
            Self::Cottage => 4.6,
            Self::Blossom => 3.55,
            Self::Stones => 1.8,
            Self::Mushroom => 1.05,
            Self::Honey => 1.25,
            Self::Block => 0.60,
            Self::GardenPath => 3.3,
            Self::Meadow => 15.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
enum Palette {
    #[default]
    Dawn,
    Berry,
    Mint,
}

impl Palette {
    fn label(self) -> &'static str {
        match self {
            Self::Dawn => "Персиковый рассвет",
            Self::Berry => "Ягодные сумерки",
            Self::Mint => "Мятный сад",
        }
    }

    fn corrections(self, channel: usize) -> [f32; 3] {
        match self {
            Self::Dawn => [[1.14, 1.03, 0.89], [1.03, 0.89, 0.83], [1.02, 1.05, 0.84]][channel],
            Self::Berry => [[0.94, 0.70, 1.22], [0.88, 0.76, 1.23], [0.88, 0.94, 1.05]][channel],
            Self::Mint => [[0.71, 1.24, 1.03], [0.76, 1.18, 1.16], [0.76, 1.14, 0.96]][channel],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct Recipe {
    kind: Kind,
    palette: Palette,
    seed: u32,
}

#[derive(Clone, Copy)]
struct Draft {
    recipe: Recipe,
    scale: f32,
    position: [f32; 2],
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            recipe: Recipe {
                seed: 23,
                ..Recipe::default()
            },
            scale: 1.0,
            position: [0.0, -5.0],
        }
    }
}

fn split(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let len = u32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?) as usize;
    if bytes.get(0..4)? != b"glTF" || bytes.get(16..20)? != b"JSON" {
        return None;
    }
    let end = 20_usize.checked_add(len)?;
    Some((bytes.get(20..end)?, bytes.get(end..)?))
}

fn recipe_from_bytes(bytes: &[u8]) -> Option<Recipe> {
    let (json, _) = split(bytes)?;
    let document: Value = serde_json::from_slice(json).ok()?;
    let data = document.pointer("/asset/extras/studio_storybook_v1")?;
    serde_json::from_value(data.clone()).ok()
}

fn variation(seed: u32, salt: u32) -> f32 {
    let mut state = seed.wrapping_add(salt.wrapping_mul(0x9e37_79b9));
    state ^= state >> 16;
    state = state.wrapping_mul(0x7feb_352d);
    state ^= state >> 15;
    (state & 65_535) as f32 / 65_535.0
}

fn build(recipe: Recipe) -> Result<Vec<u8>, String> {
    let (source, tail) =
        split(recipe.kind.bytes()).ok_or("Не загрузился шаблон сказочной модели.")?;
    let mut json: Value = serde_json::from_slice(source).map_err(|error| error.to_string())?;
    json["asset"]["extras"]["studio_storybook_v1"] =
        serde_json::to_value(recipe).map_err(|error| error.to_string())?;
    if let Some(materials) = json["materials"].as_array_mut() {
        for (i, material) in materials.iter_mut().enumerate() {
            let channel = material["name"].as_str().and_then(|name| {
                ["Palette Main", "Palette Trim", "Palette Flora"]
                    .iter()
                    .position(|prefix| name.starts_with(prefix))
            });
            if let Some(channel) = channel
                && let Some(base) = material
                    .pointer_mut("/pbrMetallicRoughness/baseColorFactor")
                    .and_then(Value::as_array_mut)
            {
                let correction = recipe.palette.corrections(channel);
                let shade = 0.89 + 0.17 * variation(recipe.seed, (i + 11) as u32);
                for (channel, scale) in base.iter_mut().zip(correction) {
                    if let Some(value) = channel.as_f64() {
                        *channel = Value::from((value * f64::from(scale * shade)).clamp(0.0, 1.0));
                    }
                }
            }
        }
    }
    if matches!(recipe.kind, Kind::Blossom | Kind::Stones | Kind::Mushroom) {
        json["nodes"][0]["scale"] = serde_json::json!([
            0.92 + 0.16 * variation(recipe.seed, 2),
            0.92 + 0.16 * variation(recipe.seed, 3),
            0.92 + 0.16 * variation(recipe.seed, 4)
        ]);
    }
    let mut json = serde_json::to_vec(&json).map_err(|error| error.to_string())?;
    json.resize(json.len().next_multiple_of(4), b' ');
    let total = 20 + json.len() + tail.len();
    let mut bytes = Vec::with_capacity(total);
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"JSON");
    bytes.extend_from_slice(&json);
    bytes.extend_from_slice(tail);
    Ok(bytes)
}

fn generate_placement(
    existing: &[Placement],
    assets: &mut Library,
    recipe: Recipe,
    position: [f32; 2],
    scale: f32,
    yaw: f32,
) -> Result<Placement, String> {
    let source = assets.import_bytes(build(recipe)?)?;
    let model = &assets
        .get(&source)
        .ok_or("Не загрузилась сказочная модель.")?
        .model;
    let mut placement = Placement::at_ground(
        existing,
        model,
        source,
        recipe.kind.label().into(),
        [position[0], 0.0, position[1]],
        recipe.kind.extent() * scale,
        yaw,
    );
    placement.solid = matches!(
        recipe.kind,
        Kind::Cottage | Kind::Blossom | Kind::Stones | Kind::Block
    );
    if matches!(recipe.kind, Kind::GardenPath | Kind::Meadow) {
        placement.paw_ground = true;
    } else if recipe.kind == Kind::Blossom {
        let size = placement.scale[0];
        placement.paw_ground = false;
        placement.material.wind_response = 0.36;
        placement.collider = Some(crate::interior::ModelCollider {
            center: [0.0, size * 0.74, 0.0],
            size: [size * 0.48, size * 1.48, size * 0.45],
        });
    }
    Ok(placement)
}

fn compose(
    scene: &mut Scene,
    assets: &mut Library,
    center: [f32; 2],
    palette: Palette,
    seed: u32,
) -> Result<Vec<u64>, String> {
    let arrangement = [
        (Kind::Cottage, [-2.65, -4.8], 1.0, 19.0, palette),
        (Kind::Cottage, [3.53, -7.5], 0.89, -16.0, Palette::Mint),
        (Kind::Blossom, [3.05, -2.6], 1.17, -10.0, palette),
        (Kind::Blossom, [-4.45, -7.7], 1.03, 149.0, Palette::Berry),
        (Kind::Stones, [3.6, -0.95], 0.79, 14.0, palette),
        (Kind::Mushroom, [-1.55, -1.14], 0.91, 6.0, Palette::Berry),
        (Kind::Honey, [-1.1, -3.49], 0.82, 15.0, palette),
        (Kind::Block, [1.93, -1.54], 0.77, -19.0, palette),
        (Kind::GardenPath, [0.45, -3.0], 1.12, -2.0, palette),
        (Kind::GardenPath, [0.52, -6.2], 1.05, 7.0, palette),
        (Kind::Meadow, [-0.2, -3.45], 1.0, 0.0, palette),
    ];
    if scene.world.models.len() + arrangement.len() > MAX_PLACEMENTS {
        return Err(format!(
            "Для сказочной поляны нужно свободных мест: {}.",
            arrangement.len()
        ));
    }
    let mut stage = assets.clone();
    let mut placements = scene.world.models.clone();
    let mut result = Vec::new();
    for (index, (kind, offset, scale, yaw, palette)) in arrangement.into_iter().enumerate() {
        let model = generate_placement(
            &placements,
            &mut stage,
            Recipe {
                kind,
                palette,
                seed: seed.wrapping_add(index as u32 * 11),
            },
            [center[0] + offset[0], center[1] + offset[1]],
            scale,
            yaw,
        )?;
        result.push(model.id);
        placements.push(model);
    }
    Placement::validate_all(&placements)?;
    scene.world.models = placements;
    scene.lighting.ground_tint.get_or_insert([0.32, 0.43, 0.25]);
    scene.environment.get_or_insert_with(|| {
        let mut environment = Environment::default();
        environment.time.start_hour = 15.0;
        environment.time.noon_bearing = 145.0;
        environment
    });
    *assets = stage;
    Ok(result)
}

pub(super) fn compose_scene(
    scene: &mut Scene,
    assets: &mut Library,
    center: [f32; 2],
) -> Result<Vec<u64>, String> {
    compose(
        scene,
        assets,
        center,
        Palette::Dawn,
        Draft::default().recipe.seed,
    )
}

impl Studio {
    pub(super) fn storybook_tools(&mut self, ui: &mut egui::Ui) {
        if self.scene.map_2d.is_some() {
            return;
        }
        ui.collapsing("Сказочная мастерская", |ui| {
            let id = ui.id().with("storybook_draft");
            let mut draft = ui
                .data(|memory| memory.get_temp::<Draft>(id))
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("storybook_kind")
                .selected_text(draft.recipe.kind.label())
                .show_ui(ui, |ui| {
                    for kind in Kind::ALL {
                        ui.selectable_value(&mut draft.recipe.kind, kind, kind.label());
                    }
                });
            recipe_controls(ui, &mut draft.recipe);
            ui.add(egui::Slider::new(&mut draft.scale, 0.35..=2.5).text("Размер объекта"));
            ui.horizontal_wrapped(|ui| {
                for (coordinate, caption) in draft.position.iter_mut().zip(["X ", "Z "]) {
                    ui.add(
                        egui::DragValue::new(coordinate)
                            .prefix(caption)
                            .range(-80.0..=80.0)
                            .speed(0.15),
                    );
                }
            });
            let available = self.scene.world.models.len() < MAX_PLACEMENTS;
            if ui
                .add_enabled(available, egui::Button::new("Добавить сказочный объект"))
                .clicked()
            {
                let mut staged = self.assets.clone();
                match generate_placement(
                    &self.scene.world.models,
                    &mut staged,
                    draft.recipe,
                    draft.position,
                    draft.scale,
                    0.0,
                ) {
                    Ok(placement) => {
                        let id = placement.id;
                        self.scene.world.models.push(placement);
                        self.assets = staged;
                        self.storybook_created(Some(id), draft.recipe.kind.label());
                    }
                    Err(error) => self.status = error,
                }
            }
            if ui.button("Разложить сказочную поляну").clicked() {
                let result = if draft.recipe.palette == Palette::Dawn
                    && draft.recipe.seed == Draft::default().recipe.seed
                {
                    compose_scene(&mut self.scene, &mut self.assets, draft.position)
                } else {
                    compose(
                        &mut self.scene,
                        &mut self.assets,
                        draft.position,
                        draft.recipe.palette,
                        draft.recipe.seed,
                    )
                };
                match result {
                    Ok(ids) => self.storybook_created(ids.first().copied(), "Сказочная поляна"),
                    Err(error) => self.status = error,
                }
            }
            ui.data_mut(|memory| memory.insert_temp(id, draft));
            ui.push_id("placed_recipe", |ui| self.storybook_selected(ui));
        });
    }

    fn storybook_created(&mut self, id: Option<u64>, caption: &str) {
        self.gizmo.select(id, false, &mut self.world_selection);
        self.view = View::Three;
        self.focus_camera();
        self.status = format!("Добавлено · {caption}");
        self.changed();
    }

    fn storybook_selected(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.world_selection else {
            return;
        };
        let Some(index) = self
            .scene
            .world
            .models
            .iter()
            .position(|placement| placement.id == id)
        else {
            return;
        };
        let item = &self.scene.world.models[index];
        let Some(bytes) = self.assets.export_glb(&item.source) else {
            return;
        };
        let Some(current) = recipe_from_bytes(bytes) else {
            return;
        };
        ui.separator();
        ui.label(format!("Выбрано · {}", item.label));
        let key = ui.id().with((id, "storybook_selected"));
        let mut recipe = ui
            .data(|memory| memory.get_temp::<Recipe>(key))
            .unwrap_or(current);
        recipe.kind = current.kind;
        recipe_controls(ui, &mut recipe);
        let apply = ui
            .add_enabled(
                item.surface.is_none() && recipe != current,
                egui::Button::new("Обновить выбранный объект"),
            )
            .on_disabled_hover_text(
                "Если у объекта заменена поверхность, сначала сбрось её в инспекторе.",
            )
            .clicked();
        ui.data_mut(|memory| memory.insert_temp(key, recipe));
        if apply {
            match self.storybook_replace(index, recipe) {
                Ok(()) => {
                    self.status = "Выбранная сказочная модель обновлена.".into();
                    self.changed();
                }
                Err(error) => self.status = error,
            }
        }
    }

    fn storybook_replace(&mut self, index: usize, recipe: Recipe) -> Result<(), String> {
        let mut staged = self.assets.clone();
        let source = staged.import_bytes(build(recipe)?)?;
        let item = &mut self.scene.world.models[index];
        let previous = staged.get(&item.source).map(|data| data.model.clone());
        let next = &staged
            .get(&source)
            .ok_or("Не загрузилась новая модель.")?
            .model;
        item.replace_visual(source, previous.as_deref(), next);
        staged.retain(asset_sources(&self.scene).map(|(source, _)| source));
        self.assets = staged;
        Ok(())
    }
}

fn recipe_controls(ui: &mut egui::Ui, recipe: &mut Recipe) {
    egui::ComboBox::from_id_salt("storybook_palette")
        .selected_text(recipe.palette.label())
        .show_ui(ui, |ui| {
            for palette in [Palette::Dawn, Palette::Berry, Palette::Mint] {
                ui.selectable_value(&mut recipe.palette, palette, palette.label());
            }
        });
    ui.add(
        egui::DragValue::new(&mut recipe.seed)
            .prefix("Вариант ")
            .range(0..=999_999),
    )
    .on_hover_text("Одно число всегда даёт одинаковые оттенки и пропорции.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storybook_models_reopen_recipes_and_compose_in_a_standard_scene() {
        let mut library = Library::default();
        for kind in Kind::ALL {
            let settings = Recipe {
                kind,
                palette: Palette::Mint,
                seed: 731,
            };
            let bytes = build(settings).unwrap();
            assert_eq!(recipe_from_bytes(&bytes), Some(settings));
            assert_eq!(bytes, build(settings).unwrap());
            let source = library.import_bytes(bytes).unwrap();
            let mesh = &library.get(&source).unwrap().model;
            assert!(mesh.primitives.len() >= 3);
        }
        let mut scene = Scene::empty();
        let ids = compose_scene(&mut scene, &mut library, [0.0, 0.0]).unwrap();
        assert_eq!(ids.len(), scene.world.models.len());
        assert!(scene.world.models.iter().all(|item| {
            library
                .export_glb(&item.source)
                .and_then(recipe_from_bytes)
                .is_some()
        }));
        scene.validate().unwrap();
    }
}
