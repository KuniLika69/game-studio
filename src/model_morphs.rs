use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet},
    ops::RangeInclusive,
    sync::Arc,
};

use glam::{Vec3, Vec4};
use gltf::accessor::{DataType, Dimensions, Iter, sparse::IndexType};
use serde::{Deserialize, Serialize};

use super::{Model, RestMesh, Vertex, view_data};

pub(crate) const MAX_TARGETS: usize = 192;
pub(crate) const OVERRIDE_RANGE: RangeInclusive<f32> = -1.0..=1.0;
const MAX_MORPH_SAMPLES: usize = 3_000_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MorphWeight {
    pub target: String,
    pub weight: f32,
}

#[derive(Clone, Debug)]
pub(super) struct MorphTarget {
    name: Arc<str>,
    rest_weight: f32,
    deltas: Arc<[MorphDelta]>,
}

#[derive(Clone, Copy, Debug)]
struct MorphDelta {
    index: u32,
    position: Vec3,
    normal: Vec3,
    tangent: Vec3,
}

impl Model {
    pub fn morph_names(&self) -> &[String] {
        &self.morph_names
    }

    pub(crate) fn morph_rest_weight(&self, name: &str) -> f32 {
        self.rig
            .iter()
            .flat_map(|rig| &rig.meshes)
            .flat_map(|mesh| &mesh.morphs)
            .find_map(|target| (target.name.as_ref() == name).then_some(target.rest_weight))
            .unwrap_or(0.0)
    }

    pub(crate) fn morph_storage_bytes(&self) -> usize {
        let mut allocations = HashSet::new();
        self.rig
            .iter()
            .flat_map(|rig| &rig.meshes)
            .flat_map(|mesh| &mesh.morphs)
            .filter(|morph| allocations.insert(morph.deltas.as_ptr()))
            .map(|morph| morph.deltas.len() * size_of::<MorphDelta>())
            .sum()
    }

    pub(super) fn refresh_morph_names(&mut self) -> Result<(), String> {
        let names: BTreeSet<&str> = self
            .rig
            .iter()
            .flat_map(|rig| &rig.meshes)
            .flat_map(|mesh| &mesh.morphs)
            .map(|target| target.name.as_ref())
            .collect();
        if names.len() > MAX_TARGETS {
            return Err("Слишком много разных форм меша GLB.".into());
        }
        self.morph_names = names.into_iter().map(str::to_owned).collect();
        self.expression_channels = crate::actor_expression::Channels::resolve(&self.morph_names);
        Ok(())
    }
}

impl RestMesh {
    pub(super) fn vertices_with_morphs<'a>(&'a self, weights: &[MorphWeight]) -> Cow<'a, [Vertex]> {
        let mut vertices = Cow::Borrowed(self.vertices.as_slice());
        for target in &self.morphs {
            let weight = weights
                .iter()
                .find(|weight| weight.target == target.name.as_ref())
                .map_or(target.rest_weight, |weight| {
                    weight.weight.clamp(*OVERRIDE_RANGE.start(), *OVERRIDE_RANGE.end())
                });
            if !weight.is_finite() || weight == 0.0 {
                continue;
            }
            let vertices = vertices.to_mut();
            for delta in target.deltas.iter() {
                let vertex = &mut vertices[delta.index as usize];
                vertex.position =
                    (Vec3::from_array(vertex.position) + delta.position * weight).to_array();
                vertex.normal =
                    (Vec3::from_array(vertex.normal) + delta.normal * weight).to_array();
                let tangent = Vec4::from_array(vertex.tangent);
                vertex.tangent = (tangent + (delta.tangent * weight).extend(0.0)).to_array();
            }
        }
        vertices
    }
}

