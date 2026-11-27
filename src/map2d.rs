use std::{
    collections::HashSet,
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use glam::Vec2;
use image::{ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};

use crate::{
    rules,
    scene::Scene,
    world::{WorldKind, WorldObject},
};

mod terrain_atlas;
#[path = "map2d_tools.rs"]
mod tooling;
#[path = "map2d_ui.rs"]
mod ui;
pub use ui::{Editor, ViewTransform};

const GROUND_PNG: &[u8] = include_bytes!("../demo_assets/map2d/terrain.png");
const PROPS_PNG: &[u8] = include_bytes!("../demo_assets/map2d/props.png");
const PERSON_PNG: &[u8] = include_bytes!("../demo_assets/map2d/player.png");
const MAX_ATLAS_BYTES: usize = 4 * 1024 * 1024;
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const MAX_PNG_EDGE: u32 = 2048;
const MAX_ATLASES: usize = 12;
const MAX_EDGE: u16 = 96;
const EPS: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Top,
    Side,
}

impl Mode {
    pub fn title(self) -> &'static str {
        match self {
            Self::Top => "2D сверху",
            Self::Side => "2D сбоку",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tile {
    pub atlas: u32,
    pub index: u16,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Collision {
    #[default]
    Empty,
    Solid,
    Platform,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Layer {
    #[default]
    Ground,
    Detail,
    Tall,
}

impl Layer {
    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Atlas {
    pub id: u32,
    pub name: String,
    pub cell: [u16; 2],
    pub size: [u16; 2],
    #[serde(with = "encoded_png")]
    pub png: Vec<u8>,
}

impl Atlas {
    fn new(id: u32, name: String, cell: Option<[u16; 2]>, png: Vec<u8>) -> Result<Self, String> {
        let texture = decode_png(&png)?;
        let size = [texture.width() as u16, texture.height() as u16];
        let atlas = Self {
            id,
            name,
            cell: cell.unwrap_or(size),
            size,
            png,
        };
        atlas.validate_metadata()?;
        Ok(atlas)
    }

    fn validate_metadata(&self) -> Result<(), String> {
        if self.id == 0
            || self.name.trim().is_empty()
            || self.name.chars().count() > 80
            || self.name.chars().any(char::is_control)
            || self.cell.iter().zip(self.size).any(|(&cell, size)| {
                !(1..=MAX_PNG_EDGE as u16).contains(&size) || cell == 0 || size % cell != 0
            })
            || self.count() > u16::MAX as u32
        {
            return Err("Размеры PNG должны делиться на размер тайла без остатка.".into());
        }
        Ok(())
    }

    pub fn count(&self) -> u32 {
        u32::from(self.size[0].checked_div(self.cell[0]).unwrap_or(0))
            * u32::from(self.size[1].checked_div(self.cell[1]).unwrap_or(0))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Map {
    pub mode: Mode,
    pub size: [u16; 2],
    pub spawn: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub respawn: Option<[u16; 2]>,
    pub layers: [Vec<Option<Tile>>; 3],
    pub colliders: Vec<Collision>,
    pub atlases: Vec<Atlas>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero: Option<Tile>,
    next_atlas: u32,
    #[serde(default)]
    next_marker: u64,
}

impl Map {
    pub fn new(mode: Mode) -> Self {
        let size = [26, 18];
        let cells = size[0] as usize * size[1] as usize;
        Self {
            mode,
            size,
            spawn: if mode == Mode::Top {
                [3.5, 10.5]
            } else {
                [3.5, 13.0]
            },
            respawn: None,
            layers: std::array::from_fn(|_| vec![None; cells]),
            colliders: vec![Collision::Empty; cells],
            atlases: [
                (1, "Плитка двора", [32, 32], GROUND_PNG),
                (2, "Деревья и фонари", [32, 64], PROPS_PNG),
            ]
            .into_iter()
            .map(|(id, name, cell, bytes)| {
                Atlas::new(id, name.into(), Some(cell), bytes.to_vec())
                    .expect("встроенный атлас 2D")
            })
            .collect(),
            hero: None,
            next_atlas: 3,
            next_marker: 1 << 40,
        }
    }

    pub fn demo(mode: Mode) -> Self {
        let mut map = Self::new(mode);
        match mode {
            Mode::Top => {
                for row in 0..map.size[1] {
                    for col in 0..map.size[0] {
                        let cell = [col, row];
                        let water = (11..=14).contains(&col);
                        let bridge = water && (9..=10).contains(&row);
                        let border = col == 0
                            || row == 0
                            || col + 1 == map.size[0]
                            || row + 1 == map.size[1];
                        let path = (8..=11).contains(&row) || (row > 3 && (col == 4 || col == 21));
                        let tile = if border {
                            9
                        } else if bridge {
                            5
                        } else if water {
                            4
                        } else if path {
                            2
                        } else {
                            3 + 8 * u16::from((col * 11 + row * 7) % 7 == 0)
                        };
                        map.paint(
                            cell,
                            Layer::Ground,
                            Some(Tile {
                                atlas: 1,
                                index: tile,
                            }),
                        );
                        if border || water && !bridge {
                            map.paint_collision(cell, Collision::Solid);
                        } else if !path && (col * 11 + row * 17) % 9 == 0 {
                            map.paint(cell, Layer::Detail, Some(Tile { atlas: 1, index: 6 }));
                        }
                    }
                }
                for (col, row, index) in [
                    (3, 4, 0),
                    (7, 4, 0),
                    (18, 4, 0),
                    (22, 4, 0),
                    (2, 7, 1),
                    (10, 8, 1),
                    (15, 8, 1),
                    (23, 8, 1),
                    (4, 13, 0),
                    (8, 14, 0),
                    (19, 14, 0),
                    (23, 14, 0),
                    (6, 7, 4),
                    (19, 7, 4),
                    (8, 6, 2),
                    (17, 13, 2),
                    (21, 9, 3),
                    (10, 13, 5),
                    (15, 13, 5),
                ] {
                    map.paint([col, row], Layer::Tall, Some(Tile { atlas: 2, index }));
                    if index != 3 {
                        map.paint_collision([col, row], Collision::Solid);
                    }
                }
            }
            Mode::Side => {
                map.resize([44, 18]);
                map.spawn = [3.5, 15.0];
                for col in 0..map.size[0] {
                    let island = col < 12 || (17..30).contains(&col) || col >= 35;
                    for row in 15..18 {
                        map.paint(
                            [col, row],
                            Layer::Ground,
                            Some(Tile {
                                atlas: 1,
                                index: if island {
                                    if row == 15 { 13 } else { 12 }
                                } else if row == 17 {
                                    14
                                } else {
                                    4
                                },
                            }),
                        );
                        if island {
                            map.paint_collision([col, row], Collision::Solid);
                        }
                    }
                }
                for col in [13, 14, 15, 31, 32, 33] {
                    map.paint(
                        [col, 13],
                        Layer::Ground,
                        Some(Tile {
                            atlas: 1,
                            index: 15,
                        }),
                    );
                    map.paint_collision([col, 13], Collision::Platform);
                }
                map.set_respawn(Some([9, 14]));
                for col in [2, 10, 19, 27, 36, 42] {
                    map.paint(
                        [col, 14],
                        Layer::Tall,
                        Some(Tile {
                            atlas: 2,
                            index: u16::from(col % 2 == 0),
                        }),
                    );
                }
            }
        }
        map
    }

    pub fn atlas(&self, id: u32) -> Option<&Atlas> {
        self.atlases.iter().find(|atlas| atlas.id == id)
    }

    pub fn import_png(&mut self, path: &Path, cell: Option<[u16; 2]>) -> Result<u32, String> {
        if self.atlases.len() >= MAX_ATLASES {
            return Err("Лимит атласов этой карты достигнут.".into());
        }
        let file = File::open(path).map_err(|err| format!("Не удалось открыть PNG ({err})."))?;
        let mut bytes = Vec::new();
        file.take(MAX_ATLAS_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| format!("Не удалось прочитать PNG ({err})."))?;
        let title = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Атлас")
            .chars()
            .filter(|c| !c.is_control())
            .take(80)
            .collect::<String>();
        let title = if title.trim().is_empty() {
            "Атлас".into()
        } else {
            title
        };
        self.insert_png(title, cell, bytes)
    }

    fn add_terrain_atlas(&mut self, style: terrain_atlas::Style) -> Result<(u32, bool), String> {
        let png = style.png()?;
        if let Some(atlas) = self.atlases.iter().find(|atlas| {
            atlas.name == style.name()
                && atlas.cell == terrain_atlas::Style::CELL
                && atlas.png == png
        }) {
            return Ok((atlas.id, false));
        }
        self.insert_png(style.name().into(), Some(terrain_atlas::Style::CELL), png)
            .map(|id| (id, true))
    }

    fn insert_png(
        &mut self,
        title: String,
        cell: Option<[u16; 2]>,
        bytes: Vec<u8>,
    ) -> Result<u32, String> {
        if self.atlases.len() >= MAX_ATLASES {
            return Err("Лимит атласов этой карты достигнут.".into());
        }
        if bytes.len() > MAX_ATLAS_BYTES
            || self.atlases.iter().map(|a| a.png.len()).sum::<usize>() + bytes.len() > MAX_PNG_BYTES
        {
            return Err("Допускается до 4 МБ на PNG и до 8 МБ на атласы карты.".into());
        }
        let id = self.next_atlas.max(
            self.atlases
                .iter()
                .map(|a| a.id)
                .max()
                .unwrap_or(0)
                .saturating_add(1),
        );
        let next = id
            .checked_add(1)
            .ok_or("Недоступен идентификатор атласа.")?;
        let atlas = Atlas::new(id, title, cell, bytes)?;
        self.atlases.push(atlas);
        self.next_atlas = next;
        Ok(id)
    }

    pub fn resize(&mut self, size: [u16; 2]) -> bool {
        let size = size.map(|side| side.clamp(4, MAX_EDGE));
        if size == self.size {
            return false;
        }
        let regrid = |indices: &mut Vec<Option<Tile>>| {
            let mut cells = vec![None; usize::from(size[0]) * usize::from(size[1])];
            for row in 0..size[1].min(self.size[1]) {
                let len = usize::from(size[0].min(self.size[0]));
                let to = usize::from(row) * usize::from(size[0]);
                let from = usize::from(row) * usize::from(self.size[0]);
                cells[to..to + len].copy_from_slice(&indices[from..from + len]);
            }
            *indices = cells;
        };
        for layer in &mut self.layers {
            regrid(layer);
        }
        let mut collisions = vec![Collision::Empty; usize::from(size[0]) * usize::from(size[1])];
        for row in 0..size[1].min(self.size[1]) {
            let len = usize::from(size[0].min(self.size[0]));
            let to = usize::from(row) * usize::from(size[0]);
            let from = usize::from(row) * usize::from(self.size[0]);
            collisions[to..to + len].copy_from_slice(&self.colliders[from..from + len]);
        }
        self.colliders = collisions;
        self.size = size;
        if self
            .respawn
            .is_some_and(|cell| self.cell_index(cell).is_none())
        {
            self.respawn = None;
        }
        for axis in 0..2 {
            self.spawn[axis] = self.spawn[axis].clamp(0.5, f32::from(size[axis]) - 0.5);
        }
        true
    }

    fn cell_index(&self, [col, row]: [u16; 2]) -> Option<usize> {
        (col < self.size[0] && row < self.size[1])
            .then(|| usize::from(row) * usize::from(self.size[0]) + usize::from(col))
    }

    pub fn paint(&mut self, cell: [u16; 2], layer: Layer, value: Option<Tile>) -> bool {
        let Some(index) = self.cell_index(cell) else {
            return false;
        };
        if value.is_some_and(|value| !self.valid_tile(value)) {
            return false;
        }
        let before = &mut self.layers[layer.index()][index];
        if *before == value {
            return false;
        }
        *before = value;
        true
    }

    pub fn paint_collision(&mut self, cell: [u16; 2], value: Collision) -> bool {
        let Some(index) = self.cell_index(cell) else {
            return false;
        };
        if self.mode == Mode::Top && value == Collision::Platform {
            return false;
        }
        let before = self.colliders[index];
        if before == value {
            return false;
        }
        self.colliders[index] = value;
        if value == Collision::Solid && occupied(self, &[], self.spawn) {
            self.colliders[index] = before;
            return false;
        }
        true
    }

    pub fn set_spawn(&mut self, position: [f32; 2]) -> bool {
        if position == self.spawn
            || !position
                .iter()
                .zip(self.size)
                .all(|(pos, size)| (0.1..=f32::from(size) - 0.1).contains(pos))
            || occupied(self, &[], position)
        {
            return false;
        }
        self.spawn = position;
        true
    }

    pub fn set_respawn(&mut self, cell: Option<[u16; 2]>) -> bool {
        if self.mode != Mode::Side
            || self.respawn == cell
            || cell.is_some_and(|cell| self.respawn_position(cell).is_none())
        {
            return false;
        }
        self.respawn = cell;
        true
    }

    fn respawn_position(&self, [col, row]: [u16; 2]) -> Option<[f32; 2]> {
        if row.saturating_add(1) >= self.size[1] || col >= self.size[0] {
            return None;
        }
        let floor = i32::from(row) + 1;
        let position = [f32::from(col) + 0.5, floor as f32];
        (self.collision_at([i32::from(col), floor]) != Collision::Empty
            && !occupied(self, &[], position))
        .then_some(position)
    }

    pub fn collision_at(&self, [x, y]: [i32; 2]) -> Collision {
        let Ok(x) = u16::try_from(x) else {
            return Collision::Empty;
        };
        let Ok(y) = u16::try_from(y) else {
            return Collision::Empty;
        };
        self.cell_index([x, y])
            .map_or(Collision::Empty, |index| self.colliders[index])
    }

    pub fn to_world(&self, [x, row]: [f32; 2]) -> [f32; 3] {
        match self.mode {
            Mode::Top => [x, 0.0, row],
            Mode::Side => [x, f32::from(self.size[1]) - row, 0.0],
        }
    }

    pub fn from_world(&self, [x, y, z]: [f32; 3]) -> [f32; 2] {
        match self.mode {
            Mode::Top => [x, z],
            Mode::Side => [x, f32::from(self.size[1]) - y],
        }
    }

    fn valid_tile(&self, tile: Tile) -> bool {
        self.atlas(tile.atlas)
            .is_some_and(|atlas| u32::from(tile.index) < atlas.count())
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let cells = usize::from(self.size[0]) * usize::from(self.size[1]);
        if self
            .size
            .iter()
            .any(|extent| !(4..=MAX_EDGE).contains(extent))
            || self
                .spawn
                .iter()
                .zip(self.size)
                .any(|(point, size)| !(0.1..=f32::from(size) - 0.1).contains(point))
            || self.layers.iter().any(|layer| layer.len() != cells)
            || self.colliders.len() != cells
            || self.mode == Mode::Top && self.colliders.contains(&Collision::Platform)
            || self
                .respawn
                .is_some_and(|cell| self.mode != Mode::Side || self.cell_index(cell).is_none())
        {
            return Err("Некорректные размеры, слои, старт или столкновения 2D-карты.".into());
        }
        let mut ids = HashSet::new();
        if self.atlases.len() > MAX_ATLASES
            || self.atlases.iter().map(|a| a.png.len()).sum::<usize>() > MAX_PNG_BYTES
        {
            return Err("Превышен лимит PNG-атласов карты.".into());
        }
        for atlas in &self.atlases {
            atlas.validate_metadata()?;
            let texture = decode_png(&atlas.png)?;
            if !ids.insert(atlas.id)
                || atlas.size != [texture.width() as u16, texture.height() as u16]
            {
                return Err("Некорректный номер или размер PNG-атласа.".into());
            }
        }
        if self
            .layers
            .iter()
            .flatten()
            .flatten()
            .any(|&tile| !self.valid_tile(tile))
            || self.hero.is_some_and(|tile| !self.valid_tile(tile))
        {
            return Err("Тайл 2D-карты ссылается на отсутствующую ячейку PNG.".into());
        }
        Ok(())
    }
}

pub fn scene(mode: Mode, demo: bool) -> Scene {
    let mut scene = Scene::empty();
    scene.map_2d = Some(if demo {
        Map::demo(mode)
    } else {
        Map::new(mode)
    });
    if demo {
        let map = scene.map_2d.as_ref().expect("new map");
        let position = map.to_world(if mode == Mode::Top {
            [22.5, 10.5]
        } else {
            [41.5, 15.0]
        });
        if add_marker(&mut scene, position).is_some() {
            scene.world.objects.last_mut().expect("new marker").name = "Метка финиша".into();
        }
        if scene.rules.add_zone(position).is_some() {
            scene.rules.zones.last_mut().expect("new zone").name = "Финиш".into();
        }
    }
    scene
}

fn add_marker(scene: &mut Scene, position: [f32; 3]) -> Option<u64> {
    let map = scene.map_2d.as_mut()?;
    if scene.world.objects.len() >= 96 {
        return None;
    }
    let id = scene
        .world
        .objects
        .iter()
        .map(|item| item.id.saturating_add(1))
        .max()
        .unwrap_or(1 << 40)
        .max(map.next_marker);
    if id >= 1 << 48 {
        return None;
    }
    map.next_marker = id + 1;
    scene.world.objects.push(WorldObject {
        id,
        name: format!("Метка {}", scene.world.objects.len() + 1),
        kind: WorldKind::Platform,
        position,
        size: [0.8; 3],
        yaw: 0.0,
        color: [0.32, 0.85, 0.73],
        solid: false,
    });
    Some(id)
}

fn decode_png(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    if bytes.len() > MAX_ATLAS_BYTES || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Выбери PNG размером до 4 МБ.".into());
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_PNG_EDGE);
    limits.max_image_height = Some(MAX_PNG_EDGE);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map(|image| image.into_rgba8())
        .map_err(|err| format!("Не удалось открыть атлас PNG ({err})."))
}

mod encoded_png {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(bytes.len() * 2);
        for &byte in bytes {
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 15) as usize] as char);
        }
        serializer.serialize_str(&encoded)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        if encoded.len() % 2 != 0 || encoded.len() / 2 > super::MAX_ATLAS_BYTES {
            return Err(D::Error::custom("недопустимый размер PNG"));
        }
        encoded
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let high = (pair[0] as char)
                    .to_digit(16)
                    .ok_or_else(|| D::Error::custom("повреждён PNG"))?;
                let low = (pair[1] as char)
                    .to_digit(16)
                    .ok_or_else(|| D::Error::custom("повреждён PNG"))?;
                Ok((high * 16 + low) as u8)
            })
            .collect()
    }
}

