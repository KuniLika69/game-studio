use glam::Vec3;

use crate::{
    Avatar, Studio, View,
    assets::{Library, Placement, Source, materials::PointLight},
    audio::Surface,
    audio_project::FootstepZone,
    environment::{Environment, WeatherPreset},
    interior::{FurnitureKind, ModelCollider},
    model::{GarmentStyle, HairStyle, HumanStyle},
    npcs::{FIRST_NPC_ID, Npc, Waypoint},
    rules::{Action, Condition, Event, GameAction},
    scene::Scene,
    story::{Choice, Node, Objective, Story},
    viewport3d::Camera,
    world::DoorSettings,
};

pub const NAME: &str = "Мастерская после дождя";
pub const WORKSHOP: &str = "Мастерская Север · 19";
pub const BENCH: &str = "Дубовый верстак с тисками";
pub const SPEAKER: &str = "Лида Горина";
pub const NPC_ID: u64 = FIRST_NPC_ID;
pub const ACCEPT: &str = "Занесу инструмент";
pub const DECLINE: &str = "Чуть позже";
pub const FINISH: &str = "Инструменты готовы";
pub const FRONT_LIGHT: &str = "Фонарь у входа";
pub const TABLE_LIGHT: &str = "Лампа над верстаком";
pub const LIGHT_CAMERA: Camera = Camera {
    target: [0.0, 1.20, -4.95],
    yaw: 0.29,
    pitch: 0.19,
    distance: 12.60,
};
const BUILDING_CENTER: [f32; 3] = [0.0, 0.12, -11.05];
const BENCH_Z: f32 = -12.70;
const PUDDLE: [f32; 2] = [0.0, -4.30];
const PUDDLE_SIZE: [f32; 2] = [5.10, 2.26];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    BlueHour,
    AmberRain,
    MorningMist,
}

impl Style {
    #[cfg(test)]
    pub const ALL: [Self; 3] = [Self::BlueHour, Self::AmberRain, Self::MorningMist];

    pub fn label(self) -> &'static str {
        match self {
            Self::BlueHour => "Синий час",
            Self::AmberRain => "Янтарный дождь",
            Self::MorningMist => "Утренний туман",
        }
    }
}

impl Studio {
    pub(super) fn rain_workshop_demo(&mut self, style: Style) {
        match create(&mut self.assets, style) {
            Ok(scene) => {
                self.set_scene(scene, None, &format!("{NAME} · {}", style.label()));
                self.view = View::Three;
                self.camera = LIGHT_CAMERA;
            }
            Err(error) => self.status = error,
        }
    }

    pub(super) fn dialogue_portrait(&self) -> Option<Camera> {
        if !self.dialogue_closeup || self.free_camera {
            return None;
        }
        let npc_id = self.story_session.active_npc()?;
        let npc = self.scene.npcs.iter().find(|npc| npc.id == npc_id)?;
        let npc_position = self.npc_position(npc_id)?;
        let player_position = self.actor_position();
        let player_speaking = self.active_speaker() == Some(crate::story::Speaker::Player);
        let (speaker, listener, height) =
            if player_speaking {
                (
                    player_position,
                    npc_position,
                    self.scene.avatar.as_ref()?.height,
                )
            } else {
                (npc_position, player_position, npc.avatar.height)
            };
        let delta = listener - speaker;
        let forward = Vec3::new(delta.x, 0.0, delta.z)
            .try_normalize()
            .unwrap_or(Vec3::Z);
        let (height_ratio, eye_delta) = if player_speaking {
            let avatar = self.scene.avatar.as_ref()?;
            let face_yaw = (self.avatar_heading + avatar.yaw).to_radians();
            let face_direction = Vec3::new(face_yaw.sin(), 0.0, face_yaw.cos());
            let side = face_direction.cross(Vec3::Y);
            let open_side = if side.dot(forward) > 0.0 { -1.0 } else { 1.0 };
            // Offset the camera away from the listener so a close foreground head
            // cannot hide the actor who is speaking.
            (0.87, face_direction * 1.30 + side * (open_side * 0.39) + Vec3::Y * 0.10)
        } else {
            (0.84, forward * 1.63 + forward.cross(Vec3::Y) * 0.43 + Vec3::Y * 0.11)
        };
        let target = speaker + Vec3::Y * (height * height_ratio);
        Some(Camera {
            target: target.to_array(),
            yaw: eye_delta.x.atan2(eye_delta.z),
            pitch: (eye_delta.y / eye_delta.length()).asin(),
            distance: eye_delta.length(),
        })
    }
}

