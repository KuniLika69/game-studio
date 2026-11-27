use crate::{
    assets::{Library, Placement, Source},
    avatar::Avatar,
    interior::FurnitureKind,
    model::{GarmentStyle, HairStyle, HumanStyle},
    npcs::{self, Npc, Waypoint},
    rules::{Action, Condition, Event},
    scene::Scene,
    story::{Choice, Node, Objective, Story},
    world::DoorSettings,
};

pub const NAME: &str = "Липовый проезд";
pub const WORKSHOP: &str = "Мастерская 04";
pub const MECHANIC: &str = "Марина";
pub const GUARD: &str = "Илья";
pub const KEY: &str = "Ключ от мастерской";
pub const ACCEPT: &str = "Найду ключ";
pub const GUARD_REPLY: &str = "Назвать поручение Марины";
pub const WRONG_REPLY: &str = "Просто осматриваюсь";
pub const LOCK_RULE: &str = "Допуск в мастерскую";
pub const GUARD_FLAG: &str = "Допуск от Ильи";
pub const QUEST: &str = "Ключ от мастерской";
pub const KEY_POSITION: [f32; 3] = [-4.40, 0.076, -10.75];
pub const GUARD_POSITION: [f32; 3] = [4.65, 0.077, -10.35];
pub const MECHANIC_POSITION: [f32; 3] = [-1.25, 0.0, -0.18];
pub const WORKSHOP_POSITION: [f32; 3] = [8.62, 0.074, -15.70];

