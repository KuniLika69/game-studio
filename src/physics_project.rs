use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    body_physics::{
        Axis, BodyKind, BodySpec, LayoutError, MAX_BODIES, MAX_SENSORS, Material, Sensor,
        SensorKind, Simulation, Solid,
    },
    world::Collider,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicsScene {
    pub bodies: Vec<NamedBody>,
    pub sensors: Vec<NamedSensor>,
    pub implicit_floor: bool,
    next_id: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedBody {
    pub name: String,
    #[serde(flatten)]
    pub spec: BodySpec,
    #[serde(default)]
    pub wind_drag: f32,
    #[serde(default)]
    pub buoyant: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedSensor {
    pub name: String,
    #[serde(flatten)]
    pub spec: Sensor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Addition {
    Crate,
    Platform,
    PressurePlate,
    OverlapTrigger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Body(u64),
    Sensor(u64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MillDemo {
    pub scene: PhysicsScene,
    pub floor: Solid,
    pub channel_floor: Solid,
    pub crate_body: u64,
    pub platform: u64,
    pub plate: u64,
    pub float_body: u64,
}

impl Default for PhysicsScene {
    fn default() -> Self {
        Self {
            bodies: Vec::new(),
            sensors: Vec::new(),
            implicit_floor: true,
            next_id: 0,
        }
    }
}

impl Selection {
    pub fn id(self) -> u64 {
        match self {
            Self::Body(id) | Self::Sensor(id) => id,
        }
    }
}

impl PhysicsScene {
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty() && self.sensors.is_empty()
    }

    pub fn is_default(&self) -> bool {
        self.is_empty() && self.next_id == 0 && self.implicit_floor
    }

    pub fn validate(&self) -> Result<(), String> {
        self.simulation().map(|_| ())
    }

    pub fn simulation(&self) -> Result<Simulation, String> {
        let mut ids = BTreeSet::new();
        if self.entries().any(|(binding, name)| {
            !valid_name(name) || binding.id() >= 1 << 54 || !ids.insert(binding.id())
        }) {
            return Err("Проверь названия и номера физических объектов.".into());
        }
        if self
            .bodies
            .iter()
            .any(|body| !(0.0..=12.0).contains(&body.wind_drag))
        {
            return Err("Сопротивление ветру должно быть от 0 до 12.".into());
        }
        let bodies: Vec<_> = self.bodies.iter().map(|body| body.spec).collect();
        let sensors: Vec<_> = self.sensors.iter().map(|sensor| sensor.spec).collect();
        Simulation::new(&bodies, &sensors).map_err(layout_message)
    }

    pub fn entries(&self) -> impl Iterator<Item = (Selection, &str)> + '_ {
        self.bodies
            .iter()
            .map(|body| (Selection::Body(body.spec.id), body.name.as_str()))
            .chain(
                self.sensors
                    .iter()
                    .map(|sensor| (Selection::Sensor(sensor.spec.id), sensor.name.as_str())),
            )
    }

    pub fn add(&mut self, addition: Addition, foot: [f32; 3]) -> Option<Selection> {
        let id = self.available_id()?;
        let (label, size) = match addition {
            Addition::Crate => ("Динамический ящик", [0.54; 3]),
            Addition::Platform => ("Движущаяся платформа", [1.2, 0.25, 1.2]),
            Addition::PressurePlate => ("Нажимная плита", [1.0, 0.08, 1.1]),
            Addition::OverlapTrigger => ("Контактный объём", [1.1, 1.6, 1.1]),
        };
        let half = size.map(|value| value * 0.5);
        let center = if addition == Addition::PressurePlate {
            center_from_top(foot, half)
        } else {
            center_from_foot(foot, half)
        };
        match addition {
            Addition::Crate | Addition::Platform => {
                let kind = if addition == Addition::Crate {
                    BodyKind::Dynamic {
                        mass: 12.0,
                        joint: None,
                    }
                } else {
                    BodyKind::Platform {
                        axis: Axis::Y,
                        travel: 1.0,
                        period: 4.0,
                    }
                };
                self.insert_body(
                    label,
                    BodySpec {
                        id,
                        center,
                        half_extents: half,
                        material: Material::default(),
                        kind,
                    },
                )
            }
            Addition::PressurePlate | Addition::OverlapTrigger => {
                let kind = if addition == Addition::PressurePlate {
                    SensorKind::Pressure { minimum_mass: 10.0 }
                } else {
                    SensorKind::Overlap
                };
                self.insert_sensor(
                    label,
                    Sensor {
                        id,
                        bounds: bounds(center, half),
                        kind,
                    },
                )
            }
        }
    }

    /// Derive a movable box from an existing collision rectangle.
    pub fn add_body_from_bounds(&mut self, collider: Collider) -> Option<Selection> {
        let id = self.available_id()?;
        let (center, half_extents) = center_half(collider);
        let kind = BodyKind::Dynamic {
            mass: 12.0,
            joint: None,
        };
        self.insert_body(
            "Тело из коллайдера",
            BodySpec {
                id,
                center,
                half_extents,
                material: Material::default(),
                kind,
            },
        )
    }

    pub fn height_at(&self, selection: Selection) -> Option<f32> {
        match selection {
            Selection::Body(id) => self
                .bodies
                .iter()
                .find(|entry| entry.spec.id == id)
                .map(|body| body.spec.center[1] - body.spec.half_extents[1]),
            Selection::Sensor(id) => {
                self.sensors
                    .iter()
                    .find(|entry| entry.spec.id == id)
                    .map(|entry| match entry.spec.kind {
                        SensorKind::Pressure { .. } => entry.spec.bounds.max[1],
                        SensorKind::Overlap => entry.spec.bounds.min[1],
                    })
            }
        }
    }

    pub fn remove(&mut self, selection: Selection) -> bool {
        match selection {
            Selection::Body(id) => {
                let before = self.bodies.len();
                self.bodies.retain(|body| body.spec.id != id);
                self.bodies.len() < before
            }
            Selection::Sensor(id) => {
                let before = self.sensors.len();
                self.sensors.retain(|sensor| sensor.spec.id != id);
                self.sensors.len() < before
            }
        }
    }

    /// Move a component while preserving its dimensions. The cursor marks the floor contact.
    pub fn place(&mut self, selection: Selection, foot: [f32; 3]) -> bool {
        match selection {
            Selection::Body(id) => {
                let Some(entry) = self.bodies.iter_mut().find(|entry| entry.spec.id == id) else {
                    return false;
                };
                let center = center_from_foot(foot, entry.spec.half_extents);
                let new = BodySpec {
                    center,
                    ..entry.spec
                };
                if new == entry.spec || Simulation::new(&[new], &[]).is_err() {
                    return false;
                }
                entry.spec = new;
            }
            Selection::Sensor(id) => {
                let Some(entry) = self.sensors.iter_mut().find(|entry| entry.spec.id == id) else {
                    return false;
                };
                let (_, half) = center_half(entry.spec.bounds);
                let center = match entry.spec.kind {
                    SensorKind::Overlap => center_from_foot(foot, half),
                    SensorKind::Pressure { .. } => center_from_top(foot, half),
                };
                let bounds = bounds(center, half);
                let new = Sensor {
                    bounds,
                    ..entry.spec
                };
                if new == entry.spec || Simulation::new(&[], &[new]).is_err() {
                    return false;
                }
                entry.spec = new;
            }
        };
        true
    }

    pub fn mill_demo() -> MillDemo {
        let mut scene = Self::default();
        let crate_body = scene.add(Addition::Crate, [-1.3, 0.0, 0.0]).unwrap().id();
        let platform = scene.add(Addition::Platform, [1.6, 0.0, 0.0]).unwrap().id();
        let plate = scene
            .add(Addition::PressurePlate, [0.0, 0.0, 0.0])
            .unwrap()
            .id();
        let float_body = scene
            .add_body_from_bounds(bounds([0.0, -0.22, 3.2], [0.31, 0.18, 0.31]))
            .unwrap()
            .id();
        let float = scene.bodies.last_mut().unwrap();
        float.name = "Поплавок мельницы".into();
        float.buoyant = true;
        scene.implicit_floor = false;
        let floor = bounds([0.0, -0.15, 0.0], [3.2, 0.15, 1.9]).into();
        let channel_floor = bounds([0.0, -0.55, 3.5], [2.0, 0.15, 1.5]).into();
        MillDemo {
            scene,
            floor,
            channel_floor,
            crate_body,
            platform,
            plate,
            float_body,
        }
    }

    fn available_id(&self) -> Option<u64> {
        let id = self
            .entries()
            .map(|(entry, _)| entry.id())
            .max()
            .unwrap_or(0)
            .checked_add(1)?;
        let id = id.max(self.next_id);
        (id < 1 << 54).then_some(id)
    }

    fn insert_body(&mut self, label: &str, spec: BodySpec) -> Option<Selection> {
        if self.bodies.len() >= MAX_BODIES || Simulation::new(&[spec], &[]).is_err() {
            return None;
        }
        self.next_id = spec.id + 1;
        self.bodies.push(NamedBody {
            name: format!("{label} {}", spec.id),
            spec,
            wind_drag: 0.0,
            buoyant: false,
        });
        Some(Selection::Body(spec.id))
    }

    fn insert_sensor(&mut self, label: &str, spec: Sensor) -> Option<Selection> {
        if self.sensors.len() >= MAX_SENSORS || Simulation::new(&[], &[spec]).is_err() {
            return None;
        }
        self.next_id = spec.id + 1;
        self.sensors.push(NamedSensor {
            name: format!("{label} {}", spec.id),
            spec,
        });
        Some(Selection::Sensor(spec.id))
    }
}

pub(super) fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control)
}

fn layout_message(error: LayoutError) -> String {
    match error {
        LayoutError::TooManyComponents => "В проекте превышен предел тел или датчиков.".into(),
        LayoutError::DuplicateId(id) => format!("Повторяется номер физического объекта #{id}."),
        LayoutError::InvalidId => {
            "Идентификатор физического объекта должен быть положительным.".into()
        }
        LayoutError::InvalidBody(id) => format!("Проверь размер и параметры тела #{id}."),
        LayoutError::InvalidSensor(id) => format!("Проверь границы и порог датчика #{id}."),
    }
}

pub(super) fn center_half(bounds: Collider) -> ([f32; 3], [f32; 3]) {
    (
        std::array::from_fn(|axis| (bounds.min[axis] + bounds.max[axis]) * 0.5),
        std::array::from_fn(|axis| (bounds.max[axis] - bounds.min[axis]) * 0.5),
    )
}

pub(super) fn bounds(center: [f32; 3], half: [f32; 3]) -> Collider {
    Collider {
        min: std::array::from_fn(|axis| center[axis] - half[axis]),
        max: std::array::from_fn(|axis| center[axis] + half[axis]),
    }
}

fn center_from_foot([x, y, z]: [f32; 3], [_, half_height, _]: [f32; 3]) -> [f32; 3] {
    [x, y + half_height, z]
}

fn center_from_top([x, y, z]: [f32; 3], [_, half_height, _]: [f32; 3]) -> [f32; 3] {
    [x, y - half_height, z]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_physics::Trigger;

    #[test]
    fn mill_crate_slides_onto_its_plate_then_releases_it() {
        let demo = PhysicsScene::mill_demo();
        let mut simulation = demo.scene.simulation().unwrap();
        assert!(simulation.push_impulse(demo.crate_body, [53.0, 0.0, 0.0]));
        let solids = [demo.floor];
        let mut pressed = false;
        for _ in 0..48 {
            let step = simulation.step(1.0 / 120.0, &solids, &[]);
            pressed |= step
                .events
                .iter()
                .any(|event| event.sensor_id == demo.plate && event.kind == Trigger::Pressed);
        }
        assert!(pressed);
        assert_eq!(simulation.plate_pressed(demo.plate), Some(true));
        assert!(simulation.push_impulse(demo.crate_body, [-80.0, 0.0, 0.0]));
        let mut release_count = 0;
        for _ in 0..75 {
            let step = simulation.step(1.0 / 120.0, &solids, &[]);
            release_count += step
                .events
                .iter()
                .filter(|event| event.sensor_id == demo.plate && event.kind == Trigger::Released)
                .count();
        }
        assert_eq!(release_count, 1);
    }

    #[test]
    fn collider_reuse_stable_ids_and_authored_placements_survive_save() {
        let original = Collider {
            min: [1.0, 0.5, -0.5],
            max: [2.0, 1.4, 1.2],
        };
        let mut scene = PhysicsScene::default();
        let first = scene.add_body_from_bounds(original).unwrap();
        let plate = scene.add(Addition::PressurePlate, [5.0, 1.0, 2.0]).unwrap();
        let mut simulation = scene.simulation().unwrap();
        let recreated = simulation.body(first.id()).unwrap().bounds;
        for axis in 0..3 {
            assert!((original.min[axis] - recreated.min[axis]).abs() < 1.0e-6);
            assert!((original.max[axis] - recreated.max[axis]).abs() < 1.0e-6);
        }
        assert_eq!(simulation.plate_pressed(plate.id()), Some(false));
        assert!(scene.place(plate, [7.0, 0.0, 2.0]));
        assert!(scene.remove(first));
        let bytes = serde_json::to_vec(&scene).unwrap();
        let mut reopened: PhysicsScene = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reopened, scene);
        let new_id = reopened.add_body_from_bounds(original).unwrap().id();
        assert!(new_id > plate.id());
        simulation = reopened.simulation().unwrap();
        assert!(simulation.body(first.id()).is_none());
        assert!(simulation.body(new_id).is_some());
    }
}
