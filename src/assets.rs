use crate::{
    ImportError,
    model::{self, Model},
    world::Collider,
};
use eframe::egui::{self, DragValue};
use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    ops::RangeInclusive,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub const MAX_PLACEMENTS: usize = 32;
const FIRST_ID: u64 = 1 << 49;
const MAX_FILE_BYTES: u64 = model::MAX_GLB_BYTES as u64;
const MAX_SCENE_BYTES: usize = 256 * 1024 * 1024;
const DRAW_DISTANCE: RangeInclusive<f32> = 2.0..=160.0;
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

pub fn render_token() -> u64 {
    NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    Crate,
    Rock,
    Knight,
    CourtyardGround,
    StoneBuilding,
    CityHuman,
    CityWoman,
    Emma,
    Ryan,
    CityStreet,
    Imported(String),
}

impl Source {
    fn bundled(&self) -> Option<&'static [u8]> {
        match self {
            Self::Crate => Some(include_bytes!("../demo_assets/crate.glb")),
            Self::Rock => Some(include_bytes!("../demo_assets/rock.glb")),
            Self::Knight => Some(include_bytes!("../demo_assets/character.glb")),
            Self::CourtyardGround => Some(include_bytes!("../demo_assets/ground.glb")),
            Self::StoneBuilding => Some(include_bytes!("../demo_assets/building.glb")),
            Self::CityHuman => Some(include_bytes!("../demo_assets/human.glb")),
            Self::CityWoman => Some(include_bytes!("../demo_assets/woman.glb")),
            Self::Emma => Some(include_bytes!("../demo_assets/emma.glb")),
            Self::Ryan => Some(include_bytes!("../demo_assets/ryan.glb")),
            Self::CityStreet => Some(include_bytes!("../demo_assets/street.glb")),
            Self::Imported(_) => None,
        }
    }

    pub fn editable_human(&self) -> bool {
        matches!(
            self,
            Self::CityHuman | Self::CityWoman | Self::Emma | Self::Ryan
        )
    }

    pub fn modern_human(&self) -> bool {
        matches!(self, Self::Emma | Self::Ryan)
    }

    pub fn human_basis(&self) -> &'static str {
        match self {
            Self::Emma => "Эмма",
            Self::Ryan => "Райан",
            Self::CityWoman => "Майя (0.9)",
            Self::CityHuman => "Алексей (0.9)",
            _ => "Импортированная модель",
        }
    }

    pub fn height(&self) -> f32 {
        match self {
            Self::Emma => 1.73,
            Self::Ryan => 1.82,
            Self::CityWoman => 1.70,
            Self::CityHuman => 1.75,
            _ => 2.15,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Self::Imported(id) = self
            && (id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err("Недопустимый идентификатор GLB.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<materials::PointLight>,
    #[serde(default, skip_serializing_if = "materials::Tuning::is_default")]
    pub material: materials::Tuning,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_graph: Option<crate::material_graph::Graph>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) surface: Option<crate::surface_palette::Surface>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) paw_ground: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collider: Option<crate::interior::ModelCollider>,
    #[serde(default, skip_serializing_if = "origin_offset")]
    pub visual_offset: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seat: Option<crate::interior::SeatAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_distance: Option<f32>,
    pub id: u64,
    pub label: String,
    pub source: Source,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub solid: bool,
}

#[path = "materials.rs"]
pub mod materials;

impl Placement {
    pub(crate) fn next_id(others: &[Self]) -> u64 {
        others
            .iter()
            .map(|item| item.id.saturating_add(1))
            .max()
            .unwrap_or(FIRST_ID)
    }

    pub fn validate_all(placements: &[Self]) -> Result<(), String> {
        let mut ids = HashSet::new();
        if placements.len() > MAX_PLACEMENTS {
            return Err("Слишком много моделей GLB в сцене.".into());
        }
        for placement in placements {
            if !(FIRST_ID..(1 << 58)).contains(&placement.id)
                || !ids.insert(placement.id)
                || placement.label.trim().is_empty()
                || placement.label.chars().count() > 100
                || !within(placement.position, -256.0..=256.0)
                || !within(placement.rotation, -3_600.0..=3_600.0)
                || !within(placement.scale, 0.001..=100.0)
                || !within(placement.visual_offset, -256.0..=256.0)
                || !placement.material.valid()
                || placement.light.is_some_and(|light| !light.valid())
                || placement
                    .material_graph
                    .as_ref()
                    .is_some_and(|graph| !graph.storage_valid())
                || placement.collider.is_some_and(|collider| !collider.valid())
                || placement.seat.is_some_and(|seat| !seat.valid())
                || placement.draw_distance.is_some_and(|m| !DRAW_DISTANCE.contains(&m))
            {
                return Err(format!(
                    "Некорректные параметры модели «{}».",
                    placement.label
                ));
            }
            placement.source.validate()?;
            if let Some(surface) = &placement.surface {
                surface.validate()?;
            }
        }
        Ok(())
    }

    pub fn at_ground(
        others: &[Self],
        model: &Model,
        source: Source,
        label: String,
        foot: [f32; 3],
        max_extent: f32,
        yaw: f32,
    ) -> Self {
        let min = Vec3::from_array(model.bounds_min);
        let max = Vec3::from_array(model.bounds_max);
        let scale = (max_extent / (max - min).max_element().max(0.001)).clamp(0.001, 100.0);
        let pivot = Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5);
        let rotation = Quat::from_rotation_y(yaw.to_radians());
        Self {
            id: Self::next_id(others),
            label,
            source,
            position: (Vec3::from_array(foot) - rotation * (pivot * scale))
                .clamp(Vec3::splat(-256.0), Vec3::splat(256.0))
                .to_array(),
            rotation: [0.0, yaw, 0.0],
            scale: [scale; 3],
            solid: true,
            material: materials::Tuning::default(),
            light: None,
            material_graph: None,
            surface: None,
            paw_ground: false,
            collider: None,
            visual_offset: [0.0; 3],
            seat: None,
            draw_distance: None,
        }
    }

    pub fn visual_position(&self) -> [f32; 3] {
        let [x, y, z] = self.rotation.map(f32::to_radians);
        let rotation = Quat::from_euler(EulerRot::XYZ, x, y, z);
        (Vec3::from_array(self.position) + rotation * Vec3::from_array(self.visual_offset))
            .to_array()
    }

    pub fn visible_from(&self, eye: Vec3, model: &Model) -> bool {
        self.draw_distance.is_none_or(|distance| {
            let low = Vec3::from_array(model.bounds_min);
            let high = Vec3::from_array(model.bounds_max);
            let scale = Vec3::from_array(self.scale);
            let [x, y, z] = self.rotation.map(f32::to_radians);
            let facing = Quat::from_euler(EulerRot::XYZ, x, y, z);
            let local_eye = facing.conjugate() * (eye - Vec3::from_array(self.visual_position()));
            let closest = local_eye.clamp(low * scale, high * scale);
            local_eye.distance_squared(closest) <= distance * distance
        })
    }

    pub fn point_light(&self) -> Option<(Vec3, materials::PointLight)> {
        let light = self.light.filter(|light| light.intensity > 0.0)?;
        let [x, y, z] = self.rotation.map(f32::to_radians);
        let rotation = Quat::from_euler(EulerRot::XYZ, x, y, z);
        Some((Vec3::from_array(self.position) + rotation * Vec3::from_array(light.offset), light))
    }

    pub fn replace_visual(&mut self, source: Source, before: Option<&Model>, after: &Model) {
        let foot = |model: &Model, scale: Vec3| {
            let low = Vec3::from_array(model.bounds_min);
            let high = Vec3::from_array(model.bounds_max);
            Vec3::new((low.x + high.x) * 0.5, low.y, (low.z + high.z) * 0.5) * scale
        };
        let extent = |model: &Model, scale: Vec3| {
            ((Vec3::from_array(model.bounds_max) - Vec3::from_array(model.bounds_min)) * scale)
                .max_element()
                .max(0.001)
        };
        let old_scale = Vec3::from_array(self.scale);
        if let Some(before) = before {
            let scale = (old_scale * (extent(before, old_scale) / extent(after, old_scale)))
                .clamp(Vec3::splat(0.001), Vec3::splat(100.0));
            self.visual_offset = (Vec3::from_array(self.visual_offset) + foot(before, old_scale)
                - foot(after, scale))
            .clamp(Vec3::splat(-256.0), Vec3::splat(256.0))
            .to_array();
            self.scale = scale.to_array();
        }
        self.source = source;
        self.surface = None;
    }

    pub(crate) fn render_source(&self) -> &Source {
        self.surface.as_ref().map_or(&self.source, |surface| &surface.overlay)
    }

    pub(crate) fn resources(&self) -> impl Iterator<Item = (&Source, &str)> {
        std::iter::once((&self.source, self.label.as_str()))
            .chain(self.surface.iter().map(|surface| (&surface.overlay, self.label.as_str())))
    }

    pub fn custom_collider(&self) -> Option<Collider> {
        self.collider
            .filter(|_| self.solid)
            .map(|collider| collider.world(self.position, self.rotation[1]))
    }

    pub fn seat_pose(&self) -> Option<crate::world::Seat> {
        self.seat
            .map(|seat| seat.world(self.id, self.position, self.rotation[1]))
    }

    pub fn bounds(&self, model: &Model) -> Collider {
        self.transform_bounds(Collider {
            min: model.bounds_min,
            max: model.bounds_max,
        })
    }

    pub fn transform_bounds(&self, local: Collider) -> Collider {
        let [x, y, z] = self.rotation.map(f32::to_radians);
        let rotation = Quat::from_euler(EulerRot::XYZ, x, y, z);
        let local_min = Vec3::from_array(local.min);
        let local_max = Vec3::from_array(local.max);
        let position = Vec3::from_array(self.visual_position());
        let scale = Vec3::from_array(self.scale);
        let center = position + rotation * ((local_min + local_max) * 0.5 * scale);
        let radius = (local_max - local_min) * 0.5 * scale;
        let extent = (rotation * Vec3::X).abs() * radius.x
            + (rotation * Vec3::Y).abs() * radius.y
            + (rotation * Vec3::Z).abs() * radius.z;
        Collider {
            min: (center - extent).to_array(),
            max: (center + extent).to_array(),
        }
    }
}

