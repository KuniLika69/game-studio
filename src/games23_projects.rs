use std::path::Path;

use crate::{
    Scene,
    assets::{Library, MAX_PLACEMENTS, Placement, Source},
    avatar::Avatar,
    environment::Environment,
    interior::ModelCollider,
    map2d::{Collision, Layer, Tile},
    model::{GarmentStyle, HairStyle, HumanStyle},
    story::{Objective, Quest},
    wind_rotor::Rotor,
    world::WorldKind,
};

pub(super) const EXPEDITION: &str = "Пассат · Переправа";
pub(super) const LENS: &str = "Огни Рыбной косы";
pub(super) const EXPEDITION_PADS: [(&str, [f32; 2]); 2] = [
    ("Медный пост у отмели", [-0.9, -4.75]),
    ("Маяк у переправы", [0.26, -14.3]),
];
pub(super) const LENS_PADS: [(&str, [u16; 2]); 2] =
    [("Медная линза", [8, 10]), ("Линза мола", [21, 12])];

pub(super) fn expedition(assets: &mut Library) -> Result<Scene, String> {
    let mut scene = Scene::empty();
    assets.ensure(&Source::Ryan, None)?;
    let mut hero = Avatar::new(
        Source::Ryan,
        "Артём Молвин".into(),
        &assets.get(&Source::Ryan).unwrap().model,
    );
    hero.human_style = Some(HumanStyle {
        garment: GarmentStyle::Sweater,
        hairstyle: HairStyle::Cropped,
        jacket_tint: [0.20, 0.39, 0.37],
        trouser_tint: [0.49, 0.38, 0.28],
        ..HumanStyle::default()
    });
    scene.avatar = Some(hero);
    scene.play_settings.camera.distance = Some(6.4);
    let terrain =
        assets.import_bytes(include_bytes!("../demo_assets/expedition/delta.glb").to_vec())?;
    let mesh = &assets.get(&terrain).unwrap().model;
    let mut visual = Placement::at_ground(
        &[],
        mesh,
        terrain,
        "Дельта · первым светом".into(),
        [0.0; 3],
        67.0,
        0.0,
    );
    visual.position = [0.0; 3];
    visual.scale = [1.0; 3];
    visual.solid = false;
    scene.world.models.push(visual);
    let mut environment = Environment::default();
    environment.time.start_hour = 8.7;
    environment.time.noon_bearing = 67.0;
    environment
        .add_water([8.35, -8.2], -0.6)
        .ok_or("Вода не создана.")?;
    let water = environment.waters.last_mut().unwrap();
    water.name = "Русло дельты".into();
    water.size = [9.3, 28.4];
    water.level = 0.18;
    water.current = [0.08, -0.57];
    water.tint = [0.075, 0.33, 0.42];
    water.wave_height = 0.032;
    environment
        .add_wind([0.0, 3.0, -9.0])
        .ok_or("Ветер не создан.")?;
    let wind = environment.winds.last_mut().unwrap();
    wind.name = "Дневной бриз с дельты".into();
    wind.size = [24.0, 10.0, 41.0];
    wind.direction = [-0.73, 0.0, 0.68];
    wind.speed = 4.9;
    wind.gust = 2.25;
    scene.environment = Some(environment);
    scene.wind_rotor = Some(Rotor {
        position: [2.82, 2.15, -14.75],
        radius: 1.27,
        response: 0.87,
        enabled_by: None,
    });
    scene.audio.footsteps.enabled = true;
    scene.audio.footsteps.default_surface = crate::audio::Surface::Gravel;

    for (index, [x, z, height]) in [
        [-3.30, 0.4, 3.5],
        [-4.2, -4.1, 4.25],
        [-3.33, -7.6, 3.3],
        [-4.4, -11.5, 4.5],
        [-3.73, -13.7, 3.8],
        [-4.02, -19.0, 4.2],
        [3.01, 2.9, 3.9],
        [4.3, -4.35, 2.6],
        [3.79, -12.4, 4.0],
    ]
    .into_iter()
    .enumerate()
    {
        let a = 0.85 + 0.07 * (index % 5) as f32;
        object(
            &mut scene,
            WorldKind::Tree,
            "Ива у дельты",
            [x, 0.0, z],
            [height * 0.69, height, height * 0.65],
            [0.23 * a, 0.38 * a, 0.19 * a],
            true,
        )?;
    }
    for (index, [x, z, s]) in [
        [-2.35, -1.2, 1.25],
        [2.98, -2.9, 0.56],
        [-3.04, -5.4, 1.22],
        [3.17, -7.8, 0.99],
        [2.83, -9.5, 0.79],
        [-2.07, -12.83, 1.05],
        [3.44, -15.9, 1.36],
        [3.27, -17.3, 0.82],
        [-2.8, -18.1, 1.51],
    ]
    .into_iter()
    .enumerate()
    {
        let a = 0.83 + 0.09 * (index % 4) as f32;
        object(
            &mut scene,
            WorldKind::Rock,
            "Валуны дельты",
            [x, 0.02, z],
            [s * 1.18, s * 0.65, s * 0.83],
            [0.27 * a, 0.31 * a, 0.33 * a],
            true,
        )?;
    }
    detail_expedition_props(&mut scene, assets)?;
    for (name, [x, z]) in EXPEDITION_PADS {
        add_zone(&mut scene, name, [x, 0.0, z])?;
        for dx in [-0.97, 0.97] {
            object(
                &mut scene,
                WorldKind::Platform,
                name,
                [x + dx, 0.0, z],
                [0.17, 1.17, 0.17],
                [0.29, 0.26, 0.18],
                false,
            )?;
            object(
                &mut scene,
                WorldKind::Crate,
                name,
                [x + dx, 1.04, z],
                [0.29, 0.26, 0.24],
                [0.83, 0.52, 0.26],
                false,
            )?;
        }
    }
    quest(
        &mut scene,
        EXPEDITION,
        "Отметь медный пост и маяк клавишей F.",
        "Переправа проверена.",
    );
    scene.validate()?;
    Ok(scene)
}

