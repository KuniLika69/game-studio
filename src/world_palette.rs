//! Reusable scene composition using ordinary editable placements and NPC routes.
use eframe::egui;
use glam::Vec3;

use crate::{
    Scene, Studio, View,
    assets::{Library, MAX_PLACEMENTS, Placement},
    avatar::Avatar,
    environment::{Environment, WeatherPreset},
    interior::ModelCollider,
    npcs::{self, Npc, Waypoint},
};

macro_rules! mesh {
    ($name:literal) => {
        include_bytes!(concat!("../demo_assets/world_palette/", $name, ".glb"))
    };
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Item {
    #[default]
    Meadow,
    Grass,
    Flowers,
    Stone,
    Tree,
    House,
    Road,
    Sky,
    Planet,
    Fox,
}

impl Item {
    const ALL: [Self; 10] = [
        Self::Meadow,
        Self::Grass,
        Self::Flowers,
        Self::Stone,
        Self::Tree,
        Self::House,
        Self::Road,
        Self::Sky,
        Self::Planet,
        Self::Fox,
    ];
    const MEADOW: [Self; 5] = [
        Self::Grass,
        Self::Flowers,
        Self::Grass,
        Self::Stone,
        Self::Tree,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Meadow => "Луг · травы, цветы и деревья",
            Self::Grass => "Трава",
            Self::Flowers => "Полевые цветы",
            Self::Stone => "Слоистый камень",
            Self::Tree => "Дерево",
            Self::House => "Кирпичный дом · внешний вид",
            Self::Road => "Брусчатая дорога",
            Self::Sky => "Небо и время суток",
            Self::Planet => "Планета с кольцами",
            Self::Fox => "Лисёнок с маршрутом",
        }
    }

    fn resource(self, lowpoly: bool) -> (&'static [u8], f32) {
        match (self, lowpoly) {
            (Self::Grass, false) => (mesh!("grass-natural"), 1.7),
            (Self::Grass, true) => (mesh!("grass-lowpoly"), 1.7),
            (Self::Flowers, false) => (mesh!("flowers-natural"), 1.56),
            (Self::Flowers, true) => (mesh!("flowers-lowpoly"), 1.56),
            (Self::Stone, false) => (include_bytes!("../demo_assets/expedition/slate.glb"), 1.13),
            (Self::Stone, true) => (mesh!("boulder-lowpoly"), 1.05),
            (Self::Tree, false) => (include_bytes!("../demo_assets/expedition/alder.glb"), 3.85),
            (Self::Tree, true) => (mesh!("larch-lowpoly"), 2.65),
            (Self::House, false) => (mesh!("house-natural"), 4.08),
            (Self::House, true) => (mesh!("house-lowpoly"), 4.08),
            (Self::Road, false) => (mesh!("road-natural"), 4.0),
            (Self::Road, true) => (mesh!("road-lowpoly"), 4.0),
            (Self::Planet, _) => (mesh!("planet"), 4.0),
            _ => unreachable!("composer handles non-mesh items explicitly"),
        }
    }

    fn scatter(self) -> bool {
        matches!(
            self,
            Self::Meadow | Self::Grass | Self::Flowers | Self::Stone | Self::Tree
        )
    }
}

#[derive(Clone, Copy)]
struct Settings {
    item: Item,
    lowpoly: bool,
    seed: u32,
    count: usize,
    radius: f32,
    center: [f32; 2],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            item: Item::Meadow,
            lowpoly: false,
            seed: 27,
            count: 8,
            radius: 4.5,
            center: [0.0, -5.0],
        }
    }
}

enum Added {
    Scenery(u64),
    Actor(u64),
    Sky,
}

