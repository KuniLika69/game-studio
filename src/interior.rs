use std::collections::HashSet;

use eframe::egui::{self, DragValue};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::{
    buildings::{FIRST_ID, LAST_ID, Surface, WALL},
    world::{Collider, DoorSettings, KeyPickup, configure_door},
};

const MAX_WALLS: usize = 12;
const MAX_FURNITURE: usize = 32;
const MAX_ID: u32 = u16::MAX as u32;
const TAG: u64 = 3 << 62;
const INNER_WALL: f32 = 0.16;
const MIN_DOOR_WIDTH: f32 = 1.10;
const MIN_DOOR_HEIGHT: f32 = 2.35;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Interior {
    pub partitions: Vec<Partition>,
    pub furniture: Vec<Furniture>,
    next_id: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Partition {
    pub id: u32,
    pub name: String,
    pub axis: Axis,
    pub position: [f32; 2],
    pub length: f32,
    pub opening: Option<Opening>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Z,
}

impl Axis {
    fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Z => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Opening {
    pub width: f32,
    pub height: f32,
    pub offset: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door: Option<DoorSettings>,
}

impl Default for Opening {
    fn default() -> Self {
        Self {
            width: 1.44,
            height: 2.50,
            offset: 0.0,
            door: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Furniture {
    pub id: u32,
    pub name: String,
    pub kind: FurnitureKind,
    pub position: [f32; 2],
    pub size: [f32; 3],
    pub quarter_turns: u8,
    pub color: [f32; 3],
    pub solid: bool,
}

/// Oriented with the model's yaw but measured in scene meters, independent of its visual scale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelCollider {
    pub center: [f32; 3],
    pub size: [f32; 3],
}

impl ModelCollider {
    pub fn from_visual_bounds(min: [f32; 3], max: [f32; 3], scale: [f32; 3]) -> Self {
        let min = Vec3::from_array(min) * Vec3::from_array(scale);
        let max = Vec3::from_array(max) * Vec3::from_array(scale);
        Self {
            center: ((min + max) * 0.5)
                .clamp(Vec3::splat(-128.0), Vec3::splat(128.0))
                .to_array(),
            size: (max - min)
                .clamp(Vec3::splat(0.03), Vec3::splat(128.0))
                .to_array(),
        }
    }

    pub fn valid(self) -> bool {
        valid_offsets(self.center)
            && self
                .size
                .into_iter()
                .all(|value| (0.03..=128.0).contains(&value))
    }

    pub fn world(self, pivot: [f32; 3], yaw: f32) -> Collider {
        let rotation = Quat::from_rotation_y(yaw.to_radians());
        let center = Vec3::from_array(pivot) + rotation * Vec3::from_array(self.center);
        let radius = Vec3::from_array(self.size) * 0.5;
        let extent = (rotation * Vec3::X).abs() * radius.x
            + Vec3::Y * radius.y
            + (rotation * Vec3::Z).abs() * radius.z;
        Collider {
            min: (center - extent).to_array(),
            max: (center + extent).to_array(),
        }
    }
}

/// A position for the hip root, a separate place to stand, and a facing angle relative to the model.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeatAnchor {
    pub sit: [f32; 3],
    pub stand: [f32; 3],
    pub yaw: f32,
}

impl SeatAnchor {
    pub fn from_collider(bounds: ModelCollider) -> Self {
        let [x, y, z] = bounds.center;
        let floor = y - bounds.size[1] * 0.5;
        Self {
            sit: [x, (floor + bounds.size[1] * 0.55).clamp(-128.0, 128.0), z],
            stand: [
                x,
                floor.clamp(-128.0, 128.0),
                (z + bounds.size[2] * 0.5 + 0.54).clamp(-128.0, 128.0),
            ],
            yaw: 0.0,
        }
    }

    pub fn valid(self) -> bool {
        valid_offsets(self.sit) && valid_offsets(self.stand) && (-360.0..=360.0).contains(&self.yaw)
    }

    pub fn world(self, id: u64, pivot: [f32; 3], yaw: f32) -> crate::world::Seat {
        let rotation = Quat::from_rotation_y(yaw.to_radians());
        let pivot = Vec3::from_array(pivot);
        let locate = |offset| (pivot + rotation * Vec3::from_array(offset)).to_array();
        crate::world::Seat {
            id,
            position: locate(self.sit),
            stand: locate(self.stand),
            yaw: (yaw + self.yaw).rem_euclid(360.0),
        }
    }
}

fn valid_offsets(values: [f32; 3]) -> bool {
    values
        .into_iter()
        .all(|value| (-128.0..=128.0).contains(&value))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FurnitureKind {
    Chair,
    Table,
    Bed,
    Sofa,
    Cabinet,
}

impl FurnitureKind {
    const ALL: [Self; 5] = [
        Self::Chair,
        Self::Table,
        Self::Bed,
        Self::Sofa,
        Self::Cabinet,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Chair => "Стул",
            Self::Table => "Стол",
            Self::Bed => "Кровать",
            Self::Sofa => "Диван",
            Self::Cabinet => "Шкаф",
        }
    }

    fn defaults(self) -> ([f32; 3], [f32; 3]) {
        match self {
            Self::Chair => ([0.57, 0.95, 0.57], [0.54, 0.35, 0.23]),
            Self::Table => ([1.31, 0.80, 0.83], [0.55, 0.37, 0.24]),
            Self::Bed => ([1.30, 0.86, 1.97], [0.33, 0.51, 0.45]),
            Self::Sofa => ([1.91, 0.91, 0.87], [0.38, 0.50, 0.56]),
            Self::Cabinet => ([1.12, 1.93, 0.53], [0.45, 0.31, 0.22]),
        }
    }
}

fn floor_half(size: [f32; 3]) -> [f32; 2] {
    [size[0] * 0.5 - WALL, size[2] * 0.5 - WALL]
}

pub(crate) fn part_id(building: u64, item: u32, index: u64, surface: Surface) -> u64 {
    TAG | ((building - FIRST_ID) << 23) | (u64::from(item) << 7) | ((surface as u64) << 4) | index
}

pub(crate) fn building_id(part_id: u64) -> Option<u64> {
    if part_id & TAG != TAG {
        return None;
    }
    let id = FIRST_ID + ((part_id & !TAG) >> 23);
    (id <= LAST_ID).then_some(id)
}

pub(crate) fn surface(part_id: u64) -> Option<Surface> {
    building_id(part_id)?;
    match (part_id >> 4) & 7 {
        1 => Some(Surface::Wall),
        2 => Some(Surface::Floor),
        5 => Some(Surface::Metal),
        6 => Some(Surface::Wood),
        _ => None,
    }
}

pub fn item_id(part_id: u64) -> Option<u32> {
    building_id(part_id)?;
    Some(((part_id >> 7) & u64::from(MAX_ID)) as u32)
}

impl Interior {
    fn allocate(&mut self) -> Option<u32> {
        let next = self
            .partitions
            .iter()
            .map(|wall| wall.id)
            .chain(self.furniture.iter().map(|item| item.id))
            .max()
            .unwrap_or(0)
            + 1;
        let id = next.max(self.next_id);
        if id > MAX_ID {
            return None;
        }
        self.next_id = id + 1;
        Some(id)
    }

    pub fn add_partition(&mut self, size: [f32; 3]) -> Option<u32> {
        if self.partitions.len() >= MAX_WALLS {
            return None;
        }
        let id = self.allocate()?;
        let half = floor_half(size);
        let shift = (self.partitions.len() as f32) * 0.60 - 0.28;
        let mut partition = Partition {
            id,
            name: format!("Перегородка {}", self.partitions.len() + 1),
            axis: Axis::X,
            position: [0.0, shift],
            length: half[0] * 2.0,
            opening: Some(Opening::default()),
        };
        partition.fit(size);
        self.partitions.push(partition);
        Some(id)
    }

    pub fn add_furniture(
        &mut self,
        kind: FurnitureKind,
        position: [f32; 2],
        size: [f32; 3],
    ) -> Option<u32> {
        if self.furniture.len() >= MAX_FURNITURE || !position.into_iter().all(f32::is_finite) {
            return None;
        }
        let id = self.allocate()?;
        let (dimensions, color) = kind.defaults();
        let mut item = Furniture {
            id,
            name: kind.label().into(),
            kind,
            position,
            size: dimensions,
            quarter_turns: 0,
            color,
            solid: true,
        };
        item.fit(size);
        self.furniture.push(item);
        Some(id)
    }

    pub fn demo(size: [f32; 3]) -> Self {
        let mut interior = Self::default();
        interior.add_partition(size);
        if let Some(opening) = interior
            .partitions
            .first_mut()
            .and_then(|wall| wall.opening.as_mut())
        {
            opening.width = 1.66;
        }
        for (kind, position, turn) in [
            (FurnitureKind::Sofa, [-size[0] * 0.35, size[2] * 0.24], 1),
            (FurnitureKind::Table, [size[0] * 0.24, size[2] * 0.25], 0),
            (FurnitureKind::Chair, [size[0] * 0.37, size[2] * 0.25], 3),
            (FurnitureKind::Bed, [size[0] * 0.28, -size[2] * 0.30], 0),
            (
                FurnitureKind::Cabinet,
                [-size[0] * 0.32, -size[2] * 0.36],
                0,
            ),
        ] {
            interior.add_furniture(kind, position, size);
            if let Some(item) = interior.furniture.last_mut() {
                item.quarter_turns = turn;
            }
        }
        interior.fit(size);
        interior
    }

    pub fn furniture_at(&self, position: [f32; 2]) -> Option<u32> {
        self.furniture.iter().rev().find_map(|item| {
            let bounds = item.bounds();
            ((bounds.min[0]..=bounds.max[0]).contains(&position[0])
                && (bounds.min[2]..=bounds.max[2]).contains(&position[1]))
            .then_some(item.id)
        })
    }

    pub fn move_furniture_at(&mut self, id: u32, position: [f32; 2], size: [f32; 3]) -> bool {
        if !position
            .into_iter()
            .zip(floor_half(size))
            .all(|(value, half)| (-half..=half).contains(&value))
        {
            return false;
        }
        let Some(item) = self.furniture.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        let previous = item.position;
        item.position = position;
        item.fit(size);
        item.position != previous
    }

    pub(crate) fn fit(&mut self, size: [f32; 3]) {
        for wall in &mut self.partitions {
            wall.fit(size);
        }
        for item in &mut self.furniture {
            item.fit(size);
        }
    }

    pub(crate) fn validate(&self, size: [f32; 3]) -> Result<(), String> {
        if self.partitions.len() > MAX_WALLS
            || self.furniture.len() > MAX_FURNITURE
            || self.next_id > MAX_ID + 1
        {
            return Err("Слишком много элементов интерьера дома".into());
        }
        let mut ids = HashSet::new();
        let mut check_id = |id| (1..=MAX_ID).contains(&id) && ids.insert(id);
        for wall in &self.partitions {
            let mut fitted = wall.clone();
            fitted.fit(size);
            if !check_id(wall.id) || !valid_name(&wall.name) || fitted != *wall {
                return Err("Некорректная перегородка внутри дома".into());
            }
        }
        for item in &self.furniture {
            let mut fitted = item.clone();
            fitted.fit(size);
            if !check_id(item.id)
                || !valid_name(&item.name)
                || item.quarter_turns > 3
                || !item.color.into_iter().all(|c| (0.0..=1.0).contains(&c))
                || fitted != *item
            {
                return Err("Некорректный предмет мебели внутри дома".into());
            }
        }
        Ok(())
    }
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control)
}

impl Partition {
    fn fit(&mut self, size: [f32; 3]) {
        let half = floor_half(size);
        let axis = self.axis.index();
        let min_length = if self.opening.is_some() { 1.5 } else { 0.40 };
        self.length = self.length.clamp(min_length, half[axis] * 2.0);
        let along = (half[axis] - self.length * 0.5).max(0.0);
        let across = half[1 - axis] - INNER_WALL * 0.5;
        self.position[axis] = self.position[axis].clamp(-along, along);
        self.position[1 - axis] = self.position[1 - axis].clamp(-across, across);
        if let Some(opening) = &mut self.opening {
            opening.width = opening.width.clamp(MIN_DOOR_WIDTH, self.length - 0.28);
            opening.height = opening.height.clamp(MIN_DOOR_HEIGHT, size[1] - 0.20);
            let limit = ((self.length - opening.width) * 0.5 - 0.14).max(0.0);
            opening.offset = opening.offset.clamp(-limit, limit);
        }
    }

    fn segment(&self, along: [f32; 2], height: [f32; 2]) -> Collider {
        let [x, z] = self.position;
        let half = INNER_WALL * 0.5;
        let [a, b] = along;
        let [bottom, top] = height;
        match self.axis {
            Axis::X => Collider {
                min: [x + a, bottom, z - half],
                max: [x + b, top, z + half],
            },
            Axis::Z => Collider {
                min: [x - half, bottom, z + a],
                max: [x + half, top, z + b],
            },
        }
    }

    pub(crate) fn door(&self) -> Option<Collider> {
        self.opening.as_ref().map(|opening| {
            let half = opening.width * 0.5;
            self.segment(
                [opening.offset - half, opening.offset + half],
                [0.0, opening.height],
            )
        })
    }

    pub(crate) fn solids(&self, wall_height: f32) -> impl Iterator<Item = Collider> {
        let half = self.length * 0.5;
        let height = [0.0, wall_height];
        let pieces = if let Some(opening) = &self.opening {
            let a = opening.offset - opening.width * 0.5;
            let b = opening.offset + opening.width * 0.5;
            [
                Some(self.segment([-half, a], height)),
                Some(self.segment([b, half], height)),
                Some(self.segment([a, b], [opening.height, wall_height])),
            ]
        } else {
            [Some(self.segment([-half, half], height)), None, None]
        };
        pieces.into_iter().flatten()
    }
}

impl Furniture {
    fn fit(&mut self, size: [f32; 3]) {
        let half = floor_half(size);
        let swapped = self.quarter_turns % 2 == 1;
        let (x, z) = if swapped { (2, 0) } else { (0, 2) };
        self.size[x] = self.size[x].clamp(0.24, (half[0] * 2.0).min(5.0));
        self.size[z] = self.size[z].clamp(0.24, (half[1] * 2.0).min(5.0));
        self.size[1] = self.size[1].clamp(0.24, (size[1] - 0.10).min(4.0));
        for ((position, half), axis) in self.position.iter_mut().zip(half).zip([x, z]) {
            let limit = (half - self.size[axis] * 0.5).max(0.0);
            *position = position.clamp(-limit, limit);
        }
    }

    pub(crate) fn bounds(&self) -> Collider {
        let [x, z] = self.position;
        let [mut w, h, mut d] = self.size;
        if self.quarter_turns % 2 == 1 {
            std::mem::swap(&mut w, &mut d);
        }
        Collider {
            min: [x - w * 0.5, 0.0, z - d * 0.5],
            max: [x + w * 0.5, h, z + d * 0.5],
        }
    }

    pub(crate) fn parts(&self, mut add: impl FnMut(Surface, [f32; 3], [f32; 3], [f32; 3])) {
        let wood = Surface::Wood;
        let plain = Surface::Metal;
        let color = self.color;
        let shade = |factor: f32| color.map(|channel| (channel * factor).min(1.0));
        let mut cube = |surface, [x, y, z]: [f32; 3], ratio: [f32; 3], color| {
            let [w, h, d] = self.size;
            let [x, z] = turn([x * w, z * d], self.quarter_turns);
            let mut size = std::array::from_fn(|i| self.size[i] * ratio[i]);
            if self.quarter_turns % 2 == 1 {
                size.swap(0, 2);
            }
            add(
                surface,
                [self.position[0] + x, y * h, self.position[1] + z],
                size,
                color,
            );
        };
        match self.kind {
            FurnitureKind::Chair | FurnitureKind::Table => {
                let chair = self.kind == FurnitureKind::Chair;
                let level = if chair { 0.48 } else { 0.93 };
                cube(wood, [0.0, level, 0.0], [1.0, 0.12, 1.0], color);
                for x in [-0.39, 0.39] {
                    for z in [-0.39, 0.39] {
                        cube(wood, [x, level * 0.5, z], [0.13, level, 0.13], shade(0.72));
                    }
                }
                if chair {
                    cube(wood, [0.0, 0.78, -0.42], [0.94, 0.44, 0.13], color);
                    cube(plain, [0.0, 0.57, 0.0], [0.89, 0.07, 0.86], shade(1.32));
                }
            }
            FurnitureKind::Bed => {
                cube(wood, [0.0, 0.20, 0.0], [1.0, 0.26, 1.0], [0.41, 0.26, 0.16]);
                cube(
                    wood,
                    [0.0, 0.55, -0.44],
                    [1.0, 0.90, 0.12],
                    [0.46, 0.32, 0.22],
                );
                cube(
                    plain,
                    [0.0, 0.43, 0.03],
                    [0.96, 0.24, 0.90],
                    [0.73, 0.70, 0.60],
                );
                cube(plain, [0.0, 0.56, 0.16], [0.97, 0.13, 0.64], color);
                for x in [-0.23, 0.23] {
                    cube(
                        plain,
                        [x, 0.59, -0.28],
                        [0.41, 0.14, 0.21],
                        [0.83, 0.80, 0.71],
                    );
                }
            }
            FurnitureKind::Sofa => {
                cube(
                    wood,
                    [0.0, 0.11, 0.0],
                    [0.90, 0.22, 0.85],
                    [0.20, 0.15, 0.10],
                );
                cube(plain, [0.0, 0.35, 0.0], [0.94, 0.33, 0.94], shade(0.66));
                cube(plain, [0.0, 0.70, -0.35], [0.99, 0.60, 0.30], color);
                for x in [-0.42, 0.42] {
                    cube(plain, [x, 0.52, 0.02], [0.16, 0.38, 0.96], shade(0.88));
                }
                for x in [-0.18, 0.18] {
                    cube(plain, [x, 0.53, 0.11], [0.35, 0.13, 0.65], shade(1.16));
                }
            }
            FurnitureKind::Cabinet => {
                cube(wood, [0.0, 0.49, -0.035], [1.0, 0.98, 0.91], shade(0.65));
                cube(wood, [0.0, 0.97, 0.0], [1.0, 0.06, 1.0], shade(0.89));
                for side in [-1.0, 1.0] {
                    cube(wood, [side * 0.24, 0.51, 0.443], [0.46, 0.85, 0.114], color);
                    cube(
                        plain,
                        [side * 0.075, 0.51, 0.495],
                        [0.035, 0.082, 0.03],
                        [0.58, 0.43, 0.24],
                    );
                }
            }
        }
    }
}

pub(crate) fn turn([x, z]: [f32; 2], quarter_turns: u8) -> [f32; 2] {
    match quarter_turns % 4 {
        1 => [z, -x],
        2 => [-x, -z],
        3 => [-z, x],
        _ => [x, z],
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    Partition(u32),
    Furniture(u32),
}

fn selection_key(building: u64) -> egui::Id {
    egui::Id::new(("building_interior", building))
}

pub fn selected_furniture(ctx: &egui::Context, building: u64) -> Option<u32> {
    match ctx.data(|data| data.get_temp::<Selection>(selection_key(building))) {
        Some(Selection::Furniture(id)) => Some(id),
        _ => None,
    }
}

pub fn select_furniture(ctx: &egui::Context, building: u64, id: u32) {
    ctx.data_mut(|data| data.insert_temp(selection_key(building), Selection::Furniture(id)));
}

pub(crate) fn inspector(
    ui: &mut egui::Ui,
    building: u64,
    interior: &mut Interior,
    size: [f32; 3],
    keys: &[KeyPickup],
) -> bool {
    let key = selection_key(building);
    let mut selected = ui.ctx().data(|data| data.get_temp::<Selection>(key));
    ui.separator();
    ui.strong("Интерьер");
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                interior.partitions.len() < MAX_WALLS,
                egui::Button::new("+ Перегородка"),
            )
            .clicked()
            && let Some(id) = interior.add_partition(size)
        {
            selected = Some(Selection::Partition(id));
            changed = true;
        }
        ui.add_enabled_ui(interior.furniture.len() < MAX_FURNITURE, |ui| {
            ui.menu_button("+ Мебель", |ui| {
                for kind in FurnitureKind::ALL {
                    if ui.button(kind.label()).clicked() {
                        let offset = (interior.furniture.len() % 3) as f32 * 0.55;
                        if let Some(id) = interior.add_furniture(kind, [0.85 + offset, -0.85], size)
                        {
                            selected = Some(Selection::Furniture(id));
                            changed = true;
                        }
                        ui.close();
                    }
                }
            });
        });
    });
    egui::ScrollArea::vertical()
        .id_salt("interior_items")
        .max_height(160.0)
        .show(ui, |ui| {
            for (item, name) in interior
                .partitions
                .iter()
                .map(|wall| (Selection::Partition(wall.id), &wall.name))
                .chain(
                    interior
                        .furniture
                        .iter()
                        .map(|item| (Selection::Furniture(item.id), &item.name)),
                )
            {
                if ui.selectable_label(selected == Some(item), name).clicked() {
                    selected = Some(item);
                }
            }
        });
    let existing = match selected {
        Some(Selection::Partition(id)) => interior
            .partitions
            .iter_mut()
            .find(|wall| wall.id == id)
            .map(|wall| {
                changed |= partition_controls(ui, wall, size, keys);
            }),
        Some(Selection::Furniture(id)) => interior
            .furniture
            .iter_mut()
            .find(|item| item.id == id)
            .map(|item| {
                changed |= furniture_controls(ui, item);
            }),
        None => None,
    };
    if existing.is_some() && ui.button("Удалить из интерьера").clicked() {
        match selected.take() {
            Some(Selection::Partition(id)) => interior.partitions.retain(|wall| wall.id != id),
            Some(Selection::Furniture(id)) => interior.furniture.retain(|item| item.id != id),
            None => {}
        }
        changed = true;
    }
    if let Some(selection) = selected {
        ui.ctx().data_mut(|data| data.insert_temp(key, selection));
    } else {
        ui.ctx().data_mut(|data| data.remove::<Selection>(key));
    }
    changed
}