pub(in crate::proof) fn detail_expedition_props(
    scene: &mut Scene,
    assets: &mut Library,
) -> Result<(), String> {
    let is_natural = |item: &crate::world::WorldObject| {
        matches!(item.kind, WorldKind::Tree | WorldKind::Rock)
    };
    let world = &mut scene.world;
    let count = world.objects.iter().filter(|item| is_natural(item)).count();
    if count == 0 {
        return Ok(());
    }
    if count + world.models.len() > MAX_PLACEMENTS {
        return Err("В сцене нет места для моделей растительности и камней.".into());
    }
    let alder = assets.import_bytes(include_bytes!("../demo_assets/expedition/alder.glb").to_vec())?;
    let slate = assets.import_bytes(include_bytes!("../demo_assets/expedition/slate.glb").to_vec())?;
    for item in world.objects.iter().filter(|item| is_natural(item)) {
        let tree = item.kind == WorldKind::Tree;
        let source = if tree { &alder } else { &slate };
        let model = &assets.get(source).unwrap().model;
        let edge = glam::Vec3::from_array(model.bounds_max) - glam::Vec3::from_array(model.bounds_min);
        let collider = item.collider();
        let size = std::array::from_fn(|i| collider.max[i] - collider.min[i]);
        let mut placement = Placement::at_ground(
            &world.models, model, source.clone(),
            if tree { "Ольха" } else { "Сланцевый валун" }.into(),
            item.position, edge.max_element(), 0.0,
        );
        placement.position = item.position;
        let extent = glam::Vec3::from_array(item.size);
        let extent = if tree { extent * glam::Vec3::new(0.78, 1.08, 0.78) } else { extent };
        placement.scale = (extent / edge).to_array();
        placement.rotation[1] = if item.id % 2 == 0 { 180.0 } else { 0.0 };
        placement.solid = item.solid;
        placement.collider = Some(ModelCollider { center: [0.0, size[1] * 0.5, 0.0], size });
        placement.draw_distance = Some(if tree { 85.0 } else { 48.0 });
        world.models.push(placement);
    }
    world.objects.retain(|item| !is_natural(item));
    Ok(())
}

