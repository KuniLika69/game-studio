//! Authored workshop chapter. Rules and assets are persisted as an ordinary project.
use crate::{
    Studio, View,
    assets::{Library, Placement, materials::PointLight},
    audio::Surface,
    audio_project::FootstepZone,
    effect_rules::{Command as Effect, Origin},
    effects::Kind,
    environment::WeatherPreset,
    interior::ModelCollider,
    npcs::Waypoint,
    rain_workshop,
    rules::{Action, Condition, Event, GameAction, RuleSet},
    scene::Scene,
    simulation_rules::Command,
    story::{Choice, Node, Objective, Speaker},
    world::DoorSettings,
};

pub const NAME: &str = "Последний свет на набережной";
pub const WEST_SURVEY: [f32; 2] = [-36.0, -3.42];
pub const EAST_SURVEY: [f32; 2] = [39.0, -3.42];
pub const EAST_STATION: [f32; 2] = [59.24, -3.42];
pub const WEST_STATION: [f32; 2] = [-59.24, -3.42];
pub const WORKSHOP_DOOR: [f32; 2] = [0.36, -6.50];
pub const INTERVIEW: [f32; 2] = [-0.93, -9.34];
pub const LEDGER: [f32; 2] = [-0.59, -11.58];
pub const EAST_ANSWER: &str = "Запитать восточный шлюз";
pub const WEST_ANSWER: &str = "Вывести воду в западный лоток";
pub const OUTCOME: &str = "Решение";

impl Studio {
    pub(super) fn flagship_demo(&mut self) -> bool {
        match create(&mut self.assets) {
            Ok(scene) => {
                self.set_scene(
                    scene,
                    None,
                    "Последний свет на набережной. Начни с западного лотка.",
                );
                self.view = View::Three;
                self.camera = rain_workshop::LIGHT_CAMERA;
                true
            }
            Err(error) => {
                self.status = error;
                false
            }
        }
    }
}

pub fn create(assets: &mut Library) -> Result<Scene, String> {
    let mut scene = rain_workshop::create(assets, rain_workshop::Style::BlueHour)?;
    structures(&mut scene, assets)?;
    let west = station(
        &mut scene,
        assets,
        "Западный лоток · проверка",
        WEST_SURVEY,
        false,
    )?;
    let _east = station(
        &mut scene,
        assets,
        "Восточное реле · проверка",
        EAST_SURVEY,
        true,
    )?;
    let west_outcome = station(
        &mut scene,
        assets,
        "Западный сброс · пуск",
        WEST_STATION,
        false,
    )?;
    let east_outcome = station(
        &mut scene,
        assets,
        "Восточный шлюз · пуск",
        EAST_STATION,
        true,
    )?;
    let east_signal = place(
        &mut scene,
        assets,
        "Янтарный маяк восточного шлюза",
        include_bytes!("../demo_assets/rain/lamp.glb"),
        [EAST_STATION[0] + 0.48, 1.94, -4.52],
        None,
    )?;
    set_light(&mut scene, [1.0, 0.57, 0.25], 4.7, 7.5);
    let west_signal = place(
        &mut scene,
        assets,
        "Голубой маяк западного лотка",
        include_bytes!("../demo_assets/rain/lamp.glb"),
        [WEST_STATION[0] - 0.48, 1.94, -4.52],
        None,
    )?;
    set_light(&mut scene, [0.23, 0.75, 1.0], 3.5, 8.0);

    let environment = scene.environment.as_mut().ok_or("Нет среды набережной.")?;
    environment.time.start_hour = 18.83;
    environment.ground_mist_height = 0.87;
    let puddle = environment
        .waters
        .first_mut()
        .ok_or("Нет лужи на мостовой.")?;
    puddle.size = [5.10, 3.38];
    puddle.level = 0.022;
    puddle.rounded_edge = true;
    let canal = environment
        .add_water([-59.25, -9.70], -0.22)
        .ok_or("Нет западного водоёма.")?;
    let basin = environment.waters.last_mut().unwrap();
    basin.name = "Западный отводной лоток".into();
    basin.size = [6.8, 4.2];
    basin.level = 0.10;
    basin.max_level = 0.60;
    basin.tint = [0.18, 0.35, 0.41];
    basin.current = [0.04, 0.0];
    basin.wave_height = 0.015;
    let cue = environment
        .weather
        .add_cue(WeatherPreset::Clear)
        .ok_or("Нет перехода дождя.")?;
    environment
        .weather
        .cues
        .iter_mut()
        .find(|row| row.id == cue)
        .unwrap()
        .start_seconds = 185.0;
    environment.weather.cues.last_mut().unwrap().blend_seconds = 13.0;
    scene.lighting.daylight = 1.07;
    scene.lighting.interior_gain = 2.37;

    let npc = scene.npcs.first_mut().ok_or("Нет собеседника.")?;
    npc.route = vec![Waypoint::at([-1.65, 0.12, -10.22])];
    let workshop = scene.world.buildings.first().ok_or("Нет мастерской.")?;
    let door = workshop.doors().next().ok_or("Нет двери мастерской.")?.id;
    let room = workshop.id;

    scene.rules = RuleSet::default();
    scene.rules.initially_disabled_objects = vec![west_signal, east_signal];
    wire(
        &mut scene,
        room,
        door,
        west,
        west_outcome,
        east_outcome,
        west_signal,
        east_signal,
        canal,
    )?;
    crate::workshop_dialogue::author_flagship(&mut scene, rain_workshop::NPC_ID)?;
    scene.audio.footsteps.gait_contacts = true;
    scene.audio.ambience.enabled = true;
    scene.validate()?;
    Ok(scene)
}

