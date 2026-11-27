use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use serde_json::json;

use super::{
    Driver, FPS, SIZE, Sink, io_error, quarter_proof,
    staged_proof::{panel_click, reload},
};
use crate::{
    Studio, configure_context,
    environment::WeatherPreset,
    game_export::GamePackage,
    offscreen::Offscreen,
    play_settings::CameraMode,
    proof_video::Video,
    studio_welcome::Page,
    viewport3d::{self, Camera},
};

const SCENE_CAMERA: Camera = Camera {
    target: [-0.1, 0.8, -7.8],
    yaw: 0.42,
    pitch: 0.30,
    distance: 11.5,
};

pub(super) fn author(output: &Path, exe: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let project = output.join("yuna-glade.robot.json");
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(&output.join("from-empty-editor.mp4"), SIZE[0], SIZE[1], FPS)?;
    let mut sink = Sink::Video(&mut video);
    let mut app = Studio {
        welcome: Some(Page::Home),
        ..Studio::default()
    };
    app.desk.recovery_root = Some(output.join("recovery"));
    let mut d = Driver::new(app, &mut display, &mut sink, Some((&project, exe)));
    d.settle(12)?;
    for text in [
        "Создать проект",
        "3D-сцена",
        "Окружение",
        "Сказочная мастерская",
        "Разложить сказочную поляну",
    ] {
        d.click(text)?;
    }
    if d.app.scene.world.models.len() < 10 {
        return Err(d.app.status.clone());
    }
    picture(&mut d, output, "editor-glade.png")?;
    d.hold(44)?;
    for text in [
        "Персонаж",
        "Собрать чиби…",
        "Юна",
        "Собрать персонажа",
        "Лицо крупно",
    ] {
        d.click(text)?;
    }
    picture(&mut d, output, "editor-yuna.png")?;
    d.hold(48)?;
    if d.point("Мимика Юны", 0).is_ok() && d.point("Улыбка", 0).is_err() {
        d.click("Мимика Юны")?;
    }
    for (expression, file) in [
        ("Улыбка", "face-smile.png"),
        ("Удивление", "face-surprised.png"),
        ("Артикуляция", "face-speech.png"),
    ] {
        d.click(expression)?;
        d.settle(10)?;
        picture(&mut d, output, file)?;
        d.hold(20)?;
    }
    d.click("Спокойно")?;
    d.click("В полный рост")?;
    d.click("Закрыть мастерскую")?;

    for text in [
        "NPC",
        "+ Создать NPC",
        "Собрать робота или зверя…",
        "Четвероногое существо",
        "Ходьба",
        "Тестовый склон",
    ] {
        d.click(text)?;
    }
    d.settle(38)?;
    picture(&mut d, output, "editor-paw-preview.png")?;
    d.hold(34)?;
    d.click("Собрать персонажа")?;
    d.app.camera = Camera {
        target: [0.75, 0.35, -4.8],
        yaw: 0.28,
        pitch: 0.61,
        distance: 12.0,
    };
    d.settle(5)?;
    d.click("Добавлять точки кликом")?;
    d.at_world([1.32, 0.0, -4.4])?;
    d.settle(4)?;
    d.at_world([0.25, 0.0, -6.9])?;
    d.settle(4)?;
    if d.app
        .scene
        .npcs
        .last()
        .is_none_or(|npc| npc.route.len() < 4)
    {
        return Err("Добавление пути через 3D не сохранилось.".into());
    }
    picture(&mut d, output, "editor-npc-route.png")?;

    if d.app.workspace == crate::editor_workspace::Workspace::Create {
        d.click("Ещё инструменты")?;
    }
    for text in [
        "Среда",
        "Погода",
        "+ Переход погоды",
        "+ Переход погоды",
        "Туман",
        "Снег",
    ] {
        d.click(text)?;
    }
    picture(&mut d, output, "editor-weather.png")?;
    d.hold(34)?;
    panel_click(&mut d, "Сохранить как…")?;
    reload(&mut d)?;
    check(&d.app)?;
    panel_click(&mut d, "Собрать и проверить")?;
    let check_name = format!(
        "{}.проверка",
        exe.file_stem().unwrap_or_default().to_string_lossy()
    );
    let report = exe.with_file_name(check_name).join("проверка.json");
    let until = Instant::now() + Duration::from_secs(65);
    while d.app.build_check.is_some() && Instant::now() < until {
        d.settle(1)?;
        thread::sleep(Duration::from_millis(24));
    }
    if !report.is_file() {
        return Err(d.app.status.clone());
    }
    d.hold(40)?;
    fs::write(output.join("editor.json"), json!({"editor_actions": d.input_actions, "route_and_weather_via_ui":true,"verified_export":true,"reopened":true}).to_string()).map_err(io_error)?;
    video.finish()
}

