use glam::Vec2;
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage, codecs::png::PngEncoder};
use serde_json::json;

use super::{Recipe, SHORE_X, Style};

#[derive(Clone, Copy)]
enum Material {
    Meadow,
    Sand,
    Stone,
    Timber,
}

pub(super) fn ground(recipe: &Recipe, beach: bool) -> Result<Vec<u8>, String> {
    let style = recipe.style;
    let material = if beach {
        Material::Sand
    } else {
        Material::Meadow
    };
    if style == Style::Natural {
        let mut xs: Vec<f32> = if beach {
            (0..=24)
                .map(|step| 4.5 + 9.3 * step as f32 / 24.0)
                .collect()
        } else {
            (0..=108)
                .map(|step| -47.0 + 51.5 * step as f32 / 108.0)
                .collect()
        };
        let mut zs = Vec::new();
        if beach {
            // Beyond the shallow strip, keep a coarse seabed under the entire bay.
            xs.extend((1..=17).map(|step| 13.8 + 4.0 * step as f32));
            zs.extend((1..=11).rev().map(|step| 14.0 + 4.0 * step as f32));
        }
        zs.extend((0..=120).map(|step| 14.0 - 62.0 * step as f32 / 120.0));
        if beach {
            zs.extend((1..=10).map(|step| -48.0 - 4.0 * step as f32));
        }
        let mut points = Vec::with_capacity(xs.len() * zs.len());
        for &z in &zs {
            for &x in &xs {
                let y = height(recipe, [x, z]);
                let uv_scale = if beach { 0.65 } else { 0.47 };
                points.push(([x, y, z], [x * uv_scale, z * uv_scale]));
            }
        }
        return indexed_mesh(
            &points,
            &grid_indices(xs.len() - 1, zs.len() - 1),
            style,
            material,
        );
    }
    let ridge = if beach { 0.0 } else { recipe.ridge_height };
    const STRIP: usize = 26;
    let strips = if !beach && ridge > 0.0 { 3 } else { 1 };
    let mut points = Vec::with_capacity(STRIP * strips);
    for band in 0..strips {
        for row in 0..=12 {
            let z = 12.0 - row as f32 * 4.5;
            let curve = 0.5 * (z * 0.18).sin() + 0.33 * (z * 0.35).cos();
            let spine = ridge * (0.82 + 0.17 * (z * 0.279).sin() + 0.13 * (z * 0.41).cos());
            let [east, west] = match (beach, band) {
                (true, _) => [[SHORE_X + 3.45 + curve, 0.004], [4.95 + curve, 0.004]],
                (false, 1) => [[-19.0, 0.006], [-29.0 + curve, spine]],
                (false, 2) => [[-29.0 + curve, spine], [-46.0, 0.09]],
                _ => [
                    [5.10 + curve, 0.006],
                    [if strips > 1 { -19.0 } else { -43.0 }, 0.006],
                ],
            };
            for [x, y] in [east, west] {
                points.push(([x, y, z], [x * 0.38, z * 0.38]));
            }
        }
    }
    mesh(&points, STRIP, style, material)
}

pub(super) fn road(recipe: &Recipe) -> Result<Vec<u8>, String> {
    if recipe.style == Style::Natural {
        let mut route = Vec::new();
        for pair in recipe.route.windows(2) {
            let from = Vec2::from_array(pair[0]);
            let to = Vec2::from_array(pair[1]);
            let count = (from.distance(to) / 0.42).ceil() as usize;
            route.extend((0..count).map(|at| from.lerp(to, at as f32 / count as f32).to_array()));
        }
        route.push(*recipe.route.last().expect("validated route"));
        let rim = ribbon(&route, recipe.road_width, 0.0, 1.32);
        let mut points = Vec::with_capacity(route.len() * 7);
        for pair in rim.chunks_exact(2) {
            let (a, uv) = pair[0];
            let (b, _) = pair[1];
            for column in 0..=6 {
                let t = column as f32 / 6.0;
                let at = [a[0] + (b[0] - a[0]) * t, a[2] + (b[2] - a[2]) * t];
                let crown = (t * std::f32::consts::PI).sin() * 0.019;
                points.push((
                    [at[0], height(recipe, at) + 0.025 + crown, at[1]],
                    [t * 1.7, uv[1]],
                ));
            }
        }
        let mut indices = grid_indices(6, route.len() - 1);
        for triangle in indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
        return indexed_mesh(&points, &indices, recipe.style, Material::Stone);
    }
    let points = ribbon(&recipe.route, recipe.road_width, 0.025, 2.65);
    mesh(&points, points.len(), recipe.style, Material::Stone)
}

