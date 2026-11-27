use std::collections::HashSet;

use eframe::egui::{self, DragValue};
use serde::{Deserialize, Serialize};

use crate::{
    interior::{self, Interior},
    scene::{PLAYER_HEIGHT, PLAYER_RADIUS, Part, PartMotion, Shape},
    world::{Collider, Door, DoorSettings, InteractionState, KeyPickup, Seat, configure_door},
};

pub const MAX_BUILDINGS: usize = 24;
pub const FIRST_ID: u64 = 1 << 58;
pub(crate) const LAST_ID: u64 = FIRST_ID + (1 << 30) - 1;
const PART_TAG: u64 = 1 << 62;
pub(crate) const WALL: f32 = 0.2;
const FLOOR: f32 = 0.14;
const ROOF: f32 = 0.20;
const MIN_DOOR_WIDTH: f32 = 1.1;
const MIN_DOOR_HEIGHT: f32 = 2.4;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Wall = 1,
    Floor = 2,
    Roof = 3,
    Glass = 4,
    Metal = 5,
    Wood = 6,
    Light = 7,
}

pub fn surface(part_id: u64) -> Option<Surface> {
    if (FIRST_ID..=LAST_ID).contains(&part_id) {
        return Some(Surface::Floor);
    }
    if part_id & (3 << 62) != PART_TAG {
        return interior::surface(part_id);
    }
    building_id(part_id)?;
    match (part_id >> 6) & 7 {
        1 => Some(Surface::Wall),
        2 => Some(Surface::Floor),
        3 => Some(Surface::Roof),
        4 => Some(Surface::Glass),
        5 => Some(Surface::Metal),
        6 => Some(Surface::Wood),
        7 => Some(Surface::Light),
        _ => None,
    }
}