pub fn create(library: &mut Library, style: Style) -> Result<Scene, String> {
    let mut scene = Scene::empty();
    scene.play_settings.camera.distance = Some(4.45);
    scene.play_settings.camera.follow_rate = 10.5;
    for actor in [Source::Ryan, Source::Emma] {
        library.ensure(&actor, None)?;
    }

    let room = scene
        .world
        .add_building_at(BUILDING_CENTER)
        .ok_or("Площадка мастерской не построена.")?;
    let building = scene.world.buildings.last_mut().unwrap();
    building.name = WORKSHOP.into();
    building.size = [8.20, 4.16, 8.0];
    building.door_width = 1.38;
    building.door_height = 2.53;
    building.door = Some(DoorSettings {
        rule_lock: true,
        hand_contact: true,
        ..DoorSettings::default()
    });
    let door = building.doors().next().ok_or("Нет входной двери.")?.id;
    building.color = [0.32, 0.43, 0.46];
    for (kind, position, name, tint, turns) in [
        (
            FurnitureKind::Sofa,
            [3.27, 1.85],
            "Кресло у двери",
            [0.24, 0.33, 0.31],
            1,
        ),
        (
            FurnitureKind::Chair,
            [1.78, -1.06],
            "Табурет мастера",
            [0.43, 0.28, 0.17],
            0,
        ),
        (
            FurnitureKind::Cabinet,
            [3.27, -2.68],
            "Шкаф из тёмного дуба",
            [0.35, 0.25, 0.17],
            1,
        ),
    ] {
        building
            .interior
            .add_furniture(kind, position, building.size);
        let item = building
            .interior
            .furniture
            .last_mut()
            .ok_or("Не создана мебель.")?;
        item.name = name.into();
        item.color = tint;
        item.quarter_turns = turns;
    }

    let (side_shift, front_shift) = match style {
        Style::BlueHour => ([0.0, 0.0], [0.0, 0.0]),
        Style::AmberRain => ([-0.68, 0.42], [0.52, -1.45]),
        Style::MorningMist => ([-1.08, 1.35], [-0.68, 0.96]),
    };
    for (name, position, size, color, rotation) in [
        (
            "Кирпичный склад",
            [-8.09 + side_shift[0], 0.07, -11.15 + front_shift[0]],
            [7.22, 6.35, 7.0],
            [0.45, 0.37, 0.33],
            0,
        ),
        (
            "Серый дощатый фронтон",
            [8.53 + side_shift[1], 0.07, -12.10 + front_shift[1]],
            [7.7, 5.35, 8.18],
            [0.39, 0.45, 0.43],
            0,
        ),
        (
            "Дом телефонной станции",
            [15.83, 0.07, -13.18],
            [6.85, 7.42, 8.2],
            [0.36, 0.39, 0.45],
            0,
        ),
        (
            "Дом у проезда",
            [-15.21, 0.07, -12.21],
            [6.6, 7.61, 8.3],
            [0.48, 0.43, 0.37],
            0,
        ),
        (
            "Тихий двор",
            [-9.31, 0.0, 4.90],
            [6.35, 6.3, 6.4],
            [0.33, 0.38, 0.40],
            1,
        ),
    ] {
        scene
            .world
            .add_building_at(position)
            .ok_or("Не создана соседняя постройка.")?;
        let building = scene.world.buildings.last_mut().unwrap();
        building.name = name.into();
        building.size = size;
        building.color = color;
        building.quarter_turns = rotation;
        building.door = Some(DoorSettings {
            rule_lock: true,
            ..DoorSettings::default()
        });
    }

    place(
        &mut scene,
        library,
        "Брусчатка, гранитный бордюр и водостоки",
        include_bytes!("../demo_assets/rain/street.glb"),
        [0.0; 3],
        Some(ModelCollider {
            center: [0.0, 0.023, -15.1],
            size: [47.95, 0.096, 17.82],
        }),
    )?;
    place(
        &mut scene,
        library,
        "Козырёк, вывеска и латунное обрамление",
        include_bytes!("../demo_assets/rain/facade.glb"),
        [0.0, 0.0, -7.05],
        None,
    )?;
    place(
        &mut scene,
        library,
        "Дубовый пол, стеллаж и светильники",
        include_bytes!("../demo_assets/rain/room.glb"),
        BUILDING_CENTER,
        None,
    )?;
    place(
        &mut scene,
        library,
        BENCH,
        include_bytes!("../demo_assets/rain/workbench.glb"),
        [-0.93, 0.12, BENCH_Z],
        Some(ModelCollider {
            center: [0.0, 0.46, 0.0],
            size: [2.22, 0.92, 0.79],
        }),
    )?;
    place(
        &mut scene,
        library,
        "Фонари, мокрая листва и гранитные кадки",
        include_bytes!("../demo_assets/rain/fixtures.glb"),
        [side_shift[0] * 0.31, 0.0, side_shift[1] * 0.18],
        None,
    )?;
    for (label, position, intensity, radius, color) in [
        (
            FRONT_LIGHT,
            [1.61, 2.45, -6.52],
            2.4,
            7.5,
            [1.0, 0.72, 0.41],
        ),
        (
            TABLE_LIGHT,
            [-0.95, 2.58, -12.42],
            1.35,
            5.1,
            [1.0, 0.68, 0.39],
        ),
        (
            "Дежурный фонарь у Лиды",
            [-3.28, 2.32, -2.10],
            1.65,
            5.1,
            [0.62, 0.77, 1.0],
        ),
    ] {
        place(
            &mut scene,
            library,
            label,
            include_bytes!("../demo_assets/rain/lamp.glb"),
            position,
            None,
        )?;
        scene.world.models.last_mut().unwrap().light = Some(PointLight {
            offset: [0.0, 0.07, 0.14],
            color,
            radius,
            intensity,
        });
    }
    atmosphere(&mut scene, style)?;
    populate(&mut scene, library)?;
    story(&mut scene, room, door)?;
    crate::workshop_dialogue::author(&mut scene, NPC_ID)?;
    add_step_samples(&mut scene)?;
    scene.validate()?;
    Ok(scene)
}