#[derive(Clone, Copy, Default)]
pub struct Input {
    pub movement: [f32; 2],
    pub sprint: bool,
    pub jump: bool,
}

pub struct Player {
    position: [f32; 2],
    velocity: [f32; 2],
    mode: Mode,
    rows: u16,
    grounded: bool,
    active_respawn: Option<[u16; 2]>,
    direction: u16,
    stride: f32,
    moving: bool,
    view: ui::View,
}

impl Player {
    pub fn new(map: &Map) -> Self {
        let position = if occupied(map, &[], map.spawn) {
            (0..map.size[1])
                .flat_map(|row| {
                    (0..map.size[0]).map(move |x| [f32::from(x) + 0.5, f32::from(row) + 0.5])
                })
                .filter(|&point| !occupied(map, &[], point))
                .min_by(|&a, &b| {
                    Vec2::from_array(a)
                        .distance_squared(Vec2::from_array(map.spawn))
                        .total_cmp(
                            &Vec2::from_array(b).distance_squared(Vec2::from_array(map.spawn)),
                        )
                })
                .unwrap_or(map.spawn)
        } else {
            map.spawn
        };
        Self {
            position,
            velocity: [0.0; 2],
            mode: map.mode,
            rows: map.size[1],
            grounded: false,
            active_respawn: None,
            direction: if map.mode == Mode::Side { 3 } else { 0 },
            stride: 0.0,
            moving: false,
            view: ui::View::default(),
        }
    }