pub(super) fn lens(atlas: &Path) -> Result<Scene, String> {
    let mut scene = crate::map2d::scene(crate::map2d::Mode::Top, false);
    let map = scene.map_2d.as_mut().unwrap();
    map.resize([28, 17]);
    map.set_spawn([3.5, 10.5]);
    let custom = map.import_png(atlas, Some([32, 32]))?;
    map.atlases.last_mut().unwrap().name = "Мозаика Рыбной косы".into();
    let tile = |index| {
        Some(Tile {
            atlas: custom,
            index,
        })
    };
    map.hero = tile(7);
    for z in 0..map.size[1] {
        for x in 0..map.size[0] {
            let shore = z == 5 + u16::from(x % 7 >= 4);
            let river = (12..=14).contains(&x);
            let bridge = river && (12..=13).contains(&z);
            let border = x == 0 || x + 1 == map.size[0] || z == 0 || z + 1 == map.size[1];
            let rock = ((10..=11).contains(&x) && (8..=10).contains(&z))
                || ((16..=18).contains(&x) && (10..=11).contains(&z))
                || ((3..=6).contains(&x) && (6..=7).contains(&z));
            let shore_solid = z < 6 || (z == 6 && shore);
            let terrain = if border || rock {
                4
            } else if bridge {
                3
            } else if shore_solid || river {
                u16::from(shore)
            } else if z == 10 || z == 12 || z == 13 {
                3
            } else if (x * 13 + z * 9) % 11 < 3 {
                9
            } else {
                2
            };
            map.paint([x, z], Layer::Ground, tile(terrain));
            if border || rock || shore_solid || (river && !bridge) {
                map.paint_collision([x, z], Collision::Solid);
            } else if terrain == 2 && (x * 41 + z * 7) % 13 == 0 {
                map.paint([x, z], Layer::Tall, tile(5));
            }
        }
    }
    for (n, (_, cell)) in LENS_PADS.into_iter().enumerate() {
        map.paint(cell, Layer::Detail, tile(if n == 0 { 6 } else { 8 }));
    }
    let coords =
        LENS_PADS.map(|(name, [x, z])| (name, map.to_world([x as f32 + 0.5, z as f32 + 0.5])));
    for (name, p) in coords {
        add_zone(&mut scene, name, p)?;
    }
    quest(
        &mut scene,
        LENS,
        "Настрой обе линзы. Проведи героя вокруг скал и через мост.",
        "Обе линзы светят.",
    );
    scene.validate()?;
    Ok(scene)
}

fn add_zone(scene: &mut Scene, name: &str, at: [f32; 3]) -> Result<(), String> {
    scene.rules.add_zone(at).ok_or("Зона отметки не создана.")?;
    let zone = scene.rules.zones.last_mut().unwrap();
    zone.name = name.into();
    zone.size = [1.9, 2.1, 1.9];
    Ok(())
}

fn quest(scene: &mut Scene, name: &str, text: &str, result: &str) {
    scene.story.quests.push(Quest {
        id: 1,
        title: name.into(),
        description: text.into(),
        completion: result.into(),
        objective: Objective::Rules {
            completion_stage: 1,
        },
    });
}

fn object(
    scene: &mut Scene,
    kind: WorldKind,
    label: &str,
    position: [f32; 3],
    size: [f32; 3],
    color: [f32; 3],
    solid: bool,
) -> Result<(), String> {
    scene.world.add(kind).ok_or("Недоступен объект пейзажа.")?;
    let item = scene.world.objects.last_mut().unwrap();
    item.name = label.into();
    item.position = position;
    item.size = size;
    item.color = color;
    item.solid = solid;
    Ok(())
}