fn add_step_samples(scene: &mut Scene) -> Result<(), String> {
    for (surface, wav_files) in [
        (
            Surface::Stone,
            [
                &include_bytes!("../demo_assets/workshop_audio/steps/stone-1.wav")[..],
                &include_bytes!("../demo_assets/workshop_audio/steps/stone-2.wav")[..],
                &include_bytes!("../demo_assets/workshop_audio/steps/stone-3.wav")[..],
            ],
        ),
        (
            Surface::Wood,
            [
                &include_bytes!("../demo_assets/workshop_audio/steps/wood-1.wav")[..],
                &include_bytes!("../demo_assets/workshop_audio/steps/wood-2.wav")[..],
                &include_bytes!("../demo_assets/workshop_audio/steps/wood-3.wav")[..],
            ],
        ),
        (
            Surface::Water,
            [
                &include_bytes!("../demo_assets/workshop_audio/steps/water-1.wav")[..],
                &include_bytes!("../demo_assets/workshop_audio/steps/water-2.wav")[..],
                &include_bytes!("../demo_assets/workshop_audio/steps/water-3.wav")[..],
            ],
        ),
    ] {
        let variants = wav_files
            .into_iter()
            .enumerate()
            .map(|(index, wav)| {
                scene.audio.import_bytes(
                    format!("Шаг · {} · {}", surface.label(), index + 1),
                    wav.to_vec(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        scene.audio.footsteps.samples.insert(surface, variants);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_styles_share_playable_setup_and_have_distinct_authored_light_and_layout() {
        let mut assets = Library::default();
        let variants = Style::ALL.map(|style| create(&mut assets, style).unwrap());
        let base = &variants[0];
        for alternate in &variants[1..] {
            assert_ne!(
                base.world.buildings[1].position,
                alternate.world.buildings[1].position
            );
            assert_ne!(base.environment, alternate.environment);
            assert_ne!(base.lighting, alternate.lighting);
            assert_eq!(base.rules, alternate.rules);
            assert_eq!(alternate.story.conversations[0].nodes[0].voice, Some(1));
        }
        assert!(
            base.world
                .models
                .iter()
                .any(|item| item.label == FRONT_LIGHT && item.point_light().is_some())
        );
        assert!(base.world.buildings[0].door.unwrap().hand_contact);
    }
}

fn place(
    scene: &mut Scene,
    library: &mut Library,
    label: &str,
    bytes: &[u8],
    position: [f32; 3],
    collider: Option<ModelCollider>,
) -> Result<(), String> {
    let source = library.import_bytes(bytes.to_vec())?;
    scene.world.models.push(Placement {
        id: Placement::next_id(&scene.world.models),
        source,
        label: label.into(),
        position,
        rotation: [0.0; 3],
        scale: [1.0; 3],
        solid: collider.is_some(),
        collider,
        light: None,
        seat: None,
        material: Default::default(),
        material_graph: None,
        surface: None,
        paw_ground: false,
        visual_offset: [0.0; 3],
        draw_distance: None,
    });
    Ok(())
}

fn atmosphere(scene: &mut Scene, style: Style) -> Result<(), String> {
    let mut environment = Environment::default();
    let (hour, bearing, daylight, interior, weather) = match style {
        Style::BlueHour => (18.36, 159.0, 1.25, 2.30, WeatherPreset::Rain),
        Style::AmberRain => (16.93, 207.0, 1.46, 2.55, WeatherPreset::Rain),
        Style::MorningMist => (6.83, 125.0, 1.15, 1.80, WeatherPreset::Fog),
    };
    environment.time.start_hour = hour;
    environment.time.noon_bearing = bearing;
    environment.weather.initial = weather;
    environment.prevailing_direction = [-0.86, 0.0, 0.34];
    let wind = environment
        .add_wind([1.3, 0.9, -4.2])
        .ok_or("Не создан ветер улицы.")?;
    let zone = environment
        .winds
        .iter_mut()
        .find(|zone| zone.id == wind)
        .unwrap();
    zone.name = "Ветер поперёк мостовой".into();
    zone.size = [20.0, 3.6, 12.0];
    zone.direction = [-0.8, 0.05, 0.44];
    zone.speed = 2.50;
    zone.gust = 1.46;
    let basin = environment
        .add_water(PUDDLE, -0.052)
        .ok_or("Не создана лужа.")?;
    let water = environment
        .waters
        .iter_mut()
        .find(|zone| zone.id == basin)
        .unwrap();
    water.name = "Лужа в просевшей мостовой".into();
    water.size = PUDDLE_SIZE;
    water.level = 0.006;
    water.max_level = 0.075;
    water.current = [0.0, 0.0];
    water.tint = [0.20, 0.34, 0.45];
    water.wave_height = 0.004;
    water.wave_hz = 1.97;
    scene.environment = Some(environment);
    scene.lighting.daylight = daylight;
    scene.lighting.interior_gain = interior;
    Ok(())
}

fn populate(scene: &mut Scene, library: &Library) -> Result<(), String> {
    let mut player = Avatar::new(
        Source::Ryan,
        "Арсен Левин".into(),
        &library
            .get(&Source::Ryan)
            .ok_or("Нет модели Райана.")?
            .model,
    );
    player.human_style = Some(HumanStyle {
        face_width: -0.11,
        jaw_width: 0.20,
        nose_length: 0.11,
        brow_raise: -0.07,
        hairstyle: HairStyle::Swept,
        hair_volume: -0.07,
        garment: GarmentStyle::Sweater,
        jacket_tint: [0.37, 0.51, 0.56],
        trouser_tint: [0.58, 0.67, 0.70],
        ..HumanStyle::default()
    });
    scene.avatar = Some(player);

    let mut npc = Avatar::new(
        Source::Emma,
        SPEAKER.into(),
        &library.get(&Source::Emma).ok_or("Нет модели Эммы.")?.model,
    );
    npc.human_style = Some(HumanStyle {
        face_width: 0.19,
        jaw_width: -0.21,
        nose_length: -0.16,
        smile: 0.06,
        brow_raise: 0.13,
        hairstyle: HairStyle::Full,
        hair_volume: 0.14,
        garment: GarmentStyle::Sweater,
        jacket_tint: [0.78, 0.57, 0.34],
        trouser_tint: [0.54, 0.62, 0.57],
        ..HumanStyle::default()
    });
    let mut npc = Npc::new(
        NPC_ID,
        npc,
        vec![
            Waypoint {
                position: [-1.49, 0.0, -2.18],
                wait: 33.0,
            },
            Waypoint::at([-1.49, 0.0, -5.24]),
            Waypoint::at([0.0, 0.07, -6.26]),
            Waypoint::at([0.03, 0.12, -8.24]),
            Waypoint {
                position: [1.51, 0.12, -10.58],
                wait: 38.0,
            },
            Waypoint::at([0.03, 0.12, -8.24]),
            Waypoint::at([0.0, 0.07, -6.26]),
            Waypoint::at([-1.49, 0.0, -5.24]),
        ],
    );
    npc.speed = 1.16;
    scene.npcs.push(npc);
    Ok(())
}

fn story(scene: &mut Scene, workshop: u64, door: u64) -> Result<(), String> {
    scene.story = Story::demo(NPC_ID, [0.0, BENCH_Z]);
    let conversation = &mut scene.story.conversations[0];
    conversation.name = SPEAKER.into();
    let node = conversation.entry;
    let option = |text: &str| Choice {
        id: 0,
        text: text.into(),
        target: None,
        starts_quest: None,
    };
    conversation.nodes = vec![Node {
        id: node,
        text: "Арсен, спасибо, что пришёл. Перейди улицу, открой дверь и отметь на верстаке, что инструменты готовы.".into(),
        choices: vec![option(ACCEPT), option(DECLINE)],
        face: None,
        voice: None,
        speaker: crate::story::Speaker::Npc,
    }];
    scene.story.migrate_answer_ids();
    let choice = scene.story.conversations[0].nodes[0].choices[0].id;
    let quest = &mut scene.story.quests[0];
    quest.title = "Вечерняя смена".into();
    quest.description = "Войди в мастерскую через дверь и нажми F возле верстака.".into();
    quest.completion = FINISH.into();
    quest.objective = Objective::Rules {
        completion_stage: 3,
    };
    let quest_id = quest.id;
    let entered = zone(
        scene,
        "Мастерская · внутри",
        [0.0, 0.12, -10.82],
        [7.6, 2.8, 7.55],
        Some(workshop),
    )?;
    let tools = zone(
        scene,
        "Отметка у верстака",
        [-0.87, 0.12, -11.79],
        [3.28, 2.2, 1.45],
        Some(workshop),
    )?;
    let wet = zone(
        scene,
        "Подошва по мокрому камню и воде",
        [PUDDLE[0], 0.0, PUDDLE[1]],
        [4.1, 1.5, 1.58],
        None,
    )?;
    scene.audio.footsteps.enabled = true;
    scene.audio.footsteps.gain = 0.68;
    scene.audio.footsteps.stride = 0.58;
    scene.audio.footsteps.zones = vec![
        FootstepZone {
            zone: wet,
            surface: Surface::Water,
        },
        FootstepZone {
            zone: entered,
            surface: Surface::Wood,
        },
    ];

    let asked = scene.rules.add_flag().ok_or("Не создан флаг поручения.")?;
    scene.rules.flags.last_mut().unwrap().name = "Лида разрешила открыть дверь".into();
    let visited = scene.rules.add_flag().ok_or("Не создан флаг входа.")?;
    scene.rules.flags.last_mut().unwrap().name = "Герой вошёл в мастерскую".into();
    let flag = |flag_id| Action::SetFlag {
        flag_id,
        value: true,
    };
    let condition = |flag_id| Condition::Flag {
        flag_id,
        value: true,
    };
    let stage = |stage| Action::SetQuestStage { quest_id, stage };
    for (name, event, conditions, actions) in [
        (
            "Поручение Лиды",
            Event::Dialogue {
                npc_id: NPC_ID,
                node_id: node,
                choice_id: choice,
            },
            vec![],
            vec![flag(asked), Action::UnlockDoor { door_id: door }, stage(1)],
        ),
        (
            "За порогом",
            Event::EnterZone { zone_id: entered },
            vec![condition(asked)],
            vec![flag(visited), stage(2)],
        ),
        (
            "Отметка на верстаке",
            Event::GameAction {
                action: GameAction::Mark,
            },
            vec![condition(visited), Condition::InZone { zone_id: tools }],
            vec![stage(3)],
        ),
    ] {
        scene
            .rules
            .add_rule()
            .ok_or("Не создано правило мастерской.")?;
        let rule = scene.rules.rules.last_mut().unwrap();
        rule.name = name.into();
        rule.event = Some(event);
        rule.conditions = conditions;
        rule.actions = actions;
    }
    Ok(())
}

fn zone(
    scene: &mut Scene,
    name: &str,
    at: [f32; 3],
    size: [f32; 3],
    building_id: Option<u64>,
) -> Result<u32, String> {
    let id = scene
        .rules
        .add_zone(at)
        .ok_or("Не создана зона мастерской.")?;
    let zone = scene.rules.zones.last_mut().unwrap();
    zone.name = name.into();
    zone.size = size;
    zone.building_id = building_id;
    Ok(id)
}