pub(super) fn pier(recipe: &Recipe) -> Result<Vec<u8>, String> {
    let end = *recipe.route.last().expect("validated route");
    let path = [end, [SHORE_X + 5.0, end[1]]];
    if recipe.style == Style::Natural {
        return wharf(path[0], [SHORE_X + 6.2, end[1]]);
    }
    let points = ribbon(&path, 1.9, 0.086, 2.15);
    mesh(&points, points.len(), recipe.style, Material::Timber)
}

type Vertex = ([f32; 3], [f32; 2]);

fn wharf(from: [f32; 2], to: [f32; 2]) -> Result<Vec<u8>, String> {
    let (mut points, mut indices) = (Vec::new(), Vec::new());
    let mut block = |min: [f32; 3], max: [f32; 3], scale: f32, offset: f32| {
        for axis in 0..3 {
            let (s, t) = ((axis + 1) % 3, (axis + 2) % 3);
            for sign in [0, 1] {
                let start = points.len() as u16;
                for (a, b) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                    let mut xyz = min;
                    xyz[axis] = if sign == 0 { min[axis] } else { max[axis] };
                    xyz[s] = if a == 0 { min[s] } else { max[s] };
                    xyz[t] = if b == 0 { min[t] } else { max[t] };
                    points.push((xyz, [xyz[s] * scale + offset, xyz[t] * scale]));
                }
                let face = if sign == 0 {
                    [0, 2, 1, 0, 3, 2]
                } else {
                    [0, 1, 2, 0, 2, 3]
                };
                indices.extend(face.map(|index| index + start));
            }
        }
    };
    let [x, z] = from;
    let length = to[0] - x;
    let boards = (length / 0.19).ceil() as usize;
    for index in 0..boards {
        let left = x + index as f32 * length / boards as f32;
        let right = x + (index + 1) as f32 * length / boards as f32;
        let width = 0.949 + ((index * 117) % 13) as f32 * 0.0020;
        let height = 0.271 + ((index * 47) % 11) as f32 * 0.0009;
        block(
            [left + 0.001, 0.196, z - width],
            [right - 0.001, height, z + width],
            0.78,
            index as f32 * 0.17,
        );
    }
    for side in [-1.0, 1.0] {
        let edge = z + side * 0.76;
        block(
            [x, 0.059, edge - 0.06],
            [to[0], 0.197, edge + 0.06],
            0.46,
            side,
        );
        for index in 0..7 {
            let at = x + (index as f32 + 0.27) / 6.5 * length;
            block(
                [at - 0.071, -0.77, edge - 0.078],
                [at + 0.071, 0.205, edge + 0.078],
                0.82,
                index as f32 * 0.41,
            );
            if index >= 4 {
                block(
                    [at - 0.09, 0.24, edge - 0.087],
                    [at + 0.09, 0.71, edge + 0.087],
                    0.86,
                    index as f32 * 0.85,
                );
            }
        }
    }
    indexed_mesh(&points, &indices, Style::Natural, Material::Timber)
}

