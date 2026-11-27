use std::{collections::HashMap, hash::Hash};

use eframe::egui;
use glam::{Mat3, Vec2, Vec3};

use crate::{
    model::{BoneRotation, Image, Material, Model, SecondaryPose, Vertex},
    sprite::{FLOOR, SIDE},
};

struct Card {
    center: Vec3,
    view: Mat3,
    scale: f32,
    side: usize,
}

pub const LIVE_SIDE: usize = 128;
pub const LIVE_DIRECTIONS: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameKey {
    pub direction: u8,
    pub frame: u8,
    pub plan: bool,
    pub swing: u8,
}

pub struct LivePose<'a> {
    pub clip: Option<usize>,
    pub elapsed: f32,
    pub yaw: f32,
    pub plan: bool,
    pub bones: &'a [BoneRotation],
    pub secondary: SecondaryPose,
}

pub fn direction(yaw: f32) -> u8 {
    (-yaw / (360.0 / f32::from(LIVE_DIRECTIONS)))
        .round()
        .rem_euclid(f32::from(LIVE_DIRECTIONS)) as u8
}

pub fn quantize_swing(pose: SecondaryPose) -> (u8, SecondaryPose) {
    let swing = Vec3::from_array(pose.garment_sway);
    let lateral = if swing.x.abs() > swing.z.abs() {
        (1, Vec3::X * swing.x.signum(), swing.x)
    } else {
        (3, Vec3::Z * swing.z.signum(), swing.z)
    };
    let (key, direction) = if lateral.2.abs() > 0.24 {
        (lateral.0 + u8::from(lateral.2 > 0.0), lateral.1 * 0.63)
    } else if swing.y.abs() > 0.34 {
        (
            5 + u8::from(swing.y > 0.0),
            Vec3::Y * swing.y.signum() * 0.64,
        )
    } else {
        return (0, SecondaryPose::default());
    };
    (
        key,
        SecondaryPose {
            hair_sway: [direction.x, -direction.z],
            hair_lift: direction.y,
            garment_sway: direction.to_array(),
            ..SecondaryPose::default()
        },
    )
}

pub struct FrameCache<Key, const CAPACITY: usize> {
    frames: HashMap<Key, (egui::TextureHandle, u64)>,
    clock: u64,
}

impl<Key, const CAPACITY: usize> Default for FrameCache<Key, CAPACITY> {
    fn default() -> Self {
        Self {
            frames: HashMap::new(),
            clock: 0,
        }
    }
}

impl<Key: Copy + Eq + Hash, const CAPACITY: usize> FrameCache<Key, CAPACITY> {
    pub fn get(&mut self, key: Key) -> Option<egui::TextureId> {
        let (texture, latest_use) = self.frames.get_mut(&key)?;
        self.clock += 1;
        *latest_use = self.clock;
        Some(texture.id())
    }

    pub fn insert(&mut self, key: Key, texture: egui::TextureHandle) -> egui::TextureId {
        if self.frames.len() >= CAPACITY
            && let Some((&oldest, _)) = self.frames.iter().min_by_key(|(_, entry)| entry.1)
        {
            self.frames.remove(&oldest);
        }
        self.clock += 1;
        let id = texture.id();
        self.frames.insert(key, (texture, self.clock));
        id
    }

    pub fn clear(&mut self) {
        self.frames.clear();
        self.clock = 0;
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&Key) -> bool) {
        self.frames.retain(|key, _| keep(key));
    }
}

#[derive(Clone, Copy)]
struct Projected {
    pixel: Vec2,
    depth: f32,
    normal: Vec3,
    uv: Vec2,
}

pub fn rasterize(model: &Model, clip: Option<usize>, elapsed: f32, facing: u8) -> Vec<u8> {
    rasterize_with_overrides(model, clip, elapsed, facing, &[])
}

pub fn rasterize_with_overrides(
    model: &Model,
    clip: Option<usize>,
    elapsed: f32,
    facing: u8,
    bone_offsets: &[BoneRotation],
) -> Vec<u8> {
    let yaw = -f32::from(facing % 4) * std::f32::consts::FRAC_PI_2;
    rasterize_pose(
        model,
        LivePose {
            clip,
            elapsed,
            yaw,
            plan: false,
            bones: bone_offsets,
            secondary: SecondaryPose::default(),
        },
        SIDE,
    )
}

pub fn rasterize_live(model: &Model, pose: LivePose<'_>) -> Vec<u8> {
    rasterize_pose(
        model,
        LivePose {
            yaw: -f32::from(direction(pose.yaw)) * std::f32::consts::TAU
                / f32::from(LIVE_DIRECTIONS),
            ..pose
        },
        LIVE_SIDE,
    )
}

fn rasterize_pose(model: &Model, pose: LivePose<'_>, side: usize) -> Vec<u8> {
    let mut pixels = vec![0; side * side * 4];
    let mut depth = vec![f32::NEG_INFINITY; side * side];
    let low = Vec3::from_array(model.bounds_min);
    let high = Vec3::from_array(model.bounds_max);
    let size = (high - low).max(Vec3::splat(0.001));
    let middle = (low + high) * 0.5;
    let card = Card {
        center: Vec3::new(middle.x, low.y, middle.z),
        view: Mat3::from_rotation_x(if pose.plan { 0.32 } else { 0.0 })
            * Mat3::from_rotation_y(pose.yaw),
        scale: (73.0 / size.y).min(82.0 / size.x.max(size.z)) * (side as f32 / SIDE as f32),
        side,
    };
    let posed = if pose.clip.is_some() || !pose.bones.is_empty() || pose.secondary.active() {
        model.pose_with_secondary(pose.clip, pose.elapsed, pose.bones, pose.secondary)
    } else {
        None
    };
    for (index, mesh) in model.primitives.iter().enumerate() {
        let vertices = posed
            .as_ref()
            .and_then(|pose| pose.get(index))
            .map_or(mesh.vertices.as_slice(), Vec::as_slice);
        let projected: Vec<_> = vertices
            .iter()
            .map(|vertex| project(&card, vertex))
            .collect();
        let maps = [
            mesh.material.base_color_texture,
            mesh.material.emissive_texture,
        ]
        .map(|index| index.and_then(|image| model.images.get(image)));
        for triangle in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]].map(|i| projected[i as usize]);
            draw_triangle(
                [a, b, c],
                &mesh.material,
                maps,
                side,
                &mut pixels,
                &mut depth,
            );
        }
    }
    pixels
}