fn structures(scene: &mut Scene, assets: &mut Library) -> Result<(), String> {
    for (label, bytes, position) in [
        (
            "Обводная дорога · мокрый и сухой гранит",
            &include_bytes!("../demo_assets/flagship/quay-ground.glb")[..],
            [0.0; 3],
        ),
        (
            "Депо · оконные ниши и мастерская",
            &include_bytes!("../demo_assets/flagship/workshop-detailed.glb")[..],
            [0.0, 0.0, -7.05],
        ),
        (
            "Набережная · конструкции, таблички и провода",
            &include_bytes!("../demo_assets/flagship/quay-fronts.glb")[..],
            [0.0; 3],
        ),
    ] {
        place(scene, assets, label, bytes, position, None)?;
    }
    for (label, x, z, size, tone) in [
        (
            "Запад · контора водосброса",
            -25.3,
            -13.2,
            [9.3, 9.3, 8.1],
            [0.39, 0.32, 0.28],
        ),
        (
            "Запад · старый вагонный склад",
            -37.7,
            -12.4,
            [11.4, 7.4, 8.9],
            [0.34, 0.42, 0.44],
        ),
        (
            "Запад · блок ремонтников",
            -51.5,
            -11.9,
            [10.7, 5.9, 7.8],
            [0.43, 0.44, 0.38],
        ),
        (
            "Запад · резервуарная башня",
            -69.2,
            -14.4,
            [11.7, 11.7, 10.5],
            [0.34, 0.36, 0.41],
        ),
        (
            "Восток · кирпичный корпус",
            26.9,
            -13.1,
            [10.4, 9.7, 9.2],
            [0.44, 0.36, 0.30],
        ),
        (
            "Восток · щитовая",
            40.8,
            -12.2,
            [10.2, 7.3, 8.6],
            [0.35, 0.43, 0.43],
        ),
        (
            "Восток · дом телеметрии",
            54.3,
            -13.9,
            [10.9, 8.8, 8.8],
            [0.40, 0.37, 0.40],
        ),
        (
            "Восток · диспетчерская башня",
            72.1,
            -14.8,
            [10.9, 11.3, 11.1],
            [0.37, 0.43, 0.42],
        ),
        (
            "Старый северный пакгауз",
            -62.0,
            12.3,
            [17.1, 9.1, 9.6],
            [0.37, 0.34, 0.43],
        ),
        (
            "Сосновое депо",
            45.4,
            14.4,
            [15.4, 10.2, 8.4],
            [0.32, 0.36, 0.40],
        ),
        (
            "Портовый склад",
            63.6,
            12.9,
            [11.3, 6.9, 8.7],
            [0.35, 0.42, 0.40],
        ),
    ] {
        scene
            .world
            .add_building_at([x, 0.05, z])
            .ok_or("Превышен лимит корпусов набережной.")?;
        let model = scene.world.buildings.last_mut().unwrap();
        model.name = label.into();
        model.size = size;
        model.color = tone;
        model.quarter_turns = if z > 0.0 { 2 } else { 0 };
        model.door = Some(DoorSettings {
            rule_lock: true,
            ..DoorSettings::default()
        });
    }
    for (index, x) in [
        -67.8, -50.4, -32.3, -18.1, -5.4, 5.6, 17.6, 30.4, 49.5, 67.7,
    ]
    .into_iter()
    .enumerate()
    {
        place(
            scene,
            assets,
            &format!("Набережная · светильник {}", index + 1),
            include_bytes!("../demo_assets/rain/lamp.glb"),
            [x, 2.92, -5.73],
            None,
        )?;
        let color = if index % 3 == 0 {
            [0.34, 0.73, 1.0]
        } else {
            [1.0, 0.61, 0.37]
        };
        set_light(scene, color, 2.43, 9.0);
    }
    Ok(())
}