pub(super) fn height(recipe: &Recipe, [x, z]: [f32; 2]) -> f32 {
    if recipe.style != Style::Natural {
        return 0.0;
    }
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // The coast is a continuous profile, including a submerged strip of real sand.
    if x >= 4.5 {
        let coast = (SHORE_X - x) * 0.12;
        let blend = smooth((x - 4.5) / 1.10);
        let bank = inland_height(recipe, [4.5, z]);
        let scallop = (z * 0.81 + x * 0.79).sin() * 0.032 * smooth((x - 7.4) * 0.81);
        return (bank * (1.0 - blend) + coast * blend + scallop).max(-0.77);
    }
    inland_height(recipe, [x, z])
}

fn inland_height(recipe: &Recipe, [x, z]: [f32; 2]) -> f32 {
    let far = ((-x - 12.0) / 17.0).clamp(0.0, 1.0);
    let ridge = (1.0 - ((x + 29.0) / 18.0).abs()).max(0.0) * far;
    let hills = ridge
        * recipe.ridge_height
        * (0.78 + 0.17 * (z * 0.19 + 1.5).cos() + 0.09 * (z * 0.41).sin());
    let edge = ((x.abs() - 0.60) / 1.6).clamp(0.0, 1.0);
    let along = ((-z - 0.9) / 2.1).clamp(0.0, 1.0);
    let berm = 0.105
        + 0.075 * (z * 2.02 + x * 1.67 + 2.6).sin()
        + 0.052 * (z * 1.38 - x * 2.59).cos()
        + 0.024 * (z * 3.03 + x * 2.72).sin();
    let boardwalk = (1.0 - ((z + 23.2) / 1.6).abs()).max(0.0);
    let ease_inland = (1.0 + x * 0.11).clamp(0.21, 1.0);
    hills + 0.006 + along * edge * ease_inland * (1.0 - boardwalk * 0.77) * berm.max(0.0)
}

fn grid_indices(columns: usize, rows: usize) -> Vec<u16> {
    let stride = columns + 1;
    let mut indices = Vec::with_capacity(columns * rows * 6);
    for row in 0..rows {
        for col in 0..columns {
            let near = (row * stride + col) as u16;
            let next = near + stride as u16;
            indices.extend([near, near + 1, next, next, near + 1, next + 1]);
        }
    }
    indices
}

fn ribbon(route: &[[f32; 2]], width: f32, y: f32, repeat: f32) -> Vec<Vertex> {
    let mut distance = 0.0;
    let mut vertices = Vec::with_capacity(route.len() * 2);
    for (index, &at) in route.iter().enumerate() {
        let point = Vec2::from_array(at);
        let before = index.checked_sub(1).map(|i| Vec2::from_array(route[i]));
        let after = route.get(index + 1).copied().map(Vec2::from_array);
        if let Some(prior) = before {
            distance += point.distance(prior);
        }
        let incoming = before.map(|prior| (point - prior).normalize());
        let outgoing = after.map(|next| (next - point).normalize());
        let forward = outgoing.or(incoming).expect("route has a segment");
        let side = Vec2::new(-forward.y, forward.x);
        let lead = incoming.unwrap_or(forward);
        let bisector = (Vec2::new(-lead.y, lead.x) + side)
            .try_normalize()
            .unwrap_or(side);
        let offset = bisector * (width * 0.5 / bisector.dot(side).abs().max(0.65));
        let a = point + offset;
        let b = point - offset;
        let v = distance / repeat;
        vertices.push(([a.x, y, a.y], [0.0, v]));
        vertices.push(([b.x, y, b.y], [1.0, v]));
    }
    vertices
}

fn mesh(
    points: &[Vertex],
    strip: usize,
    style: Style,
    material: Material,
) -> Result<Vec<u8>, String> {
    let mut indices = Vec::new();
    for start in (0..points.len()).step_by(strip) {
        for i in (start as u16..(start + strip) as u16 - 2).step_by(2) {
            indices.extend([i, i + 2, i + 1, i + 1, i + 2, i + 3]);
        }
    }
    indexed_mesh(points, &indices, style, material)
}

