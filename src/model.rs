use std::collections::{BTreeSet, HashMap};
use std::io::Cursor;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{EulerRot, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
use gltf::accessor::{DataType, Dimensions};
use gltf::animation::{Interpolation, Property, util::ReadOutputs};
use image::{ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};

use crate::ImportError;

#[path = "foot_lock.rs"]
mod foot_lock;
#[path = "quadruped_ground.rs"]
mod quadruped_ground;
pub(crate) use quadruped_ground::{GroundHit, PawContact, PawLock};
#[path = "human.rs"]
mod human;
#[path = "model_clips.rs"]
mod model_clips;
#[path = "model_morphs.rs"]
mod model_morphs;
#[path = "model_parts.rs"]
mod model_parts;
#[path = "model_look.rs"]
mod model_look;
#[path = "model_hand_ik.rs"]
mod model_hand_ik;
pub(crate) use model_hand_ik::ArmPose;
#[path = "model_hair_contact.rs"]
mod model_hair_contact;
pub use model_hair_contact::HairContacts;
use model_hair_contact::{HairColliders, HairRig};
pub use foot_lock::{FootLock, FootLockInput};
pub use human::{GarmentStyle, HairStyle, HumanStyle};
use human::{HumanProfile, ModelRegion};
pub use model_clips::MAX_IMPORTED_CLIPS;
pub use model_morphs::MorphWeight;
pub(crate) use model_morphs::{MAX_TARGETS as MAX_MORPH_TARGETS, OVERRIDE_RANGE as MORPH_OVERRIDE_RANGE};
pub use model_parts::PartSlot;
pub use model_look::CharacterLook;

pub const MAX_GLB_BYTES: usize = 64 * 1024 * 1024;
const MAX_VERTICES: usize = 600_000;
const MAX_INDICES: usize = 1_800_000;
const MAX_PRIMITIVES: usize = 256;
const MAX_NODES: usize = 4_096;
const MAX_TEXTURE_SIZE: u32 = 4_096;
const MAX_TEXTURE_BYTES: usize = 192 * 1024 * 1024;
const MAX_JOINTS: usize = 256;
const MAX_SKINS: usize = 64;
const MAX_ANIMATIONS: usize = 128;
const MAX_ANIMATION_CHANNELS: usize = 2_048;
const MAX_ANIMATION_SAMPLES: usize = 300_000;

#[derive(Clone, Debug)]
pub struct Model {
    pub primitives: Vec<Primitive>,
    pub images: Vec<Image>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    pub animations: Vec<Animation>,
    morph_names: Vec<String>,
    pub(crate) expression_channels: crate::actor_expression::Channels,
    rigid_pose_targets: Vec<usize>,
    rig: Option<Arc<Rig>>,
    human_profile: Option<Arc<HumanProfile>>,
    secondary_deltas: Option<Arc<[Vec<SecondaryDelta>]>>,
    hair_rig: Option<Arc<HairRig>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SecondaryPose {
    pub blink: f32,
    pub hair_sway: [f32; 2],
    pub hair_lift: f32,
    pub hair_follow: Option<HairFollow>,
    pub garment_sway: [f32; 3],
    pub hair_contacts: HairContacts,
}

/// Additional deflection of the free end and the individually authored short strands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HairFollow {
    pub tail_end: [f32; 3],
    pub loose_strands: [f32; 3],
}

impl SecondaryPose {
    pub fn natural(seconds: f32, stride: f32) -> Self {
        if !seconds.is_finite() || !stride.is_finite() {
            return Self::default();
        }
        let time = seconds.rem_euclid(12.7);
        let close = |start: f32, duration: f32| {
            let phase = (time - start) / duration;
            let eased = |value: f32| {
                let t = value.clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            eased(phase / 0.35) * (1.0 - eased((phase - 0.45) / 0.55))
        };
        let blink = close(1.05, 0.145)
            .max(close(4.71, 0.155))
            .max(close(8.20, 0.145))
            .max(close(8.47, 0.13))
            .max(close(11.91, 0.15));
        let stride = stride.clamp(0.0, 1.0);
        let phase = f64::from(seconds) * std::f64::consts::TAU;
        let side = (phase * 1.14 - 0.62).sin() as f32;
        let backward = (phase * 2.28 - 1.2).sin() as f32;
        Self {
            blink,
            hair_sway: [side * stride, (0.38 + 0.62 * backward) * stride],
            ..Self::default()
        }
    }

    pub fn active(self) -> bool {
        self != Self::default()
    }
}

#[derive(Clone, Debug)]
struct SecondaryDelta {
    vertex: usize,
    kind: SecondaryDeltaKind,
}

#[derive(Clone, Debug)]
enum SecondaryDeltaKind {
    Blink(Vec3),
    Hair { axes: [Vec3; 3], motion: HairMotion },
    Garment([Vec3; 3]),
}

#[derive(Clone, Copy, Debug)]
enum HairMotion {
    Root,
    Tail { end_weight: f32 },
    LooseStrand,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoneRotation {
    pub bone: String,
    pub degrees: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoneTranslation {
    pub bone: String,
    pub translation: [f32; 3],
}

impl Model {
    pub fn pose(&self, clip: Option<usize>, elapsed: f32) -> Option<Vec<Vec<Vertex>>> {
        self.pose_with_overrides(clip, elapsed, &[])
    }

    pub fn bone_names(&self) -> Vec<&str> {
        let mut names = Vec::new();
        if let Some(rig) = &self.rig {
            for skin in &rig.skins {
                for &(node, _) in &skin.joints {
                    if let Some(name) = rig.nodes[node].name.as_deref()
                        && !names.contains(&name)
                    {
                        names.push(name);
                    }
                }
            }
            names.extend(self.rigid_pose_targets.iter().filter_map(|&index| rig.nodes[index].name.as_deref()));
        }
        names
    }

    fn pose_target(&self, rig: &Rig, name: &str) -> Option<usize> {
        if !rig.skins.is_empty() {
            return rig.skins.iter().flat_map(|skin| skin.joints.iter().map(|joint| joint.0))
                .find(|&index| rig.nodes[index].name.as_deref() == Some(name));
        }
        self.rigid_pose_targets.iter().copied()
            .find(|&index| rig.nodes[index].name.as_deref() == Some(name))
    }

    pub(super) fn refresh_rigid_pose_targets(&mut self) {
        self.rigid_pose_targets.clear();
        let Some(rig) = self.rig.as_deref().filter(|rig| rig.skins.is_empty()) else { return };
        let mut name_counts = HashMap::new();
        for name in rig.nodes.iter().filter_map(|node| node.name.as_deref()) {
            *name_counts.entry(name).or_insert(0) += 1;
        }
        // Use authored articulation, excluding duplicate or static names that a pose cannot target reliably.
        let targets: BTreeSet<usize> = self.animations.iter().flat_map(|clip| &clip.channels)
            .map(|track| track.node)
            .filter(|&index| {
                let node = &rig.nodes[index];
                matches!(node.local, LocalTransform::Trs { .. })
                    && node.name.as_deref().is_some_and(|name| name_counts.get(name) == Some(&1))
            }).collect();
        self.rigid_pose_targets.extend(targets);
    }

    /// Rotation taking a pose target's rest-local axes into the imported model's axes.
    pub fn bone_rest_rotation(&self, name: &str) -> Option<Quat> {
        let rig = self.rig.as_ref()?;
        let index = self.pose_target(rig, name)?;
        let node = &rig.nodes[index];
        if !matches!(node.local, LocalTransform::Trs { .. }) {
            return None;
        }
        let (_, rotation, _) = node.rest.to_scale_rotation_translation();
        rotation.is_finite().then(|| rotation.normalize())
    }

    pub fn pose_with_overrides(
        &self,
        clip: Option<usize>,
        elapsed: f32,
        offsets: &[BoneRotation],
    ) -> Option<Vec<Vec<Vertex>>> {
        self.pose_with_secondary(clip, elapsed, offsets, SecondaryPose::default())
    }

    pub fn has_secondary_pose(&self) -> bool {
        self.secondary_deltas.is_some()
    }

    pub fn pose_with_secondary(
        &self,
        clip: Option<usize>,
        elapsed: f32,
        offsets: &[BoneRotation],
        secondary: SecondaryPose,
    ) -> Option<Vec<Vec<Vertex>>> {
        self.pose_blended(clip, elapsed, None, offsets, secondary)
    }

    pub fn pose_blended(
        &self,
        clip: Option<usize>,
        elapsed: f32,
        previous: Option<(Option<usize>, f32, f32)>,
        offsets: &[BoneRotation],
        secondary: SecondaryPose,
    ) -> Option<Vec<Vec<Vertex>>> {
        self.pose_blended_with_translations(clip, elapsed, previous, offsets, secondary, &[])
    }

    pub fn pose_blended_with_translations(
        &self,
        clip: Option<usize>,
        elapsed: f32,
        previous: Option<(Option<usize>, f32, f32)>,
        offsets: &[BoneRotation],
        secondary: SecondaryPose,
        translations: &[BoneTranslation],
    ) -> Option<Vec<Vec<Vertex>>> {
        self.pose_blended_with_morphs(
            clip,
            elapsed,
            previous,
            offsets,
            secondary,
            translations,
            &[],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn pose_blended_with_morphs(
        &self,
        clip: Option<usize>,
        elapsed: f32,
        previous: Option<(Option<usize>, f32, f32)>,
        offsets: &[BoneRotation],
        mut secondary: SecondaryPose,
        translations: &[BoneTranslation],
        morphs: &[MorphWeight],
    ) -> Option<Vec<Vec<Vertex>>> {
        let rig = self.rig.as_ref()?;
        if morphs.iter().any(|weight| {
            weight.target.eq_ignore_ascii_case("eyeBlinkLeft")
                || weight.target.eq_ignore_ascii_case("eyeBlinkRight")
        }) {
            secondary.blink = 0.0;
        }
        let animation = clip.and_then(|index| self.animations.get(index));
        let previous = previous.filter(|&(_, _, weight)| weight.is_finite() && weight < 1.0);
        let deltas = self
            .secondary_deltas
            .as_ref()
            .filter(|_| secondary.active());
        if animation.is_none()
            && previous.is_none()
            && offsets.is_empty()
            && deltas.is_none()
            && translations.is_empty()
            && morphs.is_empty()
        {
            return Some(
                self.primitives
                    .iter()
                    .map(|mesh| mesh.vertices.clone())
                    .collect(),
            );
        }
        let mut transforms = self.blended_locals(rig, animation, elapsed, previous, offsets);
        add_bone_translations(rig, &mut transforms, translations);
        let globals = model_transforms(rig, &transforms);
        let joint_matrices = joint_matrices(&rig.skins, &globals);
        let hair = self.hair_rig.as_ref().map_or_else(HairColliders::default, |guard| guard.pose(&globals, secondary.hair_contacts));
        Some(
            rig.meshes
                .iter()
                .zip(&self.primitives)
                .enumerate()
                .map(|(index, (mesh, primitive))| {
                    let mesh_deltas = deltas.map_or(&[][..], |deltas| deltas[index].as_slice());
                    mesh.posed_deformed_vertices(
                        &globals,
                        &joint_matrices,
                        mesh_deltas,
                        secondary,
                        morphs,
                        hair,
                    )
                    .zip(&primitive.vertices)
                    .map(|(vertex, rest)| {
                        if valid_position(vertex.position) {
                            vertex
                        } else {
                            *rest
                        }
                    })
                    .collect()
                })
                .collect(),
        )
    }

    pub(crate) fn rigid_boot_contacts(
        &self,
        clip: Option<usize>,
        elapsed: f32,
        previous: Option<(Option<usize>, f32, f32)>,
        offsets: &[BoneRotation],
    ) -> Option<[Vec3; 2]> {
        let rig = self.rig.as_ref().filter(|rig| rig.skins.is_empty())?;
        let boot = |name| rig.nodes.iter().position(|node| node.name.as_deref() == Some(name));
        let boots = [boot("Boot.L")?, boot("Boot.R")?];
        let current = clip.and_then(|index| self.animations.get(index));
        let globals = model_transforms(rig, &self.blended_locals(rig, current, elapsed, previous, offsets));
        let height = self.bounds_max[1] - self.bounds_min[1];
        // Estimate the point under the middle of each rigid boot in the neutral model.
        Some(boots.map(|index| {
            let rest = rig.nodes[index].rest;
            let origin = rest.transform_point3(Vec3::ZERO);
            let sole = Vec3::new(origin.x, self.bounds_min[1], origin.z + height * 0.036);
            globals[index].transform_point3(rest.inverse().transform_point3(sole))
        }))
    }

    fn blended_locals(
        &self,
        rig: &Rig,
        animation: Option<&Animation>,
        elapsed: f32,
        previous: Option<(Option<usize>, f32, f32)>,
        offsets: &[BoneRotation],
    ) -> Vec<LocalTransform> {
        let mut transforms = animation_pose(rig, animation, elapsed);
        let previous = previous.filter(|&(_, _, weight)| weight.is_finite() && weight < 1.0);
        if let Some((clip, elapsed, weight)) = previous {
            let source = animation_pose(
                rig,
                clip.and_then(|index| self.animations.get(index)),
                elapsed,
            );
            let weight = weight.max(0.0);
            for (current, previous) in transforms.iter_mut().zip(source) {
                *current = previous.interpolate(*current, weight);
            }
        }
        for offset in offsets {
            if !offset.degrees.into_iter().all(f32::is_finite) {
                continue;
            }
            if let Some(index) = self.pose_target(rig, &offset.bone)
                && let LocalTransform::Trs { rotation, .. } = &mut transforms[index]
            {
                let [x, y, z] = offset
                    .degrees
                    .map(|angle| angle.rem_euclid(360.0).to_radians());
                *rotation = (*rotation * Quat::from_euler(EulerRot::XYZ, x, y, z)).normalize();
            }
        }
        transforms
    }
}

fn model_transforms(rig: &Rig, transforms: &[LocalTransform]) -> Vec<Mat4> {
    let mut globals = Vec::with_capacity(transforms.len());
    for (&local, node) in transforms.iter().zip(&rig.nodes) {
        let parent = node.parent.map_or(Mat4::IDENTITY, |index| globals[index]);
        let global = parent * local.matrix();
        globals.push(if valid_affine(global) {
            global
        } else {
            node.rest
        });
    }
    globals
}

fn add_bone_translations(
    rig: &Rig,
    transforms: &mut [LocalTransform],
    translations: &[BoneTranslation],
) {
    for adjustment in translations {
        let delta = Vec3::from_array(adjustment.translation);
        if !delta.is_finite() {
            continue;
        }
        let joint = rig
            .skins
            .iter()
            .flat_map(|skin| skin.joints.iter().map(|joint| joint.0))
            .find(|&index| rig.nodes[index].name.as_deref() == Some(adjustment.bone.as_str()));
        let node = if rig.skins.is_empty() {
            rig.nodes.iter().position(|node| node.name.as_deref() == Some(adjustment.bone.as_str()))
        } else {
            joint
        };
        if let Some(index) = node
            && let LocalTransform::Trs { translation, .. } = &mut transforms[index]
        {
            *translation += delta;
        }
    }
}

fn animation_pose(rig: &Rig, animation: Option<&Animation>, elapsed: f32) -> Vec<LocalTransform> {
    let mut transforms: Vec<_> = rig.nodes.iter().map(|node| node.local).collect();
    if let Some(animation) = animation {
        let time = if elapsed.is_finite() && animation.duration > 0.0 {
            elapsed.rem_euclid(animation.duration)
        } else {
            0.0
        };
        for channel in &animation.channels {
            channel.apply(time, &mut transforms[channel.node]);
        }
    }
    transforms
}

#[derive(Clone, Debug)]
pub struct Animation {
    pub name: String,
    pub duration: f32,
    channels: Vec<Channel>,
}

#[derive(Clone, Debug)]
pub struct Primitive {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub material: Material,
    material_name: Option<Arc<str>>,
    region: ModelRegion,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tangent: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub base_color: [f32; 4],
    pub base_color_texture: Option<usize>,
    pub metallic_roughness_texture: Option<usize>,
    pub normal_texture: Option<usize>,
    pub emissive_texture: Option<usize>,
    pub emissive: [f32; 3],
    pub alpha_cutoff: Option<f32>,
    pub alpha_blend: bool,
    pub facial_decal: bool,
    pub metallic: f32,
    pub roughness: f32,
    pub normal_scale: f32,
    pub illustration: f32,
    pub guard_fur: bool,
    pub double_sided: bool,
}

#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
}

pub fn decode_glb(bytes: &[u8]) -> Result<Model, ImportError> {
    decode_glb_inner(bytes).map_err(|error| ImportError::new("Модель GLB", error))
}

fn decode_glb_inner(bytes: &[u8]) -> Result<Model, String> {
    if bytes.len() > MAX_GLB_BYTES {
        return Err("Для импорта нужен GLB размером до 64 МБ".into());
    }
    if !bytes.starts_with(b"glTF") {
        return Err("Выбери двоичный файл GLB, созданный в формате glTF 2.0".into());
    }
    let declared = bytes
        .get(8..12)
        .and_then(|size| size.try_into().ok())
        .map(u32::from_le_bytes);
    if declared.is_none_or(|size| size < 12 || size as usize > bytes.len()) {
        return Err("Повреждён двоичный контейнер GLB: неверный размер в заголовке.".into());
    }

    let gltf::Gltf { document, blob } =
        gltf::Gltf::from_slice_without_validation(bytes).map_err(describe_glb_error)?;
    validate_glb_references(document.as_json())?;
    let document = gltf::Document::from_json(document.into_json()).map_err(describe_glb_error)?;
    let document = &document;
    let buffer = document.buffers().next().ok_or("В GLB нет геометрии")?;
    if document.buffers().len() != 1 || !matches!(buffer.source(), gltf::buffer::Source::Bin) {
        return Err("Сохрани модель и её геометрию в одном двоичном GLB".into());
    }
    let blob = blob
        .as_deref()
        .and_then(|blob| blob.get(..buffer.length()))
        .ok_or("Двоичный буфер GLB отсутствует или обрезан")?;
    validate_ranges(document, blob)?;
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .ok_or("В GLB нет сцены")?;

    let SceneNodes {
        source: source_nodes,
        rig: nodes,
        lookup: node_indices,
    } = read_scene_nodes(scene, document.nodes().len())?;
    let animations = read_animations(document, blob, &nodes, &node_indices)?;
    let mut skins = Vec::new();
    if document.skins().len() > MAX_NODES {
        return Err("В GLB слишком много скелетов".into());
    }
    let mut skin_indices = vec![None; document.skins().len()];
    for node in &source_nodes {
        if let Some(skin) = node.skin() {
            let index = skin.index();
            if skin_indices[index].is_none() {
                if skins.len() >= MAX_SKINS {
                    return Err("В сцене GLB слишком много скелетов".into());
                }
                skin_indices[index] = Some(skins.len());
                skins.push(read_skin(skin, blob, &node_indices)?);
            }
        }
    }
    let mut morph_reader = model_morphs::MorphReader::new(bytes, blob, document)?;
    let rest: Vec<_> = nodes.iter().map(|node| node.rest).collect();
    let rest_joints = joint_matrices(&skins, &rest);
    let mut rest_meshes = Vec::new();
    let mut textures = Textures {
        is_vrm: document
            .extensions_used()
            .any(|extension| matches!(extension, "VRM" | "VRMC_vrm")),
        ..Textures::default()
    };
    let mut model = Model {
        primitives: Vec::new(),
        images: Vec::new(),
        bounds_min: [f32::INFINITY; 3],
        bounds_max: [f32::NEG_INFINITY; 3],
        animations,
        morph_names: Vec::new(),
        expression_channels: crate::actor_expression::Channels::default(),
        rigid_pose_targets: Vec::new(),
        rig: None,
        human_profile: None,
        secondary_deltas: None,
        hair_rig: None,
    };
    let mut remaining = GeometryBudget {
        vertices: MAX_VERTICES,
        indices: MAX_INDICES,
    };
    for (node_index, node) in source_nodes.into_iter().enumerate() {
        let Some(mesh) = node.mesh() else { continue };
        let skin = node.skin().and_then(|skin| skin_indices[skin.index()]);

        for source in mesh.primitives() {
            if model.primitives.len() >= MAX_PRIMITIVES {
                return Err("В GLB слишком много частей меша".into());
            }
            let mut primitive = decode_primitive(&source, blob, &mut textures, &mut remaining)
                .map_err(|error| {
                    format!("Меш {}, часть {}: {error}", mesh.index() + 1, source.index() + 1)
                })?;
            let influences = match skin {
                Some(index) => read_influences(
                    &source,
                    blob,
                    primitive.vertices.len(),
                    skins[index].joints.len(),
                )?,
                None => Vec::new(),
            };
            let morphs = morph_reader.read(&node, &mesh, &source, primitive.vertices.len())?;
            let rest_mesh = RestMesh {
                node: node_index,
                skin,
                influences,
                vertices: primitive.vertices,
                morphs,
            };
            let rest_transform = rest_mesh.rest_orientation(&rest, &rest_joints);
            if rest_transform < 0.0 {
                for triangle in primitive.indices.as_chunks_mut::<3>().0 {
                    triangle.swap(1, 2);
                }
            }
            primitive.vertices = rest_mesh.posed_vertices(&rest, &rest_joints).collect();
            rest_meshes.push(rest_mesh);
            for vertex in &primitive.vertices {
                if !valid_position(vertex.position) {
                    return Err("Координаты меша выходят за допустимый диапазон".into());
                }
                for (axis, position) in vertex.position.into_iter().enumerate() {
                    model.bounds_min[axis] = model.bounds_min[axis].min(position);
                    model.bounds_max[axis] = model.bounds_max[axis].max(position);
                }
            }
            model.primitives.push(primitive);
        }
    }

    if model.primitives.is_empty() {
        return Err("В выбранной сцене GLB нет треугольных мешей".into());
    }
    model.images = textures.images;
    if !model.animations.is_empty()
        || !skins.is_empty()
        || rest_meshes.iter().any(|mesh| !mesh.morphs.is_empty())
    {
        model.rig = Some(Arc::new(Rig {
            nodes,
            skins,
            meshes: rest_meshes,
        }));
    }
    model.refresh_morph_names()?;
    model.refresh_rigid_pose_targets();
    model.hair_rig = HairRig::prepare(&model).map(Arc::new);
    if document.as_json().asset.generator.as_deref() == Some("bake3d-rig")
        && let [mesh] = model.primitives.as_slice()
        && mesh.vertices.len() == 118_263
        && mesh.indices.len() == 450_000
        && mesh.material.base_color_texture.is_some()
    {
        let profile = HumanProfile::old_man();
        model.secondary_deltas = profile.prepare_secondary(&model);
        model.human_profile = Some(Arc::new(profile));
    } else if model
        .primitives
        .iter()
        .any(|mesh| mesh.region.studio_material())
    {
        let profile = HumanProfile::studio(&model);
        model.secondary_deltas = profile.prepare_secondary(&model);
        model.human_profile = Some(Arc::new(profile));
    } else if model
        .primitives
        .iter()
        .any(|mesh| mesh.region == ModelRegion::MayaTop)
        && model
            .primitives
            .iter()
            .any(|mesh| mesh.region == ModelRegion::MayaTrousers)
        && model.primitives.iter().any(|mesh| {
            matches!(
                mesh.region,
                ModelRegion::MayaHair | ModelRegion::MayaPonytail
            )
        })
    {
        let profile = HumanProfile::maya(&model);
        model.secondary_deltas = profile.prepare_secondary(&model);
        model.human_profile = Some(Arc::new(profile));
    }
    Ok(model)
}

fn validate_glb_references(root: &gltf::json::Root) -> Result<(), String> {
    use gltf::json::{mesh::Semantic, validation::Checked};

    // gltf проверяет границы массива POSITION уже после обращения по этому индексу.
    for (mesh_index, mesh) in root.meshes.iter().enumerate() {
        for (index, primitive) in mesh.primitives.iter().enumerate() {
            if primitive
                .attributes
                .get(&Checked::Valid(Semantic::Positions))
                .is_some_and(|accessor| accessor.value() >= root.accessors.len())
            {
                return Err(format!(
                    "Меш {}, часть {}: массив POSITION отсутствует.",
                    mesh_index + 1,
                    index + 1
                ));
            }
        }
    }
    // Валидатор gltf проверяет семплеры, но пропускает ссылки и свойства целей каналов.
    for (animation_index, animation) in root.animations.iter().enumerate() {
        for (index, channel) in animation.channels.iter().enumerate() {
            if channel.target.node.value() >= root.nodes.len()
                || matches!(channel.target.path, Checked::Invalid)
            {
                return Err(format!(
                    "Клип {}, дорожка {}: неизвестный узел или свойство цели.",
                    animation_index + 1,
                    index + 1
                ));
            }
        }
    }
    for (index, image) in root.images.iter().enumerate() {
        if (image.buffer_view.is_some() && image.mime_type.is_none())
            || (image.buffer_view.is_none() && image.uri.is_none())
        {
            return Err(format!(
                "Текстура {}: отсутствуют данные изображения или его формат.",
                index + 1
            ));
        }
    }
    Ok(())
}

fn describe_glb_error(error: gltf::Error) -> String {
    use gltf::json::validation::Error;
    match error {
        gltf::Error::Binary(error) => format!("Повреждён двоичный контейнер GLB ({error})."),
        gltf::Error::Deserialize(error) => format!(
            "Не читается описание GLB (JSON), строка {}, столбец {}.",
            error.line(),
            error.column()
        ),
        gltf::Error::Validation(errors) => {
            let details = errors.into_iter().take(8).map(|(path, error)| {
                let message = match error {
                    Error::IndexOutOfBounds => "ссылка указывает за пределы списка",
                    Error::Invalid => "неверное значение",
                    Error::Missing => "отсутствует обязательное поле",
                    Error::Oversize => "превышен допустимый размер",
                    Error::Unsupported => "расширение не поддерживается",
                };
                format!("{path} — {message}")
            });
            let mut message = String::from("Некорректный GLB");
            for detail in details {
                message.push_str(". ");
                message.push_str(&detail);
            }
            message
        }
        error => format!("Не удалось прочитать GLB ({error})."),
    }
}

fn valid_affine(transform: Mat4) -> bool {
    transform.is_finite()
        && transform.x_axis.w == 0.0
        && transform.y_axis.w == 0.0
        && transform.z_axis.w == 0.0
        && transform.w_axis.w == 1.0
}

fn valid_position(position: [f32; 3]) -> bool {
    let position = Vec3::from_array(position);
    position.is_finite() && position.abs().max_element() <= 1_000_000.0
}

#[derive(Clone, Copy, Debug)]
enum LocalTransform {
    Matrix(Mat4),
    Trs {
        translation: Vec3,
        rotation: Quat,
        scale: Vec3,
    },
}

impl LocalTransform {
    fn interpolate(self, other: Self, weight: f32) -> Self {
        match (self, other) {
            (
                Self::Trs {
                    translation: start,
                    rotation: from,
                    scale: at_start,
                },
                Self::Trs {
                    translation: end,
                    rotation: to,
                    scale: at_end,
                },
            ) => Self::Trs {
                translation: start.lerp(end, weight),
                rotation: from.slerp(to, weight).normalize(),
                scale: at_start.lerp(at_end, weight),
            },
            (_, unchanged) => unchanged,
        }
    }

    fn matrix(self) -> Mat4 {
        match self {
            Self::Matrix(matrix) => matrix,
            Self::Trs {
                translation,
                rotation,
                scale,
            } => Mat4::from_scale_rotation_translation(scale, rotation, translation),
        }
    }
}

#[derive(Clone, Debug)]
struct RigNode {
    local: LocalTransform,
    parent: Option<usize>,
    rest: Mat4,
    name: Option<String>,
}

struct SceneNodes<'a> {
    source: Vec<gltf::Node<'a>>,
    rig: Vec<RigNode>,
    lookup: Vec<Option<usize>>,
}

fn read_scene_nodes(scene: gltf::Scene<'_>, node_count: usize) -> Result<SceneNodes<'_>, String> {
    use gltf::scene::Transform;
    if node_count > MAX_NODES {
        return Err("В сцене GLB слишком много узлов".into());
    }
    let mut source_nodes = Vec::new();
    let mut nodes: Vec<RigNode> = Vec::new();
    let mut lookup = vec![None; node_count];
    let mut pending: Vec<_> = scene.nodes().map(|node| (node, None::<usize>)).collect();
    pending.reverse();
    while let Some((node, parent)) = pending.pop() {
        let index = nodes.len();
        if lookup[node.index()].replace(index).is_some() {
            return Err("Узлы сцены GLB должны образовывать дерево".into());
        }
        let local = match node.transform() {
            Transform::Matrix { matrix } => {
                LocalTransform::Matrix(Mat4::from_cols_array_2d(&matrix))
            }
            Transform::Decomposed {
                translation,
                rotation,
                scale,
            } => LocalTransform::Trs {
                translation: Vec3::from_array(translation),
                rotation: unit_quaternion(Vec4::from_array(rotation))
                    .ok_or("Некорректный поворот узла GLB")?,
                scale: Vec3::from_array(scale),
            },
        };
        let rest = parent.map_or(Mat4::IDENTITY, |index| nodes[index].rest) * local.matrix();
        if !valid_affine(rest) {
            return Err("Недопустимое преобразование узла GLB".into());
        }
        let first = pending.len();
        pending.extend(node.children().map(|child| (child, Some(index))));
        pending[first..].reverse();
        nodes.push(RigNode {
            local,
            parent,
            rest,
            name: node.name().map(str::to_owned),
        });
        source_nodes.push(node);
    }
    Ok(SceneNodes {
        source: source_nodes,
        rig: nodes,
        lookup,
    })
}

fn unit_quaternion(value: Vec4) -> Option<Quat> {
    value
        .try_normalize()
        .map(|value| Quat::from_array(value.to_array()))
}

#[derive(Clone, Debug)]
struct Rig {
    nodes: Vec<RigNode>,
    skins: Vec<Skin>,
    meshes: Vec<RestMesh>,
}

#[derive(Clone, Debug)]
struct Skin {
    joints: Vec<(usize, Mat4)>,
}

fn read_skin(skin: gltf::Skin<'_>, blob: &[u8], lookup: &[Option<usize>]) -> Result<Skin, String> {
    let joints = skin.joints();
    if joints.len() == 0 || joints.len() > MAX_JOINTS {
        return Err("Скелет должен содержать от 1 до 256 костей".into());
    }
    if skin.inverse_bind_matrices().is_some_and(|accessor| {
        accessor.data_type() != DataType::F32 || accessor.dimensions() != Dimensions::Mat4
    }) {
        return Err("Матрицы привязки скелета GLB должны иметь формат MAT4/F32.".into());
    }
    let reader = skin.reader(|_| Some(blob));
    let matrices: Vec<_> = match reader.read_inverse_bind_matrices() {
        Some(matrices) if matrices.len() == joints.len() => matrices
            .map(|matrix| Mat4::from_cols_array_2d(&matrix))
            .collect(),
        None if skin.inverse_bind_matrices().is_none() => vec![Mat4::IDENTITY; joints.len()],
        _ => {
            return Err(
                "Число исходных преобразований скелета не совпадает с числом костей".into(),
            );
        }
    };
    let joints = joints
        .zip(matrices)
        .map(|(joint, inverse_bind)| {
            if !valid_affine(inverse_bind) {
                return Err("Некорректная матрица привязки кости".to_owned());
            }
            let index =
                lookup[joint.index()].ok_or("Кость скелета находится вне выбранной сцены")?;
            Ok((index, inverse_bind))
        })
        .collect::<Result<_, _>>()?;
    Ok(Skin { joints })
}

#[derive(Clone, Debug)]
struct Influence {
    joints: [u16; 4],
    weights: [f32; 4],
}

fn read_influences(
    primitive: &gltf::Primitive<'_>,
    blob: &[u8],
    vertex_count: usize,
    joint_count: usize,
) -> Result<Vec<Influence>, String> {
    if primitive.get(&gltf::Semantic::Joints(1)).is_some()
        || primitive.get(&gltf::Semantic::Weights(1)).is_some()
    {
        return Err("Поддерживается до четырёх костей на вершину".into());
    }
    validate_vertex_attribute(
        primitive,
        gltf::Semantic::Joints(0),
        Dimensions::Vec4,
        &[DataType::U8, DataType::U16],
    )?;
    validate_vertex_attribute(
        primitive,
        gltf::Semantic::Weights(0),
        Dimensions::Vec4,
        &[DataType::U8, DataType::U16, DataType::F32],
    )?;
    let reader = primitive.reader(|_| Some(blob));
    let joints = reader
        .read_joints(0)
        .ok_or("У меша отсутствуют индексы костей")?
        .into_u16();
    let weights = reader
        .read_weights(0)
        .ok_or("У меша отсутствуют веса костей")?
        .into_f32();
    if joints.len() != vertex_count || weights.len() != vertex_count {
        return Err("Число весов скелета не совпадает с числом вершин".into());
    }
    joints
        .zip(weights)
        .map(|(joints, mut weights)| {
            if weights.iter().zip(joints).any(|(&weight, joint)| {
                !(0.0..=1.0).contains(&weight)
                    || (weight > 0.0 && usize::from(joint) >= joint_count)
            }) {
                return Err("Некорректный вес или индекс кости".to_owned());
            }
            let total: f32 = weights.iter().sum();
            if total > 0.0 {
                weights.iter_mut().for_each(|weight| *weight /= total);
            }
            Ok(Influence { joints, weights })
        })
        .collect()
}

#[derive(Clone, Debug)]
struct RestMesh {
    node: usize,
    skin: Option<usize>,
    influences: Vec<Influence>,
    vertices: Vec<Vertex>,
    morphs: Vec<model_morphs::MorphTarget>,
}

#[derive(Clone, Copy)]
struct JointTransform {
    matrix: Mat4,
    normal: Mat3,
    orientation: f32,
}

impl JointTransform {
    fn new(matrix: Mat4) -> Self {
        let linear = Mat3::from_mat4(matrix);
        let determinant = linear.determinant();
        let inverse_transpose = if determinant.is_finite() && determinant != 0.0 {
            linear.inverse().transpose()
        } else {
            Mat3::IDENTITY
        };
        let normal = if inverse_transpose.is_finite() {
            inverse_transpose
        } else {
            Mat3::IDENTITY
        };
        let orientation = if determinant < 0.0 { -1.0 } else { 1.0 };
        Self {
            matrix,
            normal,
            orientation,
        }
    }
}

fn joint_matrices(skins: &[Skin], globals: &[Mat4]) -> Vec<Vec<JointTransform>> {
    skins
        .iter()
        .map(|skin| {
            skin.joints
                .iter()
                .map(|(node, inverse_bind)| JointTransform::new(globals[*node] * *inverse_bind))
                .collect()
        })
        .collect()
}

impl RestMesh {
    fn rest_orientation(&self, globals: &[Mat4], joints: &[Vec<JointTransform>]) -> f32 {
        self.skin.map_or_else(
            || JointTransform::new(globals[self.node]).orientation,
            |index| joints[index][0].orientation,
        )
    }

    fn posed_vertices<'a>(
        &'a self,
        globals: &'a [Mat4],
        poses: &'a [Vec<JointTransform>],
    ) -> impl Iterator<Item = Vertex> + 'a {
        self.posed_deformed_vertices(globals, poses, &[], SecondaryPose::default(), &[], HairColliders::default())
    }

    fn posed_deformed_vertices<'a>(
        &'a self,
        globals: &'a [Mat4],
        poses: &'a [Vec<JointTransform>],
        secondary_deltas: &'a [SecondaryDelta],
        secondary: SecondaryPose,
        morphs: &[MorphWeight],
        hair: HairColliders,
    ) -> impl Iterator<Item = Vertex> + 'a {
        let mesh_transform = JointTransform::new(globals[self.node]);
        let skeleton = self.skin.map(|index| poses[index].as_slice());
        let mut deltas = secondary_deltas.iter().peekable();
        let vertices = self.vertices_with_morphs(morphs);
        let hair_root = Vec3::new(secondary.hair_sway[0], secondary.hair_lift, secondary.hair_sway[1]);
        let (hair_end, loose_strands) = secondary.hair_follow.map_or(
            (hair_root, hair_root),
            |follow| (Vec3::from_array(follow.tail_end), Vec3::from_array(follow.loose_strands)),
        );
        (0..vertices.len()).map(move |index| {
            let vertex = vertices[index];
            let mut position = Vec3::from_array(vertex.position);
            let mut hair_push = Vec3::ZERO;
            if let Some(delta) = deltas.next_if(|delta| delta.vertex == index) {
                let displace = |[x, y, z]: [Vec3; 3], [a, b, c]: [f32; 3]| x * a + y * b + z * c;
                position += match delta.kind {
                    SecondaryDeltaKind::Blink(amount) => amount * secondary.blink,
                    SecondaryDeltaKind::Hair { axes, motion } => {
                        let drive = match motion {
                            HairMotion::Root => hair_root,
                            HairMotion::Tail { end_weight } => hair_root.lerp(hair_end, end_weight),
                            HairMotion::LooseStrand => loose_strands,
                        };
                        hair_push = displace(axes, drive.to_array());
                        hair_push
                    }
                    SecondaryDeltaKind::Garment(axes) => displace(axes, secondary.garment_sway),
                };
            }
            let normal = Vec3::from_array(vertex.normal);
            let tangent = Vec4::from_array(vertex.tangent).truncate();
            let mut blended_position = Vec3::ZERO;
            let mut blended_normal = Vec3::ZERO;
            let mut blended_tangent = Vec3::ZERO;
            let mut blended_orientation = 0.0;
            let constrain = hair.active() && hair_push.length_squared() > 1e-12;
            let mut push = Vec3::ZERO;

            if let Some(skeleton) = skeleton {
                let influence = &self.influences[index];
                for (&joint, &weight) in influence.joints.iter().zip(&influence.weights) {
                    if weight > 0.0 {
                        let pose = skeleton[usize::from(joint)];
                        blended_position += pose.matrix.transform_point3(position) * weight;
                        if constrain {
                            push += pose.matrix.transform_vector3(hair_push) * weight;
                        }
                        blended_normal += (pose.normal * normal) * weight;
                        blended_tangent += pose.matrix.transform_vector3(tangent) * weight;
                        blended_orientation += pose.orientation * weight;
                    }
                }
                if influence.weights.iter().any(|&weight| weight > 0.0) {
                    if constrain {
                        blended_position = hair.fit(blended_position - push, blended_position);
                    }
                    let normal = blended_normal.try_normalize().unwrap_or(normal).to_array();
                    let orientation = if blended_orientation < 0.0 { -1.0 } else { 1.0 };
                    return Vertex {
                        position: blended_position.to_array(),
                        normal,
                        tangent: tangent_basis(
                            normal,
                            blended_tangent,
                            vertex.tangent[3] * orientation,
                        ),
                        ..vertex
                    };
                }
            }

            let transformed_normal = (mesh_transform.normal * normal)
                .try_normalize()
                .unwrap_or(normal)
                .to_array();
            let posed = mesh_transform.matrix.transform_point3(position);
            let posed = if constrain {
                hair.fit(posed - mesh_transform.matrix.transform_vector3(hair_push), posed)
            } else { posed };
            Vertex {
                position: posed.to_array(),
                normal: transformed_normal,
                tangent: tangent_basis(
                    transformed_normal,
                    mesh_transform.matrix.transform_vector3(tangent),
                    mesh_transform.orientation * vertex.tangent[3],
                ),
                ..vertex
            }
        })
    }
}

#[derive(Clone, Debug)]
struct Channel {
    node: usize,
    property: Property,
    interpolation: Interpolation,
    times: Arc<[f32]>,
    values: Arc<[Vec4]>,
}

fn read_animations(
    document: &gltf::Document,
    blob: &[u8],
    nodes: &[RigNode],
    lookup: &[Option<usize>],
) -> Result<Vec<Animation>, String> {
    if document.animations().len() > MAX_ANIMATIONS {
        return Err("В GLB слишком много клипов анимации".into());
    }
    let mut animations = Vec::new();
    let mut remaining_channels = MAX_ANIMATION_CHANNELS;
    let mut remaining_samples = MAX_ANIMATION_SAMPLES;
    for animation in document.animations() {
        let mut channels = Vec::new();
        let mut duration = 0.0_f32;
        for channel in animation.channels() {
            let target = channel.target();
            let property = target.property();
            let Some(node) = lookup[target.node().index()] else {
                continue;
            };
            if property == Property::MorphTargetWeights {
                continue;
            }
            if matches!(nodes[node].local, LocalTransform::Matrix(_)) {
                return Err(
                    "Для анимации узла GLB нужны отдельные координаты, поворот и масштаб".into(),
                );
            }
            if remaining_channels == 0 {
                return Err("В GLB слишком много дорожек анимации".into());
            }
            remaining_channels -= 1;

            let sampler = channel.sampler();
            let input = sampler.input();
            let output = sampler.output();
            let valid_output = if property == Property::Rotation {
                output.dimensions() == Dimensions::Vec4 && output.data_type() != DataType::U32
            } else {
                output.dimensions() == Dimensions::Vec3 && output.data_type() == DataType::F32
            };
            if input.dimensions() != Dimensions::Scalar
                || input.data_type() != DataType::F32
                || !valid_output
            {
                return Err(format!(
                    "Клип {}, дорожка {}: недопустимый формат ключей.",
                    animation.index() + 1,
                    channel.index() + 1
                ));
            }
            let interpolation = sampler.interpolation();
            let reader = channel.reader(|_| Some(blob));
            let times = reader
                .read_inputs()
                .ok_or("В дорожке GLB отсутствует время ключей")?;
            let stride = if interpolation == Interpolation::CubicSpline {
                3
            } else {
                1
            };
            let sample_count = times
                .len()
                .checked_mul(stride)
                .filter(|&count| count <= remaining_samples)
                .ok_or("В GLB слишком много ключей анимации")?;
            remaining_samples -= sample_count;
            let times: Vec<_> = times.collect();
            let invalid_time = "Время ключей GLB должно строго возрастать";
            let &last_time = times.last().ok_or(invalid_time)?;
            if times.iter().any(|time| !time.is_finite() || *time < 0.0)
                || times.windows(2).any(|window| window[0] >= window[1])
            {
                return Err(invalid_time.into());
            }

            let values: Vec<_> = match reader
                .read_outputs()
                .ok_or("В дорожке GLB отсутствуют значения ключей")?
            {
                ReadOutputs::Translations(values) | ReadOutputs::Scales(values) => values
                    .take(sample_count + 1)
                    .map(|value| Vec3::from_array(value).extend(0.0))
                    .collect(),
                ReadOutputs::Rotations(values) => values
                    .into_f32()
                    .take(sample_count + 1)
                    .map(Vec4::from_array)
                    .collect(),
                ReadOutputs::MorphTargetWeights(_) => continue,
            };
            if values.len() != sample_count || values.iter().any(|value| !value.is_finite()) {
                return Err("Некорректные значения ключей анимации GLB".into());
            }
            if property == Property::Rotation
                && values
                    .iter()
                    .skip(stride / 2)
                    .step_by(stride)
                    .any(|&value| unit_quaternion(value).is_none())
            {
                return Err("Некорректный поворот в клипе GLB".into());
            }
            duration = duration.max(last_time);
            channels.push(Channel {
                node,
                property,
                interpolation,
                times: times.into(),
                values: values.into(),
            });
        }
        if !channels.is_empty() {
            let name = animation
                .name()
                .filter(|name| !name.trim().is_empty())
                .map_or_else(|| format!("Клип {}", animation.index() + 1), str::to_owned);
            animations.push(Animation {
                name,
                duration,
                channels,
            });
        }
    }
    Ok(animations)
}

impl Channel {
    fn apply(&self, time: f32, transform: &mut LocalTransform) {
        let LocalTransform::Trs {
            translation,
            rotation,
            scale,
        } = transform
        else {
            return;
        };
        let value = self.sample(time);
        match self.property {
            Property::Translation => *translation = value.truncate(),
            Property::Scale => *scale = value.truncate(),
            Property::Rotation => {
                if let Some(value) = unit_quaternion(value) {
                    *rotation = value;
                }
            }
            Property::MorphTargetWeights => {}
        }
    }

    fn sample(&self, time: f32) -> Vec4 {
        let upper = self.times.partition_point(|&point| point <= time);
        let lower = upper.saturating_sub(1);
        let value_at = |index| {
            if self.interpolation == Interpolation::CubicSpline {
                self.values[index * 3 + 1]
            } else {
                self.values[index]
            }
        };
        let a = value_at(lower);
        if upper == 0 || upper == self.times.len() || self.interpolation == Interpolation::Step {
            return a;
        }
        let b = value_at(upper);
        let interval = self.times[upper] - self.times[lower];
        let t = (time - self.times[lower]) / interval;
        if self.interpolation == Interpolation::CubicSpline {
            let out_tangent = self.values[lower * 3 + 2];
            let in_tangent = self.values[upper * 3];
            let t2 = t * t;
            let t3 = t2 * t;
            return a * (2.0 * t3 - 3.0 * t2 + 1.0)
                + out_tangent * (interval * (t3 - 2.0 * t2 + t))
                + b * (-2.0 * t3 + 3.0 * t2)
                + in_tangent * (interval * (t3 - t2));
        }
        if self.property == Property::Rotation {
            return match (unit_quaternion(a), unit_quaternion(b)) {
                (Some(a), Some(b)) => Vec4::from_array(a.slerp(b, t).to_array()),
                _ => a,
            };
        }
        a.lerp(b, t)
    }
}

fn validate_ranges(document: &gltf::Document, blob: &[u8]) -> Result<(), String> {
    let sparse_morphs = model_morphs::sparse_targets(document);
    for view in document.views() {
        view_data(&view, blob)?;
    }
    for accessor in document.accessors() {
        if accessor.sparse().is_some() {
            if !sparse_morphs.contains(&accessor.index()) {
                return Err("Разрежённые массивы GLB поддерживаются для форм меша.".into());
            }
            model_morphs::validate_sparse(&accessor, blob)?;
            if accessor.view().is_none() {
                continue;
            }
        }
        let view = accessor.view().ok_or("Массив GLB не привязан к буферу")?;
        let size = accessor.size();
        let stride = view.stride().unwrap_or(size);
        let valid = (stride >= size)
            && accessor
                .count()
                .checked_sub(1)
                .and_then(|count| count.checked_mul(stride))
                .and_then(|end| end.checked_add(accessor.offset()))
                .and_then(|end| end.checked_add(size))
                .is_some_and(|end| end <= view.length());
        if !valid {
            return Err("Массив GLB выходит за границы своего буфера".into());
        }
    }
    Ok(())
}

fn view_data<'a>(view: &gltf::buffer::View<'_>, blob: &'a [u8]) -> Result<&'a [u8], String> {
    let start = view.offset();
    start
        .checked_add(view.length())
        .and_then(|end| blob.get(start..end))
        .ok_or_else(|| "Блок данных GLB выходит за границы буфера".into())
}

struct GeometryBudget {
    vertices: usize,
    indices: usize,
}

fn validate_vertex_attribute(
    source: &gltf::Primitive<'_>,
    semantic: gltf::Semantic,
    dimensions: Dimensions,
    data_types: &[DataType],
) -> Result<(), String> {
    if source.get(&semantic).is_some_and(|accessor| {
        accessor.dimensions() != dimensions || !data_types.contains(&accessor.data_type())
    }) {
        return Err(format!("Недопустимый формат атрибута GLB {semantic:?}."));
    }
    Ok(())
}

fn decode_primitive(
    source: &gltf::Primitive<'_>,
    blob: &[u8],
    textures: &mut Textures,
    budget: &mut GeometryBudget,
) -> Result<Primitive, String> {
    if source.mode() != gltf::mesh::Mode::Triangles {
        return Err("Для импорта экспортируй меш в виде списка треугольников".into());
    }
    use gltf::Semantic;
    validate_vertex_attribute(source, Semantic::Positions, Dimensions::Vec3, &[DataType::F32])?;
    validate_vertex_attribute(source, Semantic::Normals, Dimensions::Vec3, &[DataType::F32])?;
    validate_vertex_attribute(source, Semantic::Tangents, Dimensions::Vec4, &[DataType::F32])?;
    if source.indices().is_some_and(|accessor| {
        accessor.dimensions() != Dimensions::Scalar
            || !matches!(accessor.data_type(), DataType::U8 | DataType::U16 | DataType::U32)
    }) {
        return Err("Недопустимый формат индексов треугольников GLB.".into());
    }
    let reader = source.reader(|_| Some(blob));
    let positions = reader
        .read_positions()
        .ok_or("У меша нет координат вершин")?;
    let vertex_count = positions.len();
    let index_count = source
        .indices()
        .map_or(vertex_count, |accessor| accessor.count());
    if vertex_count > budget.vertices || index_count > budget.indices {
        return Err("Модель содержит слишком много вершин или треугольников".into());
    }
    if index_count == 0 || !index_count.is_multiple_of(3) {
        return Err("В меше незаполненные треугольники".into());
    }
    budget.vertices -= vertex_count;
    budget.indices -= index_count;

    let indices: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None if source.indices().is_none() => (0..vertex_count as u32).collect(),
        None => return Err("Не удалось прочитать индексы треугольников".into()),
    };
    if indices.len() != index_count || indices.iter().any(|&index| index as usize >= vertex_count) {
        return Err("Индекс треугольника указывает вне массива вершин".into());
    }
    let mut vertices = positions
        .map(|position| {
            if !valid_position(position) {
                return Err("Координаты меша выходят за допустимый диапазон".to_owned());
            }
            Ok(Vertex {
                position,
                ..Vertex::zeroed()
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    if let Some(normals) = reader.read_normals() {
        write_attribute(&mut vertices, normals, |vertex, normal| {
            let normal = Vec3::from_array(normal);
            vertex.normal = normal
                .try_normalize()
                .ok_or("Некорректная нормаль GLB")?
                .to_array();
            Ok(())
        })?;
    } else if source.get(&gltf::Semantic::Normals).is_some() {
        return Err("Не удалось прочитать нормали GLB".into());
    } else {
        fill_normals(&mut vertices, &indices);
    }

    let (material, tex_coord) = textures.material(source.material(), blob)?;
    // Guard fibers use UV.y as the free-tip weight even with an untextured material.
    if let Some(uv_set) = tex_coord.or(material.guard_fur.then_some(0)) {
        validate_vertex_attribute(
            source,
            Semantic::TexCoords(uv_set),
            Dimensions::Vec2,
            &[DataType::U8, DataType::U16, DataType::F32],
        )?;
        let tex_coords = reader
            .read_tex_coords(uv_set)
            .ok_or_else(|| format!("У текстуры отсутствует набор координат TEXCOORD_{uv_set}"))?;
        write_attribute(&mut vertices, tex_coords.into_f32(), |vertex, uv| {
            if !Vec2::from_array(uv).is_finite() {
                return Err("Некорректные координаты текстуры GLB".into());
            }
            vertex.uv = uv;
            Ok(())
        })?;
    }

    if let Some(tangents) = reader.read_tangents() {
        write_attribute(&mut vertices, tangents, |vertex, tangent| {
            let xyz = Vec4::from_array(tangent).truncate();
            if !xyz.is_finite() || !tangent[3].is_finite() || tangent[3] == 0.0 {
                return Err("Некорректный базис нормалей GLB".into());
            }
            vertex.tangent = tangent_basis(vertex.normal, xyz, tangent[3].signum());
            Ok(())
        })?;
    } else if source.get(&gltf::Semantic::Tangents).is_some() {
        return Err("Не удалось прочитать базис нормалей GLB".into());
    } else if material.normal_texture.is_some() {
        fill_tangents(&mut vertices, &indices);
    } else {
        for vertex in &mut vertices {
            vertex.tangent = tangent_basis(vertex.normal, Vec3::ZERO, 1.0);
        }
    }

    Ok(Primitive {
        vertices,
        indices,
        material,
        material_name: source.material().name().map(Arc::from),
        region: ModelRegion::from_material(source.material().name()),
    })
}

fn write_attribute<T>(
    vertices: &mut [Vertex],
    values: impl ExactSizeIterator<Item = T>,
    mut write: impl FnMut(&mut Vertex, T) -> Result<(), String>,
) -> Result<(), String> {
    if vertices.len() != values.len() {
        return Err("У массивов вершин GLB разная длина".into());
    }
    for (vertex, value) in vertices.iter_mut().zip(values) {
        write(vertex, value)?;
    }
    Ok(())
}

fn fill_normals(vertices: &mut [Vertex], indices: &[u32]) {
    let mut totals = vec![Vec3::ZERO; vertices.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let a = Vec3::from_array(vertices[triangle[0] as usize].position);
        let b = Vec3::from_array(vertices[triangle[1] as usize].position);
        let c = Vec3::from_array(vertices[triangle[2] as usize].position);
        let area_normal = (b - a).cross(c - a);
        for &index in triangle {
            totals[index as usize] += area_normal;
        }
    }
    for (vertex, total) in vertices.iter_mut().zip(totals) {
        vertex.normal = total.try_normalize().unwrap_or(Vec3::Y).to_array();
    }
}

fn fill_tangents(vertices: &mut [Vertex], indices: &[u32]) {
    let mut totals = vec![[Vec3::ZERO; 2]; vertices.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let [a, b, c] =
            [triangle[0], triangle[1], triangle[2]].map(|index| vertices[index as usize]);
        let edge1 = Vec3::from_array(b.position) - Vec3::from_array(a.position);
        let edge2 = Vec3::from_array(c.position) - Vec3::from_array(a.position);
        let uv1 = Vec2::from_array(b.uv) - Vec2::from_array(a.uv);
        let uv2 = Vec2::from_array(c.uv) - Vec2::from_array(a.uv);
        let determinant = uv1.perp_dot(uv2);
        if determinant.abs() < 1e-8 || !determinant.is_finite() {
            continue;
        }
        let tangent = (edge1 * uv2.y - edge2 * uv1.y) / determinant;
        let bitangent = (edge2 * uv1.x - edge1 * uv2.x) / determinant;
        for &index in triangle {
            totals[index as usize][0] += tangent;
            totals[index as usize][1] += bitangent;
        }
    }
    for (vertex, [tangent, bitangent]) in vertices.iter_mut().zip(totals) {
        let normal = Vec3::from_array(vertex.normal);
        let sign = if normal.cross(tangent).dot(bitangent) < 0.0 {
            -1.0
        } else {
            1.0
        };
        vertex.tangent = tangent_basis(vertex.normal, tangent, sign);
    }
}

fn tangent_basis(normal: [f32; 3], direction: Vec3, sign: f32) -> [f32; 4] {
    let normal = Vec3::from_array(normal);
    let tangent = (direction - normal * normal.dot(direction))
        .try_normalize()
        .unwrap_or_else(|| normal.any_orthonormal_vector());
    tangent.extend(sign).to_array()
}

#[derive(Default)]
struct Textures {
    images: Vec<Image>,
    source_indices: Vec<usize>,
    loaded_bytes: usize,
    is_vrm: bool,
}

fn vrm_facial_layer(name: &str) -> bool {
    let normalized: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    [
        "eyebrow",
        "eyeline",
        "eyelash",
        "eyehighlight",
        "eyeextra",
        "iris",
        "facebrow",
        "facedecal",
        "facialdecal",
        "faceoverlay",
        "faceblush",
        "facemakeup",
    ]
    .into_iter()
    .any(|part| normalized.contains(part))
}

impl Textures {
    fn material(
        &mut self,
        source: gltf::Material<'_>,
        blob: &[u8],
    ) -> Result<(Material, Option<u32>), String> {
        let pbr = source.pbr_metallic_roughness();
        let base_color = pbr.base_color_factor();
        let metallic = pbr.metallic_factor();
        let roughness = pbr.roughness_factor();
        let normal = source.normal_texture();
        let emissive = source.emissive_factor();
        use gltf::material::AlphaMode;
        let alpha_mode = source.alpha_mode();
        let vrm_label = self
            .is_vrm
            .then(|| source.name().unwrap_or_default().to_ascii_lowercase());
        let facial_decal =
            alpha_mode != AlphaMode::Opaque && vrm_label.as_deref().is_some_and(vrm_facial_layer);
        let vrm_cutout = facial_decal
            || vrm_label
                .as_deref()
                .is_some_and(|name| name.is_empty() || name.contains("hair"));
        let alpha_cutoff = match alpha_mode {
            AlphaMode::Mask => Some(source.alpha_cutoff().unwrap_or(0.5)),
            // Сохраняем порог покрытия для карточек лица и волос старых VRM.
            AlphaMode::Blend if vrm_cutout => Some(0.25),
            _ => None,
        };
        let normal_scale = normal.as_ref().map_or(1.0, |texture| texture.scale());
        if base_color
            .into_iter()
            .chain([metallic, roughness])
            .chain(emissive)
            .chain(alpha_cutoff)
            .any(|value| !(0.0..=1.0).contains(&value))
            || !normal_scale.is_finite()
        {
            return Err("Недопустимое значение в материале GLB".into());
        }

        let mut tex_coord = None;
        let mut load_texture = |info: Option<(gltf::Texture<'_>, u32)>| {
            let Some((texture, uv_set)) = info else {
                return Ok(None);
            };
            if tex_coord.is_some_and(|set| set != uv_set) {
                return Err(
                    "Все карты материала должны использовать общий набор координат UV".into(),
                );
            }
            tex_coord = Some(uv_set);
            self.load_image(texture.source(), blob).map(Some)
        };

        let base_color_texture = load_texture(
            pbr.base_color_texture()
                .map(|info| (info.texture(), info.tex_coord())),
        )?;
        let metallic_roughness_texture = load_texture(
            pbr.metallic_roughness_texture()
                .map(|info| (info.texture(), info.tex_coord())),
        )?;
        let normal_texture = load_texture(normal.map(|info| (info.texture(), info.tex_coord())))?;
        let emissive_texture = if let Some(info) = source.emissive_texture()
            && tex_coord.is_none_or(|set| set == info.tex_coord())
            && matches!(
                info.texture().source().source(),
                gltf::image::Source::View {
                    mime_type: "image/png" | "image/jpeg",
                    ..
                }
            ) {
            tex_coord = Some(info.tex_coord());
            Some(self.load_image(info.texture().source(), blob)?)
        } else {
            None
        };
        let material = Material {
            base_color,
            base_color_texture,
            metallic_roughness_texture,
            normal_texture,
            emissive_texture,
            emissive,
            alpha_cutoff,
            alpha_blend: alpha_mode == AlphaMode::Blend,
            facial_decal,
            metallic,
            roughness,
            normal_scale,
            illustration: 0.0,
            guard_fur: source.name().is_some_and(|name| name.starts_with("Creator | Guard Fur ")),
            double_sided: source.double_sided(),
        };
        Ok((material, tex_coord))
    }

    fn load_image(&mut self, source: gltf::Image<'_>, blob: &[u8]) -> Result<usize, String> {
        let source_index = source.index();
        if let Some(index) = self
            .source_indices
            .iter()
            .position(|&used| used == source_index)
        {
            return Ok(index);
        }
        if self.source_indices.len() >= 64 {
            return Err("В GLB слишком много текстур".into());
        }
        let (view, mime_type) = match source.source() {
            gltf::image::Source::View { view, mime_type } => (view, mime_type),
            gltf::image::Source::Uri { uri, .. } => {
                let name: String = uri.chars().take(120).collect();
                return Err(format!(
                    "Текстура {} «{name}»: изображение нужно встроить внутрь GLB при экспорте.",
                    source_index + 1
                ));
            }
        };
        let format = match mime_type {
            "image/png" => ImageFormat::Png,
            "image/jpeg" => ImageFormat::Jpeg,
            _ => return Err("Для текстур GLB поддерживаются встроенные PNG и JPEG".into()),
        };
        let mut reader = ImageReader::with_format(Cursor::new(view_data(&view, blob)?), format);
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_TEXTURE_SIZE);
        limits.max_image_height = Some(MAX_TEXTURE_SIZE);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let decoder = reader
            .into_decoder()
            .map_err(|err| format!("Ошибка чтения текстуры GLB ({err})"))?;
        use image::ImageDecoder;
        let (width, height) = decoder.dimensions();
        let bytes = (width as usize) * (height as usize) * 4;
        if !(1..=MAX_TEXTURE_SIZE).contains(&width) || !(1..=MAX_TEXTURE_SIZE).contains(&height) {
            return Err(
                "Размер текстуры должен быть от 1 до 4096 пикселей по каждой стороне".into(),
            );
        }
        if self.loaded_bytes + bytes > MAX_TEXTURE_BYTES {
            return Err("Суммарный объём текстур GLB превышает допустимый размер".into());
        }
        let rgba = image::DynamicImage::from_decoder(decoder)
            .map_err(|err| format!("Ошибка декодирования текстуры GLB ({err})"))?
            .into_rgba8();
        self.loaded_bytes += bytes;
        self.images.push(Image {
            width,
            height,
            pixels: rgba.into_raw().into(),
        });
        self.source_indices.push(source_index);
        Ok(self.images.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
    use serde_json::json;

    macro_rules! import_fixture {
        ($fixture:ident, $message:literal) => {
            #[test]
            fn $fixture() {
                let name = concat!(stringify!($fixture), ".glb");
                let bytes = include_bytes!(concat!(
                    "../tests/fixtures/",
                    stringify!($fixture),
                    ".glb"
                ));
                let error: ImportError = decode_glb(bytes).unwrap_err().with_file(name);
                let display = error.to_string();
                assert!(display.contains(name), "{display}");
                assert!(display.contains($message), "{display}");
            }
        };
    }

    import_fixture!(a1_model_header, "двоичный контейнер");
    import_fixture!(a1_model_truncated_json, "строка");
    import_fixture!(a1_model_external_texture, "missing-cloth.png");
    import_fixture!(a1_model_nan_position, "Координаты меша");
    import_fixture!(a1_model_position_reference, "POSITION отсутствует");
    import_fixture!(a1_model_texture_descriptor, "отсутствуют данные изображения");
    import_fixture!(a1_model_index_format, "формат индексов");
    import_fixture!(a1_model_animation_target, "дорожка 1");

    #[test]
    fn a4_import_preserves_glb_alpha_and_classifies_vrm_face_layers() {
        use std::borrow::Cow;

        let materials = json!([
            {"name": "Glass", "alphaMode": "BLEND", "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 0.1]}},
            {"name": "N00_000_00_Face_00_EYE_Highlight (Instance)", "alphaMode": "BLEND",
             "emissiveFactor": [0.2, 0.7, 1.0]},
            {"name": "Hair_Cards", "alphaMode": "MASK", "alphaCutoff": 0.43},
            {"name": "Base", "alphaMode": "OPAQUE", "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 0.2]}}
        ]);
        let primitives: Vec<_> = (0..4)
            .map(|index| json!({"attributes": {"POSITION": 0}, "material": index}))
            .collect();
        let mut doc = json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": primitives}],
            "materials": materials,
            "buffers": [{"byteLength": 36}],
            "bufferViews": [{"buffer": 0, "byteLength": 36}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3,
                "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]}]
        });
        let bin: Vec<_> = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        for is_vrm in [false, true] {
            if is_vrm {
                doc["extensionsUsed"] = json!(["VRMC_vrm"]);
                doc["extensions"] = json!({"VRMC_vrm": {"specVersion": "1.0"}});
            }
            let mut json = serde_json::to_vec(&doc).unwrap();
            json.resize(json.len().next_multiple_of(4), b' ');
            let glb = gltf::binary::Glb {
                header: gltf::binary::Header {
                    magic: *b"glTF",
                    version: 2,
                    length: 0,
                },
                json: Cow::Owned(json),
                bin: Some(Cow::Borrowed(&bin)),
            };
            let model = decode_glb(&glb.to_vec().unwrap()).unwrap();
            assert_eq!(model.primitives.len(), 4);
            let [glass, iris, hair, base] = std::array::from_fn(|i| model.primitives[i].material);
            assert!(glass.alpha_blend && !glass.facial_decal && glass.alpha_cutoff.is_none());
            assert_eq!(glass.base_color[3], 0.1);
            assert!(iris.alpha_blend && iris.facial_decal == is_vrm);
            assert_eq!(iris.alpha_cutoff, is_vrm.then_some(0.25));
            assert_eq!(iris.emissive, [0.2, 0.7, 1.0]);
            assert!(!hair.alpha_blend && !hair.facial_decal);
            assert_eq!(hair.alpha_cutoff, Some(0.43));
            assert!(!base.alpha_blend && !base.facial_decal && base.alpha_cutoff.is_none());
        }
    }

    #[test]
    fn mira_loads_with_gait_expression_and_moving_alpha_masked_hair() {
        let model = decode_glb(include_bytes!("../demo_assets/character-showcase/mira.glb")).unwrap();
        assert!(model.has_secondary_pose());
        assert!(model.hair_rig.is_some());
        assert!(["Idle", "Walk", "Run"].into_iter().all(|name| {
            model.animations.iter().any(|clip| clip.name == name)
        }));
        assert_eq!(model.morph_names().len(), 52);
        let face = model.primitives.iter().position(|primitive| {
            primitive.region == ModelRegion::StudioFace
        }).unwrap();
        let scalp = model.primitives.iter().position(|primitive| {
            primitive.region == ModelRegion::StudioScalp
                && primitive.material.normal_texture.is_some()
        }).unwrap();
        let wisps = model.primitives.iter().position(|primitive| {
            primitive.region == ModelRegion::StudioHairWisp && primitive.material.alpha_cutoff.is_some()
        }).unwrap();
        let pose = model.pose_blended_with_morphs(
            None, 0.0, None, &[],
            SecondaryPose { hair_sway: [0.72, -0.46], hair_lift: 0.31,
                hair_contacts: HairContacts::HeadAndShoulders, ..SecondaryPose::default() },
            &[], &[MorphWeight { target: "jawOpen".into(), weight: 0.72 }],
        ).unwrap();
        for index in [face, wisps] {
            let moved = pose[index].iter().zip(&model.primitives[index].vertices).any(|(posed, rest)| {
                Vec3::from_array(posed.position).distance(Vec3::from_array(rest.position)) > 0.004
            });
            assert!(moved, "no deformation on primitive {index}");
        }
        let rest = model.pose_with_secondary(None, 0.0, &[], SecondaryPose::default()).unwrap();
        assert!(rest[scalp].iter().zip(&pose[scalp]).all(|(rest, posed)| {
            Vec3::from_array(rest.position).distance(Vec3::from_array(posed.position)) < 1e-5
        }), "the fitted scalp shifted with loose strands");
    }

    #[test]
    fn mira_ponytail_keeps_moving_after_short_strands_settle_and_roots_stay_attached() {
        use crate::{avatar::ActorTuning, secondary_motion::State};

        let model = decode_glb(include_bytes!("../demo_assets/character-showcase/mira.glb")).unwrap();
        let rest = model.pose_with_secondary(None, 0.0, &[], SecondaryPose::default()).unwrap();
        let tail = model.primitives.iter().position(|primitive| {
            primitive.region == ModelRegion::StudioHairTip
                && primitive.material.alpha_cutoff.is_some()
        }).unwrap();
        let wisps = model.primitives.iter().position(|primitive| {
            primitive.region == ModelRegion::StudioHairWisp
        }).unwrap();
        let displacement = |index: usize, pose: &[Vec<Vertex>]| {
            rest[index].iter().zip(&pose[index]).map(|(base, posed)| {
                Vec3::from_array(base.position).distance(Vec3::from_array(posed.position))
            }).fold(0.0_f32, f32::max)
        };

        let mut state = State::default();
        let tuning = ActorTuning::default();
        let dt = 1.0 / 60.0;
        for tick in 0..36 {
            let time = tick as f32 / 35.0;
            state.advance_tuned(dt, Vec3::Z * (3.6 * time), 120.0 * time,
                Vec3::X * 7.0, tuning);
        }
        let before_brake = state.pose(0.0);
        let mut peaks = [0.0_f32; 2];
        let mut difference_from_single_spring = 0.0_f32;
        let mut root_drift = 0.0_f32;
        for _ in 0..22 {
            state.advance_tuned(dt, Vec3::ZERO, 120.0, Vec3::ZERO, tuning);
            let secondary = state.pose(0.0);
            let posed = model.pose_with_secondary(None, 0.0, &[], SecondaryPose {
                hair_contacts: HairContacts::HeadAndShoulders,
                ..secondary
            }).unwrap();
            let single = model.pose_with_secondary(None, 0.0, &[], SecondaryPose {
                hair_follow: None,
                hair_contacts: HairContacts::HeadAndShoulders,
                ..secondary
            }).unwrap();
            peaks = [peaks[0].max(displacement(tail, &posed)), peaks[1].max(displacement(wisps, &posed))];
            for (current, baseline) in posed[tail].iter().zip(&single[tail]) {
                difference_from_single_spring = difference_from_single_spring.max(
                    Vec3::from_array(current.position).distance(Vec3::from_array(baseline.position))
                );
            }
            for (base, current) in rest[wisps].iter().zip(&posed[wisps]) {
                if base.uv[1] <= 0.13 {
                    root_drift = root_drift.max(Vec3::from_array(base.position)
                        .distance(Vec3::from_array(current.position)));
                }
            }
        }
        let after_brake = state.pose(0.0);
        let before = before_brake.hair_follow.unwrap();
        let after = after_brake.hair_follow.unwrap();
        eprintln!(
            "Mira hair after braking: tail peak {:.1} mm, loose strands {:.1} mm, change vs shared spring {:.1} mm, root drift {:.3} mm",
            1000.0 * peaks[0], 1000.0 * peaks[1], 1000.0 * difference_from_single_spring,
            1000.0 * root_drift,
        );
        assert!(peaks[0] > 0.015 && peaks[0] < 0.09);
        assert!(peaks[1] > 0.002 && peaks[1] < peaks[0] * 0.7);
        assert!(difference_from_single_spring > 0.005);
        assert!(root_drift < 1e-5);
        assert!(after.tail_end[0].abs() + after.tail_end[2].abs()
            > after.loose_strands[0].abs() + after.loose_strands[2].abs());
        assert_ne!(before.tail_end, after.tail_end);
    }

    #[test]
    fn rotating_joint_keeps_bone_length_during_crossfade() {
        let local = |angle: f32| LocalTransform::Trs {
            translation: Vec3::ZERO,
            rotation: Quat::from_rotation_z(angle.to_radians()),
            scale: Vec3::ONE,
        };
        let from = local(-65.0);
        let to = local(65.0);
        let shin = Vec3::new(0.0, -0.46, 0.0);
        let half = from.interpolate(to, 0.5).matrix().transform_point3(shin);
        assert!((half.length() - shin.length()).abs() < 0.00001);
        assert!(half.distance(shin) < 0.00001);
    }

    #[test]
    fn loads_embedded_textures_and_preserves_winding_under_mirrored_nodes() {
        let mut blob = Vec::new();
        for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            blob.extend(value.to_le_bytes());
        }
        for value in [0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0] {
            blob.extend(value.to_le_bytes());
        }
        let pixel = [27, 155, 126, 255];
        PngEncoder::new(&mut blob)
            .write_image(&pixel, 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let document = json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [
                {"children": [1], "translation": [10, 0, 0], "scale": [-2, 3, 1]},
                {"mesh": 0, "translation": [1, 2, 3]},
            ],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "TEXCOORD_0": 1}, "material": 0}]}],
            "materials": [{
                "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicRoughnessTexture": {"index": 0}},
                "normalTexture": {"index": 0},
            }],
            "textures": [{"source": 0}],
            "images": [{"bufferView": 2, "mimeType": "image/png"}],
            "buffers": [{"byteLength": blob.len()}],
            "bufferViews": [
                {"buffer": 0, "byteLength": 36},
                {"buffer": 0, "byteOffset": 36, "byteLength": 24},
                {"buffer": 0, "byteOffset": 60, "byteLength": blob.len() - 60},
            ],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0,0,0], "max": [1,1,0]},
                {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC2"},
            ],
        });

        let mut bytes = serde_json::to_vec(&document).unwrap();
        bytes.resize(bytes.len().next_multiple_of(4), b' ');
        blob.resize(blob.len().next_multiple_of(4), 0);
        let mut glb = Vec::new();
        glb.extend(b"glTF");
        glb.extend(2_u32.to_le_bytes());
        glb.extend(((28 + bytes.len() + blob.len()) as u32).to_le_bytes());
        glb.extend((bytes.len() as u32).to_le_bytes());
        glb.extend(b"JSON");
        glb.extend(bytes);
        glb.extend((blob.len() as u32).to_le_bytes());
        glb.extend(b"BIN\0");
        glb.extend(blob);

        let model = decode_glb(&glb).unwrap();
        assert_eq!(model.bounds_min, [6.0, 6.0, 3.0]);
        assert_eq!(model.bounds_max, [8.0, 9.0, 3.0]);
        assert_eq!(model.images.len(), 1);
        assert_eq!(&*model.images[0].pixels, &pixel);
        let primitive = &model.primitives[0];
        assert_eq!(primitive.indices, [0, 2, 1]);
        assert_eq!(primitive.vertices[0].normal, [0.0, 0.0, 1.0]);
        assert_eq!(primitive.vertices[0].tangent, [-1.0, 0.0, 0.0, -1.0]);
        assert_eq!(primitive.material.base_color_texture, Some(0));
        assert_eq!(primitive.material.metallic_roughness_texture, Some(0));
        assert_eq!(primitive.material.normal_texture, Some(0));
        assert!(decode_glb(&glb[..glb.len() - 1]).is_err());
        assert!(model.pose(None, 0.0).is_none());
    }

    #[test]
    fn knight_walking_moves_weighted_meshes_and_rigid_bone_attachments() {
        let bytes = include_bytes!("../demo_assets/character.glb");
        let model = decode_glb(bytes).unwrap();
        for name in ["Idle", "Walking_A", "Running_A", "Jump_Full_Short"] {
            assert!(
                model
                    .animations
                    .iter()
                    .any(|clip| clip.name == name && clip.duration > 0.0)
            );
        }
        let walk = model
            .animations
            .iter()
            .position(|clip| clip.name == "Walking_A")
            .unwrap();
        let duration = model.animations[walk].duration;
        let first = model.pose(Some(walk), duration * 0.13).unwrap();
        let second = model.pose(Some(walk), duration * 0.46).unwrap();
        let mut skinned_moved = false;
        let mut attachment_moved = false;
        for (index, (from, to)) in first.iter().zip(&second).enumerate() {
            assert_eq!(from.len(), model.primitives[index].vertices.len());
            let moved = from.iter().zip(to).any(|(a, b)| {
                (Vec3::from_array(a.position) - Vec3::from_array(b.position)).length_squared()
                    > 0.001
            });
            if model.rig.as_ref().unwrap().meshes[index].skin.is_some() {
                skinned_moved |= moved;
            } else {
                attachment_moved |= moved;
            }
        }
        assert!(skinned_moved);
        assert!(attachment_moved);
    }
}