#[derive(Default, Deserialize)]
struct MeshJson {
    #[serde(default)]
    extras: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct DocumentJson {
    #[serde(default)]
    meshes: Vec<MeshJson>,
}

type MorphAccessors = (usize, [Option<usize>; 3]);

pub(super) struct MorphReader<'a> {
    blob: &'a [u8],
    mesh_names: Vec<Vec<Option<Arc<str>>>>,
    decoded: HashMap<MorphAccessors, Arc<[MorphDelta]>>,
    remaining_samples: usize,
    remaining_deltas: usize,
}

impl<'a> MorphReader<'a> {
    pub(super) fn new(
        bytes: &[u8],
        blob: &'a [u8],
        document: &gltf::Document,
    ) -> Result<Self, String> {
        let has_morphs = document
            .meshes()
            .any(|mesh| mesh.primitives().any(|p| p.morph_targets().len() != 0));
        let mut mesh_names = Vec::new();
        if has_morphs {
            let binary = gltf::binary::Glb::from_slice(bytes).map_err(|_| "Некорректный GLB.")?;
            let raw: DocumentJson = serde_json::from_slice(&binary.json)
                .map_err(|_| "Не удалось прочитать имена форм GLB.")?;
            mesh_names = raw
                .meshes
                .into_iter()
                .map(|mesh| {
                    mesh.extras
                        .as_ref()
                        .and_then(|extras| extras.get("targetNames"))
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .take(MAX_TARGETS)
                        .map(|name| {
                            name.as_str()
                                .map(str::trim)
                                .filter(|name| {
                                    !name.is_empty()
                                        && name.len() <= 128
                                        && !name.chars().any(char::is_control)
                                })
                                .map(Arc::from)
                        })
                        .collect()
                })
                .collect();
        }
        Ok(Self {
            blob,
            mesh_names,
            decoded: HashMap::new(),
            remaining_samples: MAX_MORPH_SAMPLES,
            remaining_deltas: MAX_MORPH_SAMPLES,
        })
    }