pub fn create(library: &mut Library) -> Result<Scene, String> {
    let mut scene = Scene::empty();
    for source in [Source::Emma, Source::Ryan, Source::CityStreet] {
        library.ensure(&source, None)?;
    }
    for (label, z) in [
        ("Липовый проезд · юг", 0.0),
        ("Липовый проезд · север", -36.0),
    ] {
        place(
            &mut scene,
            library,
            Source::CityStreet,
            label,
            [0.0, 0.008, z],
            36.0,
            0.0,
            true,
        );
    }
    for (name, x, z, width, height, depth, side, color) in [
        (
            "Пекарня в доме 02",
            -8.10,
            5.15,
            9.25,
            6.60,
            5.95,
            1,
            [0.58, 0.36, 0.27],
        ),
        (
            "Дом с кленовым карнизом",
            8.48,
            4.90,
            8.60,
            7.50,
            6.45,
            3,
            [0.52, 0.52, 0.43],
        ),
        (
            "Липовый дом",
            -8.20,
            -4.60,
            8.90,
            9.55,
            6.05,
            1,
            [0.46, 0.45, 0.37],
        ),
        (
            "Почтовый двор",
            8.79,
            -5.45,
            8.75,
            5.90,
            6.70,
            3,
            [0.57, 0.50, 0.38],
        ),
        (
            WORKSHOP,
            WORKSHOP_POSITION[0],
            WORKSHOP_POSITION[2],
            9.62,
            4.45,
            6.50,
            3,
            [0.33, 0.48, 0.46],
        ),
        (
            "Дом Луниных",
            -8.36,
            -17.20,
            9.70,
            8.30,
            6.00,
            1,
            [0.47, 0.38, 0.32],
        ),
        (
            "Угловой дом 16",
            8.58,
            -26.62,
            9.80,
            8.50,
            6.45,
            3,
            [0.55, 0.45, 0.36],
        ),
        (
            "Книжная лавка",
            -8.38,
            -27.92,
            9.00,
            6.20,
            6.50,
            1,
            [0.38, 0.44, 0.43],
        ),
        (
            "Дом старой типографии",
            8.70,
            -37.15,
            10.05,
            10.00,
            6.65,
            3,
            [0.40, 0.43, 0.38],
        ),
        (
            "Северный корпус",
            -8.45,
            -39.80,
            11.65,
            9.30,
            6.25,
            1,
            [0.55, 0.47, 0.40],
        ),
        (
            "Дом у выезда",
            8.50,
            -48.20,
            9.65,
            6.45,
            6.30,
            3,
            [0.43, 0.45, 0.46],
        ),
        (
            "Дом под часами",
            -8.66,
            -49.60,
            7.25,
            9.90,
            6.50,
            1,
            [0.47, 0.36, 0.31],
        ),
    ] {
        scene
            .world
            .add_building_at([x, 0.074, z])
            .ok_or("Не удалось добавить дом.")?;
        let house = scene.world.buildings.last_mut().expect("new building");
        house.name = name.into();
        house.size = [width, height, depth];
        house.quarter_turns = side;
        house.color = color;
        house.door = Some(DoorSettings {
            rule_lock: true,
            ..DoorSettings::default()
        });
        if name == WORKSHOP {
            house.door_width = 1.66;
            house.door_height = 2.60;
            for (kind, local, rotation, title, color) in [
                (
                    FurnitureKind::Table,
                    [2.75, 0.86],
                    1,
                    "Верстак",
                    [0.40, 0.27, 0.16],
                ),
                (
                    FurnitureKind::Chair,
                    [1.86, 0.87],
                    3,
                    "Табурет",
                    [0.57, 0.37, 0.19],
                ),
                (
                    FurnitureKind::Cabinet,
                    [-3.44, -2.78],
                    0,
                    "Шкаф инструментов",
                    [0.34, 0.37, 0.30],
                ),
                (
                    FurnitureKind::Cabinet,
                    [-2.11, -2.78],
                    0,
                    "Шкаф материалов",
                    [0.39, 0.42, 0.31],
                ),
                (
                    FurnitureKind::Sofa,
                    [3.92, -1.79],
                    1,
                    "Диван для посетителей",
                    [0.32, 0.38, 0.33],
                ),
                (
                    FurnitureKind::Table,
                    [-2.92, 0.23],
                    0,
                    "Стол заказов",
                    [0.44, 0.29, 0.15],
                ),
            ] {
                house.interior.add_furniture(kind, local, house.size);
                if let Some(piece) = house.interior.furniture.last_mut() {
                    piece.name = title.into();
                    piece.quarter_turns = rotation;
                    piece.color = color;
                }
            }
        }
    }
    let lantern =
        library.import_bytes(include_bytes!("../demo_assets/quarter/lantern.glb").to_vec())?;
    let planter =
        library.import_bytes(include_bytes!("../demo_assets/quarter/planter.glb").to_vec())?;
    let bench =
        library.import_bytes(include_bytes!("../demo_assets/quarter/bench.glb").to_vec())?;
    let sign = library
        .import_bytes(include_bytes!("../demo_assets/quarter/workshop_sign.glb").to_vec())?;
    for (x, z, yaw) in [
        (-4.88, 2.80, 90.0),
        (4.88, -1.60, -90.0),
        (-4.87, -8.7, 90.0),
        (4.86, -13.22, -90.0),
        (-4.87, -20.72, 90.0),
        (4.88, -25.1, -90.0),
        (-4.87, -33.0, 90.0),
        (4.88, -36.9, -90.0),
        (-4.87, -46.81, 90.0),
        (4.88, -50.75, -90.0),
    ] {
        place(
            &mut scene,
            library,
            lantern.clone(),
            "Кованый фонарь",
            [x, 0.076, z],
            3.50,
            yaw,
            false,
        );
    }
    for (x, z, yaw) in [
        (-5.36, -9.60, 90.0),
        (-5.38, -12.5, 90.0),
        (5.25, -12.1, -90.0),
        (5.24, -18.7, -90.0),
        (5.25, -2.00, -90.0),
        (-5.24, 0.1, 90.0),
        (-5.4, -23.15, 90.0),
        (5.22, -33.7, -90.0),
        (-5.39, -45.65, 90.0),
    ] {
        place(
            &mut scene,
            library,
            planter.clone(),
            "Липовая кадка",
            [x, 0.076, z],
            1.36,
            yaw,
            false,
        );
    }
    for (x, z, yaw) in [
        (-5.44, -10.95, 90.0),
        (5.36, -18.0, -90.0),
        (-5.40, -23.85, 90.0),
        (5.35, -31.95, -90.0),
    ] {
        place(
            &mut scene,
            library,
            bench.clone(),
            "Лавка из лиственницы",
            [x, 0.076, z],
            1.83,
            yaw,
            false,
        );
    }
    place(
        &mut scene,
        library,
        sign,
        "Вывеска мастерской",
        [5.30, 2.92, WORKSHOP_POSITION[2]],
        2.21,
        -90.0,
        false,
    );
    populate(&mut scene, library)?;
    Ok(scene)
}

