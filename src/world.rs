use std::{collections::HashSet, ops::RangeInclusive};

use eframe::egui::{self, DragValue};
use serde::{Deserialize, Serialize};

use crate::{
    ImportError,
    assets::Placement,
    buildings::{self, Building},
    scene::{PLAYER_HEIGHT, PLAYER_RADIUS, Part, PartMotion, Shape},
};

#[path = "interact.rs"]
mod interact;
#[path = "world_gizmo.rs"]
mod world_gizmo;
pub(crate) use interact::{Door, configure_door};
pub use interact::{DoorSettings, Interaction, InteractionSnapshot, InteractionState, Seat};

const MAX_OBJECTS: usize = 96;
const FIRST_OBJECT_ID: u64 = 1 << 40;
const LAST_OBJECT_ID: u64 = (1 << 48) - 1;
const CHILD_ID_BIT: u64 = 1 << 63;
pub const MAX_KEY_PICKUPS: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct World {
    pub objects: Vec<WorldObject>,
    pub models: Vec<Placement>,
    pub buildings: Vec<Building>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<KeyPickup>,
    next_id: u64,
    next_building_id: u64,
    next_key_id: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyPickup {
    pub id: u32,
    pub name: String,
    pub position: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldObject {
    pub id: u64,
    pub name: String,
    pub kind: WorldKind,
    /// X and Z locate the center. Y locates the base, so Y=0 rests on the floor.
    pub position: [f32; 3],
    pub size: [f32; 3],
    #[serde(default, skip_serializing_if = "zero_rotation")]
    pub yaw: f32,
    pub color: [f32; 3],
    pub solid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldKind {
    Platform,
    Wall,
    Crate,
    Tree,
    Rock,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Collider {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Collider {
    pub(crate) fn intersects_segment(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        let mut enter: f32 = 0.0;
        let mut leave: f32 = 1.0;
        for axis in 0..3 {
            let delta = to[axis] - from[axis];
            if delta.abs() < 0.00001 {
                if from[axis] < self.min[axis] || from[axis] > self.max[axis] {
                    return false;
                }
                continue;
            }
            let a = (self.min[axis] - from[axis]) / delta;
            let b = (self.max[axis] - from[axis]) / delta;
            enter = enter.max(a.min(b));
            leave = leave.min(a.max(b));
            if enter > leave {
                return false;
            }
        }
        true
    }
}

impl WorldKind {
    const ALL: [Self; 5] = [
        Self::Platform,
        Self::Wall,
        Self::Crate,
        Self::Tree,
        Self::Rock,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Platform => "Платформа",
            Self::Wall => "Стена",
            Self::Crate => "Ящик",
            Self::Tree => "Дерево",
            Self::Rock => "Камень",
        }
    }

    fn defaults(self) -> ([f32; 3], [f32; 3]) {
        match self {
            Self::Platform => ([2.0, 0.65, 1.65], [0.19, 0.50, 0.53]),
            Self::Wall => ([2.2, 1.35, 0.42], [0.39, 0.47, 0.57]),
            Self::Crate => ([0.84, 0.84, 0.84], [0.64, 0.36, 0.15]),
            Self::Tree => ([1.45, 2.3, 1.45], [0.30, 0.58, 0.41]),
            Self::Rock => ([0.85, 0.5, 0.71], [0.40, 0.43, 0.52]),
        }
    }
}

impl World {
    pub fn key(&self, id: u32) -> Option<&KeyPickup> {
        self.keys.iter().find(|key| key.id == id)
    }

    pub fn add_key_at(&mut self, position: [f32; 3]) -> Option<u32> {
        if self.keys.len() >= MAX_KEY_PICKUPS || !valid_key_position(position) {
            return None;
        }
        let id = self
            .keys
            .iter()
            .map(|key| key.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)?
            .max(self.next_key_id);
        self.next_key_id = id.checked_add(1)?;
        self.keys.push(KeyPickup {
            id,
            name: format!("Ключ {id}"),
            position,
        });
        Some(id)
    }

    pub fn remove_key(&mut self, id: u32) -> bool {
        let Some(index) = self.keys.iter().position(|key| key.id == id) else {
            return false;
        };
        self.keys.remove(index);
        for building in &mut self.buildings {
            building.clear_key(id);
        }
        true
    }

    pub fn demo() -> Self {
        let mut world = Self::default();
        for (kind, position, size) in [
            (WorldKind::Platform, [-3.0, 0.0, 0.0], [1.75, 0.58, 2.0]),
            (WorldKind::Platform, [-4.85, 0.0, 0.0], [1.78, 1.18, 1.8]),
            (WorldKind::Crate, [2.1, 0.0, 0.05], [0.90, 0.76, 0.85]),
            (WorldKind::Tree, [-2.1, 0.0, -2.25], [1.35, 2.33, 1.24]),
            (WorldKind::Rock, [0.65, 0.0, -1.9], [0.92, 0.43, 0.71]),
            (WorldKind::Wall, [3.3, 0.0, -2.65], [2.25, 1.22, 0.42]),
            (WorldKind::Tree, [4.25, 0.0, -1.1], [1.40, 2.02, 1.34]),
        ] {
            world.add(kind);
            if let Some(object) = world.objects.last_mut() {
                object.position = position;
                object.size = size;
            }
        }
        world
    }

    pub(crate) fn add(&mut self, kind: WorldKind) -> Option<u64> {
        let id = self
            .objects
            .iter()
            .map(|object| object.id.saturating_add(1))
            .max()
            .unwrap_or(FIRST_OBJECT_ID)
            .max(self.next_id);
        if self.objects.len() >= MAX_OBJECTS || id > LAST_OBJECT_ID {
            return None;
        }
        let index = self.objects.len();
        let (size, color) = kind.defaults();
        self.objects.push(WorldObject {
            id,
            name: kind.label().into(),
            kind,
            position: [
                2.0 + (index % 5) as f32 * 1.25,
                0.0,
                -2.0 - (index / 5) as f32,
            ],
            size,
            yaw: 0.0,
            color,
            solid: true,
        });
        self.next_id = id + 1;
        Some(id)
    }

    pub fn validate(&self) -> Result<(), ImportError> {
        Placement::validate_all(&self.models)
            .map_err(|error| ImportError::new("Модели окружения", error))?;
        buildings::validate_all(&self.buildings, self.next_building_id)
            .map_err(|error| ImportError::new("Здания окружения", error))?;
        let mut key_ids = HashSet::with_capacity(self.keys.len());
        if self.keys.len() > MAX_KEY_PICKUPS
            || self.keys.iter().any(|key| {
                key.id == 0
                    || key.id == u32::MAX
                    || !key_ids.insert(key.id)
                    || key.name.trim().is_empty()
                    || key.name.chars().count() > 64
                    || key.name.chars().any(char::is_control)
                    || !valid_key_position(key.position)
            })
        {
            return Err(ImportError::new(
                "Ключи окружения",
                "Некорректные параметры ключей.",
            ));
        }
        if self.buildings.iter().flat_map(Building::doors).any(|door| {
            door.settings
                .required_key
                .is_some_and(|key| !key_ids.contains(&key))
        }) {
            return Err(ImportError::new(
                "Двери окружения",
                "Дверь ссылается на отсутствующий ключ.",
            ));
        }
        if self.objects.len() > MAX_OBJECTS {
            return Err(ImportError::new(
                "Объекты окружения",
                format!("Допускается не более {MAX_OBJECTS} объектов."),
            ));
        }
        if self.next_id > LAST_OBJECT_ID + 1 {
            return Err(ImportError::new(
                "Объекты окружения",
                "Некорректный счётчик идентификаторов окружения.",
            ));
        }
        let mut ids = HashSet::with_capacity(self.objects.len());
        for (index, object) in self.objects.iter().enumerate() {
            if !(FIRST_OBJECT_ID..=LAST_OBJECT_ID).contains(&object.id) || !ids.insert(object.id) {
                return Err(ImportError::new(
                    format!("Окружение, объект №{}", index + 1),
                    format!("Некорректный идентификатор {}.", object.id),
                ));
            }
            if object.name.trim().is_empty()
                || object.name.chars().count() > 64
                || object.name.chars().any(char::is_control)
                || !object.position.iter().all(|v| (-100.0..=100.0).contains(v))
                || !object.size.iter().all(|v| (0.05..=100.0).contains(v))
                || !(-3_600.0..=3_600.0).contains(&object.yaw)
                || !object.color.iter().all(|v| (0.0..=1.0).contains(v))
            {
                return Err(ImportError::new(
                    format!("Окружение, объект №{}", index + 1),
                    "Некорректные свойства объекта.",
                ));
            }
        }
        Ok(())
    }

    pub fn colliders(&self) -> Vec<Collider> {
        self.colliders_with(&InteractionState::default())
    }

    pub fn colliders_with(&self, state: &InteractionState) -> Vec<Collider> {
        self.colliders_with_models(state, &|_| true)
    }

    pub fn colliders_with_models(
        &self,
        state: &InteractionState,
        model_enabled: &impl Fn(u64) -> bool,
    ) -> Vec<Collider> {
        let mut colliders: Vec<_> = self
            .objects
            .iter()
            .filter(|object| object.solid && state.object_active(object.id))
            .map(WorldObject::collider)
            .chain(self.buildings.iter().flat_map(Building::colliders))
            .chain(self.buildings.iter().flat_map(Building::interior_colliders))
            .chain(
                self.models
                    .iter()
                    .filter(|model| state.object_active(model.id) && model_enabled(model.id))
                    .filter_map(Placement::custom_collider),
            )
            .collect();
        for door in self.buildings.iter().flat_map(Building::doors) {
            colliders.extend(door.colliders(state.fraction(&door)));
        }
        colliders
    }

    pub(super) fn door(&self, id: u64) -> Option<Door<'_>> {
        self.buildings
            .iter()
            .flat_map(Building::doors)
            .find(|door| door.id == id)
    }

    pub(crate) fn unlock_door_by_rule(&self, state: &mut InteractionState, id: u64) -> bool {
        self.door(id)
            .is_some_and(|door| state.unlock_by_rule(&door))
    }

    pub(crate) fn door_locked(&self, state: &InteractionState, id: u64) -> bool {
        self.door(id).is_some_and(|door| state.locked(&door))
    }

    pub fn entry_crossed(
        &self,
        state: &InteractionState,
        id: u64,
        from: [f32; 3],
        to: [f32; 3],
    ) -> bool {
        self.buildings
            .iter()
            .find(|building| building.id == id)
            .is_some_and(|building| building.entry_crossed(state, from, to))
    }

    pub fn interaction_near(
        &self,
        state: &InteractionState,
        player: [f32; 3],
    ) -> Option<Interaction> {
        self.interaction_near_with_models(state, player, &|_| true)
    }

    pub fn interaction_near_with_models(
        &self,
        state: &InteractionState,
        player: [f32; 3],
        model_enabled: &impl Fn(u64) -> bool,
    ) -> Option<Interaction> {
        if let Some(id) = self.nearby_key(state, player, model_enabled) {
            return Some(Interaction::Key { id });
        }
        let distance = |point: [f32; 3]| {
            let x = point[0] - player[0];
            let y = point[1] - player[1];
            let z = point[2] - player[2];
            x * x + y * y + z * z
        };
        let doors = self
            .buildings
            .iter()
            .flat_map(Building::doors)
            .filter_map(|door| {
                let range = distance(door.interaction_position());
                (range <= 1.75_f32.powi(2)).then(|| (range, state.door_interaction(&door)))
            });
        let seats = self
            .buildings
            .iter()
            .filter(|building| building.contains_interior(player[0], player[1], player[2]))
            .flat_map(|building| {
                building
                    .seats()
                    .filter(move |seat| building.seat_accessible(seat, player, state))
            })
            .filter_map(|seat| {
                let range = distance(seat.stand).min(distance(seat.position));
                (range <= 1.27_f32.powi(2)).then_some((range, Interaction::Seat(seat)))
            });
        let mut obstacles = None;
        let model_seats = self
            .models
            .iter()
            .filter(|model| state.object_active(model.id) && model_enabled(model.id))
            .filter_map(Placement::seat_pose)
            .filter_map(|seat| {
                let range = distance(seat.stand).min(distance(seat.position));
                if range > 1.27_f32.powi(2) {
                    return None;
                }
                let [x, y, z] = seat.stand;
                let from = [player[0], player[1] + 0.72, player[2]];
                let to = [x, y + 0.72, z];
                let clear = obstacles
                    .get_or_insert_with(|| self.colliders_with_models(state, model_enabled))
                    .iter()
                    .all(|collider| {
                        let clear_to_stand = y + PLAYER_HEIGHT <= collider.min[1] + 0.015
                            || y >= collider.max[1] - 0.015
                            || x + PLAYER_RADIUS <= collider.min[0]
                            || x - PLAYER_RADIUS >= collider.max[0]
                            || z + PLAYER_RADIUS <= collider.min[2]
                            || z - PLAYER_RADIUS >= collider.max[2];
                        clear_to_stand && !collider.intersects_segment(from, to)
                    });
                clear.then_some((range, Interaction::Seat(seat)))
            });
        doors
            .chain(seats)
            .chain(model_seats)
            .min_by(|(a, _), (b, _)| a.total_cmp(b))
            .map(|(_, interaction)| interaction)
    }

    pub fn interact(&self, state: &mut InteractionState, player: [f32; 3]) -> Option<Interaction> {
        self.interact_with_models(state, player, &|_| true)
    }

    pub fn interact_with_models(
        &self,
        state: &mut InteractionState,
        player: [f32; 3],
        model_enabled: &impl Fn(u64) -> bool,
    ) -> Option<Interaction> {
        let interaction = self.interaction_near_with_models(state, player, model_enabled)?;
        match interaction {
            Interaction::Door { id, .. } => {
                if let Some(door) = self.door(id) {
                    state.toggle_player(&door);
                }
            }
            Interaction::Key { id } => state.collect_key(id),
            _ => {}
        }
        Some(interaction)
    }

    fn nearby_key(
        &self,
        state: &InteractionState,
        player: [f32; 3],
        model_enabled: &impl Fn(u64) -> bool,
    ) -> Option<u32> {
        let hand = [player[0], player[1] + 0.98, player[2]];
        let mut colliders = None;
        self.keys
            .iter()
            .filter_map(|key| {
                let [x, y, z] = key.position;
                let horizontal = (x - player[0]).powi(2) + (z - player[2]).powi(2);
                let height = y - player[1];
                (!state.has_key(key.id)
                    && horizontal <= 1.15_f32.powi(2)
                    && (-0.28..=1.30).contains(&height))
                .then_some((key, horizontal + height.abs() * 0.15))
            })
            .filter(|(key, _)| {
                let [x, y, z] = key.position;
                let target = [x, y + 0.08, z];
                colliders
                    .get_or_insert_with(|| self.colliders_with_models(state, model_enabled))
                    .iter()
                    .all(|collider| !collider.intersects_segment(hand, target))
            })
            .min_by(|(a, va), (b, vb)| va.total_cmp(vb).then_with(|| a.id.cmp(&b.id)))
            .map(|(key, _)| key.id)
    }

    pub fn advance_interactions(&self, state: &mut InteractionState, dt: f32, player: [f32; 3]) {
        if dt.is_finite() && dt > 0.0 {
            for door in self.buildings.iter().flat_map(Building::doors) {
                state.advance(&door, dt, player);
            }
        }
    }

    pub fn add_building_at(&mut self, position: [f32; 3]) -> Option<u64> {
        buildings::add(&mut self.buildings, &mut self.next_building_id, position)
    }

    pub fn building_parts(&self, view_2d: bool) -> Vec<Part> {
        self.buildings
            .iter()
            .flat_map(|building| building.render_parts(view_2d))
            .collect()
    }

    pub fn building_parts_with(&self, view_2d: bool, state: &InteractionState) -> Vec<Part> {
        self.buildings
            .iter()
            .flat_map(|building| building.render_parts_with(view_2d, state))
            .collect()
    }

    pub fn render_parts(&self) -> Vec<Part> {
        self.render_active_parts(None, None)
    }

    pub fn render_active_parts(
        &self,
        state: Option<&InteractionState>,
        wind_at: Option<&dyn Fn(glam::Vec3) -> glam::Vec3>,
    ) -> Vec<Part> {
        let mut parts = Vec::with_capacity(self.objects.len() * 2);
        for object in self
            .objects
            .iter()
            .filter(|object| state.is_none_or(|state| state.object_active(object.id)))
        {
            let start = parts.len();
            object.append_parts(&mut parts);
            if object.kind == WorldKind::Tree
                && let Some(wind_at) = wind_at
            {
                for (index, crown) in parts[start + 1..].iter_mut().enumerate() {
                    let center = glam::Vec3::from_array(crown.position);
                    let air = wind_at(center).clamp_length_max(12.0);
                    let freedom = (0.015 + index as f32 * 0.008) * object.size[1].clamp(0.65, 3.2);
                    crown.position = (center + air * freedom).to_array();
                    crown.rotation[0] += air.z * 0.8;
                    crown.rotation[2] -= air.x * 0.8;
                }
            }
        }
        parts
    }
}

fn valid_key_position([x, y, z]: [f32; 3]) -> bool {
    (-256.0..=256.0).contains(&x) && (0.0..=100.0).contains(&y) && (-256.0..=256.0).contains(&z)
}

impl WorldObject {
    pub(crate) fn editor_bounds(&self) -> Collider {
        self.rotated_bounds(self.size)
    }

    pub(crate) fn collider(&self) -> Collider {
        let scale = match self.kind {
            WorldKind::Tree => [0.24, 0.69, 0.24],
            WorldKind::Rock => [0.82, 0.90, 0.82],
            _ => [1.0; 3],
        };
        self.rotated_bounds(std::array::from_fn(|i| self.size[i] * scale[i]))
    }

    fn rotated_bounds(&self, [width, height, depth]: [f32; 3]) -> Collider {
        let (sin, cos) = self.yaw.to_radians().sin_cos();
        let half_width = (cos.abs() * width + sin.abs() * depth) * 0.5;
        let half_depth = (sin.abs() * width + cos.abs() * depth) * 0.5;
        let [x, y, z] = self.position;
        Collider {
            min: [x - half_width, y, z - half_depth],
            max: [x + half_width, y + height, z + half_depth],
        }
    }

    fn append_parts(&self, parts: &mut Vec<Part>) {
        let (sin, cos) = self.yaw.to_radians().sin_cos();
        let mut index = 0;
        let mut add = |shape, offset: [f32; 3], scale: [f32; 3], color| {
            let id = if index == 0 {
                self.id
            } else {
                CHILD_ID_BIT | (self.id * 4 + index)
            };
            index += 1;
            let [x, y, z] = std::array::from_fn(|axis| self.size[axis] * offset[axis]);
            parts.push(Part {
                id,
                name: self.name.clone(),
                shape,
                position: [
                    self.position[0] + x * cos + z * sin,
                    self.position[1] + y,
                    self.position[2] - x * sin + z * cos,
                ],
                size: std::array::from_fn(|i| self.size[i] * scale[i]),
                rotation: [0.0, self.yaw, 0.0],
                color,
                motion: PartMotion::Static,
            });
        };
        let shade = |factor: f32| self.color.map(|channel| (channel * factor).clamp(0.0, 1.0));
        match self.kind {
            WorldKind::Platform | WorldKind::Wall => {
                add(Shape::Box, [0.0, 0.44, 0.0], [1.0, 0.88, 1.0], self.color);
                add(Shape::Box, [0.0, 0.94, 0.0], [1.0, 0.12, 1.0], shade(1.23));
            }
            WorldKind::Crate => {
                add(Shape::Box, [0.0, 0.47, 0.0], [0.96, 0.94, 0.96], self.color);
                for side in [-1.0, 1.0] {
                    add(
                        Shape::Box,
                        [side * 0.37, 0.48, 0.0],
                        [0.15, 0.96, 1.0],
                        shade(1.36),
                    );
                }
                add(Shape::Box, [0.0, 0.965, 0.0], [1.0, 0.07, 1.0], shade(0.82));
            }
            WorldKind::Tree => {
                add(
                    Shape::Cylinder,
                    [0.0, 0.345, 0.0],
                    [0.24, 0.69, 0.24],
                    [0.43, 0.29, 0.18],
                );
                add(
                    Shape::Sphere,
                    [0.0, 0.67, 0.0],
                    [1.0, 0.50, 1.0],
                    self.color,
                );
                add(
                    Shape::Sphere,
                    [-0.10, 0.83, 0.02],
                    [0.66, 0.34, 0.74],
                    shade(1.17),
                );
            }
            WorldKind::Rock => add(Shape::Sphere, [0.0, 0.5, 0.0], [1.0; 3], self.color),
        }
    }
}

fn zero_rotation(angle: &f32) -> bool {
    *angle == 0.0
}

pub fn list(
    ui: &mut egui::Ui,
    world: &mut World,
    selected: &mut Option<u64>,
    active: bool,
) -> bool {
    ui.add_enabled_ui(active, |ui| {
        let mut changed = false;
        ui.menu_button("Добавить объект", |ui| {
            for kind in WorldKind::ALL {
                if ui
                    .add_enabled(
                        world.objects.len() < MAX_OBJECTS,
                        egui::Button::new(kind.label()),
                    )
                    .clicked()
                    && let Some(id) = world.add(kind)
                {
                    *selected = Some(id);
                    changed = true;
                    ui.close();
                }
            }
        });
        if ui
            .add_enabled(
                world.buildings.len() < buildings::MAX_BUILDINGS,
                egui::Button::new("Построить дом"),
            )
            .clicked()
        {
            let index = world.buildings.len();
            let position = [
                7.2 + 7.4 * (index % 4) as f32,
                0.0,
                -3.1 - 5.7 * (index / 4) as f32,
            ];
            if let Some(id) = world.add_building_at(position) {
                *selected = Some(id);
                changed = true;
            }
        }
        if world.objects.is_empty() && world.models.is_empty() && world.buildings.is_empty() {
            ui.weak("Добавь объект или построй дом.");
        }
        egui::ScrollArea::vertical()
            .id_salt("world_objects")
            .max_height(220.0)
            .show(ui, |ui| {
                for building in &world.buildings {
                    ui.push_id(building.id, |ui| {
                        if ui
                            .selectable_label(*selected == Some(building.id), &building.name)
                            .clicked()
                        {
                            *selected = Some(building.id);
                        }
                    });
                }
                for object in &world.objects {
                    ui.push_id(object.id, |ui| {
                        if ui
                            .selectable_label(*selected == Some(object.id), &object.name)
                            .clicked()
                        {
                            *selected = Some(object.id);
                        }
                    });
                }
            });
        let can_delete = world
            .objects
            .iter()
            .any(|object| Some(object.id) == *selected)
            || world
                .buildings
                .iter()
                .any(|building| Some(building.id) == *selected);
        if ui
            .add_enabled(can_delete, egui::Button::new("Удалить объект"))
            .clicked()
        {
            world.objects.retain(|object| Some(object.id) != *selected);
            world
                .buildings
                .retain(|building| Some(building.id) != *selected);
            *selected = None;
            changed = true;
        }
        changed
    })
    .inner
}

pub fn inspector(
    ui: &mut egui::Ui,
    world: &mut World,
    selected: Option<u64>,
    active: bool,
) -> bool {
    if let Some(building) = world
        .buildings
        .iter_mut()
        .find(|building| Some(building.id) == selected)
    {
        return buildings::inspector(ui, building, active, &world.keys);
    }
    let Some(object) = world
        .objects
        .iter_mut()
        .find(|object| Some(object.id) == selected)
    else {
        ui.weak("Выбери объект окружения в списке.");
        return false;
    };
    ui.add_enabled_ui(active, |ui| {
        let name = ui.add(
            egui::TextEdit::singleline(&mut object.name)
                .char_limit(64)
                .hint_text("Название"),
        );
        let mut changed = name.changed();
        if name.lost_focus() && object.name.trim().is_empty() {
            object.name = object.kind.label().into();
            changed = true;
        }
        ui.weak(object.kind.label());
        changed |= vector(
            ui,
            "Позиция · низ объекта",
            &mut object.position,
            -100.0..=100.0,
        );
        changed |= vector(ui, "Размер", &mut object.size, 0.05..=100.0);
        changed |= ui
            .add(
                DragValue::new(&mut object.yaw)
                    .prefix("Поворот Y ")
                    .suffix("°")
                    .range(-3600.0..=3600.0),
            )
            .changed();
        changed |= ui
            .horizontal(|ui| {
                ui.label("Цвет");
                crate::inspector::edit_rgb(ui, &mut object.color)
            })
            .inner;
        changed |= ui.checkbox(&mut object.solid, "Столкновения").changed();
        if object.solid && object.kind == WorldKind::Tree {
            ui.weak("Робот сталкивается со стволом дерева.");
        }
        changed
    })
    .inner
}

fn vector(
    ui: &mut egui::Ui,
    title: &str,
    values: &mut [f32; 3],
    range: RangeInclusive<f32>,
) -> bool {
    ui.label(title);
    ui.horizontal_wrapped(|ui| {
        let mut changed = false;
        for (axis, value) in ["X ", "Y ", "Z "].into_iter().zip(values) {
            changed |= ui
                .add(
                    DragValue::new(value)
                        .prefix(axis)
                        .range(range.clone())
                        .speed(0.04)
                        .max_decimals(2),
                )
                .changed();
        }
        changed
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampled_wind_bends_only_native_tree_crowns() {
        let mut world = World::default();
        world.add(WorldKind::Tree).unwrap();
        let still = world.render_parts();
        let windy = world.render_active_parts(None, Some(&|_| glam::Vec3::X * 5.0));
        assert_eq!(still[0], windy[0]);
        for (ordinary, displaced) in still[1..].iter().zip(&windy[1..]) {
            assert_eq!(ordinary.id, displaced.id);
            assert!(displaced.position[0] > ordinary.position[0] + 0.10);
            assert!(displaced.rotation[2] < ordinary.rotation[2]);
        }
        assert_eq!(still, world.render_parts());
    }
    use crate::{
        interior::{Axis, FurnitureKind},
        scene::Game,
    };

    #[test]
    fn door_motion_updates_both_views_and_walk_through_without_changing_project() {
        let mut world = World::default();
        world.add_building_at([0.0, 0.0, -3.5]).unwrap();
        world.buildings[0].door = Some(DoorSettings::default());
        let house = &world.buildings[0];
        let mut player = Game::default();
        let mut state = InteractionState::default();
        for _ in 0..70 {
            player.update(
                0.016,
                [0.0, -1.0],
                false,
                false,
                &world.colliders_with(&state),
            );
        }
        assert!(!house.contains_interior(player.x, player.height, player.z));
        let player_position = [player.x, player.height, player.z];
        let Some(Interaction::Door { id, opening: true }) =
            world.interact(&mut state, player_position)
        else {
            panic!("closed door should be reachable from its threshold");
        };
        for _ in 0..12 {
            world.advance_interactions(&mut state, 0.016, player_position);
        }
        let leaf = |plan, state: &InteractionState| {
            world
                .building_parts_with(plan, state)
                .into_iter()
                .find(|part| part.id == id)
                .unwrap()
        };
        let midway = leaf(false, &state);
        assert!(midway.rotation[1] > 0.0 && midway.rotation[1] < 90.0);
        assert_eq!(midway, leaf(true, &state));
        for _ in 0..30 {
            world.advance_interactions(&mut state, 0.016, player_position);
        }
        assert_eq!(leaf(true, &state).rotation[1], 90.0);
        for _ in 0..70 {
            player.update(
                0.016,
                [0.0, -1.0],
                false,
                false,
                &world.colliders_with(&state),
            );
        }
        assert!(house.contains_interior(player.x, player.height, player.z));
        let reopened: World = serde_json::from_slice(&serde_json::to_vec(&world).unwrap()).unwrap();
        let saved_leaf = reopened
            .building_parts(true)
            .into_iter()
            .find(|part| part.id == id)
            .unwrap();
        assert_eq!(saved_leaf.rotation[1], 0.0);
    }

    #[test]
    fn missing_door_configuration_preserves_legacy_openings() {
        let mut world = World::default();
        world.add_building_at([0.0; 3]).unwrap();
        world.buildings[0].door = Some(DoorSettings::default());
        let mut value = serde_json::to_value(&world).unwrap();
        value["buildings"][0]
            .as_object_mut()
            .unwrap()
            .remove("door");
        let legacy: World = serde_json::from_value(value).unwrap();
        assert!(legacy.buildings[0].door.is_none());
        assert_eq!(legacy.colliders(), legacy.buildings[0].colliders());
        assert!(
            legacy
                .interaction_near(&InteractionState::default(), [0.0, 0.0, 2.8])
                .is_none()
        );
    }

    #[test]
    fn closing_waits_when_player_stands_in_swing_path() {
        let mut world = World::default();
        world.add_building_at([0.0, 0.0, -3.5]).unwrap();
        world.buildings[0].door = Some(DoorSettings {
            initially_open: true,
            ..Default::default()
        });
        let door = world.buildings[0].doors().next().unwrap();
        let position = door.interaction_position();
        let mut state = InteractionState::default();
        assert_eq!(
            world.interact(&mut state, position),
            Some(Interaction::Door {
                id: door.id,
                opening: false
            })
        );
        for _ in 0..35 {
            world.advance_interactions(&mut state, 0.032, position);
        }
        assert!(state.fraction(&door) > 0.0);
        for _ in 0..35 {
            world.advance_interactions(&mut state, 0.032, [0.0; 3]);
        }
        assert_eq!(state.fraction(&door), 0.0);
    }

    #[test]
    fn turns_interior_doors_and_seats_into_world_space() {
        let mut world = World::default();
        world.add_building_at([3.0, 0.0, -4.0]).unwrap();
        let house = &mut world.buildings[0];
        house.quarter_turns = 1;
        house.door = Some(DoorSettings::default());
        house.interior.add_partition(house.size).unwrap();
        let partition = &mut house.interior.partitions[0];
        partition.axis = Axis::X;
        partition.opening.as_mut().unwrap().door = Some(DoorSettings {
            initially_open: true,
            ..Default::default()
        });
        house
            .interior
            .add_furniture(FurnitureKind::Chair, [1.63, -1.64], house.size)
            .unwrap();
        let seat = house.seats().next().unwrap();
        assert_eq!(seat.yaw, 90.0);
        assert!(seat.stand[0] > seat.position[0]);
        assert!(seat.position[1] > 0.5);
        let partition_door = house.doors().nth(1).unwrap();
        assert_eq!(partition_door.yaw, 90.0);
        let id = partition_door.id;
        let action = world.interaction_near(&InteractionState::default(), seat.stand);
        assert_eq!(action, Some(Interaction::Seat(seat)));
        assert_eq!(
            world
                .building_parts(true)
                .into_iter()
                .find(|part| part.id == id)
                .unwrap()
                .rotation[1],
            180.0
        );
    }

    #[test]
    fn positions_collider_from_object_base_and_honors_solid_toggle() {
        let mut world = World::default();
        world.add(WorldKind::Platform);
        world.objects[0].position = [2.0, 0.5, -3.0];
        world.objects[0].size = [4.0, 0.75, 2.0];
        assert_eq!(
            world.colliders(),
            [Collider {
                min: [0.0, 0.5, -4.0],
                max: [4.0, 1.25, -2.0]
            }]
        );
        world.objects[0].solid = false;
        assert!(world.colliders().is_empty());
        assert_eq!(world.render_parts().len(), 2);
    }

    #[test]
    fn retains_distinct_parts_and_objects_after_reload() {
        let mut world = World::demo();
        world.validate().unwrap();
        let ids: HashSet<_> = world.render_parts().iter().map(|part| part.id).collect();
        assert_eq!(ids.len(), world.render_parts().len());
        let removed = world.objects.pop().unwrap().id;
        let bytes = serde_json::to_vec(&world).unwrap();
        let mut loaded: World = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(loaded, world);
        assert!(loaded.add(WorldKind::Rock).unwrap() > removed);
    }

    #[test]
    fn matching_key_unlocks_only_the_selected_door_and_restores_it() {
        let mut world = World::default();
        world.add_building_at([0.0; 3]).unwrap();
        world.add_building_at([10.0, 0.0, 0.0]).unwrap();
        let key = world.add_key_at([3.5, 0.0, 3.1]).unwrap();
        let wrong = world.add_key_at([-3.5, 0.0, 3.1]).unwrap();
        for building in &mut world.buildings {
            building.door = Some(DoorSettings {
                initially_open: true,
                required_key: Some(key),
                ..Default::default()
            });
        }
        world.validate().unwrap();
        let door = world.buildings[0].doors().next().unwrap();
        let other = world.buildings[1].entrance_id().unwrap();
        let outside = [0.0, 0.0, 2.9];
        let mut state = InteractionState::default();
        assert_eq!(
            world.interact(&mut state, world.key(wrong).unwrap().position),
            Some(Interaction::Key { id: wrong })
        );
        assert_eq!(
            world.interact(&mut state, outside),
            Some(Interaction::LockedDoor { id: door.id, key })
        );
        assert_eq!(state.fraction(&door), 0.0);

        let mut player = Game::default();
        player.z = outside[2];
        for _ in 0..60 {
            player.update(
                1.0 / 60.0,
                [0.0, -1.0],
                false,
                false,
                &world.colliders_with(&state),
            );
        }
        assert!(!world.buildings[0].contains_interior(player.x, player.height, player.z));
        assert_eq!(
            world.interact(&mut state, world.key(key).unwrap().position),
            Some(Interaction::Key { id: key })
        );
        state.toggle(&door);
        assert_eq!(
            state.fraction(&door),
            0.0,
            "NPCs cannot unlock doors with the player's inventory"
        );
        assert_eq!(
            world.interact(&mut state, outside),
            Some(Interaction::Door {
                id: door.id,
                opening: true
            })
        );
        assert!(!world.door_locked(&state, door.id) && world.door_locked(&state, other));
        for _ in 0..5 {
            world.advance_interactions(&mut state, 0.05, outside);
        }
        let saved_fraction = state.fraction(&door);
        assert!((0.1..0.9).contains(&saved_fraction));
        let serialized = serde_json::to_vec(&state.snapshot()).unwrap();
        let saved: InteractionSnapshot = serde_json::from_slice(&serialized).unwrap();
        saved.validate().unwrap();
        let mut resumed = InteractionState::default();
        resumed.restore(&world, &saved);
        assert_eq!(resumed.key_ids().collect::<Vec<_>>(), [key, wrong]);
        assert_eq!(resumed.fraction(&door), saved_fraction);
        for _ in 0..12 {
            world.advance_interactions(&mut resumed, 0.05, outside);
        }
        assert_eq!(resumed.fraction(&door), 1.0);
        let mut entered = false;
        for _ in 0..60 {
            let from = [player.x, player.height, player.z];
            player.update(
                1.0 / 60.0,
                [0.0, -1.0],
                false,
                false,
                &world.colliders_with(&resumed),
            );
            entered |= world.entry_crossed(
                &resumed,
                world.buildings[0].id,
                from,
                [player.x, player.height, player.z],
            );
        }
        assert!(entered);
        assert!(world.buildings[0].contains_interior(player.x, player.height, player.z));
        assert!(world.door_locked(&resumed, other));
        assert_eq!(
            world.interact(&mut resumed, outside),
            Some(Interaction::Door {
                id: door.id,
                opening: false
            })
        );
        for _ in 0..12 {
            world.advance_interactions(&mut resumed, 0.05, outside);
        }
        assert_eq!(resumed.fraction(&door), 0.0);
        assert!(!world.door_locked(&resumed, door.id));
        assert_eq!(
            world.interact(&mut resumed, outside),
            Some(Interaction::Door {
                id: door.id,
                opening: true
            })
        );
        assert!(resumed.has_key(key));

        assert!(world.remove_key(key));
        world.validate().unwrap();
        assert!(
            world
                .buildings
                .iter()
                .all(|house| house.door.unwrap().required_key.is_none())
        );
        assert!(world.add_key_at([0.0; 3]).unwrap() > wrong);
        resumed.restore(&world, &saved);
        assert!(!resumed.has_key(key));
        resumed.snapshot().validate().unwrap();
    }

    #[test]
    fn cannot_pick_up_a_key_through_a_closed_door() {
        let mut world = World::default();
        world.add_building_at([0.0; 3]).unwrap();
        world.buildings[0].door = Some(DoorSettings::default());
        let key = world.add_key_at([0.0, 0.0, 1.92]).unwrap();
        let mut state = InteractionState::default();
        let outside = [0.0, 0.0, 2.85];
        assert!(matches!(
            world.interact(&mut state, outside),
            Some(Interaction::Door { .. })
        ));
        assert!(!state.has_key(key));
        for _ in 0..12 {
            world.advance_interactions(&mut state, 0.05, outside);
        }
        assert_eq!(
            world.interact(&mut state, outside),
            Some(Interaction::Key { id: key })
        );
        assert!(state.has_key(key));
    }
}
