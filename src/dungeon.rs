//! Deterministic floor plans for the closed-map experiment.
use std::collections::VecDeque;

pub const MIN_SIDE: u16 = 18;
pub const MAX_SIDE: u16 = 96;
pub const MIN_ROOMS: u16 = 3;
const MAX_ROOMS: u16 = 24;
const MIN_SLOT: u16 = 6;

pub type Cell = [u16; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub seed: u64,
    pub size: Cell,
    pub rooms: u16,
    pub extra_passages: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            seed: 921_731,
            size: [34, 24],
            rooms: 8,
            extra_passages: 25,
        }
    }
}

impl Settings {
    pub fn max_rooms(self) -> u16 {
        let [cols, rows] = self
            .size
            .map(|side| side.clamp(MIN_SIDE, MAX_SIDE).saturating_sub(2) / MIN_SLOT);
        (cols * rows).min(MAX_ROOMS)
    }

    fn validate(self) -> Result<(), String> {
        if self
            .size
            .iter()
            .any(|side| !(MIN_SIDE..=MAX_SIDE).contains(side))
            || !(MIN_ROOMS..=self.max_rooms()).contains(&self.rooms)
            || self.extra_passages > 100
        {
            return Err(
                "Проверь размеры карты, число комнат и долю дополнительных проходов.".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tile {
    #[default]
    Wall,
    Room,
    Passage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Room {
    min: Cell,
    max: Cell,
}

impl Room {
    pub fn center(self) -> Cell {
        std::array::from_fn(|axis| (self.min[axis] + self.max[axis]) / 2)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub settings: Settings,
    pub entrance: Cell,
    pub exit: Cell,
    pub route: Vec<Cell>,
    rooms: Vec<Room>,
    tiles: Vec<Tile>,
}

impl Layout {
    pub fn tile(&self, cell: Cell) -> Option<Tile> {
        index(self.settings.size, cell).map(|index| self.tiles[index])
    }

    pub fn floor_cells(&self) -> usize {
        self.tiles
            .iter()
            .filter(|&&tile| tile != Tile::Wall)
            .count()
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }
}

pub fn generate(settings: Settings) -> Result<Layout, String> {
    settings.validate()?;
    let size = settings.size;
    let grid = slot_grid(settings);
    let slots = usize::from(grid[0]) * usize::from(grid[1]);
    let mut random = Random(settings.seed);
    let mut rooms = vec![None; slots];
    let mut passages = Vec::new();
    let start = random.below(slots);
    rooms[start] = Some(place_room(at(grid, start), grid, size, &mut random));
    for _ in 1..settings.rooms {
        let frontier = rooms
            .iter()
            .enumerate()
            .filter(|(_, room)| room.is_some())
            .flat_map(|(from, _)| {
                neighbors(grid, at(grid, from))
                    .map(move |cell| (from, index(grid, cell).expect("neighbor is in bounds")))
            })
            .filter(|&(_, to)| rooms[to].is_none())
            .collect::<Vec<_>>();
        let &(from, to) = frontier
            .get(random.below(frontier.len()))
            .ok_or("Нет свободного места для следующей комнаты.")?;
        rooms[to] = Some(place_room(at(grid, to), grid, size, &mut random));
        passages.push(edge(from, to));
    }
    for (from, room) in rooms.iter().enumerate().filter(|(_, room)| room.is_some()) {
        debug_assert!(room.is_some());
        for next in neighbors(grid, at(grid, from)) {
            let to = index(grid, next).expect("neighbor is in bounds");
            if to > from
                && rooms[to].is_some()
                && !passages.contains(&(from, to))
                && random.below(100) < usize::from(settings.extra_passages)
            {
                passages.push((from, to));
            }
        }
    }
    let mut tiles = vec![Tile::Wall; usize::from(size[0]) * usize::from(size[1])];
    for room in rooms.iter().flatten() {
        for row in room.min[1]..=room.max[1] {
            for col in room.min[0]..=room.max[0] {
                tiles[offset(size, [col, row])] = Tile::Room;
            }
        }
    }
    for (from, to) in passages {
        let from = rooms[from]
            .expect("corridor starts in an existing room")
            .center();
        let to = rooms[to]
            .expect("corridor ends in an existing room")
            .center();
        let horizontal_first = random.below(2) != 0;
        connect(size, &mut tiles, from, to, horizontal_first);
    }
    let centers = rooms
        .iter()
        .flatten()
        .map(|room| room.center())
        .collect::<Vec<_>>();
    let root = rooms[start].expect("root exists").center();
    let farthest = |from| {
        let paths = search(size, from, |cell| tiles[offset(size, cell)] != Tile::Wall)
            .expect("room center is open");
        centers
            .iter()
            .copied()
            .max_by_key(|&cell| paths.distance[offset(size, cell)])
            .expect("at least three rooms")
    };
    let entrance = farthest(root);
    let exit = farthest(entrance);
    let route = shortest_route(size, entrance, exit, |cell| {
        tiles[offset(size, cell)] != Tile::Wall
    })
    .ok_or("Выход недостижим из входа.")?;
    Ok(Layout {
        settings,
        entrance,
        exit,
        route,
        rooms: rooms.into_iter().flatten().collect(),
        tiles,
    })
}

fn slot_grid(settings: Settings) -> Cell {
    let [width, height] = settings.size.map(|side| side - 2);
    (1..=width / MIN_SLOT)
        .flat_map(|cols| (1..=height / MIN_SLOT).map(move |rows| [cols, rows]))
        .filter(|&[cols, rows]| cols * rows >= settings.rooms)
        .min_by_key(|&[cols, rows]| {
            (
                u32::from(cols * rows - settings.rooms) * 18
                    + u32::from((width * rows).abs_diff(height * cols)),
                cols,
                rows,
            )
        })
        .expect("validated room capacity")
}

fn place_room(cell: Cell, grid: Cell, size: Cell, random: &mut Random) -> Room {
    let mut room = Room {
        min: [0; 2],
        max: [0; 2],
    };
    for axis in 0..2 {
        let length = size[axis] - 2;
        let low = 1 + cell[axis] * length / grid[axis];
        let high = 1 + (cell[axis] + 1) * length / grid[axis];
        let max_extent = high - low - 2;
        let extent = random.inclusive((max_extent * 2 / 3).max(3), max_extent);
        room.min[axis] = random.inclusive(low + 1, high - 1 - extent);
        room.max[axis] = room.min[axis] + extent - 1;
    }
    room
}

fn edge(from: usize, to: usize) -> (usize, usize) {
    (from.min(to), from.max(to))
}

fn connect(size: Cell, tiles: &mut [Tile], from: Cell, to: Cell, horizontal_first: bool) {
    let corner = if horizontal_first {
        [to[0], from[1]]
    } else {
        [from[0], to[1]]
    };
    for (from, to) in [(from, corner), (corner, to)] {
        for row in from[1].min(to[1])..=from[1].max(to[1]) {
            for col in from[0].min(to[0])..=from[0].max(to[0]) {
                let tile = &mut tiles[offset(size, [col, row])];
                if *tile == Tile::Wall {
                    *tile = Tile::Passage;
                }
            }
        }
    }
}

pub fn shortest_route(
    size: Cell,
    entrance: Cell,
    exit: Cell,
    passable: impl Fn(Cell) -> bool,
) -> Option<Vec<Cell>> {
    let goal = index(size, exit)?;
    let paths = search(size, entrance, passable)?;
    let root = index(size, entrance)?;
    let mut index = goal;
    let mut backtrack = vec![exit];
    while index != root {
        index = *paths.parent.get(index)?.as_ref()?;
        backtrack.push(at(size, index));
    }
    backtrack.reverse();
    Some(backtrack)
}

struct Search {
    parent: Vec<Option<usize>>,
    distance: Vec<usize>,
}

fn search(size: Cell, entrance: Cell, passable: impl Fn(Cell) -> bool) -> Option<Search> {
    if size.iter().any(|&side| side == 0 || side > MAX_SIDE) {
        return None;
    }
    let root = index(size, entrance)?;
    if !passable(entrance) {
        return None;
    }
    let len = usize::from(size[0]) * usize::from(size[1]);
    let mut parent = vec![None; len];
    let mut distance = vec![0; len];
    let mut open = VecDeque::from([root]);
    parent[root] = Some(root);
    while let Some(from) = open.pop_front() {
        for next in neighbors(size, at(size, from)) {
            let to = offset(size, next);
            if parent[to].is_none() && passable(next) {
                parent[to] = Some(from);
                distance[to] = distance[from] + 1;
                open.push_back(to);
            }
        }
    }
    Some(Search { parent, distance })
}

fn neighbors(size: Cell, [x, y]: Cell) -> impl Iterator<Item = Cell> {
    [
        (x + 1 < size[0]).then(|| [x + 1, y]),
        (y + 1 < size[1]).then(|| [x, y + 1]),
        (x > 0).then(|| [x - 1, y]),
        (y > 0).then(|| [x, y - 1]),
    ]
    .into_iter()
    .flatten()
}

fn offset(size: Cell, [x, y]: Cell) -> usize {
    usize::from(y) * usize::from(size[0]) + usize::from(x)
}

fn index(size: Cell, [x, y]: Cell) -> Option<usize> {
    (x < size[0] && y < size[1]).then(|| offset(size, [x, y]))
}

fn at(size: Cell, index: usize) -> Cell {
    let width = usize::from(size[0]);
    [(index % width) as u16, (index / width) as u16]
}

struct Random(u64);

impl Random {
    fn below(&mut self, limit: usize) -> usize {
        // SplitMix64 with fixed wrapping operations to keep seeds portable.
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        let value = value ^ (value >> 31);
        ((u128::from(value) * limit as u128) >> 64) as usize
    }

    fn inclusive(&mut self, low: u16, high: u16) -> u16 {
        low + self.below(usize::from(high - low + 1)) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_and_options_reproduce_exact_floor_plan() {
        let options = Settings::default();
        let a = generate(options).unwrap();
        let b = generate(options).unwrap();
        let c = generate(Settings {
            seed: options.seed + 1,
            ..options
        })
        .unwrap();
        assert_eq!(a, b);
        assert_ne!(a.tiles, c.tiles);
        assert_ne!(a.entrance, a.exit);
    }

    #[test]
    fn closed_border_and_every_floor_cell_reachable_across_seed_and_size_limits() {
        for size in [[18, 18], [34, 24], [18, 96], [96, 18], [96, 96]] {
            for seed in [0, u64::MAX].into_iter().chain(1..128) {
                let mut options = Settings {
                    seed,
                    size,
                    extra_passages: (seed % 101) as u8,
                    ..Settings::default()
                };
                options.rooms = options.max_rooms();
                let layout = generate(options).unwrap();
                assert_eq!(layout.rooms.len(), options.rooms as usize);
                let walkable = |cell| layout.tile(cell).is_some_and(|tile| tile != Tile::Wall);
                let paths = search(size, layout.entrance, walkable).unwrap();
                assert_eq!(
                    paths
                        .parent
                        .iter()
                        .filter(|parent| parent.is_some())
                        .count(),
                    layout.floor_cells()
                );
                assert_eq!(layout.route.first(), Some(&layout.entrance));
                assert_eq!(layout.route.last(), Some(&layout.exit));
                for col in 0..size[0] {
                    assert!(!walkable([col, 0]));
                    assert!(!walkable([col, size[1] - 1]));
                }
                for row in 0..size[1] {
                    assert!(!walkable([0, row]));
                    assert!(!walkable([size[0] - 1, row]));
                }
                for pair in layout.route.windows(2) {
                    assert!(pair.iter().copied().all(walkable));
                    assert_eq!(
                        pair[0][0].abs_diff(pair[1][0]) + pair[0][1].abs_diff(pair[1][1]),
                        1
                    );
                }
            }
        }
    }

    #[test]
    fn author_collision_change_can_invalidate_certificate() {
        let layout = generate(Settings::default()).unwrap();
        let exit = layout.exit;
        let within = |cell| layout.tile(cell).is_some_and(|tile| tile != Tile::Wall);
        assert_eq!(
            shortest_route(layout.settings.size, layout.entrance, exit, within),
            Some(layout.route.clone())
        );
        assert!(
            shortest_route(layout.settings.size, layout.entrance, exit, |cell| cell
                != exit
                && within(cell))
            .is_none()
        );
        assert!(shortest_route([2, 2], [9, 9], [0, 0], |cell| cell[0] < 2).is_none());
        assert!(
            generate(Settings {
                size: [0, 12],
                ..Settings::default()
            })
            .is_err()
        );
        assert!(
            generate(Settings {
                rooms: 25,
                ..Settings::default()
            })
            .is_err()
        );
    }
}
