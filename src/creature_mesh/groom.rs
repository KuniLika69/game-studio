use std::sync::OnceLock;

use glam::{Quat, Vec3};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::json;

use super::{Draft, Geometry, Glb, PANEL, PI, PRIMARY, SECONDARY, TAU, surface, v};

const COAT_SIZE: u32 = 384;
pub(super) const ROOT_COLOR: usize = 6;
pub(super) const TIP_COLOR: usize = 7;
const LIGHT_FIBER: usize = 8;
pub(super) const NOSE: usize = 9;
pub(super) const EYE: usize = 10;
pub(super) const IRIS: usize = 11;
pub(super) const INNER_EAR: usize = 12;
pub(super) const PAW_PAD: usize = 13;

pub(super) fn materials(glb: &mut Glb, draft: &Draft) {
    static IMAGES: OnceLock<[Vec<u8>; 2]> = OnceLock::new();
    for bytes in IMAGES.get_or_init(coat_textures) {
        let view = glb.bytes(bytes.iter().copied());
        glb.images
            .push(json!({"bufferView": view, "mimeType": "image/png"}));
    }
    for (index, roughness) in [(PRIMARY, 0.81), (SECONDARY, 0.87), (PANEL, 0.83)] {
        let material = &mut glb.materials[index];
        material["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index": 0});
        material["pbrMetallicRoughness"]["roughnessFactor"] = json!(roughness);
        material["normalTexture"] = json!({"index": 1, "scale": 0.75});
    }
    let tone = |light: [u8; 3], dark: [u8; 3], amount: u16| -> [u8; 3] {
        std::array::from_fn(|i| {
            ((u16::from(light[i]) * amount + u16::from(dark[i]) * (100 - amount)) / 100) as u8
        })
    };
    for (name, color) in [
        (
            "Creator | Guard Fur Roots",
            tone(draft.primary, [63, 37, 32], 92),
        ),
        (
            "Creator | Guard Fur Tips",
            tone(draft.primary, draft.secondary, 86),
        ),
        (
            "Creator | Guard Fur Pale",
            tone(draft.secondary, draft.primary, 94),
        ),
    ] {
        let mut material = surface(name, color, 0.0, 0.68, 0.0);
        material["doubleSided"] = json!(true);
        material["alphaMode"] = json!("MASK");
        material["alphaCutoff"] = json!(0.32);
        glb.materials.push(material);
    }
    glb.materials.extend([
        surface("Creator | Nose and Pads", [48, 32, 32], 0.02, 0.36, 0.0),
        surface("Creator | Cornea", [32, 21, 17], 0.0, 0.13, 0.0),
        surface(
            "Creator | Amber Iris",
            tone([192, 116, 42], draft.accent, 86),
            0.02,
            0.23,
            0.0,
        ),
        surface(
            "Creator | Inner Ear",
            tone(draft.primary, [79, 35, 37], 58),
            0.0,
            0.91,
            0.0,
        ),
        surface(
            "Creator | Paw pads",
            tone([63, 36, 32], draft.primary, 87),
            0.0,
            0.89,
            0.0,
        ),
    ]);
}

pub(super) fn noise(seed: u32) -> f32 {
    let mut value = seed;
    value = (value ^ (value >> 16)).wrapping_mul(0x7feb_352d);
    value = (value ^ (value >> 15)).wrapping_mul(0x846c_a68b);
    ((value ^ (value >> 16)) >> 8) as f32 / 16_777_216.0
}