fn station(
    scene: &mut Scene,
    assets: &mut Library,
    label: &str,
    [x, z]: [f32; 2],
    east: bool,
) -> Result<u64, String> {
    place(
        scene,
        assets,
        label,
        if east {
            include_bytes!("../demo_assets/flagship/east-relay.glb")
        } else {
            include_bytes!("../demo_assets/flagship/west-gauge.glb")
        },
        [x, 0.012, z - 1.13],
        Some(ModelCollider {
            center: [0.0, 0.69, 0.0],
            size: [1.17, 1.46, 0.93],
        }),
    )?;
    let tint = if east {
        [1.0, 0.62, 0.31]
    } else {
        [0.24, 0.72, 1.0]
    };
    set_light(scene, tint, 1.80, 4.0);
    Ok(scene.world.models.last().unwrap().id)
}

fn place(
    scene: &mut Scene,
    assets: &mut Library,
    label: &str,
    bytes: &[u8],
    position: [f32; 3],
    collider: Option<ModelCollider>,
) -> Result<u64, String> {
    let id = Placement::next_id(&scene.world.models);
    scene.world.models.push(Placement {
        id,
        label: label.into(),
        source: assets.import_bytes(bytes.to_vec())?,
        position,
        rotation: [0.0; 3],
        scale: [1.0; 3],
        solid: collider.is_some(),
        collider,
        material: Default::default(),
        material_graph: None,
        surface: None,
        paw_ground: false,
        light: None,
        visual_offset: [0.0; 3],
        draw_distance: None,
        seat: None,
    });
    Ok(id)
}

fn set_light(scene: &mut Scene, color: [f32; 3], intensity: f32, radius: f32) {
    scene.world.models.last_mut().unwrap().light = Some(PointLight {
        offset: [0.0, 0.82, 0.24],
        color,
        radius,
        intensity,
    });
}