fn origin_offset(offset: &[f32; 3]) -> bool {
    *offset == [0.0; 3]
}

fn within(values: [f32; 3], range: RangeInclusive<f32>) -> bool {
    values.iter().all(|value| range.contains(value))
}

#[derive(Clone)]
pub struct LoadedModel {
    pub token: u64,
    pub model: Arc<Model>,
    file: Option<Arc<[u8]>>,
    pub(crate) cost: usize,
}

#[derive(Clone, Default)]
pub struct Library {
    models: HashMap<Source, LoadedModel>,
}

impl Library {
    pub fn get(&self, source: &Source) -> Option<&LoadedModel> {
        self.models.get(source)
    }

    pub(crate) fn remove(&mut self, source: &Source) {
        self.models.remove(source);
    }

    pub fn retain<'a>(&mut self, sources: impl Iterator<Item = &'a Source>) {
        let sources = sources.collect::<HashSet<_>>();
        self.models.retain(|source, _| sources.contains(source));
    }

    pub(crate) fn restore_cached(&mut self, snapshots: &[(Source, LoadedModel)]) {
        for (source, asset) in snapshots {
            self.models
                .entry(source.clone())
                .or_insert_with(|| asset.clone());
        }
    }

    pub fn ensure(&mut self, source: &Source, scene: Option<&Path>) -> Result<(), ImportError> {
        if self.models.contains_key(source) {
            return Ok(());
        }
        if let Some(bytes) = source.bundled() {
            let model = model::decode_glb(bytes)
                .map_err(|error| error.with_file(format!("{source:?}.glb")))?;
            return self.store(source.clone(), model, None);
        }
        let Source::Imported(id) = source else {
            return Err("Неизвестная модель.".into());
        };
        source.validate()?;
        let scene = scene.ok_or("Сначала открой файл сцены.")?;
        let path = asset_dir(scene).join(format!("{id}.glb"));
        let bytes = read(&path)?;
        if digest(&bytes) != *id {
            return Err(ImportError::file(&path, "Контрольная сумма модели не совпадает."));
        }
        let model = model::decode_glb(&bytes).map_err(|error| error.with_file(&path))?;
        self.store(source.clone(), model, Some(bytes)).map_err(|error| error.with_file(&path))
    }

    pub fn import(&mut self, path: &Path) -> Result<Source, ImportError> {
        self.import_bytes(read(path)?).map_err(|error| error.with_file(path))
    }

    pub(crate) fn import_bytes(&mut self, bytes: Vec<u8>) -> Result<Source, ImportError> {
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(ImportError::new("Импорт GLB", "Файл GLB больше 64 МБ."));
        }
        let source = Source::Imported(digest(&bytes));
        if !self.models.contains_key(&source) {
            let model = model::decode_glb(&bytes)?;
            self.store(source.clone(), model, Some(bytes))?;
        }
        Ok(source)
    }

    pub(crate) fn export_glb(&self, source: &Source) -> Option<&[u8]> {
        self.models.get(source)?.file.as_deref()
    }

    pub(crate) fn original_glb(&self, source: &Source) -> Option<&[u8]> {
        self.export_glb(source).or_else(|| source.bundled())
    }

    pub(crate) fn insert_exported_glb(
        &mut self,
        source: &Source,
        bytes: Vec<u8>,
    ) -> Result<(), ImportError> {
        let Source::Imported(id) = source else {
            return Err("В файле игры указан неизвестный тип ресурса.".into());
        };
        if bytes.len() as u64 > MAX_FILE_BYTES || digest(&bytes) != *id {
            return Err("Встроенная модель GLB повреждена.".into());
        }
        let model = model::decode_glb(&bytes)?;
        self.store(source.clone(), model, Some(bytes))
    }

    fn store(
        &mut self,
        source: Source,
        mut model: Model,
        file: Option<Vec<u8>>,
    ) -> Result<(), ImportError> {
        if source.editable_human() {
            model.polish_human_materials();
        }
        let cost = file.as_ref().map_or(0, Vec::len)
            + model.morph_storage_bytes()
            + model
                .images
                .iter()
                .map(|image| image.pixels.len())
                .sum::<usize>()
            + model
                .primitives
                .iter()
                .map(|primitive| {
                    primitive.vertices.len() * size_of::<model::Vertex>()
                        + primitive.indices.len() * size_of::<u32>()
                })
                .sum::<usize>();
        let used = self.models.values().map(|loaded| loaded.cost).sum::<usize>();
        if used + cost > MAX_SCENE_BYTES {
            const MIB: f64 = (1024 * 1024) as f64;
            return Err(ImportError::new("Библиотека GLB", format!(
                "Превышен лимит памяти для моделей сцены (256 МБ). Занято {:.1} МБ, новой модели нужно {:.1} МБ.",
                used as f64 / MIB, cost as f64 / MIB,
            )));
        }
        self.models.insert(
            source,
            LoadedModel {
                token: render_token(),
                model: Arc::new(model),
                file: file.map(Arc::from),
                cost,
            },
        );
        Ok(())
    }

    pub fn save_files<'a>(
        &self,
        sources: impl Iterator<Item = (&'a Source, &'a str)>,
        scene: &Path,
    ) -> Result<(), String> {
        let directory = asset_dir(scene);
        let mut saved = HashSet::new();
        for (source, label) in sources {
            let Source::Imported(id) = source else {
                continue;
            };
            if !saved.insert(id) {
                continue;
            }
            let bytes = self
                .models
                .get(source)
                .and_then(|asset| asset.file.as_ref())
                .ok_or_else(|| {
                    format!(
                        "Модель «{}» недоступна. Восстанови файл GLB или удали объект.",
                        label
                    )
                })?;
            fs::create_dir_all(&directory)
                .map_err(|error| format!("Не удалось создать каталог моделей ({error})."))?;
            let path = directory.join(format!("{id}.glb"));
            let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if digest(&read(&path)?) != *id {
                        return Err(format!(
                            "В каталоге сцены файл {} повреждён.",
                            path.display()
                        ));
                    }
                    continue;
                }
                Err(error) => return Err(format!("Не удалось сохранить GLB ({error}).")),
            };
            if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(format!("Не удалось записать GLB ({error})."));
            }
        }
        Ok(())
    }
}