fn indexed_mesh(
    points: &[Vertex],
    indices: &[u16],
    style: Style,
    material: Material,
) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for &(point, _) in points {
        for (axis, value) in point.into_iter().enumerate() {
            data.extend(value.to_le_bytes());
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    let uv_start = data.len();
    for &(_, uv) in points {
        data.extend(uv.into_iter().flat_map(f32::to_le_bytes));
    }
    let indices_start = data.len();
    data.extend(indices.iter().flat_map(|index| index.to_le_bytes()));
    let indices_end = data.len();
    data.resize(data.len().next_multiple_of(4), 0);
    let mut buffer_views = vec![
        json!({"buffer":0,"byteOffset":0,"byteLength":uv_start}),
        json!({"buffer":0,"byteOffset":uv_start,"byteLength":indices_start-uv_start}),
        json!({"buffer":0,"byteOffset":indices_start,"byteLength":indices_end-indices_start}),
    ];
    let mut images = Vec::new();
    let mut textures = Vec::new();
    let mut add_texture = |bytes: &[u8], mime| {
        let view = buffer_views.len();
        buffer_views.push(json!({"buffer":0,"byteOffset":data.len(),"byteLength":bytes.len()}));
        textures.push(json!({"sampler":0,"source":images.len()}));
        images.push(json!({"bufferView":view,"mimeType":mime}));
        data.extend_from_slice(bytes);
        data.resize(data.len().next_multiple_of(4), 0);
    };
    let mut finish = json!({"doubleSided":true,"pbrMetallicRoughness":{
        "baseColorTexture":{"index":0},"metallicFactor":0.0,"roughnessFactor":0.95
    }});
    if style == Style::Natural {
        let (color, normal, rough, rough_mime) = material.maps();
        add_texture(color, "image/jpeg");
        add_texture(normal, "image/jpeg");
        add_texture(rough, rough_mime);
        finish["normalTexture"] = json!({"index":1,"scale":0.73});
        finish["pbrMetallicRoughness"]["metallicRoughnessTexture"] = json!({"index":2});
    } else {
        let pixels = RgbImage::from_fn(128, 128, |x, y| Rgb(texel(x, y, style, material)));
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(pixels.as_raw(), 128, 128, ExtendedColorType::Rgb8)
            .map_err(|error| format!("Не удалось собрать текстуру деревни ({error})."))?;
        add_texture(&png, "image/png");
    }
    let doc = json!({
        "asset":{"version":"2.0","generator":"Robot Studio"},
        "scene":0, "scenes":[{"nodes":[0]}],
        "nodes":[{"mesh":0}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":1},"indices":2,"material":0}]}],
        "materials":[finish],
        "textures":textures,
        "samplers":[{"magFilter":9729,"minFilter":9987,"wrapS":10497,"wrapT":10497}],
        "images":images,
        "buffers":[{"byteLength":data.len()}],
        "bufferViews":buffer_views,
        "accessors":[
            {"bufferView":0,"componentType":5126,"count":points.len(),"type":"VEC3","min":low,"max":high},
            {"bufferView":1,"componentType":5126,"count":points.len(),"type":"VEC2"},
            {"bufferView":2,"componentType":5123,"count":(indices_end-indices_start)/2,"type":"SCALAR"}
        ]
    });
    let mut header = serde_json::to_vec(&doc).map_err(|error| error.to_string())?;
    header.resize(header.len().next_multiple_of(4), b' ');
    let mut glb = Vec::with_capacity(28 + header.len() + data.len());
    glb.extend(b"glTF");
    glb.extend(2_u32.to_le_bytes());
    glb.extend(((28 + header.len() + data.len()) as u32).to_le_bytes());
    glb.extend((header.len() as u32).to_le_bytes());
    glb.extend(b"JSON");
    glb.extend(header);
    glb.extend((data.len() as u32).to_le_bytes());
    glb.extend(b"BIN\0");
    glb.extend(data);
    Ok(glb)
}

impl Material {
    fn maps(self) -> (&'static [u8], &'static [u8], &'static [u8], &'static str) {
        macro_rules! coastal {
            ($name:literal) => {
                (
                    include_bytes!(concat!("../../demo_assets/coastal/", $name, "-color.jpg")),
                    include_bytes!(concat!("../../demo_assets/coastal/", $name, "-normal.jpg")),
                    include_bytes!(concat!("../../demo_assets/coastal/", $name, "-rough.jpg")),
                    "image/jpeg",
                )
            };
        }
        match self {
            Self::Meadow => coastal!("meadow"),
            Self::Sand => coastal!("sand"),
            Self::Timber => coastal!("timber"),
            Self::Stone => (
                include_bytes!("../../demo_assets/surface_palette/paving-color.jpg"),
                include_bytes!("../../demo_assets/surface_palette/paving-normal.jpg"),
                include_bytes!("../../demo_assets/surface_palette/paving-rough.png"),
                "image/png",
            ),
        }
    }
}

fn texel(x: u32, y: u32, style: Style, material: Material) -> [u8; 3] {
    let natural = style == Style::Natural;
    let speck = noise(x, y) as i32 % 21 - 10;
    let grain = if natural { speck } else { speck / 4 };
    let (color, shade) = match material {
        Material::Meadow => {
            let base = if natural {
                [78_u8, 101, 57]
            } else {
                [103, 133, 68]
            };
            let patch = (noise_grid(x, y, 16) * 22.0) as i32 - 11;
            (base, patch + grain)
        }
        Material::Sand => {
            let base = if natural {
                [148, 127, 96]
            } else {
                [202, 165, 111]
            };
            let ripple = (noise_grid(x, y, 8) * 15.0) as i32 - 7;
            (base, ripple + grain)
        }
        Material::Stone => {
            let base = if natural {
                [118, 108, 96]
            } else {
                [166, 142, 108]
            };
            let row = y / 16;
            let stagger = (x + row % 2 * 8) % 128;
            let cell_x = stagger % 16;
            let cell_y = y % 16;
            let edge = if x < 6 || x > 121 { 13 } else { 0 };
            let seams = if cell_x < 1 || cell_y < 1 {
                -33
            } else if cell_x < 3 || cell_y < 3 {
                -9
            } else if cell_x > 13 || cell_y > 13 {
                14
            } else {
                0
            };
            let patch = noise(stagger / 16, row) as i32 % 22 - 11;
            (base, seams + patch + edge + grain)
        }
        Material::Timber => {
            let base = if natural {
                [108, 77, 56]
            } else {
                [165, 113, 72]
            };
            let board = if y % 16 < 2 {
                -37
            } else if y % 16 > 13 {
                8
            } else {
                0
            };
            let streak = (noise_grid(x, y * 4, 16) * 12.0) as i32 - 6;
            (base, board + streak + grain)
        }
    };
    color.map(|channel| (i32::from(channel) + shade).clamp(0, 255) as u8)
}

fn noise_grid(x: u32, y: u32, spacing: u32) -> f32 {
    let nodes = 128 / spacing;
    let [x0, y0] = [x / spacing % nodes, y / spacing % nodes];
    let [x1, y1] = [(x0 + 1) % nodes, (y0 + 1) % nodes];
    let [tx, ty] = [
        (x % spacing) as f32 / spacing as f32,
        (y % spacing) as f32 / spacing as f32,
    ];
    let value = |x, y| noise(x, y) as f32 / 255.0;
    let top = value(x0, y0) + (value(x1, y0) - value(x0, y0)) * tx;
    let bottom = value(x0, y1) + (value(x1, y1) - value(x0, y1)) * tx;
    top + (bottom - top) * ty
}

fn noise(x: u32, y: u32) -> u8 {
    let value = x.wrapping_mul(0x0045_d9f3) ^ y.wrapping_mul(0x9e37_79b9) ^ 0x7f4a_7c15;
    (value ^ (value >> 13))
        .wrapping_mul(0xd168_aaad)
        .to_le_bytes()[3]
}
