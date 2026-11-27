use std::collections::BTreeMap;

use super::{
    LocalTransform, Model, Property, Vec3, joint_matrices, model_transforms, valid_position,
};
use serde::{Deserialize, Serialize};

/// Instance overrides on named materials and articulated glTF nodes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterLook {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, [f32; 3]>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub scales: BTreeMap<String, [f32; 3]>,
}

impl CharacterLook {
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty() && self.scales.is_empty()
    }

    pub fn validate(&self) -> Result<(), String> {
        let label = |name: &str| {
            !name.trim().is_empty() && name.len() <= 128 && !name.chars().any(char::is_control)
        };
        if self.colors.len() <= 24
            && self.scales.len() <= 16
            && self
                .colors
                .iter()
                .all(|(name, value)| label(name) && value.iter().all(|v| (0.0..=1.0).contains(v)))
            && self
                .scales
                .iter()
                .all(|(name, value)| label(name) && value.iter().all(|v| (0.60..=1.45).contains(v)))
        {
            Ok(())
        } else {
            Err("Некорректные цвета или пропорции персонажа.".into())
        }
    }
}

impl Model {
    pub(crate) fn has_named_material(&self, name: &str) -> bool {
        self.primitives
            .iter()
            .any(|part| part.material_name.as_deref() == Some(name))
    }

    pub(crate) fn with_character_look(&self, look: &CharacterLook) -> Result<Self, String> {
        look.validate()?;
        let mut output = self.clone();
        for primitive in &mut output.primitives {
            if let Some(value) = primitive
                .material_name
                .as_deref()
                .and_then(|name| look.colors.get(name))
            {
                primitive.material.base_color[..3].copy_from_slice(value);
            }
        }
        if look.scales.is_empty() {
            return Ok(output);
        }
        let rig = output
            .rig
            .as_mut()
            .map(std::sync::Arc::make_mut)
            .ok_or("У модели нет каркаса.")?;
        let mut factors = vec![Vec3::ONE; rig.nodes.len()];
        for (name, factor) in &look.scales {
            let mut matches = rig
                .nodes
                .iter_mut()
                .enumerate()
                .filter(|(_, node)| node.name.as_deref() == Some(name));
            let (index, node) = matches
                .next()
                .ok_or_else(|| format!("Узла «{name}» нет в модели."))?;
            if matches.next().is_some() {
                return Err(format!("В модели несколько узлов «{name}»."));
            }
            let LocalTransform::Trs { scale, .. } = &mut node.local else {
                return Err(format!(
                    "Узел «{name}» использует матрицу без отдельных пропорций."
                ));
            };
            factors[index] = Vec3::from_array(*factor);
            *scale *= factors[index];
        }
        for clip in &mut output.animations {
            for channel in &mut clip.channels {
                let factor = factors[channel.node];
                if matches!(channel.property, Property::Scale) && factor != Vec3::ONE {
                    for sample in std::sync::Arc::make_mut(&mut channel.values) {
                        *sample *= factor.extend(1.0);
                    }
                }
            }
        }
        let locals = rig.nodes.iter().map(|node| node.local).collect::<Vec<_>>();
        let globals = model_transforms(rig, &locals);
        let joints = joint_matrices(&rig.skins, &globals);
        for (node, transform) in rig.nodes.iter_mut().zip(&globals) {
            node.rest = *transform;
        }
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        for (mesh, primitive) in rig.meshes.iter().zip(&mut output.primitives) {
            for (rest, adjusted) in primitive
                .vertices
                .iter_mut()
                .zip(mesh.posed_vertices(&globals, &joints))
            {
                if !valid_position(adjusted.position) {
                    return Err("Пропорции выводят геометрию за пределы модели.".into());
                }
                let p = Vec3::from_array(adjusted.position);
                low = low.min(p);
                high = high.max(p);
                *rest = adjusted;
            }
        }
        output.bounds_min = low.to_array();
        output.bounds_max = high.to_array();
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_color_and_proportion_survive_an_existing_animation() {
        let base = super::super::decode_glb(include_bytes!("../demo_assets/world_palette/fox.glb"))
            .unwrap();
        let look = CharacterLook {
            colors: [("Голова".into(), [0.2, 0.3, 0.7])].into(),
            scales: [("FoxHead".into(), [1.25, 1.10, 1.05])].into(),
        };
        let tuned = base.with_character_look(&look).unwrap();
        let head = |model: &Model| {
            model
                .primitives
                .iter()
                .position(|part| part.material_name.as_deref() == Some("Голова"))
                .unwrap()
        };
        let index = head(&base);
        assert_eq!(
            tuned.primitives[index].material.base_color[..3],
            [0.2, 0.3, 0.7]
        );
        let animated = |model: &Model| model.pose(Some(1), 0.18).unwrap()[index][0].position;
        assert_ne!(animated(&tuned), animated(&base));
        assert_eq!(tuned.animations.len(), base.animations.len());
        assert!(
            tuned
                .bounds_min
                .iter()
                .chain(&tuned.bounds_max)
                .all(|v| v.is_finite())
        );
    }
}