fn asset_dir(scene: &Path) -> PathBuf {
    scene.with_extension("assets")
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read(path: &Path) -> Result<Vec<u8>, ImportError> {
    let file = File::open(path).map_err(|error| ImportError::read(path, error))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ImportError::read(path, error))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(ImportError::file(path, "Файл GLB больше 64 МБ."));
    }
    Ok(bytes)
}

pub fn inspector(
    ui: &mut egui::Ui,
    placement: &mut Placement,
    enabled: bool,
    model: Option<&Model>,
) -> bool {
    let before = placement.clone();
    ui.add_enabled_ui(enabled, |ui| {
        let response = ui.add(egui::TextEdit::singleline(&mut placement.label).char_limit(100));
        if response.lost_focus() && placement.label.trim().is_empty() {
            placement.label = "Модель GLB".into();
        }
        ui.weak(if model.is_some() {
            "Модель GLB"
        } else {
            "Файл модели не найден или не загрузился"
        });
        let emits_light = placement.light.is_some();
        if emits_light {
            materials::light_inspector(ui, &mut placement.light);
        }
        vector(ui, "Позиция", &mut placement.position, -256.0..=256.0);
        vector(
            ui,
            "Поворот, °",
            &mut placement.rotation,
            -3_600.0..=3_600.0,
        );
        vector(ui, "Масштаб", &mut placement.scale, 0.001..=100.0);
        if placement.source != Source::CityStreet {
            vector(
                ui,
                "Смещение модели, м",
                &mut placement.visual_offset,
                -256.0..=256.0,
            );
        }
        ui.horizontal_wrapped(|ui| {
            let mut limited = placement.draw_distance.is_some();
            if ui.checkbox(&mut limited, "Ограничить дальность модели").changed() {
                placement.draw_distance = limited.then_some(45.0);
            }
            if let Some(distance) = &mut placement.draw_distance {
                ui.add(DragValue::new(distance).range(DRAW_DISTANCE).speed(0.5).suffix(" м"))
                    .on_hover_text("Дальность от камеры до границы модели. Действует в игре. Коллизии и свет продолжают работать.");
            }
        });
        let collision = if placement.source == Source::CityStreet {
            "Столкновения с бордюрами"
        } else {
            "Столкновения по габаритам"
        };
        ui.checkbox(
            &mut placement.solid,
            if placement.collider.is_some() {
                "Столкновения"
            } else {
                collision
            },
        );
        ui.checkbox(&mut placement.paw_ground, "Рельеф для опоры лап")
            .on_hover_text("Четвероногие берут высоту и наклон для каждой лапы с поверхности этой модели. В прибрежной деревне луг, берег и дорога подключаются автоматически.");
        materials::inspector(ui, &mut placement.material);
        if !emits_light {
            materials::light_inspector(ui, &mut placement.light);
        }
        inspect_material_graph(ui, placement, enabled);
        if placement.source != Source::CityStreet {
            model_furniture_controls(ui, placement, model);
        }
    });
    before != *placement
}

