use crate::{
    Avatar, Studio, View, assets,
    environment::{self, WeatherPreset},
    map2d,
    physics_project::{Addition, PhysicsScene},
    rules::{Action, Condition, Event, GameAction},
    scene::Scene,
    simulation_rules::{Command, Contact, Signal},
    story::{Objective, Quest},
    wind_rotor::Rotor,
    world::{Collider, WorldKind},
};

pub(crate) const NAME: &str = "Мельница у реки";
pub(crate) const MAP_NAME: &str = "Река сверху";
pub(crate) const PLATE_RULE: &str = "Плита открывает шлюз";

impl Studio {
    pub(super) fn mill_demo(&mut self, top: bool) {
        let (mut scene, title) = if top {
            (top_demo(), MAP_NAME)
        } else {
            (mill_demo(), NAME)
        };
        if !top {
            let source = assets::Source::Knight;
            if let Err(error) = self.assets.ensure(&source, None) {
                self.record_import_problem("Модель механика", error);
                return;
            }
            let Some(loaded) = self.assets.get(&source) else {
                return;
            };
            scene.avatar = Some(Avatar::new(
                source,
                "Механик".into(),
                &loaded.model,
            ));
        }
        self.set_scene(scene, None, title);
        self.view = View::Three;
        if !top {
            self.overhead = true;
            self.camera.target = [0.5, 0.55, 1.2];
            self.camera.distance = 7.4;
            self.camera.yaw = -0.65;
            self.camera.pitch = 0.36;
        }
    }
}

fn mill_demo() -> Scene {
    let mut scene = Scene::empty();
    let mut layout = PhysicsScene::mill_demo();
    let (mut environment, volumes) = environment::mill_demo();
    environment.time.hours_per_second = 0.48;

    let crate_body = layout
        .scene
        .bodies
        .iter_mut()
        .find(|body| body.spec.id == layout.crate_body)
        .unwrap();
    crate_body.name = "Ящик для шлюза".into();
    crate_body.spec.center[0] += 2.1;
    let lift = layout
        .scene
        .bodies
        .iter_mut()
        .find(|body| body.spec.id == layout.platform)
        .unwrap();
    lift.name = "Подъёмник".into();
    lift.spec.center[0] = -1.65;
    lift.spec.center[2] = -1.15;
    let plate = layout
        .scene
        .sensors
        .iter_mut()
        .find(|sensor| sensor.spec.id == layout.plate)
        .unwrap();
    plate.name = "Плита шлюза".into();
    plate.spec.bounds.min[0] += 2.1;
    plate.spec.bounds.max[0] += 2.1;
    let float_sensor = layout
        .scene
        .add(Addition::OverlapTrigger, [0.1, 0.23, 3.2])
        .unwrap()
        .id();
    let sensor = layout.scene.sensors.last_mut().unwrap();
    sensor.name = "Верхний датчик поплавка".into();
    sensor.spec.bounds = Collider {
        min: [-0.55, 0.23, 2.75],
        max: [0.86, 0.58, 3.7],
    };
    let _splash = layout
        .scene
        .add(Addition::Crate, [-1.35, 0.60, 3.3])
        .unwrap();
    let falling = layout.scene.bodies.last_mut().unwrap();
    falling.name = "Кусок дерева".into();
    falling.spec.half_extents = [0.15, 0.12, 0.17];
    falling.buoyant = true;

    scene.physics = layout.scene;
    scene.environment = Some(environment);
    let rotor_flag = add_flag(&mut scene, "Поплавок поднялся");
    let water_flag = add_flag(&mut scene, "Шлюз открыт");
    scene.wind_rotor = Some(Rotor {
        enabled_by: Some(rotor_flag),
        ..Rotor::default()
    });
    scene.play_settings.camera.distance = Some(5.5);
    add_box(
        &mut scene,
        "Берег мельницы",
        layout.floor.bounds,
        [0.35, 0.23, 0.15],
        true,
    );
    add_box(
        &mut scene,
        "Дно канала",
        layout.channel_floor.bounds,
        [0.25, 0.35, 0.34],
        true,
    );
    let gate = add_box(
        &mut scene,
        "Заслонка шлюза",
        Collider {
            min: [-2.0, -0.40, 1.94],
            max: [2.0, 0.39, 2.05],
        },
        [0.33, 0.18, 0.09],
        false,
    );
    let lamp = add_box(
        &mut scene,
        "Лампа мельницы",
        Collider {
            min: [2.91, 0.34, 0.44],
            max: [3.17, 0.58, 0.70],
        },
        [0.34, 0.91, 0.29],
        false,
    );
    add_box(
        &mut scene,
        "Опора лампы",
        Collider {
            min: [2.97, 0.0, 0.48],
            max: [3.11, 0.35, 0.66],
        },
        [0.3, 0.23, 0.15],
        false,
    );
    scene.rules.initially_disabled_objects.push(lamp);

    let quest = add_quest(
        &mut scene,
        NAME,
        "Толкни ящик на плиту шлюза",
        "Шлюз открыт, мельница работает",
        3,
    );
    add_rule(
        &mut scene,
        "Осмотреть шлюз",
        Event::GameAction {
            action: GameAction::Mark,
        },
        vec![],
        vec![stage(quest, 1)],
    );
    add_rule(
        &mut scene,
        PLATE_RULE,
        Event::Simulation(Signal::Contact {
            sensor_id: layout.plate,
            state: Contact::Pressed,
        }),
        vec![],
        vec![
            Action::Simulation(Command::Water {
                zone: volumes.water,
                height_cm: (volumes.sluice_level * 100.0).round() as i16,
                duration_tenths: 7,
            }),
            Action::SetObjectEnabled {
                object_id: gate,
                enabled: false,
            },
            stage(quest, 1),
        ],
    );
    add_rule(
        &mut scene,
        "Поплавок включает передачу",
        Event::Simulation(Signal::Contact {
            sensor_id: float_sensor,
            state: Contact::Entered,
        }),
        vec![],
        vec![Action::SetFlag {
            flag_id: rotor_flag,
            value: true,
        }],
    );
    add_rule(
        &mut scene,
        "Вода дошла до нужной отметки",
        Event::Simulation(Signal::WaterReached {
            zone: volumes.water,
        }),
        vec![],
        vec![
            Action::SetFlag {
                flag_id: water_flag,
                value: true,
            },
            stage(quest, 2),
            Action::Simulation(Command::Weather {
                preset: WeatherPreset::Rain,
                duration_tenths: 13,
            }),
        ],
    );
    add_rule(
        &mut scene,
        "Оборот мельницы включает лампу",
        Event::Simulation(Signal::RotorTurned),
        vec![Condition::Flag {
            flag_id: water_flag,
            value: true,
        }],
        vec![
            Action::SetObjectEnabled {
                object_id: lamp,
                enabled: true,
            },
            stage(quest, 3),
        ],
    );
    scene
}