fn project(card: &Card, vertex: &Vertex) -> Projected {
    let position = card.view * (Vec3::from_array(vertex.position) - card.center);
    Projected {
        pixel: Vec2::new(
            card.side as f32 * 0.5 + position.x * card.scale,
            FLOOR * (card.side as f32 / SIDE as f32) - position.y * card.scale,
        ),
        depth: position.z,
        normal: card.view * Vec3::from_array(vertex.normal),
        uv: Vec2::from_array(vertex.uv),
    }
}

fn draw_triangle(
    [a, b, c]: [Projected; 3],
    material: &Material,
    [color_map, emissive_map]: [Option<&Image>; 2],
    side: usize,
    pixels: &mut [u8],
    depth: &mut [f32],
) {
    let area = edge(a.pixel, b.pixel, c.pixel);
    if area.abs() < 0.0001 || (area > 0.0 && !material.double_sided) {
        return;
    }
    let min = a.pixel.min(b.pixel).min(c.pixel).floor();
    let max = a.pixel.max(b.pixel).max(c.pixel).ceil();
    let range = |low: f32, high: f32| {
        low.clamp(0.0, side as f32) as usize..high.clamp(0.0, side as f32) as usize
    };
    let columns = range(min.x, max.x);
    let reciprocal = area.recip();
    let facing = if area < 0.0 { 1.0 } else { -1.0 };
    let light = Vec3::new(-0.46, 0.80, 0.47).normalize();
    let base_color = Vec3::from_array([
        material.base_color[0],
        material.base_color[1],
        material.base_color[2],
    ]);
    let emissive = Vec3::from_array(material.emissive);
    for y in range(min.y, max.y) {
        for x in columns.clone() {
            let sample = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let va = edge(b.pixel, c.pixel, sample) * reciprocal;
            let vb = edge(c.pixel, a.pixel, sample) * reciprocal;
            let vc = 1.0 - va - vb;
            if va.min(vb).min(vc) < -0.00001 {
                continue;
            }
            let z = a.depth * va + b.depth * vb + c.depth * vc;
            let index = y * side + x;
            if z <= depth[index] {
                continue;
            }
            let uv = va * a.uv + vb * b.uv + vc * c.uv;
            let texel = color_map.map_or([255; 4], |image| sample_texel(image, uv));
            if material
                .alpha_cutoff
                .is_some_and(|cutoff| f32::from(texel[3]) / 255.0 * material.base_color[3] < cutoff)
            {
                continue;
            }
            let illumination_rgb = decode_srgb(texel);
            let emission = if emissive == Vec3::ZERO {
                Vec3::ZERO
            } else {
                emissive
                    * emissive_map.map_or(Vec3::ONE, |image| decode_srgb(sample_texel(image, uv)))
            };
            let normal =
                (va * a.normal + vb * b.normal + vc * c.normal).normalize_or_zero() * facing;
            let diffuse = normal.dot(light).max(0.0);
            let illumination = 0.53 + 0.59 * diffuse;
            let color = (base_color * illumination_rgb * illumination + emission)
                .to_array()
                .map(encode_srgb);
            pixels[index * 4..index * 4 + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
            depth[index] = z;
        }
    }
}

fn edge(from: Vec2, to: Vec2, point: Vec2) -> f32 {
    (to - from).perp_dot(point - from)
}

fn sample_texel(image: &Image, uv: Vec2) -> [u8; 4] {
    let texel = |coordinate: f32, dimension: u32| {
        ((coordinate.rem_euclid(1.0) * dimension as f32).floor() as u32).min(dimension - 1)
    };
    let x = texel(uv.x, image.width);
    let y = texel(uv.y, image.height);
    let offset = ((y * image.width + x) * 4) as usize;
    image.pixels[offset..offset + 4].try_into().unwrap()
}

fn decode_srgb([r, g, b, _]: [u8; 4]) -> Vec3 {
    Vec3::from_array([r, g, b].map(|value| {
        let srgb = f32::from(value) / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    }))
}

fn encode_srgb(value: f32) -> u8 {
    let linear = value.clamp(0.0, 1.0);
    let srgb = if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagonal_live_sprite_uses_its_own_hd_projection() {
        let model = crate::model::decode_glb(include_bytes!("../demo_assets/emma.glb")).unwrap();
        let clip = model
            .animations
            .iter()
            .position(|animation| animation.name == "Walk");
        let render = |yaw| {
            rasterize_live(
                &model,
                LivePose {
                    clip,
                    elapsed: 0.22,
                    yaw,
                    plan: true,
                    bones: &[],
                    secondary: SecondaryPose::default(),
                },
            )
        };
        let front = render(0.0);
        let diagonal = render(45.0);
        assert_eq!(diagonal.len(), LIVE_SIDE * LIVE_SIDE * 4);
        assert!(diagonal.chunks_exact(4).any(|pixel| pixel[3] != 0));
        assert_ne!(front, diagonal);
    }
}