    pub fn reset(&mut self, map: &Map) {
        let view = std::mem::take(&mut self.view);
        *self = Self::new(map);
        self.view = view;
    }

    pub fn resume(&mut self, map: &Map, world: [f32; 3]) {
        self.reset(map);
        let [x, row] = map.from_world(world);
        if (0.24..=f32::from(map.size[0]) - 0.24).contains(&x)
            && (-1.5..=f32::from(map.size[1]) + 0.5).contains(&row)
            && !occupied(map, &[], [x, row])
        {
            self.position = [x, row];
        }
    }

    pub fn respawn_cell(&self) -> Option<[u16; 2]> {
        self.active_respawn
    }

    pub fn restore_respawn(&mut self, map: &Map, cell: Option<[u16; 2]>) {
        self.active_respawn =
            cell.filter(|&cell| map.respawn == Some(cell) && map.respawn_position(cell).is_some());
    }

    pub fn world_position(&self) -> [f32; 3] {
        let [x, row] = self.position;
        match self.mode {
            Mode::Top => [x, 0.0, row],
            Mode::Side => [x, (f32::from(self.rows) - row).max(0.0), 0.0],
        }
    }

    pub(crate) fn contact_for_footsteps(&self) -> bool {
        self.moving && (self.mode == Mode::Top || self.grounded)
    }