impl Studio {
    pub(super) fn world_palette(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Создание окружения", |ui| {
            let id = ui.id().with("world_palette");
            let mut settings = ui
                .data(|store| store.get_temp::<Settings>(id))
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("world_kind")
                .height(300.0)
                .selected_text(settings.item.label())
                .show_ui(ui, |ui| {
                    for item in Item::ALL {
                        ui.selectable_value(&mut settings.item, item, item.label());
                    }
                });
            if !matches!(settings.item, Item::Planet | Item::Fox) {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut settings.lowpoly, false, "Натура");
                    ui.selectable_value(&mut settings.lowpoly, true, "Лоу-поли");
                });
            }
            if settings.item != Item::Sky {
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut settings.center[0])
                            .prefix("X ")
                            .range(-80.0..=80.0),
                    );
                    ui.add(
                        egui::DragValue::new(&mut settings.center[1])
                            .prefix("Z ")
                            .range(-80.0..=80.0),
                    );
                });
            }
            if settings.item.scatter() {
                let minimum = if settings.item == Item::Meadow {
                    Item::MEADOW.len()
                } else {
                    1
                };
                settings.count = settings.count.max(minimum);
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut settings.count)
                            .prefix("Объекты ")
                            .range(minimum..=18),
                    );
                    ui.add(
                        egui::DragValue::new(&mut settings.radius)
                            .prefix("Радиус ")
                            .suffix(" м")
                            .speed(0.1)
                            .range(1.0..=20.0),
                    );
                });
                ui.add(
                    egui::DragValue::new(&mut settings.seed)
                        .prefix("Зерно ")
                        .range(0..=999_999),
                );
            }
            ui.data_mut(|store| store.insert_temp(id, settings));
            if ui.button("Добавить в сцену").clicked() {
                match compose(&mut self.scene, &mut self.assets, settings) {
                    Ok(added) => {
                        match added {
                            Added::Scenery(id) => {
                                self.gizmo
                                    .select(Some(id), false, &mut self.world_selection);
                                self.focus_camera();
                            }
                            Added::Actor(id) => {
                                self.selected_npc = Some(id);
                                self.editor_tab = crate::EditorTab::Npcs;
                                self.sync_npc_assets();
                                self.focus_camera();
                            }
                            Added::Sky => {}
                        }
                        self.view = View::Three;
                        self.status = format!("Добавлено · {}", settings.item.label());
                        self.changed();
                    }
                    Err(error) => self.status = error,
                }
            }
        });
    }
}

pub(super) fn enrich_sample(
    scene: &mut Scene,
    assets: &mut Library,
    quay: bool,
) -> Result<(), String> {
    let border = if quay { [3.52, -6.06] } else { [2.75, -7.0] };
    compose(
        scene,
        assets,
        Settings {
            item: Item::Flowers,
            center: border,
            count: if quay { 1 } else { 5 },
            radius: 1.35,
            seed: 187,
            ..Settings::default()
        },
    )?;
    if !quay {
        compose(
            scene,
            assets,
            Settings {
                item: Item::Fox,
                center: [2.75, -5.55],
                ..Settings::default()
            },
        )?;
    }
    Ok(())
}