    pub(super) fn read(
        &mut self,
        node: &gltf::Node<'_>,
        mesh: &gltf::Mesh<'_>,
        source: &gltf::Primitive<'_>,
        vertex_count: usize,
    ) -> Result<Vec<MorphTarget>, String> {
        let morphs = source.morph_targets();
        let count = morphs.len();
        if count == 0 {
            return Ok(Vec::new());
        }
        if count > MAX_TARGETS {
            return Err("Слишком много форм в одной части меша GLB.".into());
        }
        let rest = node.weights().or_else(|| mesh.weights());
        if rest.is_some_and(|rest| {
            rest.len() != count || rest.iter().any(|f| !(-16.0..=16.0).contains(f))
        }) {
            return Err("Некорректные начальные веса форм GLB.".into());
        }
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, morph) in morphs.enumerate() {
            let name = self
                .mesh_names
                .get(mesh.index())
                .and_then(|names| names.get(index))
                .and_then(Clone::clone)
                .unwrap_or_else(|| {
                    format!("Меш {} / форма {}", mesh.index() + 1, index + 1).into()
                });
            if !seen.insert(Arc::clone(&name)) {
                return Err(format!("Меш GLB содержит повторяющуюся форму «{name}»."));
            }
            let delta_sources = [morph.positions(), morph.normals(), morph.tangents()];
            let deltas = self.shared_deltas(delta_sources, vertex_count, &name)?;
            // Shared streams cost memory once. Every reference still applies its deltas.
            self.remaining_deltas = self
                .remaining_deltas
                .checked_sub(deltas.len())
                .ok_or("Слишком много активных смещений в формах GLB.")?;
            if !deltas.is_empty() {
                result.push(MorphTarget {
                    name,
                    rest_weight: rest.map_or(0.0, |rest| rest[index]),
                    deltas,
                });
            }
        }
        Ok(result)
    }

    fn shared_deltas(
        &mut self,
        sources: [Option<gltf::Accessor<'_>>; 3],
        vertex_count: usize,
        name: &str,
    ) -> Result<Arc<[MorphDelta]>, String> {
        let indices = sources
            .each_ref()
            .map(|source| source.as_ref().map(gltf::Accessor::index));
        let key = (vertex_count, indices);
        if let Some(deltas) = self.decoded.get(&key) {
            return Ok(Arc::clone(deltas));
        }
        self.remaining_samples = self
            .remaining_samples
            .checked_sub(vertex_count)
            .ok_or("Слишком много вершин в формах GLB.")?;
        let [position, normal, tangent] = sources;
        let mut position = self.attribute(position, vertex_count)?;
        let mut normal = self.attribute(normal, vertex_count)?;
        let mut tangent = self.attribute(tangent, vertex_count)?;
        let mut deltas = Vec::new();
        let take = |source: &mut Option<Iter<'a, [f32; 3]>>| {
            source
                .as_mut()
                .and_then(Iterator::next)
                .map_or(Vec3::ZERO, Vec3::from_array)
        };
        for index in 0..vertex_count {
            let offsets = [take(&mut position), take(&mut normal), take(&mut tangent)];
            if offsets
                .iter()
                .any(|delta| !delta.is_finite() || delta.abs().max_element() > 1_000.0)
            {
                return Err(format!("Некорректное смещение формы GLB «{name}»."));
            }
            if offsets.iter().any(|&offset| offset != Vec3::ZERO) {
                let [position, normal, tangent] = offsets;
                deltas.push(MorphDelta {
                    index: index as u32,
                    position,
                    normal,
                    tangent,
                });
            }
        }
        let deltas: Arc<[_]> = deltas.into();
        self.decoded.insert(key, Arc::clone(&deltas));
        Ok(deltas)
    }

    fn attribute(
        &self,
        accessor: Option<gltf::Accessor<'_>>,
        count: usize,
    ) -> Result<Option<Iter<'a, [f32; 3]>>, String> {
        let Some(accessor) = accessor else {
            return Ok(None);
        };
        if accessor.data_type() != DataType::F32
            || accessor.dimensions() != Dimensions::Vec3
            || accessor.count() != count
        {
            return Err("Форма GLB должна иметь по одному вектору XYZ на каждую вершину.".into());
        }
        let blob = self.blob;
        Iter::new(accessor, move |_| Some(blob))
            .map(Some)
            .ok_or_else(|| "Не удалось прочитать смещения формы GLB.".into())
    }
}

pub(super) fn sparse_targets(document: &gltf::Document) -> BTreeSet<usize> {
    document
        .meshes()
        .flat_map(|mesh| mesh.primitives())
        .flat_map(|primitive| primitive.morph_targets())
        .flat_map(|target| [target.positions(), target.normals(), target.tangents()])
        .flatten()
        .filter(|accessor| accessor.sparse().is_some())
        .map(|accessor| accessor.index())
        .collect()
}