pub(super) fn player(package: GamePackage, output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::from_package(package, display.context());
    check(&app)?;
    if !app.game_running {
        return Err(app.status);
    }
    let mut video = Video::start(&output.join("yuna-from-exe.mp4"), SIZE[0], SIZE[1], FPS)?;
    let mut sink = Sink::Video(&mut video);
    app.scene.play_settings.camera.mode = CameraMode::Fixed;
    app.camera = SCENE_CAMERA;
    let mut d = Driver::new(app, &mut display, &mut sink, None);
    let npc_id = d.app.scene.npcs.last().unwrap().id;
    d.settle(20)?;
    quarter_proof::walk(&mut d, [0.57, -7.4])?;
    picture(&mut d, output, "glade-exe.png")?;
    quarter_proof::walk(&mut d, [0.57, -6.45])?;
    let foot = d.app.game_position();
    let heading = d.app.avatar_heading.to_radians();
    d.app.camera = Camera {
        target: [foot.x, foot.y + 0.86, foot.z],
        yaw: heading + 0.30,
        pitch: 0.085,
        distance: 3.24,
    };
    d.settle(18)?;
    picture(&mut d, output, "yuna-exe.png")?;
    d.app.camera = Camera {
        target: [foot.x, foot.y + 1.19, foot.z],
        yaw: heading + 0.23,
        pitch: 0.047,
        distance: 1.69,
    };
    d.settle(48)?;
    picture(&mut d, output, "yuna-face-exe.png")?;
    d.app.camera = SCENE_CAMERA;
    until(&mut d, 7.4)?;
    let rain = d
        .app
        .environment_frame(d.app.game_position())
        .ok_or("Нет среды в EXE.")?;
    picture(&mut d, output, "rain-exe.png")?;
    until(&mut d, 10.85)?;
    let snow = d
        .app
        .environment_frame(d.app.game_position())
        .ok_or("Нет среды в EXE.")?;
    quarter_proof::walk(&mut d, [0.69, -8.04])?;
    quarter_proof::walk(&mut d, [0.18, -6.4])?;
    let snow_time = d
        .app
        .environment_frame(d.app.game_position())
        .map_or(0.0, |frame| frame.elapsed);
    let track_count = d
        .app
        .snow_tracks
        .near(glam::Vec3::from_array(SCENE_CAMERA.target), snow_time)
        .len();
    picture(&mut d, output, "snow-exe.png")?;
    let npc = d
        .app
        .npc_simulation
        .state(npc_id)
        .ok_or("NPC не симулировался.")?
        .position();
    if rain.atmosphere.rain <= 0.3 || snow.atmosphere.snow <= 0.5 {
        return Err(format!(
            "Не сработали переходы погоды (дождь {:.2} в {:.3} с, снег {:.2} в {:.3} с).",
            rain.atmosphere.rain, rain.elapsed, snow.atmosphere.snow, snow.elapsed
        ));
    }
    fs::write(output.join("game.json"), json!({"result":"passed", "position":foot.to_array(),"npc_at":npc,"rain":rain.atmosphere.rain,"snow":snow.atmosphere.snow,"residual_surface_wetness":snow.atmosphere.surface_wet,"snow_tracks_near_camera":track_count}).to_string()).map_err(io_error)?;
    video.finish()
}

fn check(app: &Studio) -> Result<(), String> {
    let yuna = app
        .avatar_asset()
        .is_some_and(|(model, _)| model.has_named_material("Yuna | Hood"));
    if !yuna
        || app.scene.npcs.last().is_none_or(|npc| npc.route.len() < 4)
        || app.scene.world.models.len() < 10
        || app.scene.environment.as_ref().is_none_or(|env| {
            !env.weather
                .cues
                .iter()
                .any(|cue| cue.preset == WeatherPreset::Snow)
        })
    {
        return Err(format!(
            "Сцена не сохранила Юну, маршрут, объекты или снег. {}",
            app.status
        ));
    }
    Ok(())
}

fn picture(d: &mut Driver<'_, '_>, output: &Path, name: &str) -> Result<(), String> {
    d.settle(2)?;
    image::save_buffer(
        output.join(name),
        &d.last,
        SIZE[0],
        SIZE[1],
        image::ColorType::Rgb8,
    )
    .map_err(|error| error.to_string())
}

fn until(d: &mut Driver<'_, '_>, seconds: f64) -> Result<(), String> {
    for _ in 0..840 {
        if d.app
            .environment_frame(d.app.game_position())
            .is_some_and(|frame| frame.elapsed >= seconds)
        {
            return Ok(());
        }
        d.settle(1)?;
    }
    Err("Время в EXE не продвинулось.".into())
}
