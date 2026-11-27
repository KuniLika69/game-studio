use std::collections::{HashMap, HashSet};

use glam::Vec2;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    Scene,
    assets::{Library, MAX_PLACEMENTS, Placement, Source},
    environment::{Environment, WeatherPreset},
    world::Collider,
};

mod geometry;

pub(crate) const MAX_ROUTE_POINTS: usize = 10;
pub(crate) const MAX_ZONES: usize = 6;
pub(crate) const MIN_X: f32 = -18.0;
pub(crate) const MAX_X: f32 = 5.8;
pub(crate) const MIN_Z: f32 = -30.0;
pub(crate) const MAX_Z: f32 = 5.0;
const VERSION: u16 = 1;
const SHORE_X: f32 = 7.5;
const MAX_MANAGED: usize = MAX_ZONES * 16 * 2 + 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Style {
    #[default]
    Natural,
    Stylized,
}

impl Style {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Natural => "Натура",
            Self::Stylized => "Лоу-поли",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ZoneKind {
    Houses,
    Trees,
    Flowers,
    Grass,
}

impl ZoneKind {
    pub(crate) const ALL: [Self; 4] = [Self::Houses, Self::Trees, Self::Flowers, Self::Grass];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Houses => "Дома",
            Self::Trees => "Деревья",
            Self::Flowers => "Цветы",
            Self::Grass => "Трава",
        }
    }

    fn group(self) -> Group {
        if self == Self::Houses {
            Group::Houses
        } else {
            Group::Vegetation
        }
    }

    fn resource(self, style: Style) -> (&'static [u8], f32) {
        macro_rules! asset {
            ($name:literal) => {
                include_bytes!(concat!("../demo_assets/world_palette/", $name, ".glb"))
            };
        }
        match (self, style) {
            (Self::Houses, Style::Natural) => (asset!("house-natural"), 4.6),
            (Self::Houses, Style::Stylized) => (asset!("house-lowpoly"), 4.6),
            (Self::Trees, Style::Natural) => {
                (include_bytes!("../demo_assets/expedition/alder.glb"), 4.0)
            }
            (Self::Trees, Style::Stylized) => (asset!("larch-lowpoly"), 3.85),
            (Self::Flowers, Style::Natural) => (asset!("flowers-natural"), 1.75),
            (Self::Flowers, Style::Stylized) => (asset!("flowers-lowpoly"), 1.75),
            (Self::Grass, Style::Natural) => (
                include_bytes!("../demo_assets/coastal/dune-grass.glb"),
                2.50,
            ),
            (Self::Grass, Style::Stylized) => (asset!("grass-lowpoly"), 2.35),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Zone {
    pub kind: ZoneKind,
    pub center: [f32; 2],
    pub radius: f32,
    pub count: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Recipe {
    pub version: u16,
    pub seed: u32,
    pub style: Style,
    pub route: Vec<[f32; 2]>,
    pub road_width: f32,
    #[serde(default = "default_ridge_height")]
    pub ridge_height: f32,
    pub zones: Vec<Zone>,
}

fn default_ridge_height() -> f32 {
    4.8
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            version: VERSION,
            seed: 734,
            style: Style::Natural,
            route: vec![
                [0.0, 0.0],
                [0.0, -5.8],
                [1.6, -12.0],
                [0.7, -18.1],
                [3.5, -23.2],
            ],
            road_width: 2.55,
            ridge_height: default_ridge_height(),
            zones: vec![
                Zone {
                    kind: ZoneKind::Houses,
                    center: [-6.5, -12.5],
                    radius: 8.6,
                    count: 4,
                },
                Zone {
                    kind: ZoneKind::Trees,
                    center: [-10.0, -15.0],
                    radius: 12.0,
                    count: 7,
                },
                Zone {
                    kind: ZoneKind::Flowers,
                    center: [3.05, -9.7],
                    radius: 8.8,
                    count: 6,
                },
                Zone {
                    kind: ZoneKind::Grass,
                    center: [3.6, -17.0],
                    radius: 10.3,
                    count: 5,
                },
            ],
        }
    }
}

impl Recipe {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err("Версия рецепта деревни не поддерживается.".into());
        }
        if !(2..=MAX_ROUTE_POINTS).contains(&self.route.len())
            || self.route.first() != Some(&[0.0, 0.0])
            || self.route.iter().any(|&point| !on_land(point))
            || self
                .route
                .windows(2)
                .any(|pair| Vec2::from_array(pair[0]).distance(Vec2::from_array(pair[1])) < 0.8)
            || !(1.8..=4.2).contains(&self.road_width)
        {
            return Err(
                "Маршрут начинается в (0, 0). Размести 2–10 точек на суше с шагом от 0,8 м.".into(),
            );
        }
        let mut houses = 0;
        let mut greenery = 0;
        if !(0.0..=9.0).contains(&self.ridge_height) {
            return Err("Высота прибрежных холмов должна быть от 0 до 9 м.".into());
        }
        for zone in &self.zones {
            if !on_land(zone.center) || !(1.0..=16.0).contains(&zone.radius) || zone.count > 16 {
                return Err("Проверь центр, радиус и число объектов в зонах.".into());
            }
            match zone.kind.group() {
                Group::Houses => houses += u16::from(zone.count),
                _ => greenery += u16::from(zone.count),
            }
        }
        if self.zones.len() > MAX_ZONES || houses > 8 || greenery > 21 {
            return Err("Уменьши плотность. Допускается до 6 зон, 8 домов и 21 растения.".into());
        }
        Ok(())
    }
}

