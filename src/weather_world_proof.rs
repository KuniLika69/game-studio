use std::{fs, path::Path};

use glam::Vec3;
use serde_json::json;

use super::{
    Driver, FPS, SIZE, Sink, io_error,
    staged_proof::{panel_click, reload},
};
use crate::{
    Studio, configure_context,
    creature_mesh::Kind,
    editor_workspace::Workspace,
    environment::WeatherPreset,
    game_export::GamePackage,
    npcs::Waypoint,
    offscreen::Offscreen,
    play_settings::CameraMode,
    proof_video::Video,
    studio_welcome::Page,
    surface_palette::{Family, Recipe},
    viewport3d::{self, Camera},
};

pub(super) fn author(output: &Path, exe: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let project = output.join("changing-coast.robot.json");
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(&output.join("editor-from-empty.mp4"), SIZE[0], SIZE[1], FPS)?;
    let mut sink = Sink::Video(&mut video);
    let mut app = Studio {
        welcome: Some(Page::Home),
        workspace: Workspace::Create,
        ..Studio::default()
    };
    app.desk.recovery_root = Some(output.join("recovery"));
    let mut driver = Driver::new(app, &mut display, &mut sink, Some((&project, exe)));
    driver.settle(8)?;
    for label in [
        "Создать проект",
        "3D-сцена",
        "Окружение",
        "Прибрежная деревня",
        "Собрать деревню",
    ] {
        driver.click(label)?;
    }
    driver.hold(23)?;
    for label in ["Персонаж", "Собрать робота или зверя…"] {
        driver.click(label)?;
    }
    screenshot(&mut driver, &output.join("workshop.png"))?;
    driver.click("Собрать персонажа")?;
    for label in [
        "NPC",
        "+ Создать NPC",
        "Собрать робота или зверя…",
        "Четвероногое существо",
        "Собрать персонажа",
    ] {
        driver.click(label)?;
    }
    let dog = driver
        .app
        .scene
        .npcs
        .last_mut()
        .ok_or("В редакторе не создан зверь.")?;
    dog.route = vec![
        Waypoint::at([1.85, 0.0, -3.3]),
        Waypoint::at([2.75, 0.0, -7.5]),
        Waypoint::at([2.9, 0.0, -10.7]),
        Waypoint::at([0.6, 0.0, -7.6]),
    ];
    let dog_id = dog.id;
    driver.app.sync_npc_assets();
    driver.settle(5)?;
    driver.click("Изменить конструкцию…")?;
    screenshot(&mut driver, &output.join("animal-workshop.png"))?;
    driver.click("Обновить персонажа")?;

    let model = driver
        .app
        .scene
        .world
        .models
        .iter_mut()
        .find(|model| model.label == "Дорога по маршруту")
        .ok_or("Дорога не создана.")?;
    crate::surface_palette::apply(
        model,
        &mut driver.app.assets,
        Recipe {
            family: Family::Paving,
            seed: 731,
        },
    )?;
    let environment = driver
        .app
        .scene
        .environment
        .as_mut()
        .ok_or("Среда не создана.")?;
    environment.time.start_hour = 14.2;
    environment
        .add_wind([0.0, 1.0, -10.0])
        .ok_or("Нет зоны ветра.")?;
    let wind = environment.winds.last_mut().unwrap();
    wind.name = "Порывы с бухты".into();
    wind.size = [38.0, 9.0, 49.0];
    wind.direction = [0.76, 0.0, -0.65];
    wind.speed = 4.4;
    wind.gust = 2.7;
    wind.gust_hz = 0.55;
    wind.feather = 3.1;
    environment
        .weather
        .add_cue(WeatherPreset::Rain)
        .ok_or("Нет перехода к дождю.")?;
    environment
        .weather
        .add_cue(WeatherPreset::Snow)
        .ok_or("Нет перехода к снегу.")?;
    environment.weather.cues[0].start_seconds = 2.75;
    environment.weather.cues[0].blend_seconds = 1.35;
    environment.weather.cues[1].start_seconds = 8.0;
    environment.weather.cues[1].blend_seconds = 1.65;
    driver.app.changed();

    driver.click("Ещё инструменты")?;
    driver.click("Среда")?;
    driver.click("Погода")?;
    driver.click("▶ Показать погоду в движении")?;
    driver.hold(40)?;
    panel_click(&mut driver, "Сохранить как…")?;
    if !project.is_file() {
        return Err(driver.app.status.clone());
    }
    reload(&mut driver)?;
    check(&driver.app, dog_id)?;
    panel_click(&mut driver, "Собрать игру для Windows")?;
    if !exe.is_file() {
        return Err(driver.app.status.clone());
    }
    driver.hold(30)?;
    fs::write(output.join("editor.json"), json!({"input_actions":driver.input_actions,"project_reopened":true,"animal_id":dog_id,"weather_setup":"scripted scene parameters, saved in editor"}).to_string()).map_err(io_error)?;
    video.finish()
}