fn compose(scene: &mut Scene, assets: &mut Library, config: Settings) -> Result<Added, String> {
    if config.item == Item::Sky {
        let environment = scene.environment.get_or_insert_with(Environment::default);
        environment.time.start_hour = if config.lowpoly { 18.8 } else { 10.5 };
        environment.weather.initial = WeatherPreset::Clear;
        return Ok(Added::Sky);
    }
    let [x, z] = config.center;
    let mut staged = assets.clone();
    if config.item == Item::Fox {
        let id = npcs::next_id(&scene.npcs).ok_or("Все позиции персонажей заняты.")?;
        let source = staged.import_bytes(mesh!("fox").to_vec())?;
        let model = &staged.get(&source).unwrap().model;
        let mut avatar = Avatar::new(source, "Лисёнок".into(), model);
        avatar.height = 0.67;
        let mut route = vec![
            Waypoint::at([x, 0.0, z]),
            Waypoint::at([x + 1.1, 0.0, z - 1.65]),
            Waypoint::at([x - 1.3, 0.0, z - 2.15]),
        ];
        route[0].wait = 1.1;
        let mut fox = Npc::new(id, avatar, route);
        fox.decorative = true;
        fox.speed = 0.88;
        scene.npcs.push(fox);
        *assets = staged;
        return Ok(Added::Actor(id));
    }

    let count = if config.item == Item::Meadow {
        config.count.max(Item::MEADOW.len())
    } else if config.item.scatter() {
        config.count
    } else {
        1
    };
    if count + scene.world.models.len() > MAX_PLACEMENTS {
        return Err(format!(
            "Свободных мест для моделей: {}. Выбери меньше объектов.",
            MAX_PLACEMENTS.saturating_sub(scene.world.models.len())
        ));
    }
    let mut states: Vec<(Item, crate::assets::Source)> = Vec::with_capacity(5);
    let mut models = scene.world.models.clone();
    let mut rng = config.seed ^ 0xa511_e9b3;
    let mut step = || {
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (rng >> 8) as f32 / 16_777_216.0
    };
    for i in 0..count {
        let item = if config.item == Item::Meadow {
            Item::MEADOW[i % Item::MEADOW.len()]
        } else {
            config.item
        };
        let (bytes, base) = item.resource(config.lowpoly);
        let known = states
            .iter()
            .find(|(entry, _)| *entry == item)
            .map(|(_, source)| source);
        let source = match known {
            Some(source) => source.clone(),
            None => {
                let source = staged.import_bytes(bytes.to_vec())?;
                states.push((item, source.clone()));
                source
            }
        };
        let distance = if count > 1 {
            ((i as f32 + 0.50) / count as f32).sqrt() * config.radius
        } else {
            0.0
        };
        let angle = i as f32 * 2.3999631 + step() * 0.45;
        let (sin, cos) = angle.sin_cos();
        let model = &staged.get(&source).unwrap().model;
        let scale = if count > 1 {
            base * (0.80 + step() * 0.40)
        } else {
            base
        };
        let yaw = if count > 1 { step() * 360.0 } else { 0.0 };
        let height = if item == Item::Planet { 5.5 } else { 0.0 };
        let point = [x + cos * distance, height, z + sin * distance];
        let mut placement = Placement::at_ground(
            &models,
            model,
            source,
            item.label().into(),
            point,
            scale,
            yaw,
        );
        placement.solid = matches!(item, Item::Tree | Item::Stone | Item::House);
        if item == Item::Tree {
            let offset = Vec3::from_array(point) - Vec3::from_array(placement.position);
            let center = glam::Quat::from_rotation_y(-yaw.to_radians()) * offset;
            placement.collider = Some(ModelCollider {
                center: [center.x, center.y + scale * 0.26, center.z],
                size: [scale * 0.092, scale * 0.52, scale * 0.092],
            });
        }
        placement.draw_distance = match item {
            Item::Grass | Item::Flowers => Some(38.0),
            Item::Tree | Item::Stone => Some(70.0),
            _ => None,
        };
        placement.material.wind_response = match item {
            Item::Grass | Item::Flowers => 1.22,
            Item::Tree => 0.51,
            _ => 0.0,
        };
        models.push(placement);
    }
    let first = models[scene.world.models.len()].id;
    scene.world.models = models;
    *assets = staged;
    Ok(Added::Scenery(first))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_palette_is_repeatable_and_fox_exports_animated_source() {
        let mut a = Scene::empty();
        let mut b = Scene::empty();
        let mut assets = Library::default();
        compose(&mut a, &mut assets, Settings::default()).unwrap();
        compose(&mut b, &mut assets, Settings::default()).unwrap();
        assert_eq!(a.world.models, b.world.models);
        assert_eq!(a.world.models.len(), 8);
        for item in [Item::House, Item::Road, Item::Planet, Item::Sky, Item::Fox] {
            compose(
                &mut a,
                &mut assets,
                Settings {
                    item,
                    lowpoly: true,
                    ..Settings::default()
                },
            )
            .unwrap();
        }
        assert!(a.npcs[0].avatar.walk.is_some() && a.npcs[0].avatar.run.is_some());
        assert_eq!(a.npcs[0].route.len(), 3);
        assert!(assets.export_glb(&a.npcs[0].avatar.source).is_some());
        a.validate().unwrap();
    }
}
