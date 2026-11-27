use crate::{
    assets::{Library, Placement, Source},
    avatar::Avatar,
    environment::Environment,
    linked_scenes::ROOT_SCENE_ID,
    model::{GarmentStyle, HairStyle, HumanStyle},
    npcs::{FIRST_NPC_ID, Npc, Waypoint},
    rules::{Action, Comparison, Condition, Event, GameAction},
    scene::Scene,
    story::{Objective, Story},
    world::{DoorSettings, WorldKind},
};

pub const NAME: &str = "Сосновый меридиан";
pub const OBSERVATORY: &str = "Дозорная станция";
pub const LAKE: &str = "Тихая заводь";
pub const RIDGE: &str = "Ветровая гряда";
pub const PICK: &str = "Пойти по золотым знакам";
pub const TOKEN: &str = "Латунный жетон станции";
pub const CHOICE: &str = "Выбранный маршрут";
pub const KEY_AT: [f32; 3] = [1.48, 0.07, -0.85];
pub const DOOR_AT: [f32; 2] = [3.55, -2.44];
pub const FIRST_EXIT: [f32; 2] = [0.14, -12.6];
pub const SECOND_EXIT: [f32; 2] = [-0.25, -11.7];
pub const LAST_EXIT: [f32; 2] = [0.18, -12.8];
pub const STONE_EXIT: [f32; 2] = [-3.8, -12.6];

impl crate::Studio {
    pub(super) fn meridian_demo(&mut self) -> bool {
        match scene(&mut self.assets) {
            Ok(scene) => {
                self.set_scene(
                    scene,
                    None,
                    "Сосновый меридиан. Поговори с проводником клавишей E.",
                );
                self.view = crate::View::Three;
                true
            }
            Err(error) => {
                self.status = format!("Не удалось открыть «Сосновый меридиан». {error}");
                false
            }
        }
    }
}

pub fn scene(assets: &mut Library) -> Result<Scene, String> {
    for model in [Source::Emma, Source::Ryan] {
        assets.ensure(&model, None)?;
    }
    let mut root = landscape(
        assets,
        "Долина у дозорной станции",
        include_bytes!("../demo_assets/meridian/station.glb"),
        [0.16, 0.33, 0.22],
        10.75,
        2.42,
    )?;
    let mut lake = landscape(
        assets,
        "Поляна у заводи",
        include_bytes!("../demo_assets/meridian/waterfront.glb"),
        [0.30, 0.46, 0.23],
        14.25,
        4.40,
    )?;
    let mut ridge = landscape(
        assets,
        "Гряды ветровых холмов",
        include_bytes!("../demo_assets/meridian/ridge.glb"),
        [0.34, 0.40, 0.26],
        17.10,
        6.15,
    )?;

    let waters = lake.environment.as_mut().expect("landscape environment");
    waters.add_water([6.5, -6.6], -0.22);
    let pond = waters.waters.last_mut().expect("new lake");
    pond.name = "Заводь у тропы".into();
    pond.size = [7.0, 10.6];
    pond.level = 0.52;
    pond.tint = [0.14, 0.46, 0.45];
    pond.current = [0.015, 0.095];
    pond.wave_height = 0.033;

    waymark(&mut root, FIRST_EXIT, [0.96, 0.60, 0.14], "Солнечные знаки");
    waymark(
        &mut root,
        STONE_EXIT,
        [0.54, 0.63, 0.69],
        "Каменная развилка",
    );
    waymark(
        &mut lake,
        [-2.1, -5.2],
        [0.32, 0.47, 0.58],
        "Синий знак у воды",
    );
    waymark(
        &mut lake,
        SECOND_EXIT,
        [0.96, 0.61, 0.16],
        "Золотой просвет",
    );
    waymark(&mut ridge, LAST_EXIT, [1.0, 0.78, 0.39], "Башня ветров");
    for x in [-2.1, 2.55] {
        object(
            &mut ridge,
            WorldKind::Rock,
            "Сланцевый уступ",
            [x, 0.0, -7.9],
            [1.2, 0.62, 2.4],
            [0.36, 0.36, 0.35],
            true,
        );
    }
    finish_column(&mut ridge);

    // Attach the scene catalogue first, then bind rules to its stable IDs.
    let lake_id = root.linked_scenes.attach(LAKE.into(), lake)?;
    let ridge_id = root.linked_scenes.attach(RIDGE.into(), ridge)?;
    let lake = &mut root
        .linked_scenes
        .entries
        .iter_mut()
        .find(|row| row.id == lake_id)
        .expect("attached lake")
        .scene;
    local_goal(
        lake,
        "Золотой знак",
        "У синего знака поверни вдоль воды. Найди золотой столб и нажми F.",
    );
    transit(
        lake,
        SECOND_EXIT,
        "С золотой отметки на гряду",
        ridge_id,
        vec![],
    );
    let ridge = &mut root
        .linked_scenes
        .entries
        .iter_mut()
        .find(|row| row.id == ridge_id)
        .expect("attached ridge")
        .scene;
    local_goal(
        ridge,
        "Башня ветров",
        "Доберись до башни. F вернёт тебя к открытой станции.",
    );
    transit(
        ridge,
        LAST_EXIT,
        "С башни к дозорной станции",
        ROOT_SCENE_ID,
        vec![],
    );
    station(&mut root, assets, lake_id);
    root.validate()?;
    Ok(root)
}