fn coat_textures() -> [Vec<u8>; 2] {
    let sample = |x: f32, y: f32| {
        let warped =
            x * 192.0 + (y * TAU * 3.0 + x * TAU * 2.0).sin() * 1.7 + (x * TAU * 27.0).sin() * 0.36;
        let cell = warped.floor();
        let mix = warped - cell;
        let signal = |index: f32| noise((index as i32).rem_euclid(192) as u32 + 701);
        let ridge = signal(cell) + (signal(cell + 1.0) - signal(cell)) * mix;
        let thread = (warped * TAU).cos() * 0.15;
        let broad = (x * TAU * 7.0 + (y * TAU * 4.0 + x * TAU * 2.0).sin()).cos() * 0.27;
        ridge * 0.58 + thread + broad
    };
    let mut base = Vec::with_capacity((COAT_SIZE * COAT_SIZE * 3) as usize);
    let mut normal = Vec::with_capacity(base.capacity());
    let pixel = 1.0 / COAT_SIZE as f32;
    for y in 0..COAT_SIZE {
        for x in 0..COAT_SIZE {
            let (u, v) = (x as f32 * pixel, y as f32 * pixel);
            let layer = sample(u, v);
            let d_u = sample(u + pixel, v) - sample(u - pixel, v);
            let d_v = sample(u, v + pixel) - sample(u, v - pixel);
            let shade = (219.0 + layer * 41.0).clamp(170.0, 254.0) as u8;
            base.extend([shade, shade, shade]);
            let tangent_normal = Vec3::new(-d_u * 0.76, -d_v * 0.56, 1.0).normalize();
            normal.extend(
                tangent_normal
                    .to_array()
                    .map(|n| ((n * 0.5 + 0.5) * 255.0) as u8),
            );
        }
    }
    [base, normal].map(|pixels| {
        let mut buffer = Vec::new();
        PngEncoder::new(&mut buffer)
            .write_image(&pixels, COAT_SIZE, COAT_SIZE, ExtendedColorType::Rgb8)
            .expect("RGB coat texture size is fixed");
        buffer
    })
}

#[derive(Clone, Copy)]
pub(super) enum Shape {
    Ruff,
    Face,
    Tail,
    Limb,
}

#[derive(Clone, Copy)]
pub(super) struct Patch {
    pub center: Vec3,
    pub radii: Vec3,
    pub rotation: Quat,
    pub shape: Shape,
}

impl Patch {
    pub(super) fn new(center: Vec3, radii: Vec3, rotation: Quat, shape: Shape) -> Self {
        Self {
            center,
            radii,
            rotation,
            shape,
        }
    }

    fn point(self, u: Vec3) -> Vec3 {
        let t = u.z;
        let silhouette = match self.shape {
            Shape::Face => v(1.0 - t * 0.16, 1.0 - t * 0.06, 1.0),
            _ => Vec3::ONE,
        };
        self.center + self.rotation * (u * self.radii * silhouette)
    }

    fn sample(self, u: Vec3) -> (Vec3, Vec3) {
        let x = if u.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y }
            .cross(u)
            .normalize();
        let y = u.cross(x);
        let point = self.point(u);
        let dx = self.point((u + 0.007 * x).normalize()) - point;
        let dy = self.point((u + 0.007 * y).normalize()) - point;
        (point, dx.cross(dy).normalize_or_zero())
    }

    pub(super) fn coat(self, glb: &mut Glb, node: usize, material: usize) {
        let mut mesh = Geometry::default();
        const W: u16 = 49;
        const H: u16 = 31;
        for row in 0..H {
            let lat = (f32::from(row) / f32::from(H - 1) - 0.5) * (PI - 0.0001);
            for col in 0..W {
                let frac = f32::from(col) / f32::from(W - 1);
                let yaw = frac * TAU;
                let u = v(yaw.sin() * lat.cos(), lat.sin(), yaw.cos() * lat.cos());
                let (point, normal) = self.sample(u);
                mesh.vertex(point, normal);
                mesh.uv.push([frac, f32::from(row) / f32::from(H - 1)]);
            }
        }
        mesh.grid(0, W, H, false);
        glb.add(node, material, mesh);
    }

    pub(super) fn fibers(
        self,
        glb: &mut Glb,
        node: usize,
        count: usize,
        length: f32,
        seed: u32,
        pale: bool,
    ) {
        let mut meshes: [Geometry; 3] = std::array::from_fn(|_| Geometry::default());
        let direction = self.rotation
            * match self.shape {
                Shape::Ruff => v(0.0, -0.49, -0.88),
                Shape::Face => v(0.0, -0.30, -0.96),
                Shape::Tail => Vec3::Y,
                Shape::Limb => -Vec3::Y,
            };
        for index in 0..count as u32 {
            let rand = |slot| noise(seed + index * 11 + slot);
            let y = 1.0 - 2.0 * (index as f32 + rand(1)) / count as f32;
            let phi = index as f32 * 2.399_963 + rand(2) * 0.15;
            let radius = (1.0 - y * y).sqrt();
            let u = v(radius * phi.sin(), y, radius * phi.cos());
            match self.shape {
                Shape::Face if u.z > 0.39 && u.y < 0.51 => continue,
                Shape::Limb if u.y > 0.91 || u.y < -0.85 => continue,
                _ => {}
            }
            let (point, normal) = self.sample(u);
            let flow = (direction - direction.dot(normal) * normal).normalize_or_zero();
            let side = normal.cross(flow);
            if side.length_squared() < 0.1 {
                continue;
            }
            let length = length
                * (0.43 + 0.76 * rand(3))
                * if matches!(self.shape, Shape::Limb) {
                    0.72 + 0.25 * u.y
                } else {
                    1.0
                };
            let width = length * (0.054 + 0.04 * rand(4));
            let twist = (rand(5) - 0.5) * 0.39;
            let rest = point - normal * (width * 0.67);
            let mid = rest + length * (flow * 0.54 + normal * 0.27 + side * twist * 0.29);
            let tip =
                rest + length * (flow * 0.91 + normal * (0.27 + rand(6) * 0.17) + side * twist);
            let color = if pale {
                2
            } else if rand(7) > 0.52 {
                1
            } else {
                0
            };
            let mesh = &mut meshes[color];
            let start = mesh.position.len() as u16;
            for (at, bend, across) in [
                (rest - side * width, 0.0, 0.0),
                (rest + side * width, 0.0, 1.0),
                (mid - side * width * 0.64, 0.48, 0.16),
                (mid + side * width * 0.64, 0.48, 0.84),
                (tip, 1.0, 0.5),
            ] {
                mesh.vertex(at, (normal - flow * (0.2 * bend)).normalize());
                mesh.uv.push([across, bend]);
            }
            mesh.indices.extend([
                start,
                start + 2,
                start + 1,
                start + 1,
                start + 2,
                start + 3,
                start + 2,
                start + 4,
                start + 3,
            ]);
        }
        for (mesh, material) in meshes.into_iter().zip([ROOT_COLOR, TIP_COLOR, LIGHT_FIBER]) {
            if !mesh.position.is_empty() {
                glb.add(node, material, mesh);
            }
        }
    }
}