#[derive(Clone, Copy, Default)]
struct MaterialWindow {
    open: bool,
    light: crate::material_graph::PreviewLight,
}

fn material_window_id(id: u64) -> egui::Id {
    egui::Id::new(("material graph window", id))
}

pub fn material_preview(ctx: &egui::Context, id: u64) -> crate::material_graph::PreviewLight {
    ctx.data(|data| {
        data.get_temp::<MaterialWindow>(material_window_id(id))
            .map_or(crate::material_graph::PreviewLight::Scene, |state| {
                state.light
            })
    })
}

fn inspect_material_graph(ui: &mut egui::Ui, placement: &mut Placement, enabled: bool) {
    use crate::material_graph::{Graph, PreviewLight};

    let id = material_window_id(placement.id);
    let mut state: MaterialWindow = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        if ui.button("Граф материала…").clicked() {
            state.open = true;
        }
        if state.light != PreviewLight::Scene && ui.button("Свет сцены").clicked() {
            state.light = PreviewLight::Scene;
        }
    });
    if state.open && enabled {
        let mut light = state.light;
        let mut remove = false;
        let mut close = false;
        egui::Window::new("Граф материала")
            .id(id)
            .open(&mut state.open)
            .default_size(egui::vec2(780.0, 530.0))
            .min_width(680.0)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Свет для выбранной модели");
                    for (value, caption) in [
                        (PreviewLight::Scene, "Сцена"),
                        (PreviewLight::Outdoor, "На улице"),
                        (PreviewLight::Indoor, "В помещении"),
                    ] {
                        ui.selectable_value(&mut light, value, caption);
                    }
                    close = ui.button("Закрыть граф").clicked();
                });
                ui.label(&placement.label);
                let mut draft = Graph::default();
                let graph = placement.material_graph.as_mut().unwrap_or(&mut draft);
                let changed = crate::material_graph_ui::editor(ui, graph);
                if changed && placement.material_graph.is_none() {
                    placement.material_graph = Some(draft);
                }
                remove = ui
                    .add_enabled(
                        placement.material_graph.is_some(),
                        egui::Button::new("Убрать граф"),
                    )
                    .clicked();
            });
        if remove {
            placement.material_graph = None;
        }
        state.open &= !close;
        state.light = light;
    }
    ui.data_mut(|data| data.insert_temp(id, state));
}