fn landscape(
    assets: &mut Library,
    label: &str,
    terrain: &[u8],
    canopy: [f32; 3],
    hour: f32,
    wind_speed: f32,
) -> Result<Scene, String> {
    let mut scene = Scene::empty();
    let model = assets.get(&Source::Emma).expect("Emma prepared");
    let mut hero = Avatar::new(Source::Emma, "Нина Орлова".into(), &model.model);
    hero.human_style = Some(HumanStyle {
        hairstyle: HairStyle::Swept,
        garment: GarmentStyle::Sweater,
        jacket_tint: [0.79, 0.44, 0.19],
        trouser_tint: [0.26, 0.36, 0.39],
        ..HumanStyle::default()
    });
    scene.avatar = Some(hero);
    scene.play_settings.camera.distance = Some(7.6);
    let terrain = assets.import_bytes(terrain.to_vec())?;
    let model = &assets.get(&terrain).expect("scenery archived").model;
    let mut placement =
        Placement::at_ground(&[], model, terrain, label.into(), [0.0; 3], 68.0, 0.0);
    placement.position = [0.0; 3];
    placement.scale = [1.0; 3];
    placement.solid = false;
    scene.world.models.push(placement);
    let mut environment = Environment::default();
    environment.time.start_hour = hour;
    environment.time.noon_bearing = 79.0;
    environment.add_wind([0.0, 2.3, -10.0]);
    let wind = environment.winds.last_mut().expect("trail wind");
    wind.name = "Ветер вдоль долины".into();
    wind.size = [29.0, 12.0, 52.0];
    wind.direction = [0.86, 0.0, 0.47];
    wind.speed = wind_speed;
    wind.gust = 1.2;
    wind.feather = 2.8;
    scene.environment = Some(environment);
    for (index, (x, z, height)) in [
        (-3.4, -3.1, 3.9),
        (-4.65, -5.4, 3.5),
        (-3.55, -8.45, 4.4),
        (4.5, -10.0, 4.2),
        (4.0, -15.6, 4.55),
        (5.15, -18.0, 3.05),
        (-5.7, -15.0, 3.56),
        (-3.75, -19.51, 3.62),
        (2.98, -22.0, 4.26),
        (-4.92, -23.4, 4.52),
        (5.38, -26.1, 3.85),
    ]
    .into_iter()
    .enumerate()
    {
        let value = 0.87 + (index % 5) as f32 * 0.065;
        object(
            &mut scene,
            WorldKind::Tree,
            "Сосна у тропы",
            [x, 0.0, z],
            [height * 0.60, height, height * 0.58],
            canopy.map(|channel| (channel * value).min(1.0)),
            true,
        );
    }
    Ok(scene)
}