pub fn building_id(part_id: u64) -> Option<u64> {
    if (FIRST_ID..=LAST_ID).contains(&part_id) {
        return Some(part_id);
    }
    if part_id & (3 << 62) != PART_TAG {
        return interior::building_id(part_id);
    }
    let id = FIRST_ID + ((part_id & !PART_TAG) >> 9);
    (id <= LAST_ID).then_some(id)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Building {
    pub id: u64,
    pub name: String,
    /// X and Z locate the center, Y is the finished floor height.
    pub position: [f32; 3],
    /// Width, wall height, and depth in scene units.
    pub size: [f32; 3],
    pub door_width: f32,
    pub door_height: f32,
    pub door_offset: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door: Option<DoorSettings>,
    pub color: [f32; 3],
    #[serde(default)]
    pub quarter_turns: u8,
    #[serde(default)]
    pub interior: Interior,
}

pub fn add(buildings: &mut Vec<Building>, next_id: &mut u64, position: [f32; 3]) -> Option<u64> {
    let after_last = buildings
        .iter()
        .map(|building| building.id.saturating_add(1))
        .max()
        .unwrap_or(FIRST_ID);
    let id = after_last.max(*next_id);
    if buildings.len() >= MAX_BUILDINGS || id > LAST_ID {
        return None;
    }
    buildings.push(Building {
        id,
        name: "Городской дом".into(),
        position,
        size: [6.4, 3.45, 4.6],
        door_width: 1.48,
        door_height: 2.52,
        door_offset: 0.0,
        door: None,
        color: [0.33, 0.47, 0.54],
        quarter_turns: 0,
        interior: Interior::default(),
    });
    *next_id = id + 1;
    Some(id)
}

pub fn validate_all(buildings: &[Building], next_id: u64) -> Result<(), String> {
    if buildings.len() > MAX_BUILDINGS || next_id > LAST_ID + 1 {
        return Err(format!("Поддерживается до {MAX_BUILDINGS} домов на сцену"));
    }
    let mut ids = HashSet::with_capacity(buildings.len());
    for (index, house) in buildings.iter().enumerate() {
        let [x, y, z] = house.position;
        let [width, height, depth] = house.size;
        let max_offset = house.max_door_offset();
        if !(FIRST_ID..=LAST_ID).contains(&house.id)
            || !ids.insert(house.id)
            || house.name.trim().is_empty()
            || house.name.chars().count() > 64
            || house.name.chars().any(char::is_control)
            || !(-100.0..=100.0).contains(&x)
            || !(0.0..=100.0).contains(&y)
            || !(-100.0..=100.0).contains(&z)
            || !(4.0..=25.0).contains(&width)
            || !(3.0..=12.0).contains(&height)
            || !(3.6..=25.0).contains(&depth)
            || !(MIN_DOOR_WIDTH..=width - 1.4).contains(&house.door_width)
            || !(MIN_DOOR_HEIGHT..=height - 0.34).contains(&house.door_height)
            || !(-max_offset..=max_offset).contains(&house.door_offset)
            || !house.color.iter().all(|value| (0.0..=1.0).contains(value))
            || house.quarter_turns > 3
        {
            return Err(format!("Некорректные параметры дома №{}", index + 1));
        }
        house
            .interior
            .validate(house.size)
            .map_err(|error| format!("Дом №{}. {error}", index + 1))?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Bounds {
    left: f32,
    right: f32,
    floor: f32,
    ceiling: f32,
    back: f32,
    front: f32,
}

impl Building {
    fn bounds(&self) -> Bounds {
        let [x, y, z] = self.position;
        let [width, height, depth] = self.size;
        Bounds {
            left: x - width * 0.5,
            right: x + width * 0.5,
            floor: y,
            ceiling: y + height,
            back: z - depth * 0.5,
            front: z + depth * 0.5,
        }
    }

    fn max_door_offset(&self) -> f32 {
        ((self.size[0] - self.door_width) * 0.5 - 0.60).max(0.0)
    }

    fn door_unturned(&self) -> Collider {
        let Bounds { floor, front, .. } = self.bounds();
        let center = self.position[0] + self.door_offset;
        Collider {
            min: [center - self.door_width * 0.5, floor, front - WALL],
            max: [
                center + self.door_width * 0.5,
                floor + self.door_height,
                front,
            ],
        }
    }

    pub fn door_bounds(&self) -> Collider {
        self.turn_bounds(self.door_unturned())
    }

    #[cfg(test)]
    pub(crate) fn entrance_id(&self) -> Option<u64> {
        self.front_door().map(|door| door.id)
    }

    pub fn entrance_position(&self) -> [f32; 3] {
        let Bounds { floor, front, .. } = self.bounds();
        self.turn_position(
            [
                self.position[0] + self.door_offset,
                floor,
                front - WALL - 0.48,
            ],
            self.quarter_turns,
        )
    }

    pub(crate) fn clear_key(&mut self, key: u32) {
        let interior_doors = self
            .interior
            .partitions
            .iter_mut()
            .filter_map(|wall| wall.opening.as_mut()?.door.as_mut());
        for door in self.door.iter_mut().chain(interior_doors) {
            if door.required_key == Some(key) {
                door.required_key = None;
            }
        }
    }

    pub(crate) fn entry_crossed(
        &self,
        state: &InteractionState,
        from: [f32; 3],
        to: [f32; 3],
    ) -> bool {
        // Only a movement step through the inner threshold counts as entry.
        if !self.contains_interior(to[0], to[1], to[2])
            || from
                .iter()
                .zip(to)
                .any(|(&a, b)| !a.is_finite() || (a - b).abs() > 0.75)
        {
            return false;
        }
        let unturn = |position| self.turn_position(position, 4 - self.quarter_turns % 4);
        let [ax, ay, az] = unturn(from);
        let [bx, by, bz] = unturn(to);
        let opening = self.door_unturned();
        let plane = opening.min[2];
        if az < plane || bz >= plane {
            return false;
        }
        let amount = (az - plane) / (az - bz);
        let x = ax + (bx - ax) * amount;
        let y = ay + (by - ay) * amount;
        if !(opening.min[0] + PLAYER_RADIUS..=opening.max[0] - PLAYER_RADIUS).contains(&x)
            || !(opening.min[1] - 0.08..=opening.max[1] - PLAYER_HEIGHT).contains(&y)
        {
            return false;
        }
        self.front_door().is_none_or(|door| {
            let fraction = state.fraction(&door);
            if state.locked(&door) || fraction <= 0.001 {
                return false;
            }
            let middle = |[x, y, z]: [f32; 3]| [x, y + 1.0, z];
            door.colliders(fraction)
                .all(|leaf| !leaf.intersects_segment(middle(from), middle(to)))
        })
    }

    pub(crate) fn front_door(&self) -> Option<Door<'_>> {
        let building_yaw = f32::from(self.quarter_turns) * 90.0;
        self.door.map(|settings| {
            let original = self.door_unturned();
            let [x, y, z] = original.min;
            let tag = PART_TAG | ((self.id - FIRST_ID) << 9);
            Door {
                id: tag | (Surface::Wood as u64) << 6 | 60,
                latch_id: tag | (Surface::Metal as u64) << 6 | 61,
                name: "Входная дверь",
                hinge: self
                    .turn_position([x + 0.026, y + 0.012, z + WALL * 0.52], self.quarter_turns),
                width: self.door_width - 0.052,
                height: self.door_height - 0.024,
                yaw: building_yaw,
                settings,
            }
        })
    }

    pub(crate) fn doors(&self) -> impl Iterator<Item = Door<'_>> + '_ {
        let building_yaw = f32::from(self.quarter_turns) * 90.0;
        let interior = self.interior.partitions.iter().filter_map(move |wall| {
            let opening = wall.opening.as_ref()?;
            let settings = opening.door?;
            let local = wall.door()?;
            let (hinge, local_yaw) = match wall.axis {
                interior::Axis::X => ([local.min[0] + 0.024, 0.012, wall.position[1]], 0.0),
                interior::Axis::Z => ([wall.position[0], 0.012, local.min[2] + 0.024], -90.0),
            };
            Some(Door {
                id: interior::part_id(self.id, wall.id, 12, Surface::Wood),
                latch_id: interior::part_id(self.id, wall.id, 13, Surface::Metal),
                name: &wall.name,
                hinge: self.local_to_world(hinge),
                width: opening.width - 0.048,
                height: opening.height - 0.024,
                yaw: building_yaw + local_yaw,
                settings,
            })
        });
        self.front_door().into_iter().chain(interior)
    }

    fn turn_position(&self, position: [f32; 3], turns: u8) -> [f32; 3] {
        let [cx, _, cz] = self.position;
        let [x, y, z] = position;
        let dx = x - cx;
        let dz = z - cz;
        match turns % 4 {
            1 => [cx + dz, y, cz - dx],
            2 => [cx - dx, y, cz - dz],
            3 => [cx - dz, y, cz + dx],
            _ => position,
        }
    }

    fn turn_bounds(&self, bounds: Collider) -> Collider {
        let a = self.turn_position(bounds.min, self.quarter_turns);
        let b = self.turn_position(bounds.max, self.quarter_turns);
        Collider {
            min: std::array::from_fn(|axis| a[axis].min(b[axis])),
            max: std::array::from_fn(|axis| a[axis].max(b[axis])),
        }
    }

    fn local_to_world(&self, [x, y, z]: [f32; 3]) -> [f32; 3] {
        let [x, z] = interior::turn([x, z], self.quarter_turns);
        [
            self.position[0] + x,
            self.position[1] + y,
            self.position[2] + z,
        ]
    }

    fn world_to_local_xz(&self, [x, z]: [f32; 2]) -> [f32; 2] {
        let relative = [x - self.position[0], z - self.position[2]];
        interior::turn(relative, 4 - self.quarter_turns % 4)
    }

    fn local_bounds_to_world(&self, collider: Collider) -> Collider {
        let a = self.local_to_world(collider.min);
        let b = self.local_to_world(collider.max);
        Collider {
            min: std::array::from_fn(|axis| a[axis].min(b[axis])),
            max: std::array::from_fn(|axis| a[axis].max(b[axis])),
        }
    }

    pub fn furnish_demo(&mut self) {
        self.interior = Interior::demo(self.size);
    }

    pub fn furniture_at(&self, position: [f32; 2]) -> Option<u32> {
        self.interior.furniture_at(self.world_to_local_xz(position))
    }

    pub fn move_furniture_at(&mut self, item: u32, position: [f32; 2]) -> bool {
        let local = self.world_to_local_xz(position);
        self.interior.move_furniture_at(item, local, self.size)
    }

    pub fn interior_doors(&self) -> impl Iterator<Item = Collider> + '_ {
        self.interior
            .partitions
            .iter()
            .filter_map(interior::Partition::door)
            .map(|door| self.local_bounds_to_world(door))
    }

    pub fn interior_colliders(&self) -> impl Iterator<Item = Collider> + '_ {
        self.interior
            .partitions
            .iter()
            .flat_map(|wall| wall.solids(self.size[1]))
            .chain(
                self.interior
                    .furniture
                    .iter()
                    .filter(|item| item.solid)
                    .map(interior::Furniture::bounds),
            )
            .map(|bounds| self.local_bounds_to_world(bounds))
    }

    pub(crate) fn seats(&self) -> impl Iterator<Item = Seat> + '_ {
        let colliders: Vec<_> = self.interior_colliders().collect();
        self.interior.furniture.iter().filter_map(move |item| {
            let height = match item.kind {
                interior::FurnitureKind::Chair => item.size[1] * 0.605,
                interior::FurnitureKind::Sofa => item.size[1] * 0.595,
                _ => return None,
            };
            let [x, z] = item.position;
            let [side, forward] = interior::turn([0.0, item.size[2] * 0.11], item.quarter_turns);
            let position = self.local_to_world([x + side, height, z + forward]);
            let width = item.size[0] * 0.5 + 0.52;
            let depth = item.size[2] * 0.5 + 0.52;
            let stand = [[0.0, depth], [width, 0.0], [-width, 0.0], [0.0, -depth]]
                .into_iter()
                .map(|offset| {
                    let [dx, dz] = interior::turn(offset, item.quarter_turns);
                    self.local_to_world([x + dx, 0.0, z + dz])
                })
                .find(|&[x, y, z]| {
                    let radius = 0.38;
                    self.contains_interior(x - radius, y, z - radius)
                        && self.contains_interior(x + radius, y, z + radius)
                        && !colliders.iter().any(|box_bounds| {
                            y + 2.25 > box_bounds.min[1]
                                && y < box_bounds.max[1]
                                && x + radius > box_bounds.min[0]
                                && x - radius < box_bounds.max[0]
                                && z + radius > box_bounds.min[2]
                                && z - radius < box_bounds.max[2]
                        })
                })?;
            let turn = (self.quarter_turns + item.quarter_turns) % 4;
            Some(Seat {
                id: interior::part_id(self.id, item.id, 0, Surface::Wood),
                position,
                stand,
                yaw: f32::from(turn) * 90.0,
            })
        })
    }

    pub(crate) fn seat_accessible(
        &self,
        seat: &Seat,
        player: [f32; 3],
        state: &InteractionState,
    ) -> bool {
        let from = [player[0], player[1] + 0.72, player[2]];
        let to = [seat.position[0], seat.position[1] + 0.22, seat.position[2]];
        self.interior
            .partitions
            .iter()
            .flat_map(|partition| partition.solids(self.size[1]))
            .map(|bounds| self.local_bounds_to_world(bounds))
            .all(|bounds| !bounds.intersects_segment(from, to))
            && self.doors().all(|door| {
                door.colliders(state.fraction(&door))
                    .all(|bounds| !bounds.intersects_segment(from, to))
            })
    }

    pub fn contains_interior(&self, x: f32, y: f32, z: f32) -> bool {
        let [x, y, z] = self.turn_position([x, y, z], 4 - self.quarter_turns % 4);
        let Bounds {
            left,
            right,
            floor,
            ceiling,
            back,
            front,
        } = self.bounds();
        (left + WALL..=right - WALL).contains(&x)
            && (floor - 0.08..ceiling).contains(&y)
            && (back + WALL..=front - WALL).contains(&z)
    }

    fn shell(&self) -> [Collider; 8] {
        let Bounds {
            left,
            right,
            floor,
            ceiling,
            back,
            front,
        } = self.bounds();
        let Collider {
            min: [door_left, _, _],
            max: [door_right, door_top, _],
        } = self.door_unturned();
        let piece = |min, max| Collider { min, max };
        [
            piece([left, floor - FLOOR, back], [right, floor, front]),
            piece([left, floor, back], [left + WALL, ceiling, front]),
            piece([right - WALL, floor, back], [right, ceiling, front]),
            piece(
                [left + WALL, floor, back],
                [right - WALL, ceiling, back + WALL],
            ),
            piece(
                [left + WALL, floor, front - WALL],
                [door_left, ceiling, front],
            ),
            piece(
                [door_right, floor, front - WALL],
                [right - WALL, ceiling, front],
            ),
            piece(
                [door_left, door_top, front - WALL],
                [door_right, ceiling, front],
            ),
            piece(
                [left - 0.17, ceiling, back - 0.17],
                [right + 0.17, ceiling + ROOF, front + 0.17],
            ),
        ]
    }

    pub fn colliders(&self) -> [Collider; 8] {
        self.shell().map(|bounds| self.turn_bounds(bounds))
    }

    pub fn render_parts(&self, view_2d: bool) -> Vec<Part> {
        self.render_parts_with(view_2d, &InteractionState::default())
    }

    pub(crate) fn render_parts_with(&self, view_2d: bool, state: &InteractionState) -> Vec<Part> {
        let [x, y, z] = self.position;
        let [width, height, depth] = self.size;
        let Bounds {
            left,
            right,
            floor,
            ceiling,
            front,
            ..
        } = self.bounds();
        let doorway = self.door_unturned();
        let [door_left, _, _] = doorway.min;
        let [door_right, door_top, _] = doorway.max;
        let muted = [0.085, 0.105, 0.121];
        let trim = [0.25, 0.27, 0.28];
        let glass = [0.095, 0.19, 0.24];
        let light = [0.99, 0.80, 0.54];
        let stone = self.color.map(|value| (value * 0.73 + 0.10).min(1.0));
        let plinth = self.color.map(|value| value * 0.59);
        let mut parts = Vec::with_capacity(52);
        let mut piece_index = 0;
        let mut add = |show, surface: Surface, position, mut size: [f32; 3], color| {
            let id = if piece_index == 0 {
                self.id
            } else {
                PART_TAG | ((self.id - FIRST_ID) << 9) | ((surface as u64) << 6) | piece_index
            };
            piece_index += 1;
            if show {
                if self.quarter_turns % 2 == 1 {
                    size.swap(0, 2);
                }
                parts.push(Part {
                    id,
                    name: self.name.clone(),
                    shape: Shape::Box,
                    position: self.turn_position(position, self.quarter_turns),
                    rotation: [0.0; 3],
                    size,
                    color,
                    motion: PartMotion::Static,
                });
            }
        };

        for (index, collider) in self.shell().into_iter().enumerate() {
            let mut position =
                std::array::from_fn(|axis| (collider.min[axis] + collider.max[axis]) * 0.5);
            let size = std::array::from_fn(|axis| collider.max[axis] - collider.min[axis]);
            let (surface, color) = match index {
                0 => {
                    position[1] += 0.008;
                    (Surface::Floor, [0.47, 0.41, 0.34])
                }
                7 => (Surface::Roof, muted),
                _ => (Surface::Wall, self.color),
            };
            add(!view_2d || index < 6, surface, position, size, color);
        }

        let window_height = (height * 0.36).min(1.56);
        let window_y = y + (height * 0.57).min(2.20);
        for (a, b) in [(left + WALL, door_left), (door_right, right - WALL)] {
            let center = (a + b) * 0.5;
            let pane_width = ((b - a) - 0.45).clamp(0.0, 1.55);
            let show = pane_width > 0.42 && !view_2d;
            add(
                show,
                Surface::Metal,
                [center, window_y, front + 0.015],
                [pane_width + 0.13, window_height + 0.13, 0.075],
                muted,
            );
            add(
                show,
                Surface::Glass,
                [center, window_y, front + 0.060],
                [pane_width, window_height, 0.024],
                glass,
            );
            add(
                show,
                Surface::Metal,
                [center, window_y, front + 0.078],
                [0.035, window_height, 0.025],
                trim,
            );
            add(
                show,
                Surface::Metal,
                [center, window_y - window_height * 0.5 - 0.06, front + 0.100],
                [pane_width + 0.26, 0.09, 0.21],
                stone,
            );
            add(
                show,
                Surface::Metal,
                [center, window_y + window_height * 0.22, front + 0.083],
                [pane_width, 0.044, 0.032],
                muted,
            );
            add(
                show,
                Surface::Wall,
                [center, window_y + window_height * 0.5 + 0.10, front + 0.063],
                [pane_width + 0.22, 0.12, 0.13],
                stone,
            );
            if height > 5.2 {
                let upper_y = ceiling - 1.35;
                add(
                    show,
                    Surface::Metal,
                    [center, upper_y, front + 0.025],
                    [pane_width + 0.15, 1.52, 0.10],
                    muted,
                );
                add(
                    show,
                    Surface::Glass,
                    [center, upper_y, front + 0.092],
                    [pane_width, 1.38, 0.039],
                    glass,
                );
                add(
                    show,
                    Surface::Metal,
                    [center, upper_y, front + 0.119],
                    [0.042, 1.39, 0.031],
                    trim,
                );
                add(
                    show,
                    Surface::Wall,
                    [center, upper_y - 0.75, front + 0.115],
                    [pane_width + 0.29, 0.09, 0.22],
                    stone,
                );
            }
        }

        let side_pane = (depth * 0.39).min(2.0);
        for panel_x in [left, right] {
            let facing = (panel_x - x).signum();
            add(
                !view_2d,
                Surface::Metal,
                [panel_x + facing * 0.026, window_y, z],
                [0.08, window_height + 0.14, side_pane + 0.15],
                muted,
            );
            add(
                !view_2d,
                Surface::Glass,
                [panel_x + facing * 0.072, window_y, z],
                [0.026, window_height, side_pane],
                glass,
            );
            add(
                !view_2d,
                Surface::Metal,
                [panel_x + facing * 0.095, window_y, z],
                [0.026, window_height, 0.046],
                trim,
            );
        }

        let door_x = (door_left + door_right) * 0.5;
        for frame_x in [door_left - 0.036, door_right + 0.036] {
            add(
                true,
                Surface::Metal,
                [frame_x, floor + self.door_height * 0.5, front - WALL * 0.5],
                [0.072, self.door_height, WALL + 0.07],
                muted,
            );
        }
        add(
            !view_2d,
            Surface::Metal,
            [door_x, door_top + 0.037, front - WALL * 0.5],
            [self.door_width + 0.15, 0.074, WALL + 0.07],
            muted,
        );
        add(
            !view_2d,
            Surface::Roof,
            [door_x, door_top + 0.18, front + 0.32],
            [self.door_width + 0.55, 0.14, 0.70],
            muted,
        );
        add(
            !view_2d,
            Surface::Light,
            [door_x, door_top + 0.09, front + 0.19],
            [0.45, 0.043, 0.14],
            light,
        );
        add(
            !view_2d,
            Surface::Metal,
            [x - width * 0.30, ceiling + ROOF + 0.12, z - depth * 0.16],
            [0.69, 0.24, 0.58],
            trim,
        );

        add(
            true,
            Surface::Floor,
            [x + width * 0.10, floor + 0.026, z - depth * 0.18],
            [width * 0.31, 0.026, depth * 0.27],
            [0.25, 0.37, 0.39],
        );
        if self.interior.partitions.is_empty() {
            add(
                !view_2d,
                Surface::Light,
                [x, ceiling - 0.058, z - depth * 0.06],
                [1.20, 0.08, 0.32],
                light,
            );
        } else {
            for side in [-1.0, 1.0] {
                add(
                    !view_2d,
                    Surface::Light,
                    [x, ceiling - 0.058, z + depth * 0.24 * side],
                    [0.98, 0.08, 0.30],
                    light,
                );
            }
        }

        for (a, b) in [(left, door_left), (door_right, right)] {
            add(
                !view_2d,
                Surface::Wall,
                [(a + b) * 0.5, floor + 0.16, front + 0.038],
                [b - a, 0.32, 0.105],
                plinth,
            );
        }
        for (drop, thickness, projection, color) in [
            (0.065, 0.13, 0.17, muted),
            (0.21, 0.09, 0.125, stone),
            (0.30, 0.048, 0.085, trim),
        ] {
            add(
                !view_2d,
                Surface::Metal,
                [x, ceiling - drop, front + projection * 0.5],
                [width + projection * 2.0, thickness, projection],
                color,
            );
        }
        for side in [-1.0_f32, 1.0] {
            add(
                !view_2d,
                Surface::Wall,
                [x + side * (width * 0.5 + 0.057), floor + 0.16, z],
                [0.14, 0.32, depth + 0.12],
                plinth,
            );
        }
        let drain_x = if (self.id - FIRST_ID).is_multiple_of(2) {
            right - 0.19
        } else {
            left + 0.19
        };
        add(
            !view_2d,
            Surface::Metal,
            [drain_x, floor + height * 0.5 - 0.10, front + 0.095],
            [0.078, height - 0.35, 0.079],
            trim,
        );
        for level in [0.20, 0.54, 0.86] {
            add(
                !view_2d,
                Surface::Metal,
                [drain_x, floor + height * level, front + 0.096],
                [0.108, 0.04, 0.092],
                muted,
            );
        }

        self.append_interior_parts(&mut parts, view_2d);
        for door in self.doors() {
            parts.extend(door.parts(state.fraction(&door), view_2d));
        }
        parts
    }

    fn append_interior_parts(&self, parts: &mut Vec<Part>, plan: bool) {
        let mut add = |item, index, name: &str, surface, position, mut size: [f32; 3], color| {
            if self.quarter_turns % 2 == 1 {
                size.swap(0, 2);
            }
            parts.push(Part {
                id: interior::part_id(self.id, item, index, surface),
                name: name.into(),
                shape: Shape::Box,
                position: self.local_to_world(position),
                rotation: [0.0; 3],
                size,
                color,
                motion: PartMotion::Static,
            });
        };
        let partition_color = std::array::from_fn(|i| self.color[i] * 0.38 + [0.41, 0.39, 0.34][i]);
        for wall in &self.interior.partitions {
            for (index, bounds) in wall.solids(self.size[1]).enumerate() {
                if plan && index == 2 {
                    continue;
                }
                let position =
                    std::array::from_fn(|axis| (bounds.min[axis] + bounds.max[axis]) * 0.5);
                let size = std::array::from_fn(|axis| bounds.max[axis] - bounds.min[axis]);
                add(
                    wall.id,
                    index as u64,
                    &wall.name,
                    Surface::Wall,
                    position,
                    size,
                    partition_color,
                );
            }
        }
        for item in &self.interior.furniture {
            let mut index = 0;
            item.parts(|surface, position, size, color| {
                add(item.id, index, &item.name, surface, position, size, color);
                index += 1;
            });
        }
    }
}