pub(super) fn player(package: GamePackage, output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let name = display.adapter_info().name.clone();
    let mut app = Studio::from_package(package, display.context());
    let dog_id = app.scene.npcs.last().ok_or("Нет зверя в EXE.")?.id;
    check(&app, dog_id)?;
    if !app.game_running {
        return Err("Экспортированная игра не запустилась.".into());
    }
    app.scene.play_settings.camera.mode = CameraMode::Fixed;
    app.camera = Camera {
        target: [1.1, 0.66, -4.9],
        yaw: 0.73,
        pitch: 0.16,
        distance: 9.1,
    };
    let mut video = Video::start(&output.join("weather-from-exe.mp4"), SIZE[0], SIZE[1], FPS)?;
    let mut sink = Sink::Video(&mut video);
    let mut driver = Driver::new(app, &mut display, &mut sink, None);
    driver.settle(36)?;
    let clear = sample(&driver)?;
    screenshot(&mut driver, &output.join("coast-clear.png"))?;

    super::quarter_proof::walk(&mut driver, [0.25, -5.2])?;
    until(&mut driver, 4.6)?;
    let rain = sample(&driver)?;
    if rain.1 < 0.95 || rain.2 > 0.10 {
        return Err(format!("Нет дождя на отметке {rain:?}."));
    }
    let water = driver
        .app
        .scene
        .environment
        .as_ref()
        .unwrap()
        .waters
        .first()
        .ok_or("В EXE нет воды.")?;
    let [x, z] = water.center;
    let [width, _] = water.size;
    driver.app.camera = Camera {
        target: [x - width * 0.44, 0.5, z],
        yaw: 0.84,
        pitch: 0.23,
        distance: 12.0,
    };
    screenshot(&mut driver, &output.join("coast-rain.png"))?;
    driver.settle(78)?;

    driver.app.camera = Camera {
        target: [0.9, 0.62, -5.6],
        yaw: 0.53,
        pitch: 0.14,
        distance: 9.5,
    };
    until(&mut driver, 10.25)?;
    let snow = sample(&driver)?;
    if snow.2 < 0.95 || snow.1 > 0.10 || snow.3 < 1.0 {
        return Err(format!("Нет снега или ветра на отметке {snow:?}."));
    }
    screenshot(&mut driver, &output.join("coast-snow.png"))?;
    let mut positions = Vec::new();
    let mut paws = Vec::new();
    let mut moving = false;
    for frame in 0..76 {
        let state = driver
            .app
            .npc_simulation
            .state(dog_id)
            .ok_or("Нет физического состояния зверя.")?;
        let pos = state.position();
        moving |= state.motion == crate::avatar::Motion::Walk;
        positions.push(pos);
        driver.app.camera = Camera {
            target: [pos[0], pos[1] + 0.47, pos[2]],
            yaw: 1.15,
            pitch: 0.075,
            distance: 2.53,
        };
        driver.settle(1)?;
        for paw in driver
            .app
            .quadruped_contact_report(dog_id)
            .into_iter()
            .flatten()
        {
            paws.push(json!({"frame":frame,"contact":paw}));
        }
    }
    screenshot(&mut driver, &output.join("animal-exe.png"))?;
    let pos = driver
        .app
        .npc_simulation
        .state(dog_id)
        .ok_or("Нет состояния зверя.")?
        .position();
    driver.app.camera = Camera {
        target: [pos[0], pos[1] + 0.48, pos[2]],
        yaw: 1.15,
        pitch: 0.095,
        distance: 1.72,
    };
    screenshot(&mut driver, &output.join("animal-fur-detail.png"))?;
    let travel: f32 = positions
        .windows(2)
        .map(|pair| Vec3::from_array(pair[0]).distance(Vec3::from_array(pair[1])))
        .sum();
    if !moving || travel < 0.6 {
        return Err(format!("Зверь не прошёл маршрут. Пройдено {travel:.2} м."));
    }
    let data = json!({"exported_exe":true,"gpu":name,"size":SIZE,"clear":clear,"rain":rain,"snow":snow,"tuple_fields":["seconds","rain","snow","wind_m_s"],"animal_travel_m":travel,"player_m":driver.app.game_position().to_array(),"paw_contacts":paws});
    fs::write(
        output.join("game.json"),
        serde_json::to_vec_pretty(&data).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)?;
    video.finish()
}

fn sample(driver: &Driver<'_, '_>) -> Result<(f64, f32, f32, f32), String> {
    let frame = driver
        .app
        .environment_frame(driver.app.game_position())
        .ok_or("В EXE нет кадра погоды.")?;
    Ok((
        frame.elapsed,
        frame.atmosphere.rain,
        frame.atmosphere.snow,
        frame.wind.length(),
    ))
}

fn until(driver: &mut Driver<'_, '_>, seconds: f64) -> Result<(), String> {
    for _ in 0..840 {
        if sample(driver)?.0 >= seconds {
            return Ok(());
        }
        driver.settle(1)?;
    }
    Err("Время погоды в игре не продвинулось.".into())
}

fn check(app: &Studio, dog_id: u64) -> Result<(), String> {
    let robot = app.scene.avatar.as_ref().ok_or("Не загружен робот.")?;
    let dog = app
        .scene
        .npcs
        .iter()
        .find(|npc| npc.id == dog_id)
        .ok_or("Не загружен зверь.")?;
    if robot
        .generated
        .is_none_or(|recipe| recipe.kind != Kind::Robot)
        || dog
            .avatar
            .generated
            .is_none_or(|recipe| recipe.kind != Kind::Quadruped)
        || robot.walk.is_none()
        || dog.avatar.walk.is_none()
        || app
            .scene
            .environment
            .as_ref()
            .is_none_or(|env| env.weather.cues.len() != 2)
        || !app
            .scene
            .world
            .models
            .iter()
            .any(|model| model.material.wind_response > 0.0)
        || !app
            .scene
            .world
            .models
            .iter()
            .any(|model| model.surface.is_some() && app.assets.get(model.render_source()).is_some())
    {
        return Err("Не все рецепты, анимации, материалы и настройки среды загрузились.".into());
    }
    Ok(())
}

fn screenshot(driver: &mut Driver<'_, '_>, path: &Path) -> Result<(), String> {
    driver.settle(2)?;
    image::save_buffer(path, &driver.last, SIZE[0], SIZE[1], image::ColorType::Rgb8)
        .map_err(|error| error.to_string())
}