    pub fn step(
        &mut self,
        scene: &Scene,
        session: &rules::Session,
        seconds: f32,
        input: Input,
    ) -> bool {
        let Some(map) = &scene.map_2d else {
            return false;
        };
        if seconds <= 0.0 || !seconds.is_finite() {
            return false;
        }
        let mut motion = Vec2::from_array(input.movement).clamp_length_max(1.0);
        if !motion.is_finite() {
            motion = Vec2::ZERO;
        }
        if map.mode == Mode::Side {
            motion.y = 0.0;
        }
        let settings = scene.play_settings.movement_2d;
        let target = motion
            * if input.sprint {
                settings.sprint_speed
            } else {
                settings.walk_speed
            };
        let time = seconds.min(0.1);
        let steps = (time * 120.0).ceil() as u32;
        let step = time / steps as f32;
        let prev = self.position;
        let props = scene
            .world
            .objects
            .iter()
            .filter(|object| object.solid && session.object_enabled(object.id))
            .map(|object| object_bounds(map, object))
            .collect::<Vec<_>>();
        if input.jump && map.mode == Mode::Side && self.grounded {
            self.velocity[1] = -settings.jump_speed;
            self.grounded = false;
        }
        for _ in 0..steps {
            let steer = |value: f32, goal: f32| {
                let rate = if goal.abs() < value.abs() || goal * value < 0.0 {
                    settings.braking
                } else {
                    settings.acceleration
                };
                value + (goal - value).clamp(-step * rate, step * rate)
            };
            self.velocity[0] = steer(self.velocity[0], target.x);
            self.velocity[1] = if map.mode == Mode::Top {
                steer(self.velocity[1], target.y)
            } else {
                (self.velocity[1] + step * 23.0).min(15.0)
            };
            slide(map, &props, &mut self.position, 0, self.velocity[0] * step);
            let future = self.position[1] + self.velocity[1] * step;
            if map.mode == Mode::Side
                && self.velocity[1] >= 0.0
                && let Some(row) = landing_row(map, self.position, future)
            {
                self.position[1] = row;
                self.velocity[1] = 0.0;
                self.grounded = true;
            } else {
                let hit = slide(map, &props, &mut self.position, 1, self.velocity[1] * step);
                self.grounded = self.velocity[1] >= 0.0 && hit;
                if hit {
                    self.velocity[1] = 0.0;
                }
            }
            if self.grounded
                && let Some(cell) = map.respawn
                && self.active_respawn != Some(cell)
                && let Some(anchor) = map.respawn_position(cell)
                && (self.position[0] - anchor[0]).abs() <= 0.5
                && (self.position[1] - anchor[1]).abs() <= EPS * 4.0
                && !occupied(map, &props, anchor)
            {
                self.active_respawn = Some(cell);
            }
            if map.mode == Mode::Side && self.position[1] > f32::from(map.size[1]) + 1.2 {
                let respawn = self.active_respawn.and_then(|cell| {
                    let anchor = map.respawn_position(cell)?;
                    (!occupied(map, &props, anchor)).then_some((cell, anchor))
                });
                self.reset(map);
                if let Some((cell, anchor)) = respawn {
                    self.active_respawn = Some(cell);
                    self.position = anchor;
                    self.grounded = true;
                }
                return true;
            }
        }
        if motion.length_squared() > 0.01 {
            self.direction = if motion.x.abs() > motion.y.abs() {
                if motion.x < 0.0 { 2 } else { 3 }
            } else if motion.y < 0.0 {
                1
            } else {
                0
            };
        }
        let distance = (Vec2::from_array(prev) - Vec2::from_array(self.position)).length();
        self.moving = distance > 0.001 && (map.mode == Mode::Top || self.grounded);
        self.stride = (self.stride + distance * 3.0).rem_euclid(3.0);
        false
    }
}

