use std::collections::HashMap;

use super::{Animation, LocalTransform, Model, RigNode};

pub const MAX_IMPORTED_CLIPS: usize = 16;
const REST_TOLERANCE: f32 = 0.006;

impl Model {
    /// Remap channels when donor and recipient use the same rig and neutral transforms.
    pub fn with_compatible_clip(&self, donor: &Model, clip: usize) -> Result<Model, String> {
        let target_rig = self
            .rig
            .as_ref()
            .filter(|rig| !rig.skins.is_empty())
            .ok_or("У выбранного персонажа нет взвешенного скелета.")?;
        let donor_rig = donor
            .rig
            .as_ref()
            .ok_or("В GLB-источнике нет скелета анимации.")?;
        let origin = donor
            .animations
            .get(clip)
            .ok_or("Выбранный клип GLB отсутствует.")?;
        if origin.duration <= 0.0 || origin.channels.is_empty() {
            return Err("Для импорта нужен клип с ключами и положительной длительностью.".into());
        }
        let target = names(&target_rig.nodes);
        let donor_names = names(&donor_rig.nodes);
        let mut mapped = HashMap::new();
        let mut channels = Vec::with_capacity(origin.channels.len());
        for channel in &origin.channels {
            let source_index = channel.node;
            let destination = if let Some(&known) = mapped.get(&source_index) {
                known
            } else {
                let source_node = &donor_rig.nodes[source_index];
                let name = source_node
                    .name
                    .as_deref()
                    .ok_or("В клипе есть анимируемая кость без имени.")?;
                if donor_names.get(name) != Some(&Some(source_index)) {
                    return Err(format!("В GLB-источнике имя кости «{name}» неоднозначно."));
                }
                let destination = match target.get(name) {
                    Some(Some(index)) => *index,
                    Some(None) => {
                        return Err(format!(
                            "В скелете персонажа имя кости «{name}» неоднозначно."
                        ));
                    }
                    None => return Err(format!("В скелете персонажа нет кости «{name}».")),
                };
                if matches!(
                    target_rig.nodes[destination].local,
                    LocalTransform::Matrix(_)
                ) || !same_chain(
                    &donor_rig.nodes,
                    source_index,
                    &target_rig.nodes,
                    destination,
                ) {
                    return Err(format!(
                        "Кость «{name}» имеет другую иерархию или исходную позу."
                    ));
                }
                mapped.insert(source_index, destination);
                destination
            };
            let mut channel = channel.clone();
            channel.node = destination;
            channels.push(channel);
        }
        let mut output = self.clone();
        output.animations.push(Animation {
            name: unique_clip_name(&self.animations, &origin.name),
            duration: origin.duration,
            channels,
        });
        output.refresh_rigid_pose_targets();
        Ok(output)
    }
}

fn names(nodes: &[RigNode]) -> HashMap<&str, Option<usize>> {
    let mut indices = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        if let Some(name) = node.name.as_deref() {
            indices
                .entry(name)
                .and_modify(|found| *found = None)
                .or_insert(Some(index));
        }
    }
    indices
}

fn same_chain(source: &[RigNode], mut from: usize, target: &[RigNode], mut to: usize) -> bool {
    loop {
        let a = &source[from];
        let b = &target[to];
        if a.name != b.name || !close_transform(a.local.matrix(), b.local.matrix()) {
            return false;
        }
        match (a.parent, b.parent) {
            (Some(parent_a), Some(parent_b)) => {
                from = parent_a;
                to = parent_b;
            }
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn close_transform(a: glam::Mat4, b: glam::Mat4) -> bool {
    a.to_cols_array()
        .into_iter()
        .zip(b.to_cols_array())
        .all(|(a, b)| (a - b).abs() <= REST_TOLERANCE)
}

fn unique_clip_name(existing: &[Animation], name: &str) -> String {
    let taken = |name: &str| existing.iter().any(|clip| clip.name == name);
    if !taken(name) {
        return name.to_owned();
    }
    let mut index = 2_usize;
    loop {
        let candidate = format!("{name} ({index})");
        if !taken(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::decode_glb;
    use std::sync::Arc;

    #[test]
    fn appends_only_chosen_clip_and_keeps_gait_indexes() {
        let model = decode_glb(include_bytes!("../demo_assets/emma.glb")).unwrap();
        let index = model
            .animations
            .iter()
            .position(|clip| clip.name == "Wave")
            .unwrap();
        let extended = model.with_compatible_clip(&model, index).unwrap();
        let length = model.animations.len();
        assert_eq!(extended.animations.len(), length + 1);
        assert_eq!(extended.animations[length].name, "Wave (2)");
        for (before, after) in model.animations.iter().zip(&extended.animations) {
            assert_eq!(
                (&before.name, before.duration),
                (&after.name, after.duration)
            );
        }
        let original = model.pose(Some(index), 0.7).unwrap();
        let new = extended.pose(Some(length), 0.7).unwrap();
        assert!(
            original
                .iter()
                .flatten()
                .zip(new.iter().flatten())
                .all(|(a, b)| a.position == b.position)
        );
    }

    #[test]
    fn rejects_missing_ambiguous_reparented_or_changed_rest_bones() {
        let model = decode_glb(include_bytes!("../demo_assets/emma.glb")).unwrap();
        let idx = model.animations[0].channels[0].node;
        let mut changed = model.clone();
        let nodes = &mut Arc::make_mut(changed.rig.as_mut().unwrap()).nodes;
        nodes[idx].local = LocalTransform::Matrix(glam::Mat4::from_scale(glam::Vec3::splat(2.0)));
        assert!(
            model
                .with_compatible_clip(&changed, 0)
                .unwrap_err()
                .contains("исходную позу")
        );
        let mut changed = model.clone();
        let nodes = &mut Arc::make_mut(changed.rig.as_mut().unwrap()).nodes;
        let original = nodes[idx].name.clone();
        nodes[idx].name = Some("Absent_target_bone".into());
        assert!(
            model
                .with_compatible_clip(&changed, 0)
                .unwrap_err()
                .contains("нет кости")
        );
        let nodes = &mut Arc::make_mut(changed.rig.as_mut().unwrap()).nodes;
        nodes[idx].name = original.clone();
        let new_index = (idx + 1) % nodes.len();
        nodes[new_index].name = original;
        assert!(
            model
                .with_compatible_clip(&changed, 0)
                .unwrap_err()
                .contains("неоднозначно")
        );
        let mut changed = model.clone();
        let nodes = &mut Arc::make_mut(changed.rig.as_mut().unwrap()).nodes;
        let index = model.animations[0]
            .channels
            .iter()
            .map(|channel| channel.node)
            .find(|&i| nodes[i].parent.is_some())
            .unwrap();
        nodes[index].parent = None;
        assert!(
            model
                .with_compatible_clip(&changed, 0)
                .unwrap_err()
                .contains("иерархию")
        );
    }
}
