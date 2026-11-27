//! Materialize a generated layout as normal editable cells, goal and rule.
use crate::{
    dungeon::{self, Cell, Layout, Settings},
    map2d::{self, Collision, Layer, Map, Mode, Tile},
    rules::{Action, Event},
    scene::Scene,
};

pub const EXIT_NAME: &str = "Выход из подземелья";
pub const COUNTER_NAME: &str = "Выход найден";

pub fn scene(settings: Settings) -> Result<(Scene, Layout), String> {
    let layout = dungeon::generate(settings)?;
    let mut scene = map2d::scene(Mode::Top, false);
    let map = scene.map_2d.as_mut().expect("new top-down map");
    map.resize(settings.size);
    map.set_spawn(center(layout.entrance));
    for row in 0..settings.size[1] {
        for col in 0..settings.size[0] {
            let cell = [col, row];
            let tile = layout.tile(cell).expect("tile in layout");
            let index = match tile {
                dungeon::Tile::Room => 0,
                dungeon::Tile::Passage => 10,
                dungeon::Tile::Wall => 9,
            };
            map.paint(cell, Layer::Ground, Some(Tile { atlas: 1, index }));
            if tile == dungeon::Tile::Wall && !map.paint_collision(cell, Collision::Solid) {
                return Err(format!("Не удалось поставить стену в клетке {col}, {row}."));
            }
        }
    }
    map.paint(
        layout.entrance,
        Layer::Detail,
        Some(Tile { atlas: 1, index: 6 }),
    );
    map.paint(
        layout.exit,
        Layer::Detail,
        Some(Tile { atlas: 1, index: 8 }),
    );
    let exit_world = map.to_world(center(layout.exit));
    let zone_id = scene
        .rules
        .add_zone(exit_world)
        .ok_or("Нет места для зоны выхода.")?;
    let zone = scene.rules.zones.last_mut().expect("new exit zone");
    zone.name = EXIT_NAME.into();
    zone.size = [0.8, 1.5, 0.8];

    let counter_id = scene
        .rules
        .add_counter()
        .ok_or("Нет места для счётчика выхода.")?;
    scene
        .rules
        .counters
        .last_mut()
        .expect("new exit counter")
        .name = COUNTER_NAME.into();
    scene
        .rules
        .add_rule()
        .ok_or("Нет места для правила выхода.")?;
    let rule = scene.rules.rules.last_mut().expect("new exit rule");
    rule.name = format!("Найти выход · Seed {}", settings.seed);
    rule.event = Some(Event::EnterZone { zone_id });
    rule.actions = vec![Action::SetCounter {
        counter_id,
        value: 1,
    }];
    scene.validate()?;
    Ok((scene, layout))
}

pub fn route(map: &Map, exit: Cell) -> Option<Vec<Cell>> {
    if map.mode != Mode::Top {
        return None;
    }
    let entrance = map.spawn.map(|coordinate| coordinate.floor() as u16);
    dungeon::shortest_route(map.size, entrance, exit, |[x, y]| {
        map.collision_at([i32::from(x), i32::from(y)]) != Collision::Solid
    })
}

pub fn exit_cell(scene: &Scene, zone_id: u32) -> Option<Cell> {
    let map = scene.map_2d.as_ref()?;
    let zone = scene.rules.zone(zone_id)?;
    let coordinates = map.from_world(zone.position);
    if !coordinates
        .iter()
        .zip(map.size)
        .all(|(&value, limit)| (0.0..f32::from(limit)).contains(&value))
    {
        return None;
    }
    Some(coordinates.map(|coordinate| coordinate.floor() as u16))
}

pub fn center(cell: Cell) -> [f32; 2] {
    cell.map(|coordinate| f32::from(coordinate) + 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules;

    #[test]
    fn materialized_map_can_be_saved_loaded_edited_and_walked_by_native_controller() {
        let settings = Settings::default();
        let (new, layout) = scene(settings).unwrap();
        let bytes = serde_json::to_vec(&new).unwrap();
        let mut restored: Scene = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert_eq!(new, restored);

        let zone_id = restored.rules.zones[0].id;
        assert_eq!(exit_cell(&restored, zone_id), Some(layout.exit));
        let map = restored.map_2d.as_mut().unwrap();
        assert_eq!(route(map, layout.exit), Some(layout.route.clone()));
        assert!(map.paint_collision(layout.exit, Collision::Solid));
        assert!(route(map, layout.exit).is_none());
        assert!(map.paint_collision(layout.exit, Collision::Empty));
        let path = route(map, layout.exit).unwrap();
        let mut player = map2d::Player::new(map);
        let rules = rules::Session::new(&restored.rules);
        for point in path.into_iter().skip(1) {
            let [x, z] = center(point);
            let mut reached = false;
            for _ in 0..120 {
                let [px, _, pz] = player.world_position();
                let delta = glam::Vec2::new(x - px, z - pz);
                if delta.length() <= 0.06 {
                    reached = true;
                    break;
                }
                player.step(
                    &restored,
                    &rules,
                    1.0 / 60.0,
                    map2d::Input {
                        movement: (delta * 8.0).clamp_length_max(1.0).to_array(),
                        sprint: false,
                        jump: false,
                    },
                );
            }
            assert!(reached, "controller did not reach route point {point:?}");
        }
        assert!(restored.rules.zones[0].contains(player.world_position()));
    }
}