fn name_field(ui: &mut egui::Ui, name: &mut String, fallback: &str) -> bool {
    let response = ui.add(
        egui::TextEdit::singleline(name)
            .char_limit(64)
            .hint_text(fallback),
    );
    if response.lost_focus() && name.trim().is_empty() {
        *name = fallback.into();
        return true;
    }
    response.changed()
}

fn position_fields(ui: &mut egui::Ui, [x, z]: &mut [f32; 2]) -> bool {
    ui.horizontal_wrapped(|ui| {
        let x_changed = ui
            .add(
                DragValue::new(x)
                    .range(-12.5..=12.5)
                    .speed(0.04)
                    .prefix("X "),
            )
            .changed();
        x_changed
            | ui.add(
                DragValue::new(z)
                    .range(-12.5..=12.5)
                    .speed(0.04)
                    .prefix("Z "),
            )
            .changed()
    })
    .inner
}

fn partition_controls(
    ui: &mut egui::Ui,
    wall: &mut Partition,
    size: [f32; 3],
    keys: &[KeyPickup],
) -> bool {
    let mut changed = name_field(ui, &mut wall.name, "Перегородка");
    ui.label("Локальная позиция в доме");
    changed |= position_fields(ui, &mut wall.position);
    let old = wall.axis;
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut wall.axis, Axis::X, "Вдоль X");
        ui.selectable_value(&mut wall.axis, Axis::Z, "Вдоль Z");
    });
    if old != wall.axis {
        wall.position.swap(0, 1);
        changed = true;
    }
    changed |= ui
        .add(
            DragValue::new(&mut wall.length)
                .range(0.4..=24.6)
                .speed(0.04)
                .prefix("Длина "),
        )
        .changed();
    let mut has_opening = wall.opening.is_some();
    if ui.checkbox(&mut has_opening, "Дверной проём").changed() {
        wall.opening = has_opening.then(Opening::default);
        changed = true;
    }
    wall.fit(size);
    if let Some(opening) = &mut wall.opening {
        ui.horizontal_wrapped(|ui| {
            for (label, value, min, max) in [
                (
                    "Шир. ",
                    &mut opening.width,
                    MIN_DOOR_WIDTH,
                    wall.length - 0.28,
                ),
                (
                    "Выс. ",
                    &mut opening.height,
                    MIN_DOOR_HEIGHT,
                    size[1] - 0.20,
                ),
                (
                    "Сдвиг ",
                    &mut opening.offset,
                    -wall.length * 0.5,
                    wall.length * 0.5,
                ),
            ] {
                changed |= ui
                    .add(
                        DragValue::new(value)
                            .range(min..=max)
                            .speed(0.025)
                            .prefix(label),
                    )
                    .changed();
            }
        });
        changed |= ui
            .push_id(("partition_door", wall.id), |ui| {
                configure_door(ui, &mut opening.door, keys)
            })
            .inner;
    }
    changed
}

fn furniture_controls(ui: &mut egui::Ui, item: &mut Furniture) -> bool {
    let mut changed = name_field(ui, &mut item.name, item.kind.label());
    ui.label("Локальная позиция в доме");
    changed |= position_fields(ui, &mut item.position);
    ui.horizontal_wrapped(|ui| {
        for (label, value) in ["Шир. ", "Выс. ", "Глуб. "].into_iter().zip(&mut item.size)
        {
            changed |= ui
                .add(
                    DragValue::new(value)
                        .range(0.24..=5.0)
                        .speed(0.025)
                        .prefix(label),
                )
                .changed();
        }
    });
    if ui
        .button(format!(
            "Повернуть · {}°",
            u16::from(item.quarter_turns) * 90
        ))
        .clicked()
    {
        item.quarter_turns = (item.quarter_turns + 1) % 4;
        changed = true;
    }
    changed |= ui
        .horizontal(|ui| {
            ui.label("Цвет мебели");
            ui.color_edit_button_rgb(&mut item.color).changed()
        })
        .inner;
    changed |= ui.checkbox(&mut item.solid, "Столкновения").changed();
    changed
}