fn object_bounds(map: &Map, object: &WorldObject) -> [f32; 4] {
    let [x, row] = map.from_world(object.position);
    let half = object.size[0] * 0.5;
    let (top, bottom) = if map.mode == Mode::Top {
        (row - object.size[2] * 0.5, row + object.size[2] * 0.5)
    } else {
        (row - object.size[1], row)
    };
    [x - half, top, x + half, bottom]
}

fn occupied(map: &Map, objects: &[[f32; 4]], [x, row]: [f32; 2]) -> bool {
    let [left, top, right, bottom] = if map.mode == Mode::Top {
        [x - 0.24, row - 0.24, x + 0.24, row + 0.24]
    } else {
        [x - 0.24, row - 0.78, x + 0.24, row]
    };
    if (left + EPS).floor() < 0.0
        || right > f32::from(map.size[0]) + EPS
        || map.mode == Mode::Top && (top < 0.0 || bottom > f32::from(map.size[1]) + EPS)
    {
        return true;
    }
    for y in (top + EPS).floor() as i32..=(bottom - EPS).floor() as i32 {
        for x in (left + EPS).floor() as i32..=(right - EPS).floor() as i32 {
            if map.collision_at([x, y]) == Collision::Solid {
                return true;
            }
        }
    }
    objects
        .iter()
        .any(|&[l, t, r, b]| right > l + EPS && left < r - EPS && bottom > t + EPS && top < b - EPS)
}