fn model_furniture_controls(ui: &mut egui::Ui, placement: &mut Placement, model: Option<&Model>) {
    let default_collider = model.map_or(
        crate::interior::ModelCollider {
            center: [0.0, 0.45, 0.0],
            size: [0.70, 0.90, 0.70],
        },
        |model| {
            let mut collider = crate::interior::ModelCollider::from_visual_bounds(
                model.bounds_min,
                model.bounds_max,
                placement.scale,
            );
            collider.center = (Vec3::from_array(collider.center)
                + Vec3::from_array(placement.visual_offset))
            .clamp(Vec3::splat(-128.0), Vec3::splat(128.0))
            .to_array();
            collider
        },
    );
    ui.separator();
    let mut custom = placement.collider.is_some();
    if ui.checkbox(&mut custom, "Отдельный коллайдер").changed() {
        placement.collider = custom.then_some(default_collider);
        placement.solid |= custom;
    }
    if let Some(collider) = &mut placement.collider {
        vector(
            ui,
            "Центр коллайдера, м",
            &mut collider.center,
            -128.0..=128.0,
        );
        vector(ui, "Размер коллайдера, м", &mut collider.size, 0.03..=128.0);
    }
    let mut seat = placement.seat.is_some();
    if ui.checkbox(&mut seat, "Точка посадки").changed() {
        placement.seat = seat.then(|| {
            crate::interior::SeatAnchor::from_collider(
                placement.collider.unwrap_or(default_collider),
            )
        });
    }
    if let Some(anchor) = &mut placement.seat {
        vector(ui, "Положение сиденья, м", &mut anchor.sit, -128.0..=128.0);
        vector(
            ui,
            "Куда встаёт герой, м",
            &mut anchor.stand,
            -128.0..=128.0,
        );
        ui.add(
            DragValue::new(&mut anchor.yaw)
                .prefix("Поворот посадки ")
                .suffix("°")
                .range(-360.0..=360.0),
        );
        ui.weak("Смещения отсчитываются от позиции модели и поворачиваются вместе с ней.");
    }
}