fn place(
    scene: &mut Scene,
    library: &Library,
    source: Source,
    label: &str,
    foot: [f32; 3],
    size: f32,
    yaw: f32,
    solid: bool,
) {
    let asset = library.get(&source).expect("quarter asset already loaded");
    let mut item = Placement::at_ground(
        &scene.world.models,
        &asset.model,
        source,
        label.into(),
        foot,
        size,
        yaw,
    );
    item.solid = solid;
    scene.world.models.push(item);
}

fn populate(scene: &mut Scene, library: &Library) -> Result<(), String> {
    let avatar = |source: Source, name: &str, style| {
        let model = &library.get(&source).expect("actor model loaded").model;
        let mut avatar = Avatar::new(source, name.into(), model);
        avatar.human_style = Some(style);
        avatar
    };
    scene.avatar = Some(avatar(
        Source::Emma,
        "Вера Соколова",
        HumanStyle {
            face_width: -0.17,
            jaw_width: 0.11,
            nose_length: -0.13,
            hair_volume: -0.12,
            hairstyle: HairStyle::Swept,
            garment: GarmentStyle::Sweater,
            jacket_tint: [0.64, 0.84, 0.76],
            trouser_tint: [0.64, 0.73, 0.80],
            ..HumanStyle::default()
        },
    ));
    let mechanic = avatar(
        Source::Emma,
        MECHANIC,
        HumanStyle {
            face_width: 0.30,
            jaw_width: -0.22,
            nose_length: 0.17,
            hair_volume: 0.38,
            smile: 0.18,
            hairstyle: HairStyle::Full,
            garment: GarmentStyle::LongCoat,
            jacket_tint: [0.84, 0.54, 0.38],
            trouser_tint: [0.61, 0.71, 0.64],
            ..HumanStyle::default()
        },
    );
    let guard = avatar(
        Source::Ryan,
        GUARD,
        HumanStyle {
            face_width: 0.18,
            jaw_width: 0.24,
            nose_length: -0.20,
            brow_raise: -0.16,
            hairstyle: HairStyle::Cropped,
            garment: GarmentStyle::Sweater,
            jacket_tint: [0.36, 0.51, 0.58],
            trouser_tint: [0.63, 0.63, 0.67],
            ..HumanStyle::default()
        },
    );
    scene.npcs.push(Npc::new(
        npcs::FIRST_NPC_ID,
        mechanic,
        vec![Waypoint::at(MECHANIC_POSITION)],
    ));
    scene.npcs.push(Npc::new(
        npcs::FIRST_NPC_ID + 1,
        guard,
        vec![Waypoint::at(GUARD_POSITION)],
    ));

    let id = scene
        .world
        .add_key_at(KEY_POSITION)
        .ok_or("Не удалось создать ключ.")?;
    scene.world.keys.last_mut().expect("new key").name = KEY.into();
    connect_quest(scene, id)
}

