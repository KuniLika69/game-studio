use super::{Collision, Layer, Map, Tile};

#[derive(Clone, Copy)]
pub(super) struct CellRect {
    pub min: [u16; 2],
    pub max: [u16; 2],
}

impl CellRect {
    pub fn new([ax, ay]: [u16; 2], [bx, by]: [u16; 2]) -> Self {
        Self {
            min: [ax.min(bx), ay.min(by)],
            max: [ax.max(bx), ay.max(by)],
        }
    }

    fn size(self) -> [u16; 2] {
        [self.max[0] - self.min[0] + 1, self.max[1] - self.min[1] + 1]
    }

    fn clipped(self, [width, height]: [u16; 2]) -> Option<Self> {
        let [x, y] = self.min;
        (x < width && y < height).then(|| Self {
            min: self.min,
            max: [self.max[0].min(width - 1), self.max[1].min(height - 1)],
        })
    }

    pub fn cells(self) -> impl Iterator<Item = [u16; 2]> {
        let [left, top] = self.min;
        let [right, bottom] = self.max;
        (top..=bottom).flat_map(move |row| (left..=right).map(move |col| [col, row]))
    }
}

struct PatchCell {
    layers: [Option<Tile>; 3],
    collision: Collision,
}

pub(super) struct MapPatch {
    pub size: [u16; 2],
    pub tiles: bool,
    pub collisions: bool,
    cells: Vec<PatchCell>,
}

impl MapPatch {
    pub fn destination(&self, origin: [u16; 2], map_size: [u16; 2]) -> Option<CellRect> {
        let [x, row] = origin;
        CellRect::new(
            origin,
            [
                x.saturating_add(self.size[0] - 1),
                row.saturating_add(self.size[1] - 1),
            ],
        )
        .clipped(map_size)
    }
}

impl Map {
    pub(super) fn fill_tiles(&mut self, start: [u16; 2], layer: Layer, tile: Option<Tile>) -> bool {
        if tile.is_some_and(|tile| !self.valid_tile(tile)) {
            return false;
        }
        let Some(start) = self.cell_index(start) else {
            return false;
        };
        let tiles = &mut self.layers[layer.index()];
        let cells = flood(tiles, self.size, start, tile);
        for &index in &cells {
            tiles[index] = tile;
        }
        !cells.is_empty()
    }

    pub(super) fn fill_collisions(&mut self, start: [u16; 2], value: Collision) -> bool {
        if self.mode == super::Mode::Top && value == Collision::Platform {
            return false;
        }
        let Some(start) = self.cell_index(start) else {
            return false;
        };
        let width = usize::from(self.size[0]);
        flood(&self.colliders, self.size, start, value)
            .into_iter()
            .fold(false, |changed, index| {
                let cell = [(index % width) as u16, (index / width) as u16];
                self.paint_collision(cell, value) | changed
            })
    }

    pub(super) fn copy_patch(
        &self,
        area: CellRect,
        tiles: bool,
        collisions: bool,
    ) -> Option<MapPatch> {
        if !tiles && !collisions {
            return None;
        }
        let area = area.clipped(self.size)?;
        let stride = usize::from(self.size[0]);
        let cells = area
            .cells()
            .map(|[x, row]| {
                let index = usize::from(row) * stride + usize::from(x);
                PatchCell {
                    layers: std::array::from_fn(|layer| self.layers[layer][index]),
                    collision: self.colliders[index],
                }
            })
            .collect();
        Some(MapPatch {
            size: area.size(),
            tiles,
            collisions,
            cells,
        })
    }

    pub(super) fn paste_patch(&mut self, origin: [u16; 2], patch: &MapPatch) -> bool {
        let Some(destination) = patch.destination(origin, self.size) else {
            return false;
        };
        if patch.tiles
            && patch
                .cells
                .iter()
                .flat_map(|cell| cell.layers)
                .flatten()
                .any(|tile| !self.valid_tile(tile))
        {
            return false;
        }
        let mut changed = false;
        for cell in destination.cells() {
            let [x, row] = [cell[0] - origin[0], cell[1] - origin[1]];
            let source =
                &patch.cells[usize::from(row) * usize::from(patch.size[0]) + usize::from(x)];
            if patch.tiles {
                for (layer, tile) in [Layer::Ground, Layer::Detail, Layer::Tall]
                    .into_iter()
                    .zip(source.layers)
                {
                    changed |= self.paint(cell, layer, tile);
                }
            }
            if patch.collisions {
                changed |= self.paint_collision(cell, source.collision);
            }
        }
        changed
    }
}

fn flood<T: Copy + Eq>(
    cells: &[T],
    [width, height]: [u16; 2],
    start: usize,
    replace: T,
) -> Vec<usize> {
    let previous = cells[start];
    if previous == replace {
        return Vec::new();
    }
    let [width, height] = [usize::from(width), usize::from(height)];
    let mut visited = vec![false; cells.len()];
    let mut matching = vec![start];
    visited[start] = true;
    let mut next = 0;
    while let Some(&index) = matching.get(next) {
        next += 1;
        let x = index % width;
        let row = index / width;
        for neighbor in [
            (x > 0).then(|| index - 1),
            (x + 1 < width).then(|| index + 1),
            (row > 0).then(|| index - width),
            (row + 1 < height).then(|| index + width),
        ]
        .into_iter()
        .flatten()
        {
            if !visited[neighbor] && cells[neighbor] == previous {
                visited[neighbor] = true;
                matching.push(neighbor);
            }
        }
    }
    matching
}