fn vector(ui: &mut egui::Ui, title: &str, values: &mut [f32; 3], range: RangeInclusive<f32>) {
    ui.label(title);
    ui.horizontal_wrapped(|ui| {
        for (axis, value) in ["X ", "Y ", "Z "].into_iter().zip(values) {
            ui.add(
                DragValue::new(value)
                    .prefix(axis)
                    .range(range.clone())
                    .speed(0.04)
                    .max_decimals(3),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameplay_visibility_uses_rotated_mesh_bounds_and_leaves_collisions_intact() {
        let model = model::decode_glb(include_bytes!("../demo_assets/crate.glb")).unwrap();
        let mut placement = Placement::at_ground(
            &[], &model, Source::Crate, "Декор".into(), [0.0; 3], 1.0, 90.0,
        );
        placement.scale[0] *= 10.0;
        placement.visual_offset[0] = 12.0;
        let before = placement.bounds(&model);
        let json = serde_json::to_string(&placement).unwrap();
        assert!(!json.contains("draw_distance"));
        assert!(placement.visible_from(Vec3::splat(150.0), &model));
        placement.draw_distance = Some(2.0);
        let eye = Vec3::new(0.0, 1.0, -12.0);
        assert!(placement.visible_from(eye, &model));
        assert!(!placement.visible_from(eye + Vec3::X * 7.0, &model));
        let persisted: Placement = serde_json::from_str(&serde_json::to_string(&placement).unwrap()).unwrap();
        assert_eq!(persisted.draw_distance, Some(2.0));
        assert_eq!(persisted.bounds(&model), before);
    }

    #[test]
    fn replacing_an_imported_visual_preserves_seat_and_collision() {
        let bench = model::decode_glb(include_bytes!("../demo_assets/quarter/bench.glb")).unwrap();
        let alternate =
            model::decode_glb(include_bytes!("../demo_assets/quarter/bench_alt.glb")).unwrap();
        let mut placement = Placement::at_ground(
            &[],
            &bench,
            Source::Imported("a".repeat(64)),
            "Скамья".into(),
            [4.3, 0.076, 2.48],
            1.8,
            -90.0,
        );
        placement.collider = Some(crate::interior::ModelCollider {
            center: [0.0, 0.225, 0.0],
            size: [1.58, 0.46, 0.44],
        });
        placement.seat = Some(crate::interior::SeatAnchor {
            sit: [0.0, 0.49, 0.0],
            stand: [0.0, -0.066, 1.0],
            yaw: 0.0,
        });
        let before = placement.clone();
        let replacement = Source::Imported("b".repeat(64));
        placement.replace_visual(replacement.clone(), Some(&bench), &alternate);

        assert_eq!(placement.id, before.id);
        assert_eq!(placement.position, before.position);
        assert_eq!(placement.source, replacement);
        assert_eq!(placement.custom_collider(), before.custom_collider());
        assert_eq!(placement.seat_pose(), before.seat_pose());
        assert!((placement.bounds(&alternate).min[1] - 0.076).abs() < 1e-5);
    }

    #[test]
    fn imported_model_can_be_reopened_from_its_scene_directory() {
        let source_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo_assets/crate.glb");
        let scene =
            std::env::temp_dir().join(format!("studio-asset-{}.robot.json", std::process::id()));
        let mut library = Library::default();
        let source = library.import(&source_path).unwrap();
        let reference = &library.get(&source).unwrap().model;
        let placement = Placement::at_ground(
            &[],
            reference,
            source.clone(),
            "Ящик".into(),
            [2.0, 0.0, 0.0],
            1.8,
            30.0,
        );
        assert!(placement.bounds(reference).min[1].abs() < 1e-5);
        library
            .save_files(
                std::iter::once((&placement.source, placement.label.as_str())),
                &scene,
            )
            .unwrap();
        let mut restored = Library::default();
        restored.ensure(&source, Some(&scene)).unwrap();
        assert_eq!(
            restored.get(&source).unwrap().model.bounds_min,
            reference.bounds_min
        );
        if let Source::Imported(id) = source {
            let directory = asset_dir(&scene);
            fs::remove_file(directory.join(format!("{id}.glb"))).unwrap();
            fs::remove_dir(directory).unwrap();
        }
    }
}