fn slide(
    map: &Map,
    objects: &[[f32; 4]],
    position: &mut [f32; 2],
    axis: usize,
    distance: f32,
) -> bool {
    let mut target = *position;
    target[axis] += distance;
    if !occupied(map, objects, target) {
        *position = target;
        return false;
    }
    let mut free = 0.0;
    let mut blocked = 1.0;
    for _ in 0..9 {
        let mid = (free + blocked) * 0.5;
        target[axis] = position[axis] + distance * mid;
        if occupied(map, objects, target) {
            blocked = mid;
        } else {
            free = mid;
        }
    }
    position[axis] += distance * free;
    true
}

fn landing_row(map: &Map, [x, row]: [f32; 2], future: f32) -> Option<f32> {
    for y in (row - EPS).ceil() as i32..=(future + EPS).floor() as i32 {
        for col in (x - 0.24 + EPS).floor() as i32..=(x + 0.24 - EPS).floor() as i32 {
            if map.collision_at([col, y]) == Collision::Platform {
                return Some(y as f32);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_sets_have_distinct_textures_and_persist_with_both_map_modes() {
        for mode in [Mode::Top, Mode::Side] {
            let mut scene = scene(mode, false);
            let map = scene.map_2d.as_mut().unwrap();
            let collisions = map.colliders.clone();
            let mut styles = HashSet::new();
            for (row, style) in terrain_atlas::Style::ALL.into_iter().enumerate() {
                let (id, inserted) = map.add_terrain_atlas(style).unwrap();
                assert!(inserted);
                assert_eq!(map.add_terrain_atlas(style).unwrap(), (id, false));
                let atlas = map.atlas(id).unwrap();
                assert_eq!(atlas.cell, [32, 32]);
                assert_eq!(atlas.count(), 8);
                let texture = decode_png(&atlas.png).unwrap();
                assert_eq!(texture.get_pixel(0, 0).0[3], 255);
                assert_eq!(texture.get_pixel(64, 32).0[3], 0);
                assert!(
                    texture
                        .enumerate_pixels()
                        .any(|(x, y, pixel)| { x >= 64 && y >= 32 && pixel.0[3] == 255 })
                );
                assert!(styles.insert(atlas.png.clone()));
                let cell = [2, row as u16 + 2];
                assert!(map.paint(
                    cell,
                    Layer::Ground,
                    Some(Tile {
                        atlas: id,
                        index: 0
                    })
                ));
                assert!(map.paint(
                    cell,
                    Layer::Detail,
                    Some(Tile {
                        atlas: id,
                        index: 6
                    })
                ));
            }
            assert_eq!(map.colliders, collisions);
            let json = serde_json::to_vec(&scene).unwrap();
            let reopened: Scene = serde_json::from_slice(&json).unwrap();
            reopened.validate().unwrap();
            assert_eq!(reopened.map_2d, scene.map_2d);
        }
    }

    #[test]
    fn top_map_preserves_a_separate_collision_layer_and_bridged_route() {
        let mut scene = scene(Mode::Top, true);
        let map = scene.map_2d.as_mut().unwrap();
        let guard = [6, 10];
        let tile = map.layers[0][map.cell_index(guard).unwrap()];
        assert!(map.paint_collision(guard, Collision::Solid));
        assert_eq!(map.layers[0][map.cell_index(guard).unwrap()], tile);
        let mut player = Player::new(map);
        let rules = rules::Session::default();
        let right = Input {
            movement: [1.0, 0.0],
            ..Input::default()
        };
        for _ in 0..180 {
            player.step(&scene, &rules, 1.0 / 60.0, right);
        }
        assert!((5.73..=5.77).contains(&player.world_position()[0]));
        scene
            .map_2d
            .as_mut()
            .unwrap()
            .paint_collision(guard, Collision::Empty);
        for _ in 0..180 {
            player.step(&scene, &rules, 1.0 / 60.0, right);
        }
        let [x, y, z] = player.world_position();
        assert!(x > 16.0 && y == 0.0 && z == 10.5, "{x} {y} {z}");
        scene.validate().unwrap();
    }

    #[test]
    fn side_player_jumps_up_through_platform_lands_on_top_and_respawns_in_pit() {
        let mut scene = scene(Mode::Side, false);
        let map = scene.map_2d.as_mut().unwrap();
        assert!(map.set_spawn([3.5, 15.0]));
        for x in 2..8 {
            map.paint_collision([x, 15], Collision::Solid);
        }
        map.paint_collision([3, 13], Collision::Platform);
        let mut player = Player::new(map);
        let rules = rules::Session::default();
        player.step(&scene, &rules, 1.0 / 60.0, Input::default());
        assert!(player.grounded);
        player.step(
            &scene,
            &rules,
            1.0 / 60.0,
            Input {
                jump: true,
                ..Input::default()
            },
        );
        let mut apex = player.position[1];
        for _ in 0..100 {
            player.step(&scene, &rules, 1.0 / 60.0, Input::default());
            apex = apex.min(player.position[1]);
        }
        assert!(apex < 13.0, "jump apex {apex}");
        assert_eq!(player.position[1], 13.0);
        for _ in 0..240 {
            if player.step(
                &scene,
                &rules,
                1.0 / 60.0,
                Input {
                    movement: [1.0, 0.0],
                    sprint: true,
                    jump: false,
                },
            ) {
                assert_eq!(player.world_position(), [3.5, 3.0, 0.0]);
                return;
            }
        }
        panic!("player did not respawn after falling off the level");
    }
}