fn connect_quest(scene: &mut Scene, key: u32) -> Result<(), String> {
    let mechanic = npcs::FIRST_NPC_ID;
    let guard = mechanic + 1;
    scene.story = Story::demo(mechanic, [0.0, 0.0]);
    let option = |text: &str| Choice {
        id: 0,
        text: text.into(),
        target: None,
        starts_quest: None,
    };
    let dialogue = &mut scene.story.conversations[0];
    dialogue.name = MECHANIC.into();
    dialogue.nodes = vec![Node {
        face: None,
        voice: None,
        speaker: crate::story::Speaker::Npc,
        id: dialogue.entry,
        text: "Подменишь меня на минутку? Ключ у лавки на противоположном от мастерской тротуаре. Найди его и зайди в мастерскую 04. Если дверь останется запертой, назови Илье моё поручение.".into(),
        choices: vec![option(ACCEPT), option("Может, позже")],
    }];
    let mut guard_dialogue = dialogue.clone();
    guard_dialogue.npc_id = guard;
    guard_dialogue.name = GUARD.into();
    guard_dialogue.nodes[0].text =
        "Илья, дежурный по двору. Ты по поручению Марины? Тогда внесу тебя в журнал и дам допуск."
            .into();
    guard_dialogue.nodes[0].choices = vec![option(GUARD_REPLY), option(WRONG_REPLY)];
    scene.story.conversations.push(guard_dialogue);
    scene.story.migrate_answer_ids();

    let quest = &mut scene.story.quests[0];
    quest.title = QUEST.into();
    quest.description = "Найди латунный ключ у лавки и войди в мастерскую 04.".into();
    quest.completion = "Мастерская открыта. Ты внутри.".into();
    quest.objective = Objective::Rules {
        completion_stage: 4,
    };
    let quest_id = quest.id;
    let house = scene
        .world
        .buildings
        .iter()
        .find(|house| house.name == WORKSHOP)
        .ok_or("Мастерская не создана.")?;
    let door = house
        .doors()
        .next()
        .ok_or("У мастерской отсутствует дверь.")?
        .id;
    let house_id = house.id;

    let mut flag = |name: &str| -> Result<u32, String> {
        let id = scene.rules.add_flag().ok_or("Не удалось добавить флаг.")?;
        scene.rules.flags.last_mut().expect("new flag").name = name.into();
        Ok(id)
    };
    let assignment = flag("Поручение Марины")?;
    let access = flag(GUARD_FLAG)?;
    let open = flag("Вход разблокирован")?;
    let zone_position = [7.37, WORKSHOP_POSITION[1], WORKSHOP_POSITION[2]];
    let zone_id = scene
        .rules
        .add_zone(zone_position)
        .ok_or("Не удалось создать зону.")?;
    let zone = scene.rules.zones.last_mut().expect("new zone");
    zone.name = "Мастерская · за порогом".into();
    zone.size = [3.30, 2.6, 5.90];
    zone.building_id = Some(house_id);

    let dialogue_event = |index: usize| {
        let talk = &scene.story.conversations[index];
        Event::Dialogue {
            npc_id: talk.npc_id,
            node_id: talk.entry,
            choice_id: talk.nodes[0].choices[0].id,
        }
    };
    let stage = |stage| Action::SetQuestStage { quest_id, stage };
    let flag_is = |flag_id, value| Condition::Flag { flag_id, value };
    let set = |flag_id| Action::SetFlag {
        flag_id,
        value: true,
    };
    for (name, event, conditions, actions) in [
        (
            "Поручение Марины",
            dialogue_event(0),
            vec![],
            vec![set(assignment), stage(1)],
        ),
        (
            "Ключ найден",
            Event::Pickup { item_id: key },
            vec![flag_is(assignment, true), flag_is(open, false)],
            vec![stage(2)],
        ),
        (
            "Ответ Илье",
            dialogue_event(1),
            vec![flag_is(assignment, true)],
            vec![set(access)],
        ),
        (
            LOCK_RULE,
            Event::Door { door_id: door },
            vec![
                Condition::HasItem { item_id: key },
                flag_is(assignment, true),
            ],
            vec![Action::UnlockDoor { door_id: door }, set(open), stage(3)],
        ),
        (
            "Вход в мастерскую",
            Event::EnterZone { zone_id },
            vec![flag_is(assignment, true), flag_is(open, true)],
            vec![stage(4)],
        ),
    ] {
        scene
            .rules
            .add_rule()
            .ok_or("Не удалось добавить правило.")?;
        let rule = scene.rules.rules.last_mut().expect("new rule");
        rule.name = name.into();
        rule.event = Some(event);
        rule.conditions = conditions;
        rule.actions = actions;
    }
    Ok(())
}
