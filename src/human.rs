use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
};

use glam::{Mat3, Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use crate::ImportError;

use super::{
    HairMotion, Image, Influence, JointTransform, Model, Primitive, SecondaryDelta, SecondaryDeltaKind, Vertex,
    joint_matrices, tangent_basis,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModelRegion {
    Other,
    MayaTop,
    MayaTrousers,
    MayaHair,
    MayaPonytail,
    MayaFace,
    MayaLashes,
    MayaEyes,
    MayaTeeth,
    StudioTop,
    StudioTrousers,
    StudioScalp,
    StudioHair,
    StudioHairTip,
    StudioHairWisp,
    StudioFace,
    StudioSkin,
    StudioLashes,
    StudioEyes,
    StudioTeeth,
}

impl ModelRegion {
    pub(super) fn from_material(name: Option<&str>) -> Self {
        if let Some(name) = name.and_then(|name| name.strip_prefix("Studio | ")) {
            return match name.trim().to_ascii_lowercase().as_str() {
                "top" | "shirt" | "jacket" | "dress" => Self::StudioTop,
                "trousers" | "pants" => Self::StudioTrousers,
                "scalp" => Self::StudioScalp,
                "hair" => Self::StudioHair,
                "hair tip" | "ponytail" => Self::StudioHairTip,
                "hair wisp" => Self::StudioHairWisp,
                "face" | "head" => Self::StudioFace,
                "skin" => Self::StudioSkin,
                "lashes" | "eyelashes" => Self::StudioLashes,
                "eyes" | "eye" | "iris" | "cornea" => Self::StudioEyes,
                "teeth" => Self::StudioTeeth,
                _ => Self::Other,
            };
        }
        let Some(name) = name.and_then(|name| name.strip_prefix("Maya | ")) else {
            return Self::Other;
        };
        let name = name.to_ascii_lowercase();
        if name.contains("trouser") {
            Self::MayaTrousers
        } else if name.contains("full-length top") {
            Self::MayaTop
        } else if name.contains("ponytail") {
            Self::MayaPonytail
        } else if name.contains("hair roots") || name.contains("hairline") {
            Self::MayaHair
        } else if name.contains("eyelash") {
            Self::MayaLashes
        } else if name == "anatomical body.body" {
            Self::MayaFace
        } else if name.contains("cornea")
            || name.contains("iris")
            || name == "anatomical body.low-poly"
            || name.contains("eye") && !name.contains("lash") && !name.contains("brow")
        {
            Self::MayaEyes
        } else if name.contains("teeth") {
            Self::MayaTeeth
        } else {
            Self::Other
        }
    }

    pub(super) fn studio_material(self) -> bool {
        matches!(
            self,
            Self::StudioTop
                | Self::StudioTrousers
                | Self::StudioScalp
                | Self::StudioHair
                | Self::StudioHairTip
                | Self::StudioHairWisp
                | Self::StudioFace
                | Self::StudioSkin
                | Self::StudioLashes
                | Self::StudioEyes
                | Self::StudioTeeth
        )
    }

    fn clothing_slot(self) -> Option<usize> {
        match self {
            Self::MayaTop | Self::StudioTop => Some(0),
            Self::MayaTrousers | Self::StudioTrousers => Some(1),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(super) enum HumanProfile {
    OldMan(OnceLock<ClothingMask>),
    Maya(FemaleLandmarks),
    Studio(StudioProfile),
}

#[derive(Debug)]
pub(super) struct StudioProfile {
    landmarks: FemaleLandmarks,
    garment_seams: HashSet<[i32; 3]>,
    nape_skin: HashSet<[i32; 3]>,
    shared_atlases: OnceLock<HashMap<usize, ClothingMask>>,
}

#[derive(Debug)]
pub(super) struct FemaleLandmarks {
    floor: f32,
    scale: f32,
    axis_x: f32,
    eye_y: f32,
    eye_z: f32,
    face_scale_y: f32,
}

impl HumanProfile {
    pub(super) fn old_man() -> Self {
        Self::OldMan(OnceLock::new())
    }

    pub(super) fn maya(model: &Model) -> Self {
        Self::Maya(FemaleLandmarks::from_model(
            model,
            ModelRegion::MayaEyes,
            ModelRegion::MayaTeeth,
        ))
    }

    pub(super) fn studio(model: &Model) -> Self {
        let mut joins = HashMap::<(Option<usize>, [i32; 3]), u8>::new();
        for mesh in &model.primitives {
            let slot = mesh.region.clothing_slot().unwrap_or(2);
            for vertex in &mesh.vertices {
                let position = vertex_position_key(vertex.position);
                *joins
                    .entry((mesh.material.base_color_texture, position))
                    .or_default() |= 1 << slot;
            }
        }
        Self::Studio(StudioProfile {
            landmarks: FemaleLandmarks::from_model(
                model,
                ModelRegion::StudioEyes,
                ModelRegion::StudioTeeth,
            ),
            garment_seams: joins
                .into_iter()
                .filter_map(|((_, point), slots)| (slots.count_ones() >= 2).then_some(point))
                .collect(),
            nape_skin: studio_nape_skin(model),
            shared_atlases: OnceLock::new(),
        })
    }

    fn offset(&self, style: &HumanStyle, region: ModelRegion, world: Vec3) -> Vec3 {
        match self {
            Self::OldMan(_) => style.offset(world),
            Self::Maya(landmarks) => landmarks.offset(style, region, world),
            Self::Studio(profile) => profile.landmarks.offset(style, region, world),
        }
    }

    fn pinned_garment_vertex(&self, region: ModelRegion, position: [f32; 3]) -> bool {
        region.clothing_slot().is_some()
            && matches!(self, Self::Studio(profile) if profile.garment_seams.contains(&vertex_position_key(position)))
    }

    fn is_nape_skin(&self, region: ModelRegion, position: [f32; 3]) -> bool {
        region == ModelRegion::StudioHairTip
            && matches!(self, Self::Studio(profile) if profile.nape_skin.contains(&vertex_position_key(position)))
    }
}

fn vertex_position_key(position: [f32; 3]) -> [i32; 3] {
    position.map(|value| (value * 10_000.0).round() as i32)
}

fn studio_nape_skin(model: &Model) -> HashSet<[i32; 3]> {
    let Some(rig) = &model.rig else {
        return HashSet::new();
    };
    let face_textures: HashSet<_> = model
        .primitives
        .iter()
        .filter(|mesh| mesh.region == ModelRegion::StudioFace)
        .filter_map(|mesh| mesh.material.base_color_texture)
        .collect();
    let mut nape = HashSet::new();
    for (mesh, rest) in model.primitives.iter().zip(&rig.meshes) {
        if mesh.region != ModelRegion::StudioHairTip
            || !mesh
                .material
                .base_color_texture
                .is_some_and(|texture| face_textures.contains(&texture))
        {
            continue;
        }
        let Some(skin) = rest.skin.map(|index| &rig.skins[index]) else {
            continue;
        };
        // The face atlas can contain neck triangles grouped into the ponytail core.
        // Their original neck/spine weights distinguish skin from the free hair.
        let neck_joints: Vec<_> = skin
            .joints
            .iter()
            .map(|&(node, _)| {
                rig.nodes[node].name.as_deref().is_some_and(|name| {
                    let name = name.rsplit(':').next().unwrap_or(name);
                    name.starts_with("Spine") || name == "Neck"
                })
            })
            .collect();
        for (vertex, influence) in mesh.vertices.iter().zip(&rest.influences) {
            let neck_weight = influence
                .joints
                .into_iter()
                .zip(influence.weights)
                .filter(|&(joint, weight)| weight > 0.0 && neck_joints[usize::from(joint)])
                .map(|(_, weight)| weight)
                .sum::<f32>();
            if neck_weight >= 0.35 {
                nape.insert(vertex_position_key(vertex.position));
            }
        }
    }
    nape
}

impl StudioProfile {
    fn wardrobe_atlases(&self, source: &Model) -> HashMap<usize, ClothingMask> {
        let mut groups = HashMap::<usize, Vec<&Primitive>>::new();
        for mesh in &source.primitives {
            if mesh.region.clothing_slot().is_some()
                && let Some(index) = mesh.material.base_color_texture
            {
                groups.entry(index).or_default().push(mesh);
            }
        }
        groups
            .into_iter()
            .filter_map(|(index, meshes)| {
                if !(0..2).all(|slot| {
                    meshes
                        .iter()
                        .any(|mesh| mesh.region.clothing_slot() == Some(slot))
                }) {
                    return None;
                }
                let image = &source.images[index];
                let body_y = |y: f32| (y - self.landmarks.floor) * self.landmarks.scale;
                let torso_x = |x: f32| (x - self.landmarks.axis_x).abs() * self.landmarks.scale;
                let mut sums = [Vec3::ZERO; 2];
                let mut counts = [0_u32; 2];
                for mesh in &meshes {
                    let Some(slot) = mesh.region.clothing_slot() else {
                        continue;
                    };
                    for vertex in &mesh.vertices {
                        let [x, y, _] = vertex.position;
                        let y = body_y(y);
                        if (slot == 0 && (1.04..=1.41).contains(&y) && torso_x(x) < 0.25)
                            || (slot == 1 && (0.27..=0.70).contains(&y))
                        {
                            let uv = Vec2::from_array(vertex.uv).clamp(Vec2::ZERO, Vec2::ONE);
                            let u = ((uv.x * image.width as f32) as u32).min(image.width - 1);
                            let v = ((uv.y * image.height as f32) as u32).min(image.height - 1);
                            let offset = ((v * image.width + u) * 4) as usize;
                            sums[slot] += wardrobe_rgb(&image.pixels[offset..offset + 3]);
                            counts[slot] += 1;
                        }
                    }
                }
                let colors =
                    std::array::from_fn::<_, 2, _>(|slot| sums[slot] / counts[slot].max(1) as f32);
                let can_distinguish = counts.into_iter().all(|count| count > 5)
                    && colors[0].distance_squared(colors[1]) > 0.003;
                let mask = ClothingMask::rasterize_parts(meshes, image, |mesh, point, rgb| {
                    let Some(slot) = mesh.region.clothing_slot() else {
                        return [0, 0];
                    };
                    let y = body_y(point.y);
                    if !can_distinguish || !(0.77..1.04).contains(&y) || torso_x(point.x) > 0.27 {
                        return if slot == 0 { [255, 0] } else { [0, 255] };
                    }
                    // Source atlas color disambiguates shirt pixels that crossed a material split at the waist.
                    let color = wardrobe_rgb(rgb);
                    let upper = color.distance_squared(colors[0]);
                    let lower = color.distance_squared(colors[1]);
                    let affinity = (lower - upper) / (lower + upper + 0.012);
                    let author_bias = if slot == 0 { 0.095 } else { -0.095 };
                    let height_bias = ((y - 0.90) * 1.17).clamp(-0.14, 0.14);
                    let top = (smooth(-0.15, 0.15, affinity + author_bias + height_bias) * 255.0)
                        .round() as u8;
                    [top, 255 - top]
                });
                Some((index, mask))
            })
            .collect()
    }
}

fn wardrobe_rgb(rgb: &[u8]) -> Vec3 {
    Vec3::new(f32::from(rgb[0]), f32::from(rgb[1]), f32::from(rgb[2])) / 255.0
}

impl FemaleLandmarks {
    fn from_model(model: &Model, eyes: ModelRegion, teeth: ModelRegion) -> Self {
        let min = Vec3::from_array(model.bounds_min);
        let max = Vec3::from_array(model.bounds_max);
        let scale = 1.75 / (max.y - min.y).max(0.01);
        let eye = center_of_region(model, eyes).unwrap_or(Vec3::new(
            (min.x + max.x) * 0.5,
            min.y + 1.624 / scale,
            max.z - 0.04 / scale,
        ));
        let mouth = center_of_region(model, teeth);
        let face_scale_y = mouth.map_or(scale, |mouth| {
            (0.067 / (eye.y - mouth.y).max(0.02)).clamp(scale * 0.7, scale * 1.8)
        });
        Self {
            floor: min.y,
            scale,
            axis_x: eye.x,
            eye_y: eye.y,
            eye_z: eye.z,
            face_scale_y,
        }
    }
}

impl HumanProfile {
    pub(super) fn prepare_secondary(&self, model: &Model) -> Option<Arc<[Vec<SecondaryDelta>]>> {
        let rig = model.rig.as_ref()?;
        let rest: Vec<_> = rig.nodes.iter().map(|node| node.rest).collect();
        let joints = joint_matrices(&rig.skins, &rest);
        let streams: Vec<_> = rig
            .meshes
            .iter()
            .zip(&model.primitives)
            .map(|(mesh, primitive)| {
                let tagged = matches!(
                    primitive.region,
                    ModelRegion::MayaFace
                        | ModelRegion::MayaLashes
                        | ModelRegion::MayaPonytail
                        | ModelRegion::StudioFace
                        | ModelRegion::StudioSkin
                        | ModelRegion::StudioLashes
                        | ModelRegion::StudioHair
                        | ModelRegion::StudioHairTip
                        | ModelRegion::StudioHairWisp
                ) || primitive.region.clothing_slot().is_some();
                if !tagged && !matches!(self, Self::OldMan(_)) {
                    return Vec::new();
                }
                let rigid_linear = Mat3::from_mat4(rest[mesh.node]);
                let skin = mesh.skin.map(|index| joints[index].as_slice());
                let bounds = region_bounds(&primitive.vertices);
                primitive
                    .vertices
                    .iter()
                    .enumerate()
                    .filter_map(|(index, vertex)| {
                        if self.pinned_garment_vertex(primitive.region, vertex.position)
                            || self.is_nape_skin(primitive.region, vertex.position)
                        {
                            return None;
                        }
                        let world = Vec3::from_array(vertex.position);
                        let secondary = match self {
                            Self::OldMan(_) => legacy_secondary(world),
                            Self::Maya(landmarks) => {
                                landmarks.secondary_world(primitive.region, world, bounds, vertex.uv)
                            }
                            Self::Studio(profile) => {
                                profile
                                    .landmarks
                                    .secondary_world(primitive.region, world, bounds, vertex.uv)
                            }
                        }?;
                        let bind = bind_linear(rigid_linear, skin, mesh.influences.get(index));
                        let determinant = bind.determinant();
                        if !determinant.is_finite() || determinant.abs() < 1e-9 {
                            return None;
                        }
                        let inverse = bind.inverse();
                        Some(SecondaryDelta {
                            vertex: index,
                            kind: match secondary {
                                SecondaryDeltaKind::Blink(amount) => {
                                    SecondaryDeltaKind::Blink(inverse * amount)
                                }
                                SecondaryDeltaKind::Hair { axes, motion } => {
                                    SecondaryDeltaKind::Hair { axes: axes.map(|axis| inverse * axis), motion }
                                }
                                SecondaryDeltaKind::Garment(axes) => {
                                    SecondaryDeltaKind::Garment(axes.map(|axis| inverse * axis))
                                }
                            },
                        })
                    })
                    .collect()
            })
            .collect();
        streams
            .iter()
            .any(|stream| !stream.is_empty())
            .then(|| streams.into())
    }
}

fn center_of_region(model: &Model, region: ModelRegion) -> Option<Vec3> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for vertex in model
        .primitives
        .iter()
        .filter(|mesh| mesh.region == region)
        .flat_map(|mesh| &mesh.vertices)
    {
        let point = Vec3::from_array(vertex.position);
        min = min.min(point);
        max = max.max(point);
    }
    min.is_finite().then(|| (min + max) * 0.5)
}

fn region_bounds(vertices: &[Vertex]) -> [Vec3; 2] {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for vertex in vertices {
        let point = Vec3::from_array(vertex.position);
        min = min.min(point);
        max = max.max(point);
    }
    [min, max]
}

fn garment_axes(weight: f32) -> [Vec3; 3] {
    [
        Vec3::X * (0.028 * weight),
        Vec3::Y * (0.021 * weight),
        Vec3::Z * (0.028 * weight),
    ]
}

fn legacy_secondary(world: Vec3) -> Option<SecondaryDeltaKind> {
    let hem = (1.0 - smooth(0.76, 0.98, world.y))
        * smooth(0.61, 0.72, world.y)
        * (1.0 - smooth(0.18, 0.29, world.x.abs()));
    let cuffs = (1.0 - smooth(0.19, 0.33, world.y))
        * smooth(0.11, 0.18, world.y)
        * (1.0 - smooth(0.20, 0.27, world.x.abs()));
    let weight = (hem * 0.58).max(cuffs * 0.43);
    (weight > 0.012).then(|| SecondaryDeltaKind::Garment(garment_axes(weight)))
}

fn bind_linear(
    rigid: Mat3,
    skin: Option<&[JointTransform]>,
    influence: Option<&Influence>,
) -> Mat3 {
    if let (Some(skin), Some(influence)) = (skin, influence) {
        let mut weighted = Mat3::ZERO;
        let mut total = 0.0;
        for (&joint, &weight) in influence.joints.iter().zip(&influence.weights) {
            if weight > 0.0 {
                weighted += Mat3::from_mat4(skin[usize::from(joint)].matrix) * weight;
                total += weight;
            }
        }
        if total > 0.0 {
            return weighted;
        }
    }
    rigid
}

impl FemaleLandmarks {
    fn face_coordinates(&self, world: Vec3) -> Vec3 {
        Vec3::new(
            (world.x - self.axis_x) * self.scale,
            (world.y - self.eye_y) * self.face_scale_y + 1.624,
            (world.z - self.eye_z) * self.scale + 0.094,
        )
    }

    fn secondary_world(
        &self,
        region: ModelRegion,
        world: Vec3,
        [min, max]: [Vec3; 2],
        uv: [f32; 2],
    ) -> Option<SecondaryDeltaKind> {
        if let Some(slot) = region.clothing_slot() {
            let height = (max.y - min.y).max(0.01);
            let relative = (world.y - min.y) / height;
            let center_weight =
                1.0 - smooth(0.21, 0.34, ((world.x - self.axis_x) * self.scale).abs());
            let weight = if slot == 0 {
                (1.0 - smooth(0.085, 0.37, relative)) * center_weight * 0.76
            } else {
                (1.0 - smooth(0.045, 0.245, relative)) * 0.49
            };
            return (weight > 0.012)
                .then(|| SecondaryDeltaKind::Garment(garment_axes(weight / self.scale)));
        }
        let face = self.face_coordinates(world);
        let conversion = Vec3::new(self.scale, self.face_scale_y, self.scale);
        if matches!(
            region,
            ModelRegion::MayaPonytail | ModelRegion::StudioHair | ModelRegion::StudioHairTip | ModelRegion::StudioHairWisp
        ) {
            let length = (max.y - min.y).max(0.005);
            let distance_from_root = 1.0 - (world.y - min.y) / length;
            let (tip, motion) = match region {
                ModelRegion::MayaPonytail => {
                    let bend = (1.0 - smooth(1.47, 1.65, face.y))
                        * (1.0 - smooth(-0.055, -0.012, face.z));
                    (bend, HairMotion::Tail { end_weight: smooth(0.17, 0.82, bend) })
                }
                ModelRegion::StudioHair => (
                    smooth(0.42, 0.87, distance_from_root)
                        * (smooth(0.045, 0.24, length * self.scale) * 0.78 + 0.12),
                    HairMotion::Root,
                ),
                ModelRegion::StudioHairWisp => (
                    // Wisp cards store root to tip along V, even when they curve back upward.
                    smooth(0.13, 1.0, uv[1]).powi(2) * 0.55,
                    HairMotion::LooseStrand,
                ),
                _ => (
                    smooth(0.33, 1.0, distance_from_root),
                    HairMotion::Tail { end_weight: smooth(0.46, 1.0, distance_from_root) },
                ),
            };
            return (tip > 0.012).then(|| {
                SecondaryDeltaKind::Hair {
                    axes: [
                        Vec3::new(0.041, 0.003, 0.001) * tip / conversion,
                        Vec3::new(0.0, 0.023, 0.0) * tip / conversion,
                        Vec3::new(0.0, 0.012, -0.029) * tip / conversion,
                    ],
                    motion,
                }
            });
        }
        let row = smooth(0.011, 0.026, face.x.abs()) * (1.0 - smooth(0.052, 0.074, face.x.abs()));
        let front = smooth(0.054, 0.094, face.z);
        let eye_y = face.y - 1.624;
        let rim = 1.0 - smooth(0.008, 0.031, eye_y.abs());
        let blink = Vec3::new(0.0, -eye_y * 0.93, 0.0039) * row * front * rim / conversion;
        (blink.length_squared() >= 1e-9).then_some(SecondaryDeltaKind::Blink(blink))
    }

    fn offset(&self, style: &HumanStyle, region: ModelRegion, world: Vec3) -> Vec3 {
        let [x, y, z] = world.to_array();
        let body = Vec3::new(
            (x - self.axis_x) * self.scale,
            (y - self.floor) * self.scale,
            z * self.scale,
        );
        match region {
            ModelRegion::MayaTop | ModelRegion::StudioTop => {
                let mantle =
                    (1.0 - smooth(0.22, 0.35, body.x.abs())) * (1.0 - smooth(1.40, 1.55, body.y));
                style.garment_offset(body, mantle, true) / self.scale
            }
            ModelRegion::MayaTrousers | ModelRegion::StudioTrousers => {
                let leg_center = body.x.signum() * (0.125 - 0.03 * smooth(0.22, 0.75, body.y));
                Vec3::new((body.x - leg_center) * 0.37, 0.0, body.z * 0.30)
                    * (style.trouser_fit * smooth(0.015, 0.056, body.x.abs()) / self.scale)
            }
            ModelRegion::StudioScalp => {
                style.face_shape(self.face_coordinates(world))
                    / Vec3::new(self.scale, self.face_scale_y, self.scale)
            }
            _ => {
                let face = self.face_coordinates(world);
                let offset = if matches!(
                    region,
                    ModelRegion::MayaHair
                        | ModelRegion::MayaPonytail
                        | ModelRegion::StudioHair
                        | ModelRegion::StudioHairTip
                        | ModelRegion::StudioHairWisp
                ) {
                    let crown = smooth(1.60, 1.73, face.y);
                    let tail =
                        (1.0 - smooth(1.51, 1.64, face.y)) * (1.0 - smooth(-0.045, 0.005, face.z));
                    let shape = match style.hairstyle {
                        HairStyle::Original => Vec3::ZERO,
                        HairStyle::Cropped => {
                            Vec3::new(-face.x * 0.26, (1.61 - face.y) * 0.70, 0.047) * tail
                        }
                        HairStyle::Full => Vec3::new(face.x * 0.27, -0.038, -0.039) * tail,
                        HairStyle::Swept => Vec3::new(0.081, 0.018, -0.023) * tail,
                    };
                    style.face_shape(face) + style.hair_offset(face, 1.0, crown) + shape
                } else {
                    style.face_shape(face) + style.expression(face)
                };
                offset / Vec3::new(self.scale, self.face_scale_y, self.scale)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HairStyle {
    #[default]
    Original,
    Cropped,
    Full,
    Swept,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GarmentStyle {
    #[default]
    Original,
    Sweater,
    LongCoat,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HumanStyle {
    pub face_width: f32,
    pub jaw_width: f32,
    pub nose_length: f32,
    pub eye_scale: f32,
    pub illustration: f32,
    pub hair_volume: f32,
    pub jacket_fit: f32,
    pub trouser_fit: f32,
    pub jacket_tint: [f32; 3],
    pub trouser_tint: [f32; 3],
    pub hairstyle: HairStyle,
    pub garment: GarmentStyle,
    pub smile: f32,
    pub brow_raise: f32,
    pub mouth_open: f32,
    pub blink: f32,
}

impl Default for HumanStyle {
    fn default() -> Self {
        Self {
            face_width: 0.0,
            jaw_width: 0.0,
            nose_length: 0.0,
            eye_scale: 0.0,
            illustration: 0.0,
            hair_volume: 0.0,
            jacket_fit: 0.0,
            trouser_fit: 0.0,
            jacket_tint: [1.0; 3],
            trouser_tint: [1.0; 3],
            hairstyle: HairStyle::Original,
            garment: GarmentStyle::Original,
            smile: 0.0,
            brow_raise: 0.0,
            mouth_open: 0.0,
            blink: 0.0,
        }
    }
}

impl HumanStyle {
    pub fn anime() -> Self {
        Self {
            face_width: 0.32,
            jaw_width: -0.79,
            nose_length: -0.82,
            eye_scale: 0.93,
            illustration: 0.82,
            hairstyle: HairStyle::Swept,
            hair_volume: 0.54,
            garment: GarmentStyle::Sweater,
            jacket_tint: [0.68, 0.40, 0.96],
            trouser_tint: [0.33, 0.48, 0.81],
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), ImportError> {
        if self
            .shape_values()
            .iter()
            .all(|value| (-1.0..=1.0).contains(value))
            && self
                .jacket_tint
                .into_iter()
                .chain(self.trouser_tint)
                .chain([self.mouth_open, self.blink, self.illustration])
                .all(|value| (0.0..=1.0).contains(&value))
        {
            Ok(())
        } else {
            Err(ImportError::new(
                "Внешность персонажа",
                "Параметры формы должны быть в диапазоне −1…1, цвета и мимика в диапазоне 0…1.",
            ))
        }
    }

    fn shape_values(&self) -> [f32; 9] {
        [
            self.face_width,
            self.jaw_width,
            self.nose_length,
            self.eye_scale,
            self.hair_volume,
            self.jacket_fit,
            self.trouser_fit,
            self.smile,
            self.brow_raise,
        ]
    }

    fn geometry_changed(&self) -> bool {
        self.shape_values().into_iter().any(|value| value != 0.0)
            || self.mouth_open != 0.0
            || self.blink != 0.0
            || self.hairstyle != HairStyle::Original
            || self.garment != GarmentStyle::Original
    }

    fn expression(&self, point: Vec3) -> Vec3 {
        // Landmarks are expressed in the normalized head frame of the two bundled presets.
        let [x, y, z] = point.to_array();
        let side = x.abs();
        let front = smooth(0.052, 0.098, z);
        let mouth = smooth(1.521, 1.554, y)
            * (1.0 - smooth(1.570, 1.593, y))
            * (1.0 - smooth(0.039, 0.055, side))
            * front;
        let corner = smooth(0.008, 0.035, side);
        let lip_y = y - 1.557;
        let eye_row = smooth(0.010, 0.030, side) * (1.0 - smooth(0.055, 0.073, side));
        let eye_y = y - 1.624;
        let eyelid = (1.0 - smooth(0.004, 0.021, eye_y.abs())) * eye_row * front;
        let brow = smooth(1.629, 1.652, y) * (1.0 - smooth(1.665, 1.697, y)) * eye_row * front;
        Vec3::new(
            x * self.smile * 0.15 * mouth,
            self.smile * (0.018 * corner - 0.003) * mouth
                + self.mouth_open * (lip_y * 0.62 - 0.004) * mouth
                + self.brow_raise * 0.011 * brow
                - self.blink * eye_y * 0.84 * eyelid,
            self.mouth_open * 0.006 * mouth + self.blink * 0.003 * eyelid,
        )
    }

    fn hair_offset(&self, point: Vec3, hair: f32, crown: f32) -> Vec3 {
        let [x, y, z] = point.to_array();
        let radial = Vec3::new(x * 0.18, (y - 1.61) * 0.22, (z + 0.024) * 0.17);
        let volume = radial * (self.hair_volume * hair);
        let preset = match self.hairstyle {
            HairStyle::Original => Vec3::ZERO,
            HairStyle::Cropped => -radial * (0.50 * hair) - Vec3::Y * (0.019 * crown),
            HairStyle::Full => radial * (0.95 * hair) + Vec3::Y * (0.020 * crown),
            HairStyle::Swept => Vec3::new(0.041 * crown, -x * 0.17 * crown, 0.032 * hair),
        };
        volume + preset
    }

    fn garment_offset(&self, point: Vec3, torso: f32, separate_mesh: bool) -> Vec3 {
        let [x, y, z] = point.to_array();
        let fit_weight = if separate_mesh {
            1.0
        } else {
            smooth(0.81, 0.93, y)
        };
        let fit =
            Vec3::new(x * 0.26, 0.0, (z + 0.018) * 0.32) * (self.jacket_fit * torso * fit_weight);
        let hem = if separate_mesh {
            1.0
        } else {
            smooth(0.58, 0.89, y)
        };
        let coat_extension = if separate_mesh { 0.19 } else { 0.14 };
        let preset = match self.garment {
            GarmentStyle::Original => Vec3::ZERO,
            GarmentStyle::Sweater => {
                Vec3::new(
                    -x * 0.11,
                    0.055 * (1.0 - smooth(0.84, 1.16, y)),
                    -(z + 0.018) * 0.14,
                ) * fit_weight
            }
            GarmentStyle::LongCoat => {
                Vec3::new(
                    x * 0.14,
                    -coat_extension * (1.0 - smooth(0.89, 1.26, y)),
                    (z + 0.018) * 0.20,
                ) * hem
            }
        };
        fit + preset * torso
    }

    fn face_shape(&self, point: Vec3) -> Vec3 {
        let [x, y, z] = point.to_array();
        let head = smooth(1.49, 1.56, y);
        let jaw = smooth(1.50, 1.53, y) * (1.0 - smooth(1.58, 1.63, y)) * smooth(-0.035, 0.06, z);
        let nose = smooth(1.575, 1.603, y)
            * (1.0 - smooth(1.63, 1.655, y))
            * (1.0 - smooth(0.016, 0.045, x.abs()))
            * smooth(0.074, 0.113, z);
        let eye_x = x - 0.036 * x.signum();
        let eye_y = y - 1.624;
        // Deform the eye and surrounding eyelids on the same continuous surface.
        let eye = (1.0 - smooth(0.010, 0.035, eye_x.abs()))
            * (1.0 - smooth(0.010, 0.045, eye_y.abs()))
            * smooth(0.051, 0.082, z)
            * self.eye_scale;
        Vec3::new(
            x * (self.face_width * 0.25 * head + self.jaw_width * 0.32 * jaw) + eye_x * 0.26 * eye,
            eye_y * 0.56 * eye,
            self.jaw_width * 0.007 * jaw + self.nose_length * 0.034 * nose,
        )
    }

    fn offset(&self, point: Vec3) -> Vec3 {
        // These landmarks are in the bundled Bake3D model's bind pose, in meters.
        let [x, y, z] = point.to_array();
        let side = x.abs();
        let mut offset = Vec3::ZERO;
        if y > 1.48 {
            let crown = smooth(1.65, 1.72, y);
            let back_hair = smooth(1.53, 1.61, y) * (1.0 - smooth(-0.038, 0.012, z));
            let hair = crown.max(back_hair);
            offset += self.face_shape(point)
                + self.hair_offset(point, hair, crown)
                + self.expression(point);
        }
        if y > 0.58 && y < 1.51 {
            let torso = (1.0 - smooth(1.41, 1.51, y)) * (1.0 - smooth(0.22, 0.33, side));
            offset += self.garment_offset(point, torso, false);
        }
        if y > 0.11 && y < 0.86 {
            let silhouette = smooth(0.11, 0.18, y)
                * (1.0 - smooth(0.72, 0.86, y))
                * smooth(0.012, 0.066, side)
                * (1.0 - smooth(0.23, 0.28, side));
            let leg_center = x.signum() * (0.163 - 0.05 * smooth(0.16, 0.70, y));
            offset.x += (x - leg_center) * self.trouser_fit * 0.42 * silhouette;
            offset.z += (z + 0.049) * self.trouser_fit * 0.34 * silhouette;
        }
        offset
    }
}

fn smooth(min: f32, max: f32, value: f32) -> f32 {
    let t = ((value - min) / (max - min)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Model {
    /// Prepare the bundled human materials once, before their images are shared by actors.
    pub fn polish_human_materials(&mut self) {
        match self.human_profile.as_deref() {
            Some(HumanProfile::Maya(_) | HumanProfile::Studio(_)) => {
                let mut cloth_surfaces = HashMap::new();
                for mesh in &mut self.primitives {
                    match mesh.region.clothing_slot() {
                        Some(slot) => {
                            mesh.material.metallic = 0.0;
                            mesh.material.roughness = if slot == 0 { 0.72 } else { 0.84 };
                            mesh.material.normal_scale = 0.80;
                            if let Some(index) = mesh.material.normal_texture {
                                let surface = *cloth_surfaces.entry(index).or_insert_with(|| {
                                    let map = cloth_surface(&self.images[index]);
                                    add_cloth_weave(&mut self.images[index], |_, _| 1.0);
                                    let surface = self.images.len();
                                    self.images.push(map);
                                    surface
                                });
                                mesh.material.metallic_roughness_texture = Some(surface);
                            }
                        }
                        None if matches!(
                            mesh.region,
                            ModelRegion::MayaFace
                                | ModelRegion::StudioFace
                                | ModelRegion::StudioSkin
                        ) =>
                        {
                            mesh.material.roughness = 0.69;
                        }
                        _ => {}
                    }
                }
            }
            Some(HumanProfile::OldMan(mask)) => {
                let mesh = &self.primitives[0];
                let Some(base) = mesh.material.base_color_texture else {
                    return;
                };
                let mask = mask.get_or_init(|| ClothingMask::rasterize(mesh, &self.images[base]));
                if let Some(index) = mesh.material.normal_texture {
                    let image = &mut self.images[index];
                    let extent = [image.width, image.height];
                    add_cloth_weave(image, |x, y| {
                        let [jacket, trousers] = mask.sample(x, y, extent);
                        0.40 * jacket + 0.62 * trousers
                    });
                }
                if let Some(index) = mesh.material.metallic_roughness_texture {
                    let image = &mut self.images[index];
                    mask.adjust_roughness(image, [0.83, 0.89], [0.24, 0.42]);
                }
            }
            None => {}
        }
    }

    pub fn customize_human(&self, style: &HumanStyle) -> Model {
        let mut result = self.clone();
        let Some(profile) = &self.human_profile else {
            return result;
        };
        if style.validate().is_err() {
            return result;
        }

        if style.jacket_tint != [1.0; 3]
            || style.trouser_tint != [1.0; 3]
            || style.garment != GarmentStyle::Original
        {
            match profile.as_ref() {
                HumanProfile::OldMan(mask) => {
                    let mesh = &self.primitives[0];
                    if let Some(index) = mesh.material.base_color_texture {
                        let source = &self.images[index];
                        let mask = mask.get_or_init(|| ClothingMask::rasterize(mesh, source));
                        result.images[index].pixels = mask.tint(source, style);
                        if let Some(index) = mesh.material.metallic_roughness_texture
                            && let Some(roughness) = garment_roughness(style.garment)
                        {
                            mask.adjust_roughness(
                                &mut result.images[index],
                                [roughness, 0.0],
                                [0.30, 0.0],
                            );
                        }
                    }
                }
                HumanProfile::Maya(_) => result.style_separate_clothes(style, None),
                HumanProfile::Studio(profile) => result.style_separate_clothes(
                    style,
                    Some(
                        profile
                            .shared_atlases
                            .get_or_init(|| profile.wardrobe_atlases(self)),
                    ),
                ),
            }
        }
        if style.geometry_changed() {
            result.deform_human(style, profile);
        }
        if style.illustration > 0.0 {
            for mesh in &mut result.primitives {
                mesh.material.illustration = style.illustration;
                mesh.material.normal_scale *= 1.0 - style.illustration * 0.79;
                mesh.material.roughness += (0.84 - mesh.material.roughness) * style.illustration * 0.60;
            }
        }
        result
    }

    fn style_separate_clothes(
        &mut self,
        style: &HumanStyle,
        atlases: Option<&HashMap<usize, ClothingMask>>,
    ) {
        let mut textures = Vec::new();
        for mesh in &mut self.primitives {
            let Some(slot) = mesh.region.clothing_slot() else {
                continue;
            };
            let (tint, amount) = if slot == 0 {
                (style.jacket_tint, [1.0, 0.0])
            } else {
                (style.trouser_tint, [0.0, 1.0])
            };
            let upper_style = slot == 0 && style.garment != GarmentStyle::Original;
            let mask = mesh
                .material
                .base_color_texture
                .and_then(|index| atlases.and_then(|atlases| atlases.get(&index)));
            if tint == [1.0; 3] && !upper_style && mask.is_none() {
                continue;
            }
            if let Some(base) = mesh.material.base_color_texture {
                let region = mask.map_or(mesh.region, |_| ModelRegion::Other);
                let index = textures
                    .iter()
                    .find_map(|&(source, cached_region, output)| {
                        (source == base && cached_region == region).then_some(output)
                    })
                    .unwrap_or_else(|| {
                        let mut image = self.images[base].clone();
                        image.pixels = if let Some(mask) = mask {
                            mask.tint(&image, style)
                        } else {
                            colorize(&image, |_| amount, style)
                        };
                        let index = self.images.len();
                        self.images.push(image);
                        textures.push((base, region, index));
                        index
                    });
                mesh.material.base_color_texture = Some(index);
            } else {
                for (channel, value) in mesh.material.base_color[..3].iter_mut().enumerate() {
                    *value *= 0.14 + 0.86 * tint[channel];
                }
            }
            if upper_style && let Some(target) = garment_roughness(style.garment) {
                mesh.material.roughness = mesh.material.roughness * 0.50 + target * 0.50;
            }
        }
    }

    fn deform_human(&mut self, style: &HumanStyle, profile: &HumanProfile) {
        let Some(rig) = &mut self.rig else { return };
        let rig = Arc::make_mut(rig);
        let rest: Vec<_> = rig.nodes.iter().map(|node| node.rest).collect();
        let joints = joint_matrices(&rig.skins, &rest);
        self.bounds_min = [f32::INFINITY; 3];
        self.bounds_max = [f32::NEG_INFINITY; 3];
        for (mesh, posed) in rig.meshes.iter_mut().zip(&mut self.primitives) {
            let mesh_linear = Mat3::from_mat4(rest[mesh.node]);
            let skin = mesh.skin.map(|index| joints[index].as_slice());
            for (index, vertex) in mesh.vertices.iter_mut().enumerate() {
                let position = posed.vertices[index].position;
                if profile.pinned_garment_vertex(posed.region, position) {
                    continue;
                }
                let region = if profile.is_nape_skin(posed.region, position) {
                    ModelRegion::StudioFace
                } else {
                    posed.region
                };
                let deform = |point| profile.offset(style, region, point);
                let rigged = Vec3::from_array(position);
                let offset = deform(rigged);
                if offset.length_squared() < 1e-14 {
                    continue;
                }

                let bind_linear = bind_linear(mesh_linear, skin, mesh.influences.get(index));
                let determinant = bind_linear.determinant();
                if !determinant.is_finite() || determinant.abs() < 1e-9 {
                    continue;
                }
                let inverse = bind_linear.inverse();
                vertex.position = (Vec3::from_array(vertex.position) + inverse * offset).to_array();

                // The derivative of the same deformation keeps the supplied smooth normals
                // and the tangent basis aligned with the edited surface and normal map.
                let step = 0.00025;
                let derivative = [Vec3::X, Vec3::Y, Vec3::Z].map(|axis| {
                    axis + (deform(rigged + step * axis) - deform(rigged - step * axis))
                        / (2.0 * step)
                });
                let jacobian = inverse
                    * Mat3::from_cols(derivative[0], derivative[1], derivative[2])
                    * bind_linear;
                if jacobian.determinant().abs() > 1e-6 {
                    let normal = Vec3::from_array(vertex.normal);
                    let normal = (jacobian.inverse().transpose() * normal)
                        .try_normalize()
                        .unwrap_or(normal);
                    let tangent = jacobian * Vec4::from_array(vertex.tangent).truncate();
                    vertex.normal = normal.to_array();
                    vertex.tangent = tangent_basis(vertex.normal, tangent, vertex.tangent[3]);
                }
            }

            for (target, source) in posed
                .vertices
                .iter_mut()
                .zip(mesh.posed_vertices(&rest, &joints))
            {
                *target = source;
                for (axis, value) in source.position.into_iter().enumerate() {
                    self.bounds_min[axis] = self.bounds_min[axis].min(value);
                    self.bounds_max[axis] = self.bounds_max[axis].max(value);
                }
            }
        }
    }
}

fn cloth_weave(x: u32, y: u32, extent: [u32; 2]) -> [i16; 2] {
    const THREAD: [i16; 8] = [0, 5, 8, 5, 0, -5, -8, -5];
    // Register the weave to the fabric UVs even when the maps use different resolutions.
    let u = x * 1024 / extent[0];
    let v = y * 1024 / extent[1];
    [
        THREAD[((u + ((v / 4) % 2) * 4) % 8) as usize],
        THREAD[((v + ((u / 4) % 2) * 4) % 8) as usize],
    ]
}

fn add_cloth_weave(image: &mut Image, strength: impl Fn(u32, u32) -> f32) {
    let extent = [image.width, image.height];
    for (index, pixel) in Arc::make_mut(&mut image.pixels)
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        let x = index as u32 % extent[0];
        let y = index as u32 / extent[0];
        let weight = strength(x, y);
        if weight != 0.0 {
            for (value, thread) in pixel[..2].iter_mut().zip(cloth_weave(x, y, extent)) {
                *value = (f32::from(*value) + f32::from(thread) * weight).round() as u8;
            }
        }
    }
}

fn cloth_surface(normal: &Image) -> Image {
    let width = normal.width.min(384);
    let height = normal.height.min(384);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let nx = texel_center(x, width, normal.width);
            let ny = texel_center(y, height, normal.height);
            let index = ((ny * normal.width + nx) * 4) as usize;
            let bend =
                normal.pixels[index].abs_diff(128) + normal.pixels[index + 1].abs_diff(128) / 2;
            // The existing folds remain slightly smoother than the flat yarn.
            pixels.extend([255, 252 - bend.min(60) / 2, 0, 255]);
        }
    }
    Image {
        width,
        height,
        pixels: pixels.into(),
    }
}

fn texel_center(index: u32, source_extent: u32, target_extent: u32) -> u32 {
    ((index * 2 + 1) * target_extent) / (source_extent * 2)
}

#[derive(Debug)]
pub(super) struct ClothingMask {
    extent: [u32; 2],
    weights: Vec<[u8; 2]>,
}

impl ClothingMask {
    fn rasterize(mesh: &Primitive, image: &Image) -> Self {
        Self::rasterize_parts([mesh], image, |_, position, rgb| {
            garment_weights(position, rgb)
        })
    }

    fn rasterize_parts<'a>(
        meshes: impl IntoIterator<Item = &'a Primitive>,
        image: &Image,
        classify: impl Fn(&Primitive, Vec3, &[u8]) -> [u8; 2],
    ) -> Self {
        let width = image.width as usize;
        let height = image.height as usize;
        let extent = Vec2::new(width as f32, height as f32);
        let mut weights = vec![[0; 2]; width * height];
        let mut covered = vec![false; weights.len()];

        // Rasterizing model-space positions into the actual atlas avoids tinting unrelated
        // UV islands and limits the edits to the garments on this specific source asset.
        for (mesh, triangle) in meshes.into_iter().flat_map(|mesh| {
            mesh.indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(move |triangle| (mesh, triangle))
        }) {
            let vertices = triangle.map(|index| &mesh.vertices[index as usize]);
            let [a, b, c] =
                vertices.map(|vertex| Vec2::from_array(vertex.uv) * extent - Vec2::splat(0.5));
            let ab = b - a;
            let ac = c - a;
            let area = ab.perp_dot(ac);
            if area.abs() < 1e-7 {
                continue;
            }
            let min = a.min(b).min(c).ceil().max(Vec2::ZERO);
            let max = a.max(b).max(c).floor().min(extent - Vec2::ONE);
            let positions = vertices.map(|vertex| Vec3::from_array(vertex.position));
            for y in min.y as usize..=max.y as usize {
                for x in min.x as usize..=max.x as usize {
                    let point = Vec2::new(x as f32, y as f32) - a;
                    let wb = point.perp_dot(ac) / area;
                    let wc = ab.perp_dot(point) / area;
                    let wa = 1.0 - wb - wc;
                    if wa.min(wb).min(wc) < -0.0001 {
                        continue;
                    }
                    let at = y * width + x;
                    let position = wa * positions[0] + wb * positions[1] + wc * positions[2];
                    let channels = &image.pixels[at * 4..at * 4 + 3];
                    let region = classify(mesh, position, channels);
                    weights[at][0] = weights[at][0].max(region[0]);
                    weights[at][1] = weights[at][1].max(region[1]);
                    covered[at] = true;
                }
            }
        }

        // Copy mask values into a two-texel atlas gutter so bilinear sampling at a seam
        // doesn't reveal the untinted original. Covered face/shirt pixels stay protected.
        for _ in 0..2 {
            let previous = weights.clone();
            for y in 1..height.saturating_sub(1) {
                for x in 1..width.saturating_sub(1) {
                    let at = y * width + x;
                    if covered[at] {
                        continue;
                    }
                    for neighbor in [at - 1, at + 1, at - width, at + width] {
                        weights[at][0] = weights[at][0].max(previous[neighbor][0]);
                        weights[at][1] = weights[at][1].max(previous[neighbor][1]);
                    }
                }
            }
        }
        Self {
            extent: [image.width, image.height],
            weights,
        }
    }

    fn tint(&self, image: &Image, style: &HumanStyle) -> Arc<[u8]> {
        colorize(
            image,
            |index| self.weights[index].map(|value| f32::from(value) / 255.0),
            style,
        )
    }

    fn sample(&self, x: u32, y: u32, from: [u32; 2]) -> [f32; 2] {
        let [width, height] = self.extent;
        let u = texel_center(x, from[0], width);
        let v = texel_center(y, from[1], height);
        self.weights[(v * width + u) as usize].map(|weight| f32::from(weight) / 255.0)
    }

    fn adjust_roughness(&self, image: &mut Image, targets: [f32; 2], strengths: [f32; 2]) {
        let extent = [image.width, image.height];
        let [upper_target, lower_target] = targets.map(|target| target * 255.0);
        for (index, pixel) in Arc::make_mut(&mut image.pixels)
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            let [jacket, trousers] =
                self.sample(index as u32 % extent[0], index as u32 / extent[0], extent);
            let prior = f32::from(pixel[1]);
            pixel[1] = (prior
                + (upper_target - prior) * jacket * strengths[0]
                + (lower_target - prior) * trousers * strengths[1])
                .round() as u8;
        }
    }
}

fn garment_roughness(style: GarmentStyle) -> Option<f32> {
    match style {
        GarmentStyle::Original => None,
        GarmentStyle::Sweater => Some(0.91),
        GarmentStyle::LongCoat => Some(0.68),
    }
}

fn colorize(image: &Image, weights: impl Fn(usize) -> [f32; 2], style: &HumanStyle) -> Arc<[u8]> {
    let mut pixels = Arc::clone(&image.pixels);
    let rgba = Arc::make_mut(&mut pixels).as_chunks_mut::<4>().0;
    let scales =
        [style.jacket_tint, style.trouser_tint].map(|color| color.map(|value| 0.14 + 0.86 * value));
    let mix_gray = [style.jacket_tint, style.trouser_tint]
        .map(|color| 0.62 * (1.0 - color.into_iter().fold(1.0_f32, f32::min)));
    for (index, pixel) in rgba.iter_mut().enumerate() {
        let [jacket, trousers] = weights(index);
        if jacket == 0.0 && trousers == 0.0 {
            continue;
        }
        let trousers = trousers.min(1.0 - jacket);
        let luminance =
            f32::from(pixel[0]) * 0.22 + f32::from(pixel[1]) * 0.68 + f32::from(pixel[2]) * 0.10;
        let fabric = match style.garment {
            GarmentStyle::Original => 1.0,
            GarmentStyle::Sweater => {
                let x = index % image.width as usize;
                let ribs = [0.89, 0.92, 0.98, 1.02, 0.98, 0.92];
                ribs[x % ribs.len()]
            }
            GarmentStyle::LongCoat => 0.83,
        };
        for (channel, value) in pixel[..3].iter_mut().enumerate() {
            let raw = f32::from(*value);
            let upper = (raw + (luminance - raw) * mix_gray[0]) * scales[0][channel] * fabric;
            let lower = (raw + (luminance - raw) * mix_gray[1]) * scales[1][channel];
            *value = (raw + (upper - raw) * jacket + (lower - raw) * trousers).round() as u8;
        }
    }
    pixels
}

fn garment_weights(position: Vec3, pixel: &[u8]) -> [u8; 2] {
    let [x, y, z] = position.to_array();
    let side = x.abs();
    let [r, g, b] = [pixel[0], pixel[1], pixel[2]].map(f32::from);
    let sleeve = smooth(0.22, 0.30, side);
    let cuff = 0.83 + 0.055 * sleeve;
    let skin = smooth(19.0, 33.0, r - g) * smooth(10.0, 17.0, g - b);
    let shirt_radius = (0.02 + (y - 1.21) * 0.49).max(0.02);
    let shirt = smooth(1.21, 1.29, y)
        * smooth(0.035, 0.09, z)
        * (1.0 - smooth(shirt_radius, shirt_radius + 0.018, side))
        * smooth(-7.0, 6.0, b - r);
    let neck = smooth(1.45, 1.51, y) * skin;
    let jacket = smooth(cuff, cuff + 0.034, y)
        * (1.0 - smooth(1.47, 1.55, y))
        * (1.0 - sleeve * skin * (1.0 - smooth(0.92, 0.98, y)))
        * (1.0 - shirt)
        * (1.0 - neck);
    let trousers = smooth(0.119, 0.155, y)
        * (1.0 - smooth(0.826, 0.866, y))
        * (1.0 - smooth(0.207, 0.238, side))
        * (1.0 - jacket);
    [jacket, trousers].map(|weight| (weight * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::super::{BoneRotation, SecondaryPose, Vertex, decode_glb};
    use super::*;

    #[test]
    fn a1_model_human_style() {
        let style: HumanStyle = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/a1_model_human_style.json"
        ))
        .unwrap();
        let error: ImportError = style.validate().unwrap_err();
        assert!(error.to_string().contains("Внешность персонажа"));
    }

    #[test]
    fn material_polish_protects_eyes_and_resamples_boris_clothing_mask() {
        let mut maya = decode_glb(include_bytes!("../demo_assets/woman.glb")).unwrap();
        let previous: Vec<_> = maya.primitives.iter().map(|mesh| mesh.material).collect();
        maya.polish_human_materials();
        for (mesh, original) in maya.primitives.iter().zip(previous) {
            assert_eq!(
                mesh.material.base_color_texture,
                original.base_color_texture
            );
            match mesh.region {
                ModelRegion::MayaEyes => {
                    assert_eq!(mesh.material.roughness, original.roughness);
                    assert_eq!(mesh.material.normal_texture, original.normal_texture);
                }
                ModelRegion::MayaFace => assert!(mesh.material.roughness > original.roughness),
                ModelRegion::MayaTop | ModelRegion::MayaTrousers => {
                    assert!(mesh.material.normal_scale > original.normal_scale);
                    let surface = &maya.images[mesh.material.metallic_roughness_texture.unwrap()];
                    assert!(surface.pixels.chunks_exact(4).all(|pixel| pixel[2] == 0));
                }
                _ => {}
            }
        }

        let mut boris = decode_glb(include_bytes!("../demo_assets/human.glb")).unwrap();
        let material = boris.primitives[0].material;
        let base = material.base_color_texture.unwrap();
        let roughness = material.metallic_roughness_texture.unwrap();
        assert_ne!(boris.images[base].width, boris.images[roughness].width);
        let previous_roughness = boris.images[roughness].pixels.clone();
        boris.polish_human_materials();
        let Some(HumanProfile::OldMan(mask)) = boris.human_profile.as_deref() else {
            panic!("Boris uses the shared clothing atlas");
        };
        let mask = mask.get().unwrap();
        let image = &boris.images[roughness];
        let mut changed = 0;
        for (index, (before, after)) in previous_roughness
            .chunks_exact(4)
            .zip(image.pixels.chunks_exact(4))
            .enumerate()
        {
            if before != after {
                let [jacket, trousers] = mask.sample(
                    index as u32 % image.width,
                    index as u32 / image.width,
                    [image.width, image.height],
                );
                assert!(jacket > 0.0 || trousers > 0.0);
                assert_eq!(
                    [before[0], before[2], before[3]],
                    [after[0], after[2], after[3]]
                );
                changed += 1;
            }
        }
        assert!(changed > 1_000);
    }

    #[test]
    fn bundled_human_morphs_and_tints_clothes_without_replacing_its_rig() {
        let original = decode_glb(include_bytes!("../demo_assets/human.glb")).unwrap();
        let style = HumanStyle {
            face_width: 0.8,
            nose_length: 0.8,
            jacket_fit: -0.5,
            trouser_fit: 0.65,
            jacket_tint: [0.22, 0.49, 1.0],
            trouser_tint: [0.74, 0.30, 0.22],
            ..HumanStyle::default()
        };
        let edited = original.customize_human(&style);
        let before = &original.primitives[0];
        let after = &edited.primitives[0];
        assert_eq!(after.indices, before.indices);
        let moving_face = before
            .vertices
            .iter()
            .zip(&after.vertices)
            .filter(|(a, b)| a.position[1] > 1.58 && (a.position[0] - b.position[0]).abs() > 0.005)
            .count();
        assert!(moving_face > 200);
        assert!(
            before
                .vertices
                .iter()
                .zip(&after.vertices)
                .any(|(a, b)| a.normal != b.normal)
        );

        let atlas = before.material.base_color_texture.unwrap();
        let sample = |image: &Image, uv: [f32; 2]| {
            let x = ((uv[0] * image.width as f32) as usize).min(image.width as usize - 1);
            let y = ((uv[1] * image.height as f32) as usize).min(image.height as usize - 1);
            let offset = (y * image.width as usize + x) * 4;
            [
                image.pixels[offset],
                image.pixels[offset + 1],
                image.pixels[offset + 2],
            ]
        };
        let pixels_changed = |region: fn([f32; 3]) -> bool| {
            before
                .vertices
                .iter()
                .filter(|vertex| {
                    region(vertex.position) && {
                        sample(&original.images[atlas], vertex.uv)
                            != sample(&edited.images[atlas], vertex.uv)
                    }
                })
                .count()
        };
        assert!(pixels_changed(|[x, y, _]| x.abs() < 0.20 && (1.05..1.18).contains(&y)) > 400);
        assert!(pixels_changed(|[_, y, _]| (0.29..0.52).contains(&y)) > 2_000);
        assert!(pixels_changed(|[_, y, _]| y > 1.59) < 80);
        for texture in [
            before.material.normal_texture,
            before.material.metallic_roughness_texture,
        ]
        .into_iter()
        .flatten()
        {
            assert!(Arc::ptr_eq(
                &original.images[texture].pixels,
                &edited.images[texture].pixels
            ));
        }

        assert_eq!(edited.bone_names(), original.bone_names());
        assert!(edited.bone_names().contains(&"forearmL"));
        let walk = edited
            .animations
            .iter()
            .position(|clip| clip.name == "Walk")
            .unwrap();
        let base_pose = original.pose(Some(walk), 0.36).unwrap();
        let edited_pose = edited.pose(Some(walk), 0.36).unwrap();
        assert!(
            base_pose[0]
                .iter()
                .zip(&edited_pose[0])
                .any(|(a, b)| a.position != b.position)
        );

        let offset = BoneRotation {
            bone: "head".into(),
            degrees: [0.0, 26.0, 0.0],
        };
        let pose = edited.pose_with_overrides(None, 0.0, &[offset]).unwrap();
        assert!(pose[0].iter().zip(&after.vertices).any(|(a, b)| {
            b.position[1] > 1.58
                && (Vec3::from_array(a.position) - Vec3::from_array(b.position)).length() > 0.012
        }));
    }

    #[test]
    fn old_style_json_uses_neutral_defaults_for_the_new_options() {
        let style: HumanStyle = serde_json::from_str(r#"{"face_width":0.25}"#).unwrap();
        assert_eq!(style.face_width, 0.25);
        assert_eq!(style.hairstyle, HairStyle::Original);
        assert_eq!(style.garment, GarmentStyle::Original);
        assert_eq!([style.eye_scale, style.illustration], [0.0; 2]);
        assert_eq!(
            [style.smile, style.brow_raise, style.mouth_open, style.blink],
            [0.0; 4]
        );
        style.validate().unwrap();
        assert!(
            HumanStyle {
                blink: -0.2,
                ..style
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn anime_style_changes_eye_geometry_and_uses_illustrated_materials() {
        let base = decode_glb(include_bytes!("../demo_assets/emma.glb")).unwrap();
        let style = HumanStyle::anime();
        style.validate().unwrap();
        let saved = serde_json::to_vec(&style).unwrap();
        let loaded: HumanStyle = serde_json::from_slice(&saved).unwrap();
        assert_eq!(style, loaded);
        let neutral_eyes = base.customize_human(&HumanStyle { eye_scale: 0.0, ..style });
        let styled = base.customize_human(&loaded);
        let max_eye_change = neutral_eyes.primitives.iter().zip(&styled.primitives)
            .filter(|(_, after)| after.region == ModelRegion::StudioEyes)
            .flat_map(|(before, after)| before.vertices.iter().zip(&after.vertices))
            .map(|(before, after)| (before.position[1] - after.position[1]).abs())
            .fold(0.0, f32::max);
        assert!(max_eye_change > 0.002, "largest eye change was {max_eye_change}");
        assert!(styled.primitives.iter().all(|part| part.material.illustration == style.illustration));
        assert_eq!(base.animations.len(), styled.animations.len());
        let walk = crate::avatar::Avatar::new(crate::assets::Source::Emma, "Альта".into(), &styled)
            .walk
            .unwrap();
        assert!(styled.pose(Some(walk), 0.3).is_some());
    }

    #[test]
    fn maya_styles_move_ponytail_hem_and_expression_and_preserve_motion() {
        let original = decode_glb(include_bytes!("../demo_assets/woman.glb")).unwrap();
        let Some(HumanProfile::Maya(frame)) = original.human_profile.as_deref() else {
            panic!("bundled Maya materials were not classified");
        };
        assert!((1.54..1.60).contains(&frame.eye_y));
        assert!((0.08..0.15).contains(&frame.eye_z));
        let style = HumanStyle {
            garment: GarmentStyle::LongCoat,
            hairstyle: HairStyle::Swept,
            smile: 1.0,
            brow_raise: 0.8,
            mouth_open: 0.65,
            blink: 0.55,
            jacket_tint: [0.25, 0.67, 1.0],
            trouser_tint: [0.83, 0.46, 0.22],
            ..HumanStyle::default()
        };
        let styled = original.customize_human(&style);
        let moved = |region, threshold| {
            original
                .primitives
                .iter()
                .zip(&styled.primitives)
                .filter(|(mesh, _)| mesh.region == region)
                .flat_map(|(mesh, changed)| mesh.vertices.iter().zip(&changed.vertices))
                .filter(|(base, new)| {
                    (Vec3::from_array(base.position) - Vec3::from_array(new.position)).length()
                        > threshold
                })
                .count()
        };
        assert!(moved(ModelRegion::MayaPonytail, 0.012) > 15);
        assert!(moved(ModelRegion::MayaTop, 0.045) > 15);
        assert!(moved(ModelRegion::MayaEyes, 0.002) > 3);

        for (source, edited) in original.primitives.iter().zip(&styled.primitives) {
            if matches!(
                source.region,
                ModelRegion::MayaTop | ModelRegion::MayaTrousers
            ) {
                assert_ne!(
                    source.material.base_color_texture,
                    edited.material.base_color_texture
                );
                let base = &original.images[source.material.base_color_texture.unwrap()].pixels;
                let variant = &styled.images[edited.material.base_color_texture.unwrap()].pixels;
                assert_ne!(base, variant);
            } else {
                assert_eq!(
                    source.material.base_color_texture,
                    edited.material.base_color_texture
                );
            }
            assert_eq!(
                source.material.normal_texture,
                edited.material.normal_texture
            );
        }
        let walk = styled
            .animations
            .iter()
            .position(|clip| clip.name == "Walk_Relaxed")
            .unwrap();
        assert_eq!(original.bone_names(), styled.bone_names());
        let first = styled.pose(Some(walk), 0.29).unwrap();
        let second = styled.pose(Some(walk), 0.77).unwrap();
        assert!(
            first
                .iter()
                .zip(&second)
                .any(|(a, b)| a.iter().zip(b).any(|(a, b)| a.position != b.position))
        );
    }

    #[test]
    fn maya_secondary_pose_moves_eyelids_and_ponytail_on_the_live_rig() {
        let maya = decode_glb(include_bytes!("../demo_assets/woman.glb")).unwrap();
        assert!(maya.has_secondary_pose());
        let blink = maya
            .pose_with_secondary(
                None,
                0.0,
                &[],
                SecondaryPose {
                    blink: 1.0,
                    ..SecondaryPose::default()
                },
            )
            .unwrap();
        let sway = maya
            .pose_with_secondary(
                None,
                0.0,
                &[],
                SecondaryPose {
                    hair_sway: [0.85, 0.55],
                    ..SecondaryPose::default()
                },
            )
            .unwrap();
        let moved = |region, deformed: &[Vec<Vertex>], threshold: f32| {
            maya.primitives
                .iter()
                .zip(deformed)
                .filter(|(mesh, _)| mesh.region == region)
                .flat_map(|(mesh, deformed)| mesh.vertices.iter().zip(deformed))
                .filter(|(vertex, deformed)| {
                    (Vec3::from_array(vertex.position) - Vec3::from_array(deformed.position))
                        .length()
                        > threshold
                })
                .count()
        };
        assert!(moved(ModelRegion::MayaFace, &blink, 0.001) > 12);
        assert!(moved(ModelRegion::MayaLashes, &blink, 0.001) > 6);
        assert_eq!(moved(ModelRegion::MayaEyes, &blink, 0.00001), 0);
        assert!(moved(ModelRegion::MayaPonytail, &sway, 0.012) > 15);
        assert_eq!(moved(ModelRegion::MayaHair, &sway, 0.00001), 0);
        assert_eq!(SecondaryPose::natural(0.0, 0.0), SecondaryPose::default());
        assert!(SecondaryPose::natural(1.11, 0.0).blink > 0.9);
    }

    #[test]
    fn spring_motion_deforms_clothes_on_live_skin_without_moving_face() {
        let maya = decode_glb(include_bytes!("../demo_assets/woman.glb")).unwrap();
        let mut state = crate::secondary_motion::State::default();
        state.advance(0.016, Vec3::ZERO, 0.0);
        for frame in 1..=12 {
            state.advance(0.016, Vec3::X * (frame as f32 * 0.29), 0.0);
        }
        let animated = maya
            .pose_with_secondary(None, 0.0, &[], state.pose(0.0))
            .unwrap();
        let moved = |region, threshold| {
            maya.primitives
                .iter()
                .zip(&animated)
                .filter(|(mesh, _)| mesh.region == region)
                .flat_map(|(mesh, vertices)| mesh.vertices.iter().zip(vertices))
                .filter(|(before, after)| {
                    (Vec3::from_array(before.position) - Vec3::from_array(after.position)).length()
                        > threshold
                })
                .count()
        };
        assert!(moved(ModelRegion::MayaTop, 0.005) > 16);
        assert!(moved(ModelRegion::MayaTrousers, 0.004) > 16);
        assert!(moved(ModelRegion::MayaPonytail, 0.007) > 16);
        assert_eq!(moved(ModelRegion::MayaFace, 0.00001), 0);
    }

    #[test]
    fn studio_people_support_separate_wardrobes_and_physical_surface_motion() {
        for (name, bytes) in [
            ("Emma", include_bytes!("../demo_assets/emma.glb").as_slice()),
            ("Ryan", include_bytes!("../demo_assets/ryan.glb").as_slice()),
        ] {
            let actor = decode_glb(bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
            let profile = actor.human_profile.as_deref().unwrap();
            assert!(matches!(profile, HumanProfile::Studio(_)));
            assert!(actor.has_secondary_pose());
            let styled = actor.customize_human(&HumanStyle {
                jacket_tint: [0.62, 0.83, 0.26],
                trouser_tint: [0.20, 0.70, 0.84],
                nose_length: 0.55,
                hairstyle: HairStyle::Full,
                ..HumanStyle::default()
            });
            for slot in [ModelRegion::StudioTop, ModelRegion::StudioTrousers] {
                assert!(
                    actor
                        .primitives
                        .iter()
                        .zip(&styled.primitives)
                        .any(|(a, b)| a.region == slot
                            && (a.material.base_color_texture != b.material.base_color_texture
                                || a.material.base_color != b.material.base_color)),
                    "{name} wardrobe {slot:?}"
                );
            }
            let garment_texture = |model: &Model, region| {
                model
                    .primitives
                    .iter()
                    .find(|mesh| mesh.region == region)
                    .unwrap()
                    .material
                    .base_color_texture
            };
            assert_eq!(
                garment_texture(&styled, ModelRegion::StudioTop),
                garment_texture(&styled, ModelRegion::StudioTrousers),
                "{name} shared wardrobe atlas"
            );
            let pose = styled
                .pose_with_secondary(
                    None,
                    0.0,
                    &[],
                    super::super::SecondaryPose {
                        blink: 0.9,
                        hair_sway: [0.75, 0.35],
                        hair_lift: -0.40,
                        garment_sway: [0.61, -0.52, 0.48],
                        ..Default::default()
                    },
                )
                .unwrap();
            let mut pinned_seam = 0;
            let mut pinned_nape = 0;
            for ((source, edited), animated) in
                actor.primitives.iter().zip(&styled.primitives).zip(&pose)
            {
                for ((source_vertex, edited_vertex), animated_vertex) in
                    source.vertices.iter().zip(&edited.vertices).zip(animated)
                {
                    if profile.pinned_garment_vertex(source.region, source_vertex.position) {
                        pinned_seam += 1;
                        assert_eq!(source_vertex.position, edited_vertex.position);
                    } else if profile.is_nape_skin(source.region, source_vertex.position) {
                        pinned_nape += 1;
                    } else {
                        continue;
                    }
                    let distance = Vec3::from_array(edited_vertex.position)
                        .distance(Vec3::from_array(animated_vertex.position));
                    assert!(
                        distance < 1e-5,
                        "{name} protected vertex moved {distance} m"
                    );
                }
            }
            assert!(pinned_seam > 10, "{name} has no pinned garment seam");
            if name == "Emma" {
                assert!(pinned_nape > 5, "{name} has no pinned skin at the nape");
            }
            let moves = |regions: &[ModelRegion], threshold| {
                styled
                    .primitives
                    .iter()
                    .zip(&pose)
                    .filter(|(mesh, _)| regions.contains(&mesh.region))
                    .flat_map(|(mesh, output)| mesh.vertices.iter().zip(output))
                    .filter(|(a, b)| {
                        (Vec3::from_array(a.position) - Vec3::from_array(b.position)).length()
                            > threshold
                    })
                    .count()
            };
            assert!(
                moves(
                    &[ModelRegion::StudioTop, ModelRegion::StudioTrousers],
                    0.005
                ) > 8,
                "{name} cloth"
            );
            assert!(
                moves(
                    &[ModelRegion::StudioHair, ModelRegion::StudioHairTip],
                    0.0015
                ) > 8,
                "{name} hair"
            );
            assert!(
                moves(&[ModelRegion::StudioFace, ModelRegion::StudioLashes], 0.001) > 4,
                "{name} eyelids"
            );
            assert_eq!(moves(&[ModelRegion::StudioEyes], 0.0001), 0);
        }
    }
}