fn station(scene: &mut Scene, assets: &Library, next: u32) {
    scene
        .world
        .add_building_at([3.55, 0.04, -5.65])
        .expect("station building");
    let item = scene.world.add_key_at(KEY_AT).expect("station token");
    scene.world.keys[0].name = TOKEN.into();
    let building = scene.world.buildings.last_mut().expect("station created");
    building.name = OBSERVATORY.into();
    building.size = [5.2, 3.9, 5.0];
    building.door_width = 1.44;
    building.color = [0.40, 0.43, 0.28];
    building.door = Some(DoorSettings {
        required_key: Some(item),
        ..DoorSettings::default()
    });
    let door = building.doors().next().expect("station entrance").id;

    let source = Source::Ryan;
    let npc = Avatar::new(
        source.clone(),
        "Проводник Лев".into(),
        &assets.get(&source).unwrap().model,
    );
    scene.npcs.push(Npc::new(
        FIRST_NPC_ID,
        npc,
        vec![Waypoint::at([-0.82, 0.0, -0.62])],
    ));
    scene.story = Story::demo(FIRST_NPC_ID, FIRST_EXIT);
    let talk = &mut scene.story.conversations[0];
    talk.name = "Проводник Лев".into();
    talk.nodes.truncate(1);
    let line = &mut talk.nodes[0];
    line.text = "За грядой старая башня. Возьми жетон у входа, открой станцию и выбери солнечную тропу или каменную развилку. У знака нажми F.".into();
    line.choices.truncate(2);
    for (choice, title) in line
        .choices
        .iter_mut()
        .zip([PICK, "Пойти по каменным знакам"])
    {
        choice.text = title.into();
        choice.starts_quest = None;
        choice.target = None;
    }
    let quest = &mut scene.story.quests[0];
    quest.title = "Сосновый меридиан · Станция".into();
    quest.description = "Выбери маршрут. Подбери жетон, открой станцию и найди нужный знак.".into();
    quest.completion = "Допуск открыт. Следуй выбранной тропой.".into();
    quest.objective = Objective::Rules {
        completion_stage: 3,
    };

    let choice = scene.rules.add_counter().expect("route decision counter");
    scene.rules.counters.last_mut().unwrap().name = CHOICE.into();
    for (reply, value, label) in [(1, 1, PICK), (2, 2, "Выбрать каменную развилку")]
    {
        rule(
            scene,
            label,
            Event::Dialogue {
                npc_id: FIRST_NPC_ID,
                node_id: 1,
                choice_id: reply,
            },
            vec![],
            vec![
                Action::SetCounter {
                    counter_id: choice,
                    value,
                },
                Action::SetQuestStage {
                    quest_id: 1,
                    stage: 1,
                },
            ],
            false,
        );
    }
    rule(
        scene,
        "Собрать жетон",
        Event::Pickup { item_id: item },
        vec![],
        vec![Action::SetQuestStage {
            quest_id: 1,
            stage: 2,
        }],
        true,
    );
    rule(
        scene,
        "Открыть дозорную",
        Event::Door { door_id: door },
        vec![Condition::HasItem { item_id: item }],
        vec![Action::SetQuestStage {
            quest_id: 1,
            stage: 3,
        }],
        true,
    );
    for (position, value, label) in [
        (FIRST_EXIT, 1, "Солнечная тропа"),
        (STONE_EXIT, 2, "Каменная тропа"),
    ] {
        transit(
            scene,
            position,
            label,
            next,
            vec![
                Condition::HasItem { item_id: item },
                Condition::Counter {
                    counter_id: choice,
                    comparison: Comparison::Equal,
                    value,
                },
            ],
        );
    }
}

fn local_goal(scene: &mut Scene, name: &str, description: &str) {
    scene.story.quests.push(crate::story::Quest {
        id: 1,
        title: name.into(),
        description: description.into(),
        completion: "Маршрут открыт.".into(),
        objective: Objective::Rules {
            completion_stage: 2,
        },
    });
    let zone = scene.rules.add_zone([0.0; 3]).expect("entry");
    scene.rules.zones.last_mut().unwrap().name = "Начало тропы".into();
    // The first activation gives the immediate navigation clue to the player.
    rule(
        scene,
        "Показать цель",
        Event::GameAction {
            action: GameAction::Mark,
        },
        vec![Condition::InZone { zone_id: zone }],
        vec![Action::SetQuestStage {
            quest_id: 1,
            stage: 1,
        }],
        true,
    );
}

fn transit(
    scene: &mut Scene,
    [x, z]: [f32; 2],
    name: &str,
    destination: u32,
    mut only: Vec<Condition>,
) {
    let zone_id = scene.rules.add_zone([x, 0.0, z]).expect("trail endpoint");
    let zone = scene.rules.zones.last_mut().unwrap();
    zone.name = name.into();
    zone.size = [2.8, 2.8, 2.6];
    only.push(Condition::InZone { zone_id });
    rule(
        scene,
        name,
        Event::GameAction {
            action: GameAction::Mark,
        },
        only,
        vec![Action::Transition {
            scene_id: destination,
        }],
        false,
    );
}

fn rule(
    scene: &mut Scene,
    name: &str,
    event: Event,
    conditions: Vec<Condition>,
    actions: Vec<Action>,
    once: bool,
) {
    scene.rules.add_rule().expect("trail rule");
    let rule = scene.rules.rules.last_mut().unwrap();
    rule.name = name.into();
    rule.event = Some(event);
    rule.conditions = conditions;
    rule.actions = actions;
    rule.once = once;
}

fn waymark(scene: &mut Scene, [x, z]: [f32; 2], color: [f32; 3], label: &str) {
    for axis in [-1.0, 1.0] {
        object(
            scene,
            WorldKind::Platform,
            label,
            [x + axis * 1.1, 0.0, z],
            [0.16, 1.18, 0.16],
            [0.31, 0.23, 0.15],
            false,
        );
        object(
            scene,
            WorldKind::Platform,
            label,
            [x + axis * 1.1, 1.04, z],
            [0.34, 0.28, 0.23],
            color,
            false,
        );
    }
}

