use std::collections::HashMap;

use glam::{Affine3A, EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

use super::{
    LocalTransform, MAX_INDICES, MAX_NODES, MAX_PRIMITIVES, MAX_SKINS, MAX_TEXTURE_BYTES,
    MAX_VERTICES, Material, Model, ModelRegion, RestMesh, RigNode, Skin, valid_affine,
    valid_position,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartSlot {
    Head,
    Hair,
    Clothing,
    Accessory,
}

impl PartSlot {
    pub const ALL: [Self; 4] = [Self::Head, Self::Hair, Self::Clothing, Self::Accessory];

    pub fn label(self) -> &'static str {
        match self {
            Self::Head => "Голова",
            Self::Hair => "Причёска",
            Self::Clothing => "Одежда",
            Self::Accessory => "Аксессуар",
        }
    }

    fn includes(self, region: ModelRegion) -> bool {
        use ModelRegion::*;
        match self {
            Self::Head => matches!(
                region,
                MayaFace
                    | MayaLashes
                    | MayaEyes
                    | MayaTeeth
                    | StudioFace
                    | StudioLashes
                    | StudioEyes
                    | StudioTeeth
            ),
            Self::Hair => matches!(
                region,
                MayaHair | MayaPonytail | StudioScalp | StudioHair | StudioHairTip | StudioHairWisp
            ),
            Self::Clothing => matches!(region, MayaTop | MayaTrousers | StudioTop | StudioTrousers),
            Self::Accessory => false,
        }
    }
}

impl Model {
    /// Unambiguous glTF node names available as attachment anchors, including rigid animated rigs.
    pub fn attachment_bones(&self) -> Vec<&str> {
        self.attachment_nodes()
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    pub fn valid_attachment_transform(offset: [f32; 3], degrees: [f32; 3], scale: f32) -> bool {
        offset.iter().all(|value| (-30.0..=30.0).contains(value))
            && degrees
                .iter()
                .all(|value| (-3600.0..=3600.0).contains(value))
            && (0.01..=50.0).contains(&scale)
    }

    /// Attach the donor's rest geometry at its GLB origin, in the anchor's local axes.
    /// Keep the character's body bounds for height normalization and animation controls.
    pub fn with_bone_attachment(
        &self,
        detail: &Model,
        bone: &str,
        offset: [f32; 3],
        degrees: [f32; 3],
        scale: f32,
    ) -> Result<Model, String> {
        if !Self::valid_attachment_transform(offset, degrees, scale) {
            return Err("У детали недопустимые смещение, поворот или масштаб.".into());
        }
        if detail.primitives.is_empty() {
            return Err("В выбранном GLB нет геометрии детали.".into());
        }
        let base = self
            .rig
            .as_ref()
            .ok_or("У персонажа нет скелета или анимированных узлов.")?;
        let anchor = self
            .attachment_nodes()
            .into_iter()
            .find_map(|(name, node)| (name == bone).then_some(node))
            .ok_or(
                "Выбранный узел отсутствует, имеет повторяющееся имя или вырожденный масштаб.",
            )?;
        let combined = self.primitives.iter().chain(&detail.primitives);
        if self.primitives.len() + detail.primitives.len() > MAX_PRIMITIVES
            || combined
                .clone()
                .map(|mesh| mesh.vertices.len())
                .sum::<usize>()
                > MAX_VERTICES
            || combined.map(|mesh| mesh.indices.len()).sum::<usize>() > MAX_INDICES
            || base.nodes.len() == MAX_NODES
            || self
                .images
                .iter()
                .chain(&detail.images)
                .map(|image| image.pixels.len())
                .sum::<usize>()
                > MAX_TEXTURE_BYTES
        {
            return Err("Деталь превышает лимит ресурсов персонажа.".into());
        }

        let bone_rest = base.nodes[anchor].rest;
        let (_, axes, origin) = bone_rest.to_scale_rotation_translation();
        if !axes.is_finite() || axes.length_squared() <= f32::EPSILON {
            return Err("У выбранного узла некорректный поворот.".into());
        }
        let [x, y, z] = degrees.map(f32::to_radians);
        let transform = Affine3A::from_scale_rotation_translation(
            Vec3::splat(scale),
            Quat::from_euler(EulerRot::XYZ, x, y, z),
            Vec3::from_array(offset),
        );
        let framed_rest = Affine3A::from_rotation_translation(axes.normalize(), origin) * transform;
        // Compensate for rest units from the donor rig. Pose deltas still follow the bone.
        let local = Mat4::from(Affine3A::from_mat4(bone_rest).inverse() * framed_rest);
        let rest = bone_rest * local;
        if !valid_affine(local) || !valid_affine(rest) {
            return Err("Не удалось вычислить привязку к выбранному узлу.".into());
        }

        let mut output = self.clone();
        let mut rig = base.as_ref().clone();
        let node = rig.nodes.len();
        rig.nodes.push(RigNode {
            local: LocalTransform::Matrix(local),
            parent: Some(anchor),
            rest,
            name: None,
        });
        let globals: Vec<_> = rig.nodes.iter().map(|node| node.rest).collect();
        let texture_offset = output.images.len();
        output.images.extend(detail.images.iter().cloned());
        for mesh in &detail.primitives {
            let rest_mesh = RestMesh {
                node,
                skin: None,
                influences: Vec::new(),
                vertices: mesh.vertices.clone(),
                morphs: Vec::new(),
            };
            let mut primitive = mesh.clone();
            primitive.region = ModelRegion::Other;
            primitive.vertices = rest_mesh.posed_vertices(&globals, &[]).collect();
            if primitive
                .vertices
                .iter()
                .any(|vertex| !valid_position(vertex.position))
            {
                return Err("Координаты детали выходят за допустимый диапазон.".into());
            }
            remap_material_textures(&mut primitive.material, texture_offset);
            output.primitives.push(primitive);
            rig.meshes.push(rest_mesh);
        }
        if let Some(deltas) = &self.secondary_deltas {
            let mut extended = deltas.to_vec();
            extended.resize_with(output.primitives.len(), Vec::new);
            output.secondary_deltas = Some(extended.into());
        }
        output.rig = Some(rig.into());
        Ok(output)
    }

    fn attachment_nodes(&self) -> Vec<(&str, usize)> {
        let Some(rig) = &self.rig else {
            return Vec::new();
        };
        let mut counts = HashMap::new();
        for name in rig.nodes.iter().filter_map(|node| node.name.as_deref()) {
            *counts.entry(name).or_insert(0_usize) += 1;
        }
        rig.nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let name = node.name.as_deref()?;
                let determinant = node.rest.determinant();
                (!name.trim().is_empty()
                    && name.chars().count() <= 128
                    && counts[name] == 1
                    && determinant.is_finite()
                    && determinant != 0.0)
                    .then_some((name, index))
            })
            .collect()
    }

    /// Transplant weighted primitives onto an existing skeleton without copying its clips.
    pub fn with_character_part(&self, detail: &Model, slot: PartSlot) -> Result<Model, String> {
        let error = || {
            "Скелет детали несовместим. Экспортируй взвешенный меш с костями и исходной позой этого персонажа.".to_owned()
        };
        let base = self.rig.as_ref().ok_or_else(error)?;
        let detail_rig = detail.rig.as_ref().ok_or_else(error)?;
        if slot != PartSlot::Accessory
            && !self
                .primitives
                .iter()
                .any(|part| slot.includes(part.region))
        {
            return Err("В исходной модели для этой детали нужны отдельные материалы головы, волос или одежды.".into());
        }

        let mut names = HashMap::new();
        for skin in &base.skins {
            for &(joint, _) in &skin.joints {
                let node = &base.nodes[joint];
                if let Some(name) = node.name.as_deref()
                    && names.insert(name, joint).is_some_and(|old| old != joint)
                {
                    return Err("В скелете исходной модели повторяются имена костей.".into());
                }
            }
        }

        let has_regions = detail
            .primitives
            .iter()
            .any(|part| slot.includes(part.region));
        let added: Vec<_> = detail
            .primitives
            .iter()
            .zip(&detail_rig.meshes)
            .filter(|(part, _)| !has_regions || slot.includes(part.region))
            .collect();
        if added.is_empty() {
            return Err("В выбранном GLB нет геометрии детали.".into());
        }
        let retained: Vec<_> = self
            .primitives
            .iter()
            .enumerate()
            .filter(|(_, part)| !slot.includes(part.region))
            .map(|(index, _)| index)
            .collect();
        let vertex_count = retained
            .iter()
            .map(|&index| self.primitives[index].vertices.len())
            .sum::<usize>()
            + added
                .iter()
                .map(|(part, _)| part.vertices.len())
                .sum::<usize>();
        if retained.len() + added.len() > MAX_PRIMITIVES
            || vertex_count > MAX_VERTICES
            || base.skins.len() + detail_rig.skins.len() > MAX_SKINS
            || self
                .images
                .iter()
                .chain(&detail.images)
                .map(|image| image.pixels.len())
                .sum::<usize>()
                > MAX_TEXTURE_BYTES
        {
            return Err("Деталь превышает лимит ресурсов персонажа.".into());
        }
        let mut rig = base.as_ref().clone();
        let mut output = self.clone();
        rig.meshes = retained
            .iter()
            .map(|&index| base.meshes[index].clone())
            .collect();
        output.primitives = retained
            .iter()
            .map(|&index| self.primitives[index].clone())
            .collect();
        let mut secondary = self.secondary_deltas.as_ref().map(|frames| {
            retained
                .iter()
                .map(|&index| frames[index].clone())
                .collect::<Vec<_>>()
        });
        let texture_offset = output.images.len();
        output.images.extend(detail.images.iter().cloned());
        let mut mapped_skins = HashMap::new();
        for (primitive, rest) in added {
            let skin_index = rest.skin.ok_or_else(error)?;
            if rest
                .influences
                .iter()
                .any(|influence| influence.weights.iter().sum::<f32>() < 0.9)
            {
                return Err(error());
            }
            let skin = if let Some(&known) = mapped_skins.get(&skin_index) {
                known
            } else {
                let joints = detail_rig.skins[skin_index]
                    .joints
                    .iter()
                    .map(|&(index, bind)| {
                        let node = &detail_rig.nodes[index];
                        let mapped = names
                            .get(node.name.as_deref().ok_or_else(error)?)
                            .copied()
                            .ok_or_else(error)?;
                        if node
                            .rest
                            .to_cols_array()
                            .iter()
                            .zip(base.nodes[mapped].rest.to_cols_array())
                            .any(|(&source, target)| (source - target).abs() > 0.006)
                        {
                            return Err(error());
                        }
                        Ok((mapped, bind))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let next = rig.skins.len();
                rig.skins.push(Skin { joints });
                mapped_skins.insert(skin_index, next);
                next
            };
            let mut mesh = rest.clone();
            mesh.skin = Some(skin);
            mesh.node = rig.skins[skin].joints[0].0;
            rig.meshes.push(mesh);
            let mut primitive = primitive.clone();
            remap_material_textures(&mut primitive.material, texture_offset);
            output.primitives.push(primitive);
            if let Some(secondary) = &mut secondary {
                secondary.push(Vec::new());
            }
        }
        output.rig = Some(rig.into());
        output.refresh_morph_names()?;
        output.secondary_deltas = secondary.map(Into::into);
        Ok(output)
    }
}

fn remap_material_textures(material: &mut Material, offset: usize) {
    for index in [
        &mut material.base_color_texture,
        &mut material.metallic_roughness_texture,
        &mut material.normal_texture,
        &mut material.emissive_texture,
    ] {
        *index = index.map(|index| index + offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::decode_glb;

    #[test]
    fn a_weighted_head_uses_the_existing_clips_and_rejects_an_unrigged_mesh() {
        let model = decode_glb(include_bytes!("../demo_assets/emma.glb")).unwrap();
        let crate_mesh = decode_glb(include_bytes!("../demo_assets/crate.glb")).unwrap();
        assert!(
            model
                .with_character_part(&crate_mesh, PartSlot::Head)
                .is_err()
        );
        let changed = model.with_character_part(&model, PartSlot::Head).unwrap();
        assert_eq!(model.animations.len(), changed.animations.len());
        assert_eq!(model.primitives.len(), changed.primitives.len());
        assert_eq!(
            changed.pose(Some(0), 0.3).unwrap().len(),
            changed.primitives.len()
        );
    }

    #[test]
    fn rigid_attachment_uses_unskinned_animated_node_and_keeps_its_body_scale() {
        let mut fox = decode_glb(include_bytes!("../demo_assets/world_palette/fox.glb")).unwrap();
        let detail = decode_glb(include_bytes!("../demo_assets/crate.glb")).unwrap();
        assert!(fox.rig.as_ref().unwrap().skins.is_empty());
        assert!(fox.bone_names().contains(&"FoxTail"));
        assert!(fox.attachment_bones().contains(&"FoxTail"));
        assert!(detail.attachment_bones().is_empty());
        assert!(
            fox.with_bone_attachment(&detail, "Missing", [0.0; 3], [0.0; 3], 0.2)
                .is_err()
        );
        assert!(
            fox.with_bone_attachment(&detail, "FoxTail", [f32::NAN; 3], [0.0; 3], 0.2)
                .is_err()
        );

        // Imported skeletons often carry a conversion between source units and metres.
        let source_units = 100.0;
        let rig = std::sync::Arc::make_mut(fox.rig.as_mut().unwrap());
        for node in &mut rig.nodes {
            node.rest = Mat4::from_scale(Vec3::splat(source_units)) * node.rest;
            if node.parent.is_none() {
                let LocalTransform::Trs { scale, .. } = &mut node.local else {
                    panic!("fixture root must use TRS")
                };
                *scale *= source_units;
            }
        }
        let original_parts = fox.primitives.len();
        let attached = fox
            .with_bone_attachment(
                &detail,
                "FoxTail",
                [0.12, 0.08, -0.2],
                [15.0, 32.0, 0.0],
                0.2,
            )
            .unwrap();
        assert_eq!(
            attached.primitives.len(),
            original_parts + detail.primitives.len()
        );
        assert_eq!(fox.bounds_min, attached.bounds_min);
        assert_eq!(fox.bounds_max, attached.bounds_max);
        let original_edge = |a: &super::super::Vertex, b: &super::super::Vertex| {
            Vec3::from_array(a.position).distance(Vec3::from_array(b.position))
        };
        let donor = &detail.primitives[0].vertices;
        let attachment = &attached.primitives[original_parts].vertices;
        let second = donor
            .iter()
            .position(|vertex| original_edge(&donor[0], vertex) > 0.15)
            .unwrap();
        assert!(
            (original_edge(&attachment[0], &attachment[second])
                - 0.2 * original_edge(&donor[0], &donor[second]))
            .abs()
                < 1e-4
        );

        let clip = attached
            .animations
            .iter()
            .position(|clip| clip.name == "Walk")
            .unwrap();
        let px = |time| {
            Vec3::from_array(attached.pose(Some(clip), time).unwrap()[original_parts][0].position)
        };
        let opposite = fox.animations[clip].duration * 0.5;
        let travel = px(0.0).distance(px(opposite));
        assert!(travel > 0.015, "tail attachment travelled only {travel} m");
        let base = fox.rig.as_ref().unwrap();
        let anchor = fox
            .attachment_nodes()
            .into_iter()
            .find(|(name, _)| *name == "FoxTail")
            .unwrap()
            .1;
        let pose = super::super::animation_pose(base, fox.animations.get(clip), opposite);
        let matrices = super::super::model_transforms(base, &pose);
        let expected = (matrices[anchor] * base.nodes[anchor].rest.inverse())
            .transform_point3(Vec3::from_array(attachment[0].position));
        assert!(expected.distance(px(opposite)) < 1e-4);
    }

    #[test]
    fn rigid_attachment_survives_skinned_pose_and_secondary_motion() {
        let model =
            decode_glb(include_bytes!("../demo_assets/character-showcase/mira.glb")).unwrap();
        let detail = decode_glb(include_bytes!("../demo_assets/crate.glb")).unwrap();
        let bone = "mixamorig:RightHand";
        assert!(model.attachment_bones().contains(&bone));
        let attached = model
            .with_bone_attachment(&detail, bone, [0.0, -0.08, 0.0], [0.0; 3], 0.12)
            .unwrap();
        assert_eq!(model.morph_names(), attached.morph_names());
        let rest = &attached.primitives[model.primitives.len()].vertices[0];
        let arms = [super::super::BoneRotation {
            bone: bone.to_owned(),
            degrees: [40.0, 0.0, 0.0],
        }];
        let secondary = super::super::SecondaryPose {
            hair_lift: 0.4,
            ..Default::default()
        };
        let posed = attached
            .pose_with_secondary(None, 0.0, &arms, secondary)
            .unwrap();
        assert_eq!(posed.len(), attached.primitives.len());
        assert!(
            Vec3::from_array(rest.position)
                .distance(Vec3::from_array(posed[model.primitives.len()][0].position))
                > 0.005
        );
    }
}