fn on_land([x, z]: [f32; 2]) -> bool {
    (MIN_X..=MAX_X).contains(&x) && (MIN_Z..=MAX_Z).contains(&z)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum Group {
    Coast,
    Route,
    Houses,
    Vegetation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    All,
    Route,
    Houses,
    Vegetation,
}

impl Scope {
    fn includes(self, group: Group) -> bool {
        self == Self::All
            || matches!(
                (self, group),
                (Self::Route, Group::Route)
                    | (Self::Houses, Group::Houses)
                    | (Self::Vegetation, Group::Vegetation)
            )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Managed {
    group: Group,
    slot: u16,
    id: u64,
    signature: [u8; 32],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    locked: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct State {
    pub recipe: Recipe,
    managed: Vec<Managed>,
}

impl State {
    pub(crate) fn supports_surface(&self, id: u64) -> bool {
        self.managed
            .iter()
            .any(|item| item.id == id && matches!(item.group, Group::Coast | Group::Route))
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        self.recipe.validate()?;
        let mut ids = HashSet::new();
        let mut slots = HashSet::new();
        if self.managed.len() > MAX_MANAGED
            || self.managed.iter().any(|item| {
                item.id == 0 || !ids.insert(item.id) || !slots.insert((item.group, item.slot))
            })
        {
            return Err("В рецепте деревни повторяются управляемые объекты.".into());
        }
        Ok(())
    }

    pub(crate) fn houses(&self) -> impl Iterator<Item = (u64, bool)> + '_ {
        self.managed
            .iter()
            .filter(|item| item.group == Group::Houses)
            .map(|item| (item.id, item.locked))
    }
}

pub(crate) fn lock_house(scene: &mut Scene, id: u64, locked: bool) -> bool {
    let Some(item) = scene.village.as_mut().and_then(|state| {
        state
            .managed
            .iter_mut()
            .find(|item| item.id == id && item.group == Group::Houses)
    }) else {
        return false;
    };
    if item.locked == locked {
        return false;
    }
    item.locked = locked;
    true
}

pub(crate) fn manually_changed(scene: &Scene, id: u64) -> bool {
    let item = scene
        .village
        .as_ref()
        .and_then(|state| state.managed.iter().find(|item| item.id == id));
    item.is_some_and(|item| {
        scene
            .world
            .models
            .iter()
            .find(|model| model.id == id)
            .is_some_and(|model| signature(model) != item.signature)
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Report {
    pub created: usize,
    pub preserved: usize,
    pub omitted: usize,
    pub route_clear: bool,
}

pub(crate) fn generate(
    scene: &mut Scene,
    assets: &mut Library,
    recipe: Recipe,
    scope: Scope,
) -> Result<Report, String> {
    recipe.validate()?;
    if scene.map_2d.is_some() {
        return Err("Открой 3D-сцену для деревни.".into());
    }
    let scope = if scene.village.is_none() {
        Scope::All
    } else {
        scope
    };
    let mut draft = scene.clone();
    let mut library = assets.clone();
    let mut retained = Vec::new();
    let mut reusable = HashMap::new();
    let mut report = Report::default();
    if let Some(state) = draft.village.take() {
        for item in state.managed {
            let model = draft.world.models.iter().find(|model| model.id == item.id);
            if scope.includes(item.group)
                && !item.locked
                && model.is_some_and(|model| signature(model) == item.signature)
            {
                reusable.insert((item.group, item.slot), item.id);
            } else {
                if scope.includes(item.group) && model.is_some() {
                    report.preserved += 1;
                }
                retained.push(item);
            }
        }
    }
    let disposable: HashSet<_> = reusable.values().copied().collect();
    let next_id = Placement::next_id(&draft.world.models);
    draft
        .world
        .models
        .retain(|model| !disposable.contains(&model.id));
    let mut generator = Generator {
        scene: &mut draft,
        assets: &mut library,
        recipe: &recipe,
        next_id,
        managed: retained,
        reusable,
        report,
    };
    generator.build(scope)?;
    generator.report.route_clear = !generator.colliders().iter().any(|bounds| {
        let clearance = 0.39;
        let expanded = Collider {
            min: [
                bounds.min[0] - clearance,
                bounds.min[1],
                bounds.min[2] - clearance,
            ],
            max: [
                bounds.max[0] + clearance,
                bounds.max[1],
                bounds.max[2] + clearance,
            ],
        };
        recipe.route.windows(2).any(|pair| {
            expanded
                .intersects_segment([pair[0][0], 0.6, pair[0][1]], [pair[1][0], 0.6, pair[1][1]])
        })
    });
    let report = generator.report;
    draft.village = Some(State {
        recipe: recipe.clone(),
        managed: generator.managed,
    });
    draft.validate()?;
    *scene = draft;
    *assets = library;
    Ok(report)
}

struct Generator<'a> {
    scene: &'a mut Scene,
    assets: &'a mut Library,
    recipe: &'a Recipe,
    next_id: u64,
    managed: Vec<Managed>,
    reusable: HashMap<(Group, u16), u64>,
    report: Report,
}

impl Generator<'_> {
    fn build(&mut self, scope: Scope) -> Result<(), String> {
        if scope.includes(Group::Coast) {
            self.surface(
                Group::Coast,
                0,
                "Прибрежный луг",
                geometry::ground(self.recipe, false)?,
            )?;
            self.surface(
                Group::Coast,
                1,
                "Песчаный берег",
                geometry::ground(self.recipe, true)?,
            )?;
            if !self
                .scene
                .environment
                .as_ref()
                .is_some_and(|env| env.waters.iter().any(|zone| zone.name == "Морская бухта"))
            {
                let env = self.scene.environment.get_or_insert_with(|| {
                    let mut env = Environment::default();
                    env.time.start_hour = 10.75;
                    env.time.noon_bearing = 108.0;
                    env.weather.initial = WeatherPreset::Clear;
                    env
                });
                env.add_water([42.0, -13.0], -0.75)
                    .ok_or("Не удалось добавить бухту.")?;
                let sea = env.waters.last_mut().unwrap();
                sea.name = "Морская бухта".into();
                sea.size = [69.0, 139.0];
                sea.rounded_edge = true;
                sea.level = 0.038;
                sea.max_level = 0.32;
                sea.tint = [0.09, 0.34, 0.36];
                sea.wave_height = 0.072;
                sea.current = [0.04, 0.02];
                if self.recipe.style == Style::Natural {
                    sea.shore = Some(crate::environment::WaterShore {
                        origin: [SHORE_X, 0.0],
                        seaward: [1.0, 0.0],
                        slope: 0.12,
                    });
                }
            }
        }
        if scope.includes(Group::Route) {
            self.surface(
                Group::Route,
                0,
                "Дорога по маршруту",
                geometry::road(self.recipe)?,
            )?;
            self.surface(
                Group::Route,
                1,
                "Деревянный причал",
                geometry::pier(self.recipe)?,
            )?;
        }
        for group in [Group::Houses, Group::Vegetation] {
            if scope.includes(group) {
                self.scatter(group)?;
            }
        }
        Ok(())
    }

    fn surface(
        &mut self,
        group: Group,
        slot: u16,
        label: &str,
        glb: Vec<u8>,
    ) -> Result<(), String> {
        if self.kept(group, slot) {
            return Ok(());
        }
        let source = self.assets.import_bytes(glb)?;
        let model = &self.assets.get(&source).expect("loaded surface").model;
        let mut item = Placement::at_ground(&[], model, source, label.into(), [0.0; 3], 1.0, 0.0);
        item.position = [0.0; 3];
        item.scale = [1.0; 3];
        item.solid = false;
        self.place(item, group, slot)
    }

    fn scatter(&mut self, group: Group) -> Result<(), String> {
        let recipe = self.recipe;
        let mut blockers = self.colliders();
        let mut occupied = Vec::<[f32; 2]>::new();
        let mut sources = Vec::new();
        for (zone_index, zone) in recipe
            .zones
            .iter()
            .enumerate()
            .filter(|(_, zone)| zone.kind.group() == group)
        {
            let mut rng = Rng(self.recipe.seed ^ (zone_index as u32 + 1).wrapping_mul(0x9e37_79b9));
            let solid = matches!(zone.kind, ZoneKind::Houses | ZoneKind::Trees);
            for index in 0..zone.count {
                let slot = (zone_index as u16) * 32 + u16::from(index);
                let kept = self.kept(group, slot);
                let variant = if zone.kind == ZoneKind::Houses && recipe.style == Style::Natural {
                    (recipe.seed as usize + usize::from(index) + zone_index * 2) % 3
                } else {
                    0
                };
                let source = if kept {
                    None
                } else {
                    Some(self.zone_asset(zone.kind, variant, &mut sources)?)
                };
                let mut placed = false;
                for _ in 0..64 {
                    let angle = std::f32::consts::TAU * rng.unit();
                    let distance = rng.unit().sqrt() * zone.radius;
                    let point = [
                        zone.center[0] + angle.cos() * distance,
                        zone.center[1] + angle.sin() * distance,
                    ];
                    let (_, scale) = zone.kind.resource(self.recipe.style);
                    let extent = scale * (0.80 + rng.unit() * 0.38);
                    let yaw = if group == Group::Houses {
                        -13.0 + rng.unit() * 26.0
                    } else {
                        rng.unit() * 360.0
                    };
                    if kept {
                        placed = true;
                        break;
                    }
                    let gap = if solid { extent * 0.60 + 0.45 } else { 0.58 };
                    if !on_land(point)
                        || route_distance(point, &self.recipe.route)
                            < self.recipe.road_width * 0.5 + gap
                        || occupied.iter().any(|&other| {
                            Vec2::from_array(other).distance(Vec2::from_array(point)) < 0.72
                        })
                        || blockers.iter().any(|bounds| near(*bounds, point, gap))
                    {
                        continue;
                    }
                    if self.scene.world.models.len() >= MAX_PLACEMENTS {
                        break;
                    }
                    let model = &self
                        .assets
                        .get(source.as_ref().unwrap())
                        .expect("loaded village resource")
                        .model;
                    let label = match zone.kind {
                        ZoneKind::Houses => {
                            format!("Дом {}.{}", zone_index + 1, u16::from(index) + 1)
                        }
                        _ => format!(
                            "{} {}.{}",
                            zone.kind.label(),
                            zone_index + 1,
                            u16::from(index) + 1
                        ),
                    };
                    let mut item = Placement::at_ground(
                        &[],
                        model,
                        source.as_ref().unwrap().clone(),
                        label,
                        [point[0], geometry::height(recipe, point), point[1]],
                        extent,
                        yaw,
                    );
                    item.solid = solid;
                    if solid {
                        blockers.push(item.bounds(model));
                    }
                    if group == Group::Vegetation {
                        item.draw_distance = Some(if solid { 70.0 } else { 38.0 });
                        item.material.wind_response = if solid { 0.45 } else { 1.18 };
                    }
                    self.place(item, group, slot)?;
                    occupied.push(point);
                    placed = true;
                    break;
                }
                if !placed {
                    self.report.omitted += 1;
                }
            }
        }
        Ok(())
    }

    fn zone_asset(
        &mut self,
        kind: ZoneKind,
        variant: usize,
        known: &mut Vec<((ZoneKind, usize), Source)>,
    ) -> Result<Source, String> {
        let key = (kind, variant);
        if let Some((_, source)) = known.iter().find(|(item, _)| *item == key) {
            return Ok(source.clone());
        }
        let bytes = if kind == ZoneKind::Houses && self.recipe.style == Style::Natural {
            const COTTAGES: [&[u8]; 3] = [
                include_bytes!("../demo_assets/coastal/cottage-1.glb"),
                include_bytes!("../demo_assets/coastal/cottage-2.glb"),
                include_bytes!("../demo_assets/coastal/cottage-3.glb"),
            ];
            COTTAGES[variant]
        } else {
            kind.resource(self.recipe.style).0
        };
        let source = self.assets.import_bytes(bytes.to_vec())?;
        known.push((key, source.clone()));
        Ok(source)
    }

    fn kept(&self, group: Group, slot: u16) -> bool {
        self.managed
            .iter()
            .any(|entry| entry.group == group && entry.slot == slot)
    }

    fn place(&mut self, mut item: Placement, group: Group, slot: u16) -> Result<(), String> {
        if self.scene.world.models.len() >= MAX_PLACEMENTS {
            return Err("Освободи места для моделей деревни.".into());
        }
        item.id = self
            .reusable
            .get(&(group, slot))
            .copied()
            .unwrap_or_else(|| {
                let id = self.next_id;
                self.next_id = self.next_id.saturating_add(1);
                id
            });
        self.managed.push(Managed {
            group,
            slot,
            id: item.id,
            signature: signature(&item),
            locked: false,
        });
        self.scene.world.models.push(item);
        self.report.created += 1;
        Ok(())
    }

    fn colliders(&self) -> Vec<Collider> {
        let mut bounds = self.scene.world.colliders();
        bounds.extend(
            self.scene
                .world
                .models
                .iter()
                .filter(|item| item.solid && item.collider.is_none())
                .filter_map(|item| {
                    self.assets
                        .get(&item.source)
                        .map(|asset| item.bounds(&asset.model))
                }),
        );
        bounds
    }
}

fn signature(item: &Placement) -> [u8; 32] {
    Sha256::digest(serde_json::to_vec(item).expect("serializable placement")).into()
}

fn near(bounds: Collider, [x, z]: [f32; 2], gap: f32) -> bool {
    x > bounds.min[0] - gap
        && x < bounds.max[0] + gap
        && z > bounds.min[2] - gap
        && z < bounds.max[2] + gap
}

pub(crate) fn route_distance(point: [f32; 2], route: &[[f32; 2]]) -> f32 {
    let point = Vec2::from_array(point);
    route
        .windows(2)
        .map(|pair| {
            let start = Vec2::from_array(pair[0]);
            let segment = Vec2::from_array(pair[1]) - start;
            let fraction = ((point - start).dot(segment) / segment.length_squared().max(0.0001))
                .clamp(0.0, 1.0);
            point.distance(start + segment * fraction)
        })
        .fold(f32::INFINITY, f32::min)
}

struct Rng(u32);

impl Rng {
    fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / 16_777_216.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_village_seed_roundtrips_and_selectively_regenerates_with_houses_protected() {
        let recipe = Recipe::default();
        let mut scene = Scene::empty();
        let mut assets = Library::default();
        let report = generate(&mut scene, &mut assets, recipe.clone(), Scope::All).unwrap();
        assert!(report.route_clear, "{report:?}");
        assert!(report.created >= 17, "{report:?}");

        let mut second = Scene::empty();
        generate(&mut second, &mut assets, recipe.clone(), Scope::All).unwrap();
        assert_eq!(scene, second);
        let bytes = serde_json::to_vec(&scene).unwrap();
        let restored: Scene = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, scene);
        restored.validate().unwrap();

        let houses: Vec<_> = scene
            .village
            .as_ref()
            .unwrap()
            .houses()
            .map(|(id, _)| id)
            .collect();
        assert!(houses.len() >= 2, "{houses:?}");
        let edited = scene
            .world
            .models
            .iter_mut()
            .find(|item| item.id == houses[0])
            .unwrap();
        edited.label = "Мой рыбацкий дом".into();
        let edited = edited.clone();
        assert!(lock_house(&mut scene, houses[1], true));
        let locked = scene
            .world
            .models
            .iter()
            .find(|item| item.id == houses[1])
            .unwrap()
            .clone();
        let mut manual = locked.clone();
        manual.id = Placement::next_id(&scene.world.models);
        manual.label = "Дом, добавленный вручную".into();
        manual.position = [-16.4, 0.0, -26.0];
        scene.world.models.push(manual.clone());

        let coast_and_houses: Vec<_> = scene
            .world
            .models
            .iter()
            .filter(|item| {
                item.label == "Дорога по маршруту"
                    || item.label.contains("Дом")
                    || item.id == edited.id
            })
            .cloned()
            .collect();
        let flora_before: Vec<_> = scene
            .world
            .models
            .iter()
            .filter(|item| item.draw_distance.is_some())
            .cloned()
            .collect();
        let next = Recipe {
            seed: recipe.seed + 1,
            ..recipe
        };
        generate(&mut scene, &mut assets, next.clone(), Scope::Vegetation).unwrap();
        assert!(
            coast_and_houses
                .iter()
                .all(|item| scene.world.models.contains(item))
        );
        assert_ne!(
            flora_before,
            scene
                .world
                .models
                .iter()
                .filter(|item| item.draw_distance.is_some())
                .cloned()
                .collect::<Vec<_>>()
        );
        generate(&mut scene, &mut assets, next.clone(), Scope::Houses).unwrap();
        assert!(scene.world.models.contains(&edited));
        assert!(scene.world.models.contains(&locked));
        assert!(scene.world.models.contains(&manual));

        let road = scene
            .world
            .models
            .iter()
            .find(|item| item.label == "Дорога по маршруту")
            .unwrap()
            .clone();
        let mut rerouted = next;
        rerouted.route[2][0] += 1.8;
        generate(&mut scene, &mut assets, rerouted, Scope::Route).unwrap();
        let changed_road = scene
            .world
            .models
            .iter()
            .find(|item| item.id == road.id)
            .unwrap();
        assert_ne!(changed_road.source, road.source);
        assert!(assets.export_glb(&road.source).is_some());
        scene.validate().unwrap();
    }
}