pub(super) fn ear(glb: &mut Glb, node: usize, side: f32, size: f32, coat: usize, inner: usize) {
    const AROUND: u16 = 19;
    let mut outer = Geometry::default();
    let rings = [
        [-0.036, -0.007, 0.045, 0.032],
        [0.022, 0.013, 0.081, 0.036],
        [0.092, 0.042, 0.066, 0.025],
        [0.160, 0.077, 0.047, 0.017],
        [0.224, 0.108, 0.029, 0.012],
        [0.270, 0.130, 0.012, 0.007],
        [0.282, 0.133, 0.001, 0.001],
    ];
    for (row, [y, x, width, depth]) in rings.into_iter().enumerate() {
        for col in 0..AROUND {
            let u = f32::from(col) / f32::from(AROUND - 1);
            let (s, c) = (u * TAU).sin_cos();
            outer.vertex(
                v(x * side + width * s, y * 0.73, depth * c - y * 0.175) * size,
                v(s, 0.17 + y * 0.7, c).normalize(),
            );
            outer.uv.push([u, row as f32 / (rings.len() - 1) as f32]);
        }
    }
    outer.grid(0, AROUND, rings.len() as u16, false);
    glb.add(node, coat, outer);
    let lining = Patch::new(
        v(0.049 * side, 0.061, 0.016) * size,
        v(0.037, 0.094, 0.013) * size,
        Quat::from_euler(glam::EulerRot::XYZ, -0.17, 0.0, -0.44 * side),
        Shape::Limb,
    );
    lining.coat(glb, node, inner);
}

pub(super) fn whiskers(glb: &mut Glb, node: usize, size: f32) {
    let mut mesh = Geometry::default();
    for side in [-1.0, 1.0] {
        for strand in 0..5 {
            let phase = strand as f32 - 2.0;
            let base = mesh.position.len() as u16;
            for k in 0..5 {
                let t = k as f32 * 0.25;
                let f = t * t;
                let point = v(
                    side * (0.077 + 0.017 * phase * 0.10 + t * (0.108 + phase * 0.013)),
                    -0.038 + phase * 0.007 - 0.013 * f + phase * 0.009 * f,
                    0.213 + 0.007 * phase - 0.069 * f,
                );
                for (across, width) in [(0.0, -1.0), (1.0, 1.0)] {
                    let offset = Vec3::Y * (width * (0.00145 * (1.0 - t) + 0.00019));
                    mesh.vertex(
                        (point + offset) * size,
                        v(side * 0.75, 0.42, 0.51).normalize(),
                    );
                    mesh.uv.push([across, t]);
                }
            }
            mesh.grid(base, 2, 5, side < 0.0);
        }
    }
    glb.add(node, LIGHT_FIBER, mesh);
}