pub fn inspector(
    ui: &mut egui::Ui,
    house: &mut Building,
    active: bool,
    keys: &[KeyPickup],
) -> bool {
    ui.add_enabled_ui(active, |ui| {
        let name = ui.add(
            egui::TextEdit::singleline(&mut house.name)
                .char_limit(64)
                .hint_text("Название дома"),
        );
        let mut changed = name.changed();
        if name.lost_focus() && house.name.trim().is_empty() {
            house.name = "Городской дом".into();
            changed = true;
        }

        ui.label("Позиция пола");
        ui.horizontal_wrapped(|ui| {
            for (axis, (caption, value)) in ["X ", "Y ", "Z "]
                .into_iter()
                .zip(&mut house.position)
                .enumerate()
            {
                let min = if axis == 1 { 0.0 } else { -100.0 };
                changed |= ui
                    .add(
                        DragValue::new(value)
                            .prefix(caption)
                            .range(min..=100.0)
                            .speed(0.04),
                    )
                    .changed();
            }
        });
        for (axis, (caption, min, max)) in [
            ("Ширина", 4.0, 25.0),
            ("Высота", 3.0, 12.0),
            ("Глубина", 3.6, 25.0),
        ]
        .into_iter()
        .enumerate()
        {
            ui.horizontal(|ui| {
                ui.label(caption);
                changed |= ui
                    .add(
                        DragValue::new(&mut house.size[axis])
                            .range(min..=max)
                            .speed(0.04),
                    )
                    .changed();
            });
        }
        ui.separator();
        let old_turns = house.quarter_turns;
        let directions = ["+Z", "+X", "−Z", "−X"];
        egui::ComboBox::from_label("Фасад")
            .selected_text(directions[usize::from(house.quarter_turns % 4)])
            .show_ui(ui, |ui| {
                for (turns, direction) in directions.into_iter().enumerate() {
                    ui.selectable_value(&mut house.quarter_turns, turns as u8, direction);
                }
            });
        changed |= old_turns != house.quarter_turns;
        ui.label("Дверной проём");
        ui.horizontal_wrapped(|ui| {
            changed |= ui
                .add(
                    DragValue::new(&mut house.door_width)
                        .prefix("Шир. ")
                        .range(MIN_DOOR_WIDTH..=house.size[0] - 1.4)
                        .speed(0.025),
                )
                .changed();
            changed |= ui
                .add(
                    DragValue::new(&mut house.door_height)
                        .prefix("Выс. ")
                        .range(MIN_DOOR_HEIGHT..=house.size[1] - 0.34)
                        .speed(0.025),
                )
                .changed();
            let offset = house.max_door_offset();
            changed |= ui
                .add(
                    DragValue::new(&mut house.door_offset)
                        .prefix("Сдвиг ")
                        .range(-offset..=offset)
                        .speed(0.025),
                )
                .changed();
        });
        changed |= ui
            .push_id(("facade_door", house.id), |ui| {
                configure_door(ui, &mut house.door, keys)
            })
            .inner;
        ui.horizontal(|ui| {
            ui.label("Цвет стен");
            changed |= crate::inspector::edit_rgb(ui, &mut house.color);
        });
        changed |= interior::inspector(ui, house.id, &mut house.interior, house.size, keys);
        house.interior.fit(house.size);
        changed
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Game;

    #[test]
    fn player_walks_through_door_but_cannot_pass_front_or_back_walls() {
        let mut houses = Vec::new();
        let mut next_id = FIRST_ID;
        add(&mut houses, &mut next_id, [0.0, 0.0, -3.5]).unwrap();
        let house = &houses[0];
        let colliders = house.colliders();
        let mut player = Game::default();
        let mut against_wall = Game::default();
        against_wall.x = 1.45;
        for _ in 0..100 {
            player.update(0.016, [0.0, -1.0], false, false, &colliders);
            against_wall.update(0.016, [0.0, -1.0], false, false, &colliders);
        }
        assert!(house.contains_interior(player.x, player.height, player.z));
        assert!(against_wall.z > house.door_bounds().max[2]);
        for _ in 0..120 {
            player.update(0.016, [0.0, -1.0], true, false, &colliders);
        }
        assert!(house.contains_interior(player.x, player.height, player.z));
        assert!(player.z > house.bounds().back + WALL);
        assert_eq!(player.height, 0.0);
    }

    #[test]
    fn roofless_geometry_retains_front_opening_and_pickable_house_ids() {
        let mut homes = Vec::new();
        let mut next_id = FIRST_ID;
        let id = add(&mut homes, &mut next_id, [0.0; 3]).unwrap();
        let house = &homes[0];
        let cutaway = house.render_parts(true);
        let exterior = house.render_parts(false);
        assert!(cutaway.len() < exterior.len());
        assert!(exterior.iter().all(|part| building_id(part.id) == Some(id)));
        assert!(
            exterior
                .iter()
                .any(|part| surface(part.id) == Some(Surface::Glass))
        );
        assert!(
            cutaway
                .iter()
                .all(|part| surface(part.id) != Some(Surface::Roof))
        );
        let ids: HashSet<_> = exterior.iter().map(|part| part.id).collect();
        assert_eq!(ids.len(), exterior.len());
    }

    #[test]
    fn rotated_facade_entrance_and_world_roundtrip() {
        let mut world = crate::world::World::default();
        let id = world.add_building_at([-3.5, 0.0, 0.0]).unwrap();
        world.buildings[0].quarter_turns = 1;
        world.validate().unwrap();
        let encoded = serde_json::to_vec(&world).unwrap();
        let decoded: crate::world::World = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, world);

        let building = &decoded.buildings[0];
        assert_eq!(building.id, id);
        let bounds = building.door_bounds();
        assert!(bounds.min[2] < 0.0 && bounds.max[2] > 0.0);
        let mut player = Game::default();
        let colliders = decoded.colliders();
        for _ in 0..100 {
            player.update(0.016, [-1.0, 0.0], false, false, &colliders);
        }
        assert!(building.contains_interior(player.x, player.height, player.z));
        assert_eq!(decoded.building_parts(true)[0].size[0], building.size[2]);
    }

    #[test]
    fn rotated_demo_rooms_have_open_doors_and_solid_dividers() {
        let mut world = crate::world::World::default();
        let id = world.add_building_at([-7.4, 0.0, -2.8]).unwrap();
        let house = &mut world.buildings[0];
        house.size = [7.3, 3.45, 5.9];
        house.quarter_turns = 1;
        house.furnish_demo();
        let opening = house.interior_doors().next().unwrap();
        assert!(opening.min[2] < house.position[2] && opening.max[2] > house.position[2]);

        let mut through = Game::default();
        through.x = -3.0;
        through.z = -2.8;
        let mut against = Game::default();
        against.x = -6.0;
        against.z = -3.96;
        let colliders = world.colliders();
        for _ in 0..175 {
            through.update(0.016, [-1.0, 0.0], false, false, &colliders);
            against.update(0.016, [-1.0, 0.0], false, false, &colliders);
        }
        assert!(through.x < opening.min[0] - 0.4, "x: {}", through.x);
        assert!(against.x > opening.max[0], "x: {}", against.x);

        let house = &world.buildings[0];
        let partition_id = house.interior.partitions[0].id;
        for (plan, wall_pieces) in [(false, 3), (true, 2)] {
            assert_eq!(
                house
                    .render_parts(plan)
                    .iter()
                    .filter(|part| building_id(part.id) == Some(id)
                        && interior::item_id(part.id) == Some(partition_id))
                    .count(),
                wall_pieces
            );
        }
    }

    #[test]
    fn furniture_moves_on_a_turned_plan_and_roundtrips_with_collision() {
        let mut world = crate::world::World::default();
        world.add_building_at([2.0, 0.75, -4.0]).unwrap();
        let house = &mut world.buildings[0];
        house.quarter_turns = 3;
        let item = house
            .interior
            .add_furniture(interior::FurnitureKind::Table, [0.0; 2], house.size)
            .unwrap();
        let target = [2.65, -3.0];
        assert!(house.move_furniture_at(item, target));
        assert!(house.contains_interior(target[0], 0.75, target[1]));
        assert_eq!(house.furniture_at(target), Some(item));
        assert!(!house.move_furniture_at(item, [20.0, -7.0]));
        let collider = house.interior_colliders().next().unwrap();
        assert_eq!(collider.min[1], 0.75);
        assert!((collider.min[0]..collider.max[0]).contains(&target[0]));
        assert!((collider.min[2]..collider.max[2]).contains(&target[1]));
        world.validate().unwrap();
        let loaded: crate::world::World =
            serde_json::from_slice(&serde_json::to_vec(&world).unwrap()).unwrap();
        assert_eq!(loaded, world);
        assert!(loaded.colliders().contains(&collider));
        let mut legacy = serde_json::to_value(world).unwrap();
        legacy["buildings"][0]
            .as_object_mut()
            .unwrap()
            .remove("interior");
        let older: crate::world::World = serde_json::from_value(legacy).unwrap();
        assert!(older.buildings[0].interior.furniture.is_empty());
        older.validate().unwrap();
    }
}