pub(super) fn validate_sparse(accessor: &gltf::Accessor<'_>, blob: &[u8]) -> Result<(), String> {
    let invalid = || "Некорректный разрежённый массив формы GLB.".to_owned();
    let sparse = accessor.sparse().ok_or_else(invalid)?;
    if !(1..=accessor.count()).contains(&sparse.count())
        || (accessor.view().is_none() && accessor.offset() != 0)
    {
        return Err(invalid());
    }
    let indices = sparse.indices();
    let values = sparse.values();
    let check = |view: gltf::buffer::View<'_>, offset: usize, element: usize| {
        if view.stride().is_some() {
            return Err(invalid());
        }
        let length = sparse.count().checked_mul(element).ok_or_else(invalid)?;
        view_data(&view, blob)?
            .get(offset..offset.checked_add(length).ok_or_else(invalid)?)
            .ok_or_else(invalid)
    };
    check(values.view(), values.offset(), accessor.size())?;
    let index_type = indices.index_type();
    let bytes = check(indices.view(), indices.offset(), index_type.size())?;
    let mut previous = None;
    for bytes in bytes.chunks_exact(index_type.size()) {
        let index = match index_type {
            IndexType::U8 => u32::from(bytes[0]),
            IndexType::U16 => u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
            IndexType::U32 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        } as usize;
        if index >= accessor.count() || previous.is_some_and(|previous| previous >= index) {
            return Err(invalid());
        }
        previous = Some(index);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SecondaryPose, decode_glb};

    fn pose(model: &Model, target: &str, weight: f32) -> Vec<Vec<Vertex>> {
        model
            .pose_blended_with_morphs(
                None,
                0.0,
                None,
                &[],
                SecondaryPose::default(),
                &[],
                &[MorphWeight {
                    target: target.into(),
                    weight,
                }],
            )
            .unwrap()
    }

    #[test]
    fn applies_real_glb_mouth_blink_and_gaze_before_skinning() {
        for (name, bytes) in [
            ("Emma", include_bytes!("../demo_assets/emma.glb").as_slice()),
            ("Ryan", include_bytes!("../demo_assets/ryan.glb").as_slice()),
        ] {
            let model = decode_glb(bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
            for target in [
                "eyeBlinkLeft",
                "jawOpen",
                "eyeLookDownRight",
                "mouthSmileLeft",
            ] {
                assert!(
                    model.morph_names().iter().any(|name| name == target),
                    "{name}: {target}"
                );
                let idle = pose(&model, target, 0.0);
                let half = pose(&model, target, 0.5);
                let full = pose(&model, target, 1.0);
                let mut moved = 0;
                for ((rest, half), full) in idle
                    .iter()
                    .flatten()
                    .zip(half.iter().flatten())
                    .zip(full.iter().flatten())
                {
                    let rest = Vec3::from_array(rest.position);
                    let half = Vec3::from_array(half.position);
                    let full = Vec3::from_array(full.position);
                    if rest.distance(full) > 1.0e-5 {
                        moved += 1;
                        assert!(
                            rest.y > 1.4,
                            "{name} {target} moved vertices outside head: {rest}"
                        );
                        assert!(half.distance(rest.lerp(full, 0.5)) < 0.00001);
                    }
                }
                assert!(
                    moved > 2,
                    "{name} {target} produced no visible mesh deformation"
                );
                println!("{name} {target}: {moved} moved head vertices");
            }
        }
    }

    fn sparse_fixture(displaced_vertex: u8) -> Vec<u8> {
        let doc = serde_json::json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0, "weights":[0.25]}],
            "meshes": [{"extras": {"targetNames": ["preparedSmile"]}, "primitives": [
                {"attributes": {"POSITION": 0}, "targets": [{"POSITION": 1}]}
            ]}],
            "buffers": [{"byteLength": 52}],
            "bufferViews": [
                {"buffer":0,"byteOffset":0,"byteLength":36},
                {"buffer":0,"byteOffset":36,"byteLength":1},
                {"buffer":0,"byteOffset":40,"byteLength":12}
            ],
            "accessors": [
                {"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0.0,0.0,0.0],"max":[1.0,1.0,0.0]},
                {"componentType":5126,"count":3,"type":"VEC3","min":[0.0,0.0,0.0],"max":[0.0,0.0,2.0],
                 "sparse":{"count":1,"indices":{"bufferView":1,"componentType":5121},"values":{"bufferView":2}}}
            ]
        });
        let mut json = serde_json::to_vec(&doc).unwrap();
        json.resize(json.len().next_multiple_of(4), b' ');
        let mut bin = Vec::new();
        for point in [[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            bin.extend(point.into_iter().flat_map(f32::to_le_bytes));
        }
        bin.extend([displaced_vertex, 0, 0, 0]);
        bin.extend([0.0_f32, 0.0, 2.0].into_iter().flat_map(f32::to_le_bytes));
        gltf::binary::Glb {
            header: gltf::binary::Header {
                magic: *b"glTF",
                version: 2,
                length: 0,
            },
            json: Cow::Owned(json),
            bin: Some(Cow::Owned(bin)),
        }
        .to_vec()
        .unwrap()
    }

    #[test]
    fn sparse_face_mesh_includes_node_default_and_checks_sparse_indices() {
        let model = decode_glb(&sparse_fixture(2)).unwrap();
        assert_eq!(model.morph_names(), &["preparedSmile"]);
        assert_eq!(model.morph_rest_weight("preparedSmile"), 0.25);
        assert_eq!(model.primitives[0].vertices[2].position, [0.0, 1.0, 0.5]);
        assert_eq!(
            pose(&model, "preparedSmile", 1.0)[0][2].position,
            [0.0, 1.0, 2.0]
        );
        assert_eq!(
            pose(&model, "preparedSmile", 0.0)[0][2].position,
            [0.0, 1.0, 0.0]
        );
        assert_eq!(
            pose(&model, "preparedSmile", -0.5)[0][2].position,
            [0.0, 1.0, -1.0]
        );
        let shapes = [MorphWeight { target: "preparedSmile".into(), weight: -0.5 }];
        let mut weights = vec![MorphWeight { target: "preparedSmile".into(), weight: 0.75 }];
        crate::character_shapes::compose(&model, &shapes, &mut weights);
        assert_eq!(weights.len(), 1);
        assert_eq!(weights[0].weight, 0.75);
        weights.clear();
        crate::character_shapes::compose(&model, &shapes, &mut weights);
        assert_eq!(weights, shapes);
        assert!(decode_glb(&sparse_fixture(3)).is_err());
    }

    #[test]
    fn shared_morph_stream_keeps_each_nodes_default_weight() {
        let original = sparse_fixture(2);
        let mut binary = gltf::binary::Glb::from_slice(&original).unwrap();
        let mut doc: serde_json::Value = serde_json::from_slice(&binary.json).unwrap();
        doc["nodes"].as_array_mut().unwrap().push(serde_json::json!({
            "mesh": 0,
            "weights": [0.75]
        }));
        doc["scenes"][0]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(1));
        let mut json = serde_json::to_vec(&doc).unwrap();
        json.resize(json.len().next_multiple_of(4), b' ');
        binary.json = Cow::Owned(json);

        let model = decode_glb(&binary.to_vec().unwrap()).unwrap();
        let meshes = &model.rig.as_ref().unwrap().meshes;
        assert!(Arc::ptr_eq(&meshes[0].morphs[0].deltas, &meshes[1].morphs[0].deltas));
        assert_eq!(model.morph_storage_bytes(), size_of::<MorphDelta>());
        assert_eq!(model.primitives[0].vertices[2].position[2], 0.5);
        assert_eq!(model.primitives[1].vertices[2].position[2], 1.5);
    }

    #[test]
    fn vrm_blend_cutouts_preserve_existing_glb_material_modes() {
        let original = sparse_fixture(2);
        let mut binary = gltf::binary::Glb::from_slice(&original).unwrap();
        let mut doc: serde_json::Value = serde_json::from_slice(&binary.json).unwrap();
        doc["materials"] = serde_json::json!([{"alphaMode": "BLEND"}]);
        doc["meshes"][0]["primitives"][0]["material"] = serde_json::json!(0);

        for is_vrm in [false, true] {
            if is_vrm {
                doc["extensionsUsed"] = serde_json::json!(["VRMC_vrm"]);
                doc["extensions"] = serde_json::json!({"VRMC_vrm": {"specVersion": "1.0"}});
            }
            let mut json = serde_json::to_vec(&doc).unwrap();
            json.resize(json.len().next_multiple_of(4), b' ');
            binary.json = Cow::Owned(json);
            let model = decode_glb(&binary.to_vec().unwrap()).unwrap();
            assert_eq!(model.primitives[0].material.alpha_cutoff, is_vrm.then_some(0.25));
        }
    }
}
