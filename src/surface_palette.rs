use std::{borrow::Cow, collections::HashMap};

use eframe::egui;
use gltf::binary::{Glb, Header};
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage, codecs::png::PngEncoder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Studio,
    assets::{Library, Placement, Source},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Family {
    #[default]
    Forest,
    Pebbles,
    Paving,
    Brick,
    Arcane,
}

impl Family {
    const ALL: [Self; 5] = [
        Self::Forest,
        Self::Pebbles,
        Self::Paving,
        Self::Brick,
        Self::Arcane,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Forest => "Лесная почва",
            Self::Pebbles => "Речная галька",
            Self::Paving => "Городская мостовая",
            Self::Brick => "Старый кирпич",
            Self::Arcane => "Зачарованный камень",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Recipe {
    pub family: Family,
    pub seed: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Surface {
    pub recipe: Recipe,
    pub overlay: Source,
}

impl Surface {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !matches!(self.overlay, Source::Imported(_)) {
            return Err("Карты поверхности должны входить в созданный GLB.".into());
        }
        self.overlay.validate()
    }
}

struct ImageMap {
    bytes: Cow<'static, [u8]>,
    mime: &'static str,
}

enum MapKind {
    Color,
    Normal,
    Roughness,
    Emission,
}

impl Recipe {
    fn map(self, kind: MapKind) -> Result<ImageMap, String> {
        macro_rules! scan {
            ($name:literal, $kind:expr) => {{
                let (bytes, mime): (&[u8], &str) = match $kind {
                    MapKind::Color => (
                        include_bytes!(concat!(
                            "../demo_assets/surface_palette/",
                            $name,
                            "-color.jpg"
                        )),
                        "image/jpeg",
                    ),
                    MapKind::Normal => (
                        include_bytes!(concat!(
                            "../demo_assets/surface_palette/",
                            $name,
                            "-normal.jpg"
                        )),
                        "image/jpeg",
                    ),
                    MapKind::Roughness => (
                        include_bytes!(concat!(
                            "../demo_assets/surface_palette/",
                            $name,
                            "-rough.png"
                        )),
                        "image/png",
                    ),
                    MapKind::Emission => unreachable!("only the arcane family emits"),
                };
                Ok(ImageMap {
                    bytes: Cow::Borrowed(bytes),
                    mime,
                })
            }};
        }
        match self.family {
            Family::Forest => scan!("forest", kind),
            Family::Pebbles => scan!("pebbles", kind),
            Family::Paving => scan!("paving", kind),
            Family::Brick => scan!("brick", kind),
            Family::Arcane => Arcane::new(self.seed).image(kind),
        }
    }
}

pub(crate) fn apply(
    item: &mut Placement,
    library: &mut Library,
    recipe: Recipe,
) -> Result<usize, String> {
    let original = library
        .original_glb(&item.source)
        .ok_or("Исходный GLB не загружен.")?;
    let (bytes, count) = resurface(original, recipe)?;
    let overlay = library.import_bytes(bytes)?;
    item.surface = Some(Surface { recipe, overlay });
    Ok(count)
}

fn resurface(original: &[u8], recipe: Recipe) -> Result<(Vec<u8>, usize), String> {
    let original =
        Glb::from_slice(original).map_err(|error| format!("GLB не прочитан ({error})."))?;
    let mut doc: Value = serde_json::from_slice(&original.json)
        .map_err(|error| format!("Структура GLB не прочитана ({error})."))?;
    let materials = doc["materials"].as_array().cloned().unwrap_or_default();
    let mut targets = Vec::new();
    for (mesh_index, mesh) in doc["meshes"].as_array().into_iter().flatten().enumerate() {
        for (index, primitive) in mesh["primitives"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let material_index = primitive["material"].as_u64().map(|value| value as usize);
            let authored = material_index.and_then(|index| materials.get(index));
            if primitive["attributes"]["TEXCOORD_0"].is_number()
                && authored.is_none_or(opaque_surface)
            {
                targets.push((mesh_index, index, material_index));
            }
        }
    }
    if targets.is_empty() {
        return Err("Нужна непрозрачная часть модели с UV0.".into());
    }

    let mut binary = original
        .bin
        .ok_or("В GLB нет двоичных данных.")?
        .into_owned();
    let sampler = append(
        &mut doc,
        "samplers",
        json!({
            "magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497
        }),
    )?;
    let arcane = (recipe.family == Family::Arcane).then(|| Arcane::new(recipe.seed));
    let maps = |kind| match &arcane {
        Some(arcane) => arcane.image(kind),
        None => recipe.map(kind),
    };
    let color = embed(&mut doc, &mut binary, sampler, maps(MapKind::Color)?)?;
    let normal = embed(&mut doc, &mut binary, sampler, maps(MapKind::Normal)?)?;
    let roughness = embed(&mut doc, &mut binary, sampler, maps(MapKind::Roughness)?)?;
    let emission = if arcane.is_some() {
        Some(embed(
            &mut doc,
            &mut binary,
            sampler,
            maps(MapKind::Emission)?,
        )?)
    } else {
        None
    };

    let mut variants = HashMap::new();
    for &(mesh, primitive, authored) in &targets {
        let index = if let Some(&index) = variants.get(&authored) {
            index
        } else {
            let mut material = authored
                .and_then(|id| materials.get(id))
                .cloned()
                .unwrap_or_else(|| json!({}));
            material["pbrMetallicRoughness"] = json!({
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "baseColorTexture": {"index": color},
                "metallicRoughnessTexture": {"index": roughness},
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0
            });
            material["normalTexture"] = json!({"index": normal, "scale": 0.9});
            if let Some(emission) = emission {
                material["emissiveTexture"] = json!({"index": emission});
                material["emissiveFactor"] = json!([0.8, 0.8, 0.8]);
            }
            let index = append(&mut doc, "materials", material)?;
            variants.insert(authored, index);
            index
        };
        doc["meshes"][mesh]["primitives"][primitive]["material"] = json!(index);
    }
    binary.resize(binary.len().next_multiple_of(4), 0);
    doc["buffers"][0]["byteLength"] = json!(binary.len());
    let json = serde_json::to_vec(&doc).map_err(|error| error.to_string())?;
    let bytes = Glb {
        header: Header {
            magic: *b"glTF",
            version: 2,
            length: 0,
        },
        json: Cow::Owned(json),
        bin: Some(Cow::Owned(binary)),
    }
    .to_vec()
    .map_err(|error| error.to_string())?;
    Ok((bytes, targets.len()))
}

fn opaque_surface(material: &Value) -> bool {
    material["alphaMode"].as_str().unwrap_or("OPAQUE") == "OPAQUE"
        && material["pbrMetallicRoughness"]["baseColorFactor"][3]
            .as_f64()
            .is_none_or(|alpha| alpha >= 0.995)
        && material["extensions"]["KHR_materials_unlit"].is_null()
}

fn append(doc: &mut Value, key: &str, value: Value) -> Result<usize, String> {
    if doc[key].is_null() {
        doc[key] = json!([]);
    }
    let array = doc[key]
        .as_array_mut()
        .ok_or_else(|| format!("GLB: некорректный раздел {key}."))?;
    let index = array.len();
    array.push(value);
    Ok(index)
}

fn embed(
    doc: &mut Value,
    binary: &mut Vec<u8>,
    sampler: usize,
    image: ImageMap,
) -> Result<usize, String> {
    binary.resize(binary.len().next_multiple_of(4), 0);
    let offset = binary.len();
    binary.extend_from_slice(&image.bytes);
    let view = append(
        doc,
        "bufferViews",
        json!({
            "buffer": 0, "byteOffset": offset, "byteLength": image.bytes.len()
        }),
    )?;
    let source = append(
        doc,
        "images",
        json!({"bufferView": view, "mimeType": image.mime}),
    )?;
    append(
        doc,
        "textures",
        json!({"sampler": sampler, "source": source}),
    )
}

const ARCANE_SIZE: u32 = 384;
const STONE_GRID: i32 = 7;

struct Arcane {
    seed: u32,
    relief: Vec<f32>,
    veins: Vec<f32>,
    grain: Vec<f32>,
}

impl Arcane {
    fn new(seed: u32) -> Self {
        let mut relief = Vec::with_capacity(ARCANE_SIZE.pow(2) as usize);
        let mut veins = Vec::with_capacity(relief.capacity());
        let mut grain = Vec::with_capacity(relief.capacity());
        for y in 0..ARCANE_SIZE {
            for x in 0..ARCANE_SIZE {
                let scale = STONE_GRID as f32 / ARCANE_SIZE as f32;
                let [u, v] = [x as f32 * scale, y as f32 * scale];
                let low = [u.floor() as i32, v.floor() as i32];
                let mut best = f32::INFINITY;
                let mut second = f32::INFINITY;
                for oy in -1..=1 {
                    for ox in -1..=1 {
                        let [cx, cy] = [low[0] + ox, low[1] + oy];
                        let key = hash(
                            seed,
                            cx.rem_euclid(STONE_GRID) as u32,
                            cy.rem_euclid(STONE_GRID) as u32,
                        );
                        let a = 0.22 + (key & 255) as f32 * (0.56 / 255.0);
                        let b = 0.22 + ((key >> 8) & 255) as f32 * (0.56 / 255.0);
                        let distance = (cx as f32 + a - u).powi(2) + (cy as f32 + b - v).powi(2);
                        if distance < best {
                            second = best;
                            best = distance;
                        } else {
                            second = second.min(distance);
                        }
                    }
                }
                let dust = (hash(seed ^ 0x517c_3841, x, y) >> 24) as f32 / 255.0;
                let edge = (second.sqrt() - best.sqrt()) * (1.0 + (dust - 0.5) * 0.23);
                let seam = (edge * 19.0).clamp(0.0, 1.0);
                let ridge = (1.0 - best.sqrt()).clamp(0.0, 1.0);
                relief.push(seam * (0.71 + 0.26 * ridge) + dust * 0.017);
                veins.push((1.0 - edge * 30.0).clamp(0.0, 1.0).powi(2));
                grain.push(dust);
            }
        }
        Self {
            seed,
            relief,
            veins,
            grain,
        }
    }

    fn image(&self, kind: MapKind) -> Result<ImageMap, String> {
        let get = |x: u32, y: u32| ((y % ARCANE_SIZE) * ARCANE_SIZE + x % ARCANE_SIZE) as usize;
        let blue = self.seed.is_multiple_of(2);
        let palette = if blue {
            [62.0, 63.0, 97.0]
        } else {
            [85.0, 60.0, 76.0]
        };
        let magic = if blue {
            [64.0, 229.0, 255.0]
        } else {
            [251.0, 126.0, 62.0]
        };
        let image = RgbImage::from_fn(ARCANE_SIZE, ARCANE_SIZE, |x, y| {
            let index = get(x, y);
            let stone = self.relief[index];
            let light = self.veins[index];
            let grain = self.grain[index];
            let channels = match kind {
                MapKind::Color => std::array::from_fn(|channel| {
                    let base = palette[channel] * (0.42 + 0.67 * stone) + grain * 13.0;
                    (base + (magic[channel] - base) * light * 0.73).clamp(0.0, 255.0) as u8
                }),
                MapKind::Normal => {
                    let dx = self.relief[get(x + 1, y)] - self.relief[get(x + ARCANE_SIZE - 1, y)];
                    let dy = self.relief[get(x, y + 1)] - self.relief[get(x, y + ARCANE_SIZE - 1)];
                    let normal = glam::Vec3::new(-dx * 1.55, dy * 1.55, 1.0).normalize();
                    normal
                        .to_array()
                        .map(|component| (component * 127.5 + 127.5) as u8)
                }
                MapKind::Roughness => [255, (108.0 + stone * 83.0 + grain * 22.0) as u8, 0],
                MapKind::Emission => magic.map(|channel| (channel * light * 0.86) as u8),
            };
            Rgb(channels)
        });
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(
                image.as_raw(),
                ARCANE_SIZE,
                ARCANE_SIZE,
                ExtendedColorType::Rgb8,
            )
            .map_err(|error| error.to_string())?;
        Ok(ImageMap {
            bytes: Cow::Owned(bytes),
            mime: "image/png",
        })
    }
}

fn hash(seed: u32, x: u32, y: u32) -> u32 {
    let value = seed ^ x.wrapping_mul(0x85eb_ca6b) ^ y.wrapping_mul(0xc2b2_ae35);
    let value = (value ^ (value >> 16)).wrapping_mul(0x7feb_352d);
    (value ^ (value >> 15)).wrapping_mul(0x846c_a68b)
}

#[derive(Clone)]
struct Panel {
    applied: Option<Recipe>,
    draft: Recipe,
    preview: Option<(Recipe, egui::TextureHandle)>,
}

impl Studio {
    pub(crate) fn surface_palette(&mut self, ui: &mut egui::Ui, id: u64) {
        if self.game_running {
            return;
        }
        let Some(index) = self
            .scene
            .world
            .models
            .iter()
            .position(|item| item.id == id)
        else {
            return;
        };
        let applied = self.scene.world.models[index]
            .surface
            .as_ref()
            .map(|surface| surface.recipe);
        ui.push_id(id, |ui| {
            ui.collapsing("Библиотека поверхностей", |ui| {
                let cache = ui.id().with("surface_preview");
                let mut state = ui
                    .data(|data| data.get_temp::<Panel>(cache))
                    .filter(|panel| panel.applied == applied)
                    .unwrap_or_else(|| Panel {
                        applied,
                        draft: applied.unwrap_or_default(),
                        preview: None,
                    });
                egui::ComboBox::from_id_salt("surface_family")
                    .selected_text(state.draft.family.label())
                    .show_ui(ui, |ui| {
                        for family in Family::ALL {
                            ui.selectable_value(&mut state.draft.family, family, family.label());
                        }
                    });
                if state.draft.family == Family::Arcane {
                    ui.add(
                        egui::DragValue::new(&mut state.draft.seed)
                            .range(0..=u32::MAX)
                            .prefix("Зерно узора "),
                    );
                }
                if ui.button("Применить поверхность").clicked() {
                    let item = &mut self.scene.world.models[index];
                    match apply(item, &mut self.assets, state.draft) {
                        Ok(count) => {
                            state.applied = Some(state.draft);
                            self.status = format!(
                                "{} · частей с новой поверхностью: {count}",
                                state.draft.family.label()
                            );
                            self.changed();
                        }
                        Err(error) => {
                            self.status = format!("Не удалось применить поверхность. {error}")
                        }
                    }
                }
                if state.applied.is_some() && ui.button("Вернуть исходную поверхность").clicked()
                {
                    self.scene.world.models[index].surface = None;
                    state.applied = None;
                    self.status = "Исходная поверхность восстановлена.".into();
                    self.changed();
                }
                if !state
                    .preview
                    .as_ref()
                    .is_some_and(|(recipe, _)| *recipe == state.draft)
                {
                    state.preview =
                        preview(ui.ctx(), state.draft).map(|image| (state.draft, image));
                }
                if let Some((_, texture)) = &state.preview {
                    let side = ui.available_width().clamp(72.0, 152.0);
                    ui.add(egui::Image::new((texture.id(), egui::Vec2::splat(side))));
                }
                ui.weak("Цвет, нормали и шероховатость для непрозрачных частей с UV0.");
                ui.data_mut(|data| data.insert_temp(cache, state));
            })
        });
    }
}

fn preview(ctx: &egui::Context, recipe: Recipe) -> Option<egui::TextureHandle> {
    let map = recipe.map(MapKind::Color).ok()?;
    let image = image::load_from_memory(&map.bytes)
        .ok()?
        .thumbnail(256, 256)
        .to_rgba8();
    Some(ctx.load_texture(
        "surface-palette-swatch",
        egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Scene, interior::ModelCollider};

    #[test]
    fn pbr_families_save_on_their_own_overlay_without_replacing_geometry() {
        let source_bytes = include_bytes!("../demo_assets/world_palette/road-natural.glb");
        let mut assets = Library::default();
        let source = assets.import_bytes(source_bytes.to_vec()).unwrap();
        let base = assets.get(&source).unwrap().model.clone();
        let mut item = Placement::at_ground(
            &[],
            &base,
            source.clone(),
            "Тестовая площадка".into(),
            [2.0, 0.0, -2.0],
            5.5,
            17.0,
        );
        item.collider = Some(ModelCollider {
            center: [0.0, 0.5, 0.0],
            size: [2.0, 1.0, 2.0],
        });
        let original = item.clone();
        for family in [
            Family::Forest,
            Family::Paving,
            Family::Brick,
            Family::Arcane,
        ] {
            let recipe = Recipe { family, seed: 666 };
            assert!(apply(&mut item, &mut assets, recipe).unwrap() > 0);
            let overlay = item.render_source().clone();
            assert_ne!(overlay, source);
            let model = &assets.get(&overlay).unwrap().model;
            assert_eq!(base.bounds_min, model.bounds_min);
            assert_eq!(base.bounds_max, model.bounds_max);
            assert_eq!(model.primitives.len(), base.primitives.len());
            for (old, new) in base.primitives.iter().zip(&model.primitives) {
                assert_eq!(old.indices, new.indices);
                assert_eq!(
                    old.vertices.iter().map(|v| v.position).collect::<Vec<_>>(),
                    new.vertices.iter().map(|v| v.position).collect::<Vec<_>>(),
                );
            }
            let change = &model.primitives[0].material;
            for index in [
                change.base_color_texture,
                change.normal_texture,
                change.metallic_roughness_texture,
            ] {
                let image = &model.images[index.unwrap()];
                let size = if family == Family::Arcane {
                    ARCANE_SIZE
                } else {
                    1024
                };
                assert_eq!([image.width, image.height], [size; 2]);
            }
            if family == Family::Arcane {
                assert!(change.emissive_texture.is_some());
            }
            let mut copy = item.clone();
            copy.surface = None;
            assert_eq!(copy, original);
            assert_eq!(
                item.resources()
                    .map(|(source, _)| source.clone())
                    .collect::<Vec<_>>(),
                [source.clone(), overlay]
            );
        }
        let current = item.render_source().clone();
        apply(
            &mut item,
            &mut assets,
            Recipe {
                family: Family::Arcane,
                seed: 666,
            },
        )
        .unwrap();
        assert_eq!(item.render_source(), &current);
        apply(
            &mut item,
            &mut assets,
            Recipe {
                family: Family::Arcane,
                seed: 667,
            },
        )
        .unwrap();
        assert_ne!(item.render_source(), &current);
        let mut scene = Scene::empty();
        scene.world.models.push(item);
        scene.validate().unwrap();
        let decoded: Scene = serde_json::from_slice(&serde_json::to_vec(&scene).unwrap()).unwrap();
        assert_eq!(scene, decoded);
        assert!(
            crate::asset_sources(&decoded)
                .any(|(source, _)| source == decoded.world.models[0].render_source())
        );
    }

    #[test]
    #[ignore = "Uses the offscreen GPU and writes texture workshop screenshots"]
    fn capture_surface_workshop() {
        capture().unwrap();
    }

    fn capture() -> Result<(), String> {
        use crate::{
            EditorTab, configure_context,
            environment::Environment,
            offscreen::Offscreen,
            proof_input,
            viewport3d::{Camera, VIEWPORT_MSAA},
        };
        use std::{fs, path::Path};

        const SIZE: [u32; 2] = [1440, 960];
        let output = Path::new("verification/surface-workshop");
        fs::create_dir_all(output).map_err(|error| error.to_string())?;
        let mut display = Offscreen::new(SIZE, VIEWPORT_MSAA)?;
        configure_context(display.context());
        let mut input = proof_input::Driver::new(SIZE);
        let mut app = Studio::default();
        app.desk.recovery_root = Some(output.join("recovery"));
        let raw = include_bytes!("../demo_assets/world_palette/road-natural.glb");
        let source = app.assets.import_bytes(raw.to_vec())?;
        let model = app.assets.get(&source).unwrap().model.clone();
        let mut scene = Scene::empty();
        let mut environment = Environment::default();
        environment.time.start_hour = 16.2;
        scene.environment = Some(environment);
        for (x, title) in [
            (-4.7, "Лесная площадка"),
            (0.0, "Мостовая"),
            (4.7, "Плита с магическими прожилками"),
        ] {
            scene.world.models.push(Placement::at_ground(
                &scene.world.models,
                &model,
                source.clone(),
                title.into(),
                [x, 0.0, -3.8],
                4.6,
                0.0,
            ));
        }
        app.set_scene(scene, None, "Сравнение материалов");
        app.editor_tab = EditorTab::World;
        app.camera = Camera {
            yaw: 0.10,
            pitch: 0.71,
            distance: 15.8,
            target: [0.0, -0.04, -3.63],
        };
        let ids: Vec<_> = app.scene.world.models.iter().map(|item| item.id).collect();
        app.world_selection = Some(ids[0]);

        let save = |image: &[u8], title: &str| {
            image::save_buffer(
                output.join(title),
                image,
                SIZE[0],
                SIZE[1],
                image::ColorType::Rgb8,
            )
            .map_err(|error| error.to_string())
        };
        let mut frame = |display: &mut Offscreen, app: &mut Studio, events| {
            display.frame(input.raw(events), |ctx| app.run_frame(ctx))
        };
        let render = |display: &mut Offscreen,
                      app: &mut Studio,
                      frame: &mut dyn FnMut(
            &mut Offscreen,
            &mut Studio,
            Vec<egui::Event>,
        ) -> Result<Vec<u8>, String>| {
            frame(display, app, vec![egui::Event::PointerGone])?;
            frame(display, app, Vec::new())?;
            frame(display, app, Vec::new())
        };
        let click = |display: &mut Offscreen,
                     app: &mut Studio,
                     frame: &mut dyn FnMut(
            &mut Offscreen,
            &mut Studio,
            Vec<egui::Event>,
        ) -> Result<Vec<u8>, String>,
                     label| {
            let location = display
                .last_output()
                .and_then(|output| proof_input::find_label(output, label, 0))
                .ok_or_else(|| format!("В инспекторе нет «{label}»."))?;
            frame(display, app, proof_input::hover(location))?;
            frame(display, app, proof_input::click(location))?;
            for _ in 0..10 {
                frame(display, app, Vec::new())?;
            }
            Ok::<_, String>(())
        };
        let before = render(&mut display, &mut app, &mut frame)?;
        save(&before, "before.png")?;

        for (id, family) in ids
            .into_iter()
            .zip([Family::Forest, Family::Paving, Family::Arcane])
        {
            app.world_selection = Some(id);
            render(&mut display, &mut app, &mut frame)?;
            click(
                &mut display,
                &mut app,
                &mut frame,
                "Библиотека поверхностей",
            )?;
            if family != Family::Forest {
                click(&mut display, &mut app, &mut frame, Family::Forest.label())?;
                click(&mut display, &mut app, &mut frame, family.label())?;
            }
            click(&mut display, &mut app, &mut frame, "Применить поверхность")?;
            let selected = app
                .scene
                .world
                .models
                .iter()
                .find(|item| item.id == id)
                .unwrap();
            if selected.surface.as_ref().map(|item| item.recipe.family) != Some(family) {
                return Err(app.status);
            }
        }
        let after = render(&mut display, &mut app, &mut frame)?;
        if after == before {
            return Err("Применение поверхности не изменило кадр редактора.".into());
        }
        save(&after, "applied.png")?;
        let project = output.join("materials.robot.json");
        app.assets
            .save_files(crate::asset_sources(&app.scene), &project)?;
        app.scene.save(&project)?;
        let restored = Scene::load(&project)?;
        if restored != app.scene {
            return Err("Проект с поверхностями сохранился с изменениями.".into());
        }
        let mut reopened = Library::default();
        for (source, _) in crate::asset_sources(&restored) {
            reopened.ensure(source, Some(&project))?;
        }
        if !restored
            .world
            .models
            .iter()
            .all(|item| reopened.get(item.render_source()).is_some())
        {
            return Err("При открытии не загружены карты поверхности.".into());
        }
        let last = app.scene.world.models.last().unwrap().id;
        app.world_selection = Some(last);
        click(
            &mut display,
            &mut app,
            &mut frame,
            "Вернуть исходную поверхность",
        )?;
        if app.scene.world.models.last().unwrap().surface.is_some() {
            return Err("Кнопка не вернула исходную поверхность.".into());
        }
        fs::write(
            output.join("verification.json"),
            serde_json::to_vec_pretty(&json!({
                "capture_pixels": SIZE,
                "editor_clicked_apply_for": ["forest", "paving", "arcane"],
                "original_material_restored_by_click": true,
                "scene_and_all_overlay_glbs_reopened": true,
                "geometry_and_collision_unchanged_checked_by_unit_test": true,
            }))
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())
    }
}