fn finish_column(scene: &mut Scene) {
    for (offset, width, height, color) in [
        (0.0, 1.5, 0.26, [0.40, 0.42, 0.38]),
        (0.26, 0.90, 2.44, [0.33, 0.43, 0.46]),
        (2.72, 1.24, 0.32, [0.92, 0.70, 0.32]),
        (3.05, 0.55, 0.44, [0.35, 0.42, 0.46]),
    ] {
        object(
            scene,
            WorldKind::Platform,
            "Маяк меридиана",
            [2.3, offset, -13.2],
            [width, height, width],
            color,
            true,
        );
    }
}

fn object(
    scene: &mut Scene,
    kind: WorldKind,
    label: &str,
    position: [f32; 3],
    size: [f32; 3],
    color: [f32; 3],
    solid: bool,
) {
    scene.world.add(kind).expect("scenery object");
    let object = scene.world.objects.last_mut().unwrap();
    object.name = label.into();
    object.position = position;
    object.size = size;
    object.color = color;
    object.solid = solid;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Studio, world::Interaction};

    #[test]
    fn three_locations_restore_station_after_a_project_reload() {
        let mut app = Studio::default();
        let root = scene(&mut app.assets).unwrap();
        app.set_scene(root, None, "");
        let context = eframe::egui::Context::default();
        app.start_play(&context, false);
        let active = |app: &Studio| app.link_playthrough.as_ref().unwrap().active;
        assert_eq!(active(&app), ROOT_SCENE_ID);

        assert!(app.story_session.open(&app.scene.story, FIRST_NPC_ID));
        let chosen = app.story_session.choose_event(&app.scene.story, 0).unwrap();
        app.dispatch_rule(chosen);
        let token = app.scene.world.keys[0].id;
        let door = app.scene.world.buildings[0].doors().next().unwrap().id;
        assert_eq!(
            app.scene.world.interact(&mut app.interactions, KEY_AT),
            Some(Interaction::Key { id: token })
        );
        let at_door = [DOOR_AT[0], 0.0, DOOR_AT[1]];
        assert_eq!(
            app.scene.world.interact(&mut app.interactions, at_door),
            Some(Interaction::Door {
                id: door,
                opening: true
            })
        );
        for _ in 0..10 {
            app.scene
                .world
                .advance_interactions(&mut app.interactions, 0.05, at_door);
        }
        let travel = |app: &mut Studio, [x, z]: [f32; 2]| {
            app.game.x = x;
            app.game.z = z;
            app.dispatch_rule(Event::GameAction {
                action: GameAction::Mark,
            });
            app.check_scene_transition(&context);
        };
        travel(&mut app, STONE_EXIT);
        assert_eq!(active(&app), 1);
        travel(&mut app, FIRST_EXIT);
        assert_eq!(active(&app), 2);
        travel(&mut app, SECOND_EXIT);
        assert_eq!(active(&app), 3);
        app.game.z = -4.0;
        app.stop();

        let checkpoint = app.scene.checkpoint.as_ref().unwrap();
        assert_eq!(checkpoint.scene_id, Some(3));
        assert_eq!(
            checkpoint.visited.keys().copied().collect::<Vec<_>>(),
            [1, 2]
        );
        let saved_scene = serde_json::to_vec(&app.scene).unwrap();
        let reopened: Scene = serde_json::from_slice(&saved_scene).unwrap();
        reopened.validate().unwrap();
        app.set_scene(reopened, None, "");
        app.start_play(&context, true);
        assert_eq!(active(&app), 3);
        assert_eq!(app.game.z, -4.0);
        travel(&mut app, LAST_EXIT);
        assert_eq!(active(&app), 1);
        assert!(app.interactions.has_key(token));
        let door = app.scene.world.door(door).unwrap();
        assert_eq!(app.interactions.fraction(&door), 1.0);
        assert_eq!(app.rule_session.counter(&app.scene.rules, 1), Some(1));

        assert!(app.story_session.open(&app.scene.story, FIRST_NPC_ID));
        let alternative = app.story_session.choose_event(&app.scene.story, 1).unwrap();
        app.dispatch_rule(alternative);
        assert_eq!(app.rule_session.counter(&app.scene.rules, 1), Some(2));
        travel(&mut app, FIRST_EXIT);
        assert_eq!(active(&app), 1);
        travel(&mut app, STONE_EXIT);
        assert_eq!(active(&app), 2);
    }
}
