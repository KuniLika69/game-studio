use std::{fs::File, io::Read, path::Path};

use serde::Deserialize;

use crate::{Avatar, ImportError, Scene, rules::RuleSet, story::Story, world::World};

const VERSION: u32 = 1;
const MAX_BYTES: u64 = 256 * 1024;
const SAMPLE: &[u8] = include_bytes!("../demo_assets/gameplay/watchkeeper.mechanics.json");
pub(super) const SAMPLE_NAME: &str = "Маршрут смотрителя";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pack {
    format_version: u32,
    #[serde(default)]
    avatar: Option<Avatar>,
    world: World,
    story: Story,
    rules: RuleSet,
}

pub(super) fn matches(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".mechanics.json"))
}

pub(super) fn sample() -> Result<Scene, ImportError> {
    decode(SAMPLE).map_err(|error| error.with_file("watchkeeper.mechanics.json"))
}

pub(super) fn load(path: &Path) -> Result<Scene, ImportError> {
    let mut data = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut data))
        .map_err(|error| ImportError::read(path, error))?;
    decode(&data).map_err(|error| error.with_file(path))
}

fn decode(data: &[u8]) -> Result<Scene, ImportError> {
    let error = |detail: String| ImportError::new("Пакет механик", detail);
    if data.len() as u64 > MAX_BYTES {
        return Err(error("Файл больше 256 КиБ.".into()));
    }
    let pack: Pack = serde_json::from_slice(data).map_err(|reason| {
        error(format!(
            "Проверьте JSON и значения полей в строке {}, столбце {}.",
            reason.line(),
            reason.column()
        ))
    })?;
    if pack.format_version != VERSION {
        return Err(error(format!(
            "Версия {} пока не поддерживается.",
            pack.format_version
        )));
    }
    let mut scene = Scene::empty();
    scene.avatar = pack.avatar;
    scene.world = pack.world;
    scene.story = pack.story;
    scene.rules = pack.rules;
    scene.validate_import()?;
    for rule in scene
        .rules
        .rules
        .iter()
        .filter(|rule| rule.enabled && !rule.draft)
    {
        if !rule.bindings_ready() || !rule.authored_targets_exist(&scene) {
            return Err(error(format!(
                "Проверьте событие и цели правила «{}» #{}.",
                rule.name, rule.id
            )));
        }
    }
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rules::{Event, Session},
        world::{Interaction, InteractionState},
    };

    fn run_route(scene: &Scene, allowed_id: u32, denied_id: u32) {
        let world = &scene.world;
        let door = world.buildings[0].front_door().unwrap();
        assert!(door.settings.rule_lock);
        assert_eq!(door.settings.required_key, None);
        let action = Event::Door { door_id: door.id };
        let at_door = door.interaction_position();
        let mut interactions = InteractionState::default();
        let mut runtime = Session::new(&scene.rules);
        for id in [denied_id, allowed_id] {
            let position = world.key(id).unwrap().position;
            assert_eq!(
                world.interact(&mut interactions, position),
                Some(Interaction::Key { id })
            );
            runtime.dispatch_at(
                &scene.rules,
                world,
                &scene.story,
                &mut interactions,
                Event::Pickup { item_id: id },
                position,
            );
            runtime.dispatch_at(
                &scene.rules,
                world,
                &scene.story,
                &mut interactions,
                action,
                at_door,
            );
            if id == denied_id {
                assert_eq!(runtime.quest_stage(7), None);
                assert!(matches!(
                    world.interact(&mut interactions, at_door),
                    Some(Interaction::RuleLockedDoor { .. })
                ));
            }
        }
        assert_eq!(runtime.quest_stage(7), Some(2));
        assert!(matches!(
            world.interact(&mut interactions, at_door),
            Some(Interaction::Door { opening: true, .. })
        ));
        let zone = &scene.rules.zones[0];
        let [x, _, z] = zone.position;
        assert_eq!(
            runtime.move_player(
                &scene.rules,
                world,
                &scene.story,
                &mut interactions,
                at_door,
                [x, 0.2, z]
            ),
            1
        );
        assert_eq!(runtime.quest_stage(7), Some(3));
    }

    #[test]
    fn a6_pack_routes_pickup_door_and_zone_through_authored_rules() {
        run_route(&sample().unwrap(), 41, 42);
    }

    #[test]
    fn a6_json_edits_change_access_without_changing_engine_code() {
        let original = std::str::from_utf8(SAMPLE).unwrap();
        let changed = original
            .replace("\"item_id\": 41", "\"item_id\": 42")
            .replace("Маршрут смотрителя", "Новый маршрут из данных");
        let scene = decode(changed.as_bytes()).unwrap();
        assert_eq!(
            scene.story.quest(7).unwrap().title,
            "Новый маршрут из данных"
        );
        run_route(&scene, 42, 41);
    }

    #[test]
    fn a6_pack_rejects_dangling_authored_targets() {
        let original = std::str::from_utf8(SAMPLE).unwrap();
        let invalid = original.replace("\"zone_id\": 17", "\"zone_id\": 99");
        assert!(matches!(
            decode(invalid.as_bytes()),
            Err(ImportError::Invalid { .. })
        ));
    }
}
