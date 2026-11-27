use serde_json::{Value, json};

use crate::{
    rules::{Action, Condition},
    scene::Scene,
};

const MAX_BYTES: usize = 24 * 1024;
const SECTION_BYTES: usize = (MAX_BYTES - 512) / 7;
const ROUTE_LIMIT: usize = 32;
const TEXT_LIMIT: usize = 180;

/// JSON context for proposing known edits. Resource payloads and mutable play state stay local.
pub fn catalogue(scene: &Scene) -> String {
    let mut truncated = false;
    let objects = section(
        scene
            .world
            .objects
            .iter()
            .map(|object| {
                json!({
                    "id": object.id.to_string(), "name": object.name, "kind": object.kind,
                    "position": object.position,
                })
            })
            .chain(scene.world.models.iter().map(|model| {
                json!({
                    "id": model.id.to_string(), "name": model.label, "kind": "GLB",
                    "position": model.position,
                })
            })),
        &mut truncated,
    );
    let npcs = section(
        scene.npcs.iter().map(|npc| {
            let waypoints: Vec<_> = npc
                .route
                .iter()
                .take(ROUTE_LIMIT)
                .enumerate()
                .map(|(index, point)| json!({ "index": index, "position": point.position }))
                .collect();
            json!({
                "id": npc.id.to_string(), "name": npc.avatar.name, "waypoints": waypoints,
                "waypoints_total": npc.route.len(), "truncated": npc.route.len() > ROUTE_LIMIT,
            })
        }),
        &mut truncated,
    );
    let replies = section(
        scene.story.conversations.iter().flat_map(|conversation| {
            conversation.nodes.iter().flat_map(move |node| {
                std::iter::once((None, node.text.as_str()))
                    .chain(node.choices.iter().map(|choice| (Some(choice.id), choice.text.as_str())))
                    .map(move |(choice, text)| {
                        let (text, truncated) = short_text(text);
                        json!({
                            "npc_id": conversation.npc_id.to_string(), "npc_name": conversation.name,
                            "node_id": node.id, "choice_id": choice, "text": text,
                            "truncated": truncated,
                        })
                    })
            })
        }),
        &mut truncated,
    );
    let rules = section(
        scene.rules.rules.iter().map(|rule| {
            let counters: Vec<_> = rule
                .actions
                .iter()
                .filter_map(|action| {
                    let (id, mode, value) = match *action {
                        Action::SetCounter { counter_id, value } => (counter_id, "set", value),
                        Action::AddCounter { counter_id, delta } => (counter_id, "add", delta),
                        _ => return None,
                    };
                    Some(json!({ "counter_id": id, "mode": mode, "value": value }))
                })
                .collect();
            let thresholds: Vec<_> = rule
                .conditions
                .iter()
                .filter_map(|condition| match *condition {
                    Condition::Counter {
                        counter_id,
                        comparison,
                        value,
                    } => Some(json!({
                        "counter_id": counter_id, "comparison": comparison, "value": value,
                    })),
                    _ => None,
                })
                .collect();
            json!({
                "id": rule.id, "name": rule.name, "enabled": rule.enabled, "draft": rule.draft,
                "event": rule.event, "conditions": rule.conditions, "actions": rule.actions,
                "counter_actions": counters, "counter_conditions": thresholds,
            })
        }),
        &mut truncated,
    );
    let counters = section(
        scene.rules.counters.iter().map(
            |counter| json!({ "id": counter.id, "name": counter.name, "initial": counter.initial }),
        ),
        &mut truncated,
    );
    let zones = section(
        scene.rules.zones.iter().map(|zone| {
            json!({
                "id": zone.id, "name": zone.name, "position": zone.position, "size": zone.size,
            })
        }),
        &mut truncated,
    );
    let locations = section(
        scene
            .linked_scenes
            .targets()
            .map(|(id, name)| json!({"id":id,"name":name})),
        &mut truncated,
    );
    json!({
        "objects": objects, "npcs": npcs, "replies": replies, "rules": rules,
        "counters": counters, "zones": zones, "locations": locations, "truncated": truncated,
    })
    .to_string()
}

fn section(entries: impl Iterator<Item = Value>, truncated: &mut bool) -> Vec<Value> {
    let mut values = Vec::new();
    let mut bytes = 2;
    for value in entries {
        let size = value.to_string().len() + usize::from(!values.is_empty());
        if bytes + size > SECTION_BYTES {
            *truncated = true;
            continue;
        }
        *truncated |= value.get("truncated").and_then(Value::as_bool) == Some(true);
        bytes += size;
        values.push(value);
    }
    values
}

fn short_text(text: &str) -> (&str, bool) {
    let end = text
        .char_indices()
        .nth(TEXT_LIMIT)
        .map_or(text.len(), |(at, _)| at);
    (&text[..end], end < text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        npcs::FIRST_NPC_ID,
        scene_patch::{Base, Op, Proposal},
        story::Story,
        world::WorldKind,
    };

    #[test]
    fn catalogue_caps_row_groups_and_keeps_exact_ids_and_known_values() {
        let mut scene = Scene::empty();
        while scene.world.add(WorldKind::Crate).is_some() {
            scene.world.objects.last_mut().unwrap().name = "Я".repeat(80);
        }
        let zone = scene.rules.add_zone([2.0, 0.0, 5.0]).unwrap();
        let count = scene.rules.add_counter().unwrap();
        scene.rules.counters[0].initial = 4;
        scene.story = Story::demo(FIRST_NPC_ID, [0.0; 2]);
        scene.story.conversations[0].nodes[0].text = "Я".repeat(300);
        let context = catalogue(&scene);
        assert!(context.len() <= MAX_BYTES);
        let data: Value = serde_json::from_str(&context).unwrap();
        assert_eq!(data["truncated"], true);
        assert_eq!(data["zones"][0]["id"], zone);
        assert_eq!(data["zones"][0]["position"], json!([2.0, 0.0, 5.0]));
        assert_eq!(
            data["counters"][0],
            json!({ "id": count, "name": "Счётчик 1", "initial": 4 })
        );
        let reply = &data["replies"][0];
        assert_eq!(reply["npc_id"], FIRST_NPC_ID.to_string());
        assert_eq!(reply["truncated"], true);
        assert_eq!(reply["text"].as_str().unwrap().chars().count(), TEXT_LIMIT);
        assert!(data["objects"].as_array().unwrap().len() < scene.world.objects.len());
    }

    #[test]
    fn wire_can_copy_large_ids_from_catalogue_without_number_rounding() {
        let id = FIRST_NPC_ID + 7;
        let scene = Scene::empty();
        let base = Base::capture(&scene).unwrap();
        for wire_id in [json!(id.to_string()), json!(id)] {
            let wire = json!({
                "ops": [{ "op": "set_npc_waypoint", "npc_id": wire_id, "waypoint": 0, "position": [1, 0, 1] }]
            });
            let parsed = Proposal::decode_ops(wire.to_string().as_bytes(), base.clone()).unwrap();
            assert!(matches!(parsed.ops[0], Op::SetNpcWaypoint { npc_id, .. } if npc_id == id));
            assert_eq!(
                serde_json::to_value(parsed).unwrap()["ops"][0]["npc_id"],
                id.to_string()
            );
        }
    }
}