#[allow(clippy::too_many_arguments)]
fn wire(
    scene: &mut Scene,
    room: u64,
    door: u64,
    mist_anchor: u64,
    west_outcome: u64,
    east_outcome: u64,
    west_signal: u64,
    east_signal: u64,
    canal: u32,
) -> Result<(), String> {
    let start = zone(scene, "Пролог", [-2.38, -0.25], [2.4, 1.7, 1.8], None)?;
    let west = zone(
        scene,
        "Проверить лоток на западе",
        WEST_SURVEY,
        [3.4, 1.8, 2.32],
        None,
    )?;
    let east = zone(
        scene,
        "Проверить реле на востоке",
        EAST_SURVEY,
        [3.4, 1.8, 2.32],
        None,
    )?;
    let west_end = zone(
        scene,
        "Запустить сброс на западе",
        WEST_STATION,
        [3.4, 1.8, 2.32],
        None,
    )?;
    let east_end = zone(
        scene,
        "Подать питание на востоке",
        EAST_STATION,
        [3.4, 1.8, 2.32],
        None,
    )?;
    let enter = zone(
        scene,
        "Войти в мастерскую",
        [0.0, -10.82],
        [7.6, 2.8, 7.55],
        Some(room),
    )?;
    let ledger = zone(
        scene,
        "Заполнить журнал у верстака",
        LEDGER,
        [3.28, 2.2, 1.45],
        Some(room),
    )?;
    let wet = zone(
        scene,
        "Лужа на старой мостовой",
        [0.0, -4.3],
        [4.1, 1.5, 2.65],
        None,
    )?;
    scene.audio.footsteps.zones = vec![
        FootstepZone {
            zone: wet,
            surface: Surface::Water,
        },
        FootstepZone {
            zone: enter,
            surface: Surface::Wood,
        },
    ];

    let west_checked = flag(scene, "Запад проверен")?;
    let east_checked = flag(scene, "Восток проверен")?;
    let inside = flag(scene, "Вход в мастерскую")?;
    let decision = flag(scene, "Решение принято")?;
    let choose_east = flag(scene, "Выбран восток")?;
    let executed = flag(scene, "Пуск выполнен")?;
    let value = scene
        .rules
        .add_counter()
        .ok_or("Нет счётчика результата.")?;
    scene.rules.counters.last_mut().unwrap().name = OUTCOME.into();
    let quest = scene
        .story
        .quests
        .first_mut()
        .ok_or("Нет цели набережной.")?;
    quest.title = "Последняя смена".into();
    quest.description = "F у западного лотка, затем у восточного реле. Вернись в «Север». Выбери пуск у Лиды, выполни его и отметь итог у верстака.".into();
    quest.completion = "Решение внесено в журнал".into();
    quest.objective = Objective::Rules {
        completion_stage: 7,
    };
    let quest_id = quest.id;
    let conversation = scene
        .story
        .conversations
        .first_mut()
        .ok_or("Нет разговора.")?;
    let choice = |text: &str, to| Choice {
        id: 0,
        text: text.into(),
        target: Some(to),
        starts_quest: None,
    };
    let node = |id, speaker, text: &str, choices| Node {
        id,
        speaker,
        text: text.into(),
        choices,
        face: None,
        voice: None,
    };
    conversation.nodes = vec![
        node(
            1,
            Speaker::Npc,
            "Ты проверил обе линии. Восточный шлюз можно запитать, чтобы снова зажечь набережную. Или оставим питание выключенным и спустим дождевую воду в западный лоток. Решай, какой путь безопаснее.",
            vec![choice(EAST_ANSWER, 2), choice(WEST_ANSWER, 3)],
        ),
        node(
            2,
            Speaker::Player,
            "Запитаю восточный шлюз. Проверю огонь на набережной и вернусь к журналу.",
            vec![],
        ),
        node(
            3,
            Speaker::Player,
            "Оставлю питание выключенным. Проверю западный лоток и отмечу сброс в журнале.",
            vec![],
        ),
    ];
    conversation.entry = 1;
    scene.story.migrate_answer_ids();
    let choices = &scene.story.conversations[0].nodes[0].choices;
    let (east_id, west_id) = (choices[0].id, choices[1].id);
    let reply = |choice_id| Event::Dialogue {
        npc_id: rain_workshop::NPC_ID,
        node_id: 1,
        choice_id,
    };
    let upon = |zone_id| Event::EnterZone { zone_id };
    let mark = Event::GameAction {
        action: GameAction::Mark,
    };
    let stage = |stage| Action::SetQuestStage { quest_id, stage };
    let check = |flag_id, value| Condition::Flag { flag_id, value };
    let location = |zone_id| Condition::InZone { zone_id };
    let set = |flag_id, value| Action::SetFlag { flag_id, value };
    let mut smoke = |name: &str, color, intensity| -> Result<u32, String> {
        let id = scene
            .effects
            .create(Kind::Smoke)
            .ok_or("Нет шаблона пара.")?;
        let p = &mut scene.effects.presets.last_mut().unwrap().settings;
        p.name = name.into();
        p.color = color;
        p.intensity = intensity;
        p.duration = 7.4;
        Ok(id)
    };
    let mist = smoke("Контровой пар от лотка", [0.64, 0.76, 0.78], 1.10)?;
    let exhaust = smoke("Пар успешного пуска", [0.70, 0.68, 0.59], 1.29)?;
    let effect = |preset, anchor| {
        Action::PlayEffect(Effect {
            preset,
            origin: Origin::Object(anchor),
        })
    };
    for (title, event, conditions, actions) in [
        (
            "Начать обход",
            upon(start),
            vec![check(west_checked, false)],
            vec![stage(1)],
        ),
        (
            "Записать западное давление",
            mark,
            vec![location(west)],
            vec![set(west_checked, true), stage(2), effect(mist, mist_anchor)],
        ),
        (
            "Записать восточное реле и открыть мастерскую",
            mark,
            vec![location(east), check(west_checked, true)],
            vec![
                set(east_checked, true),
                Action::UnlockDoor { door_id: door },
                stage(3),
            ],
        ),
        (
            "Войти через порог мастерской",
            upon(enter),
            vec![check(east_checked, true)],
            vec![set(inside, true), stage(4)],
        ),
        (
            "Выбор восточного шлюза",
            reply(east_id),
            vec![check(inside, true), check(decision, false)],
            vec![set(decision, true), set(choose_east, true), stage(5)],
        ),
        (
            "Выбор западного лотка",
            reply(west_id),
            vec![check(inside, true), check(decision, false)],
            vec![set(decision, true), set(choose_east, false), stage(5)],
        ),
        (
            "Подать напряжение и зажечь маяк",
            mark,
            vec![
                location(east_end),
                check(decision, true),
                check(choose_east, true),
            ],
            vec![
                set(executed, true),
                Action::SetCounter {
                    counter_id: value,
                    value: 1,
                },
                Action::SetObjectEnabled {
                    object_id: east_signal,
                    enabled: true,
                },
                stage(6),
                effect(exhaust, east_outcome),
            ],
        ),
        (
            "Спустить воду и зажечь голубой маяк",
            mark,
            vec![
                location(west_end),
                check(decision, true),
                check(choose_east, false),
            ],
            vec![
                set(executed, true),
                Action::SetCounter {
                    counter_id: value,
                    value: 2,
                },
                Action::SetObjectEnabled {
                    object_id: west_signal,
                    enabled: true,
                },
                Action::Simulation(Command::Water {
                    zone: canal,
                    height_cm: -12,
                    duration_tenths: 14,
                }),
                stage(6),
                effect(exhaust, west_outcome),
            ],
        ),
        (
            "Отметить исход в журнале",
            mark,
            vec![location(ledger), check(executed, true)],
            vec![stage(7)],
        ),
    ] {
        scene.rules.add_rule().ok_or("Превышен лимит правил.")?;
        let rule = scene.rules.rules.last_mut().unwrap();
        rule.name = title.into();
        rule.event = Some(event);
        rule.conditions = conditions;
        rule.actions = actions;
    }
    Ok(())
}

fn zone(
    scene: &mut Scene,
    name: &str,
    [x, z]: [f32; 2],
    size: [f32; 3],
    building: Option<u64>,
) -> Result<u32, String> {
    let id = scene
        .rules
        .add_zone([x, if building.is_some() { 0.12 } else { 0.0 }, z])
        .ok_or("Нет зоны маршрута.")?;
    let zone = scene.rules.zones.last_mut().unwrap();
    zone.name = name.into();
    zone.size = size;
    zone.building_id = building;
    Ok(id)
}

fn flag(scene: &mut Scene, label: &str) -> Result<u32, String> {
    let id = scene.rules.add_flag().ok_or("Нет флага маршрута.")?;
    scene.rules.flags.last_mut().unwrap().name = label.into();
    Ok(id)
}
