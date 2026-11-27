use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage, codecs::png::PngEncoder};

const CELL: u16 = 32;
const COLUMNS: u16 = 4;
const ROWS: u16 = 2;
const CLEAR: Rgba<u8> = Rgba([0; 4]);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Style {
    #[default]
    Forest,
    Ruins,
    Neon,
}

struct Colors {
    ground: [u8; 3],
    stone: [u8; 3],
    dirt: [u8; 3],
    leaf: [u8; 3],
    water: [u8; 3],
    light: [u8; 3],
}

impl Style {
    pub(super) const ALL: [Self; 3] = [Self::Forest, Self::Ruins, Self::Neon];
    pub(super) const CELL: [u16; 2] = [CELL; 2];

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Forest => "Лес в сумерках",
            Self::Ruins => "Тёплые руины",
            Self::Neon => "Неоновая станция",
        }
    }

    pub(super) fn tile_name(self, index: u16) -> Option<&'static str> {
        let names = match self {
            Self::Forest => [
                "Мох",
                "Замшелая кладка",
                "Каменная тропа",
                "Пруд",
                "Край дернины",
                "Корни и грунт",
                "Папоротник",
                "Светящийся гриб",
            ],
            Self::Ruins => [
                "Песок",
                "Старая кладка",
                "Мозаика",
                "Вода канала",
                "Песчаный уступ",
                "Каменистый грунт",
                "Обломки",
                "Фонарь",
            ],
            Self::Neon => [
                "Композит",
                "Металлическая стена",
                "Разметка пола",
                "Жидкий охладитель",
                "Край платформы",
                "Ребристая обшивка",
                "Вентиляция",
                "Сигнальный маяк",
            ],
        };
        names.get(usize::from(index)).copied()
    }

    fn colors(self) -> Colors {
        match self {
            Self::Forest => Colors {
                ground: [46, 83, 68],
                stone: [78, 97, 99],
                dirt: [65, 55, 58],
                leaf: [91, 151, 93],
                water: [34, 64, 90],
                light: [162, 218, 140],
            },
            Self::Ruins => Colors {
                ground: [169, 113, 74],
                stone: [172, 129, 98],
                dirt: [108, 73, 71],
                leaf: [210, 165, 104],
                water: [53, 115, 112],
                light: [255, 210, 133],
            },
            Self::Neon => Colors {
                ground: [48, 40, 73],
                stone: [63, 58, 98],
                dirt: [39, 30, 63],
                leaf: [159, 80, 215],
                water: [40, 32, 115],
                light: [111, 242, 231],
            },
        }
    }

    pub(super) fn png(self) -> Result<Vec<u8>, String> {
        let width = u32::from(CELL * COLUMNS);
        let height = u32::from(CELL * ROWS);
        let colors = self.colors();
        let image = RgbaImage::from_fn(width, height, |x, y| {
            let edge = u32::from(CELL);
            let tile = x / edge + y / edge * u32::from(COLUMNS);
            self.pixel(tile, (x % edge) as i32, (y % edge) as i32, &colors)
        });
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
            .map_err(|error| format!("Не удалось создать атлас ({error})."))?;
        Ok(png)
    }

    fn pixel(self, tile: u32, x: i32, y: i32, colors: &Colors) -> Rgba<u8> {
        let speck = grain(x, y);
        match tile {
            0 => match self {
                Self::Forest if speck > 10 => shade(colors.leaf, -27),
                Self::Ruins if (x + 2 * y) % 17 < 2 && speck > 0 => shade(colors.leaf, -25),
                Self::Neon if x % 16 == 0 || y % 16 == 0 => shade(colors.ground, -18),
                Self::Neon if x % 16 == 2 && y % 16 == 2 => shade(colors.light, -43),
                _ => shade(colors.ground, speck),
            },
            1 => {
                let face_x = (x + y / 8 * 8) % 16;
                let face_y = y % 8;
                let edge = if face_x == 0 || face_y == 7 {
                    -38
                } else if face_x == 1 || face_y == 0 {
                    19
                } else {
                    0
                };
                shade(colors.stone, edge + speck / 2)
            }
            2 => {
                let dx = (x % 16 - 8).abs();
                let dy = (y % 16 - 8).abs();
                if dx + dy == 6 {
                    shade(colors.light, -35)
                } else if dx == 8 || dy == 8 {
                    shade(colors.stone, -31)
                } else {
                    shade(colors.stone, 12 + speck / 3)
                }
            }
            3 => {
                let waves = [0, 0, 1, 1, 2, 2, 1, 1];
                let wave = (y + waves[((x + y / 8 * 3) % 8) as usize]) % 8;
                let reflected = if wave == 1 && (x + y / 8 * 5) % 16 < 9 {
                    28
                } else if wave < 4 {
                    6
                } else {
                    -4
                };
                shade(colors.water, reflected + speck / 3)
            }
            4 if y < 3 => shade(colors.leaf, 24 - 10 * y + speck / 3),
            4 if y <= 5 + (x / 4 % 3) => shade(colors.leaf, -17 + speck / 2),
            4 | 5 => {
                let chip = match self {
                    Self::Neon if x % 8 < 2 => 19,
                    _ if (x + 2 * y) % 13 == 0 && speck > 2 => 18,
                    _ => 0,
                };
                shade(colors.dirt, speck + chip)
            }
            6 => self.decoration(x, y, colors),
            _ => self.beacon(x, y, colors),
        }
    }

    fn decoration(self, x: i32, y: i32, colors: &Colors) -> Rgba<u8> {
        let dx = (x - 16).abs();
        match self {
            Self::Forest if (8..=27).contains(&y) && dx <= 1 => shade(colors.leaf, -8),
            Self::Forest
                if (10..=24).contains(&y) && dx <= (29 - y) / 2 && (y + dx / 2) % 4 <= 1 =>
            {
                shade(colors.leaf, 25 - dx * 3)
            }
            Self::Ruins
                if (x - 13).abs() + (y - 20).abs() * 2 <= 9
                    || (x - 22).abs() * 2 + (y - 24).abs() <= 9 =>
            {
                shade(colors.stone, 40 - y * 2 + grain(x, y))
            }
            Self::Neon if dx <= 10 && (10..=23).contains(&y) => {
                if dx >= 9 || y == 10 || y == 23 {
                    shade(colors.leaf, 15)
                } else {
                    shade(colors.dirt, 30 * (y % 3 == 0) as i32)
                }
            }
            _ => CLEAR,
        }
    }

    fn beacon(self, x: i32, y: i32, colors: &Colors) -> Rgba<u8> {
        let dx = (x - 16).abs();
        let head = match self {
            Self::Forest => dx * dx + (y - 14) * (y - 14) * 3 <= 54,
            Self::Ruins => dx <= 5 && (10..=18).contains(&y),
            Self::Neon => dx + (y - 13).abs() <= 7,
        };
        if head {
            shade(colors.light, 16 - dx * 3)
        } else if (17..=25).contains(&y) && dx <= 2 || (25..=27).contains(&y) && dx <= 6 {
            shade(colors.stone, 22 - dx * 3)
        } else {
            CLEAR
        }
    }
}

fn grain(x: i32, y: i32) -> i32 {
    let hash = (x * 1877 + y * 2767) ^ (x * y * 101);
    hash % 27 - 13
}

fn shade(rgb: [u8; 3], offset: i32) -> Rgba<u8> {
    let [r, g, b] = rgb.map(|channel| (i32::from(channel) + offset).clamp(0, 255) as u8);
    Rgba([r, g, b, 255])
}