fn top_demo() -> Scene {
    let mut scene = map2d::scene(map2d::Mode::Top, true);
    let (mut environment, volumes) = environment::mill_demo();
    let volume = environment
        .waters
        .iter_mut()
        .find(|area| area.id == volumes.water)
        .unwrap();
    volume.name = "Канал на плане".into();
    volume.center = [12.0, 11.1];
    volume.size = [10.1, 3.6];
    scene.environment = Some(environment);
    let quest = add_quest(
        &mut scene,
        MAP_NAME,
        "Нажми F, чтобы поднять воду",
        "Канал заполнен",
        2,
    );
    add_rule(
        &mut scene,
        "Поднять воду в канале",
        Event::GameAction {
            action: GameAction::Mark,
        },
        vec![],
        vec![
            Action::Simulation(Command::Water {
                zone: volumes.water,
                height_cm: 42,
                duration_tenths: 12,
            }),
            stage(quest, 1),
        ],
    );
    add_rule(
        &mut scene,
        "Канал заполнен",
        Event::Simulation(Signal::WaterReached {
            zone: volumes.water,
        }),
        vec![],
        vec![stage(quest, 2)],
    );
    scene
}

fn add_box(scene: &mut Scene, name: &str, bounds: Collider, color: [f32; 3], solid: bool) -> u64 {
    let id = scene.world.add(WorldKind::Platform).unwrap();
    let object = scene.world.objects.last_mut().unwrap();
    object.name = name.into();
    object.position = [
        (bounds.min[0] + bounds.max[0]) * 0.5,
        bounds.min[1],
        (bounds.min[2] + bounds.max[2]) * 0.5,
    ];
    object.size = std::array::from_fn(|axis| bounds.max[axis] - bounds.min[axis]);
    object.color = color;
    object.solid = solid;
    id
}

fn add_flag(scene: &mut Scene, name: &str) -> u32 {
    let id = scene.rules.add_flag().unwrap();
    scene.rules.flags.last_mut().unwrap().name = name.into();
    id
}

fn add_quest(
    scene: &mut Scene,
    title: &str,
    description: &str,
    completion: &str,
    completion_stage: u16,
) -> u32 {
    let id = 1;
    scene.story.quests.push(Quest {
        id,
        title: title.into(),
        description: description.into(),
        completion: completion.into(),
        objective: Objective::Rules { completion_stage },
    });
    id
}

fn stage(quest_id: u32, stage: u16) -> Action {
    Action::SetQuestStage { quest_id, stage }
}

fn add_rule(
    scene: &mut Scene,
    name: &str,
    event: Event,
    conditions: Vec<Condition>,
    actions: Vec<Action>,
) {
    scene.rules.add_rule().unwrap();
    let rule = scene.rules.rules.last_mut().unwrap();
    rule.name = name.into();
    rule.event = Some(event);
    rule.conditions = conditions;
    rule.actions = actions;
}
