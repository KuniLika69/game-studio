//! Exercises the environment palette in the editor and follows its fox in the exported game.
use std::{fs, path::Path};

use eframe::egui::{Event, Modifiers, MouseWheelUnit, vec2};
use glam::Vec3;
use serde_json::json;

use super::{
    Driver, FPS, SIZE, Sink, io_error,
    staged_proof::{number, panel_click, reload},
};
use crate::{
    EditorTab, Studio, View,
    avatar::Motion,
    configure_context,
    editor_workspace::Workspace,
    game_export::GamePackage,
    offscreen::Offscreen,
    proof_video::Video,
    studio_welcome::Page,
    viewport3d::{self, Camera},
};

const MEADOW: &str = "Луг · травы, цветы и деревья";
const HOUSE: &str = "Кирпичный дом · внешний вид";
const ROAD: &str = "Брусчатая дорога";
const SKY: &str = "Небо и время суток";
const FOX: &str = "Лисёнок с маршрутом";
const COMPOSE: &str = "Создание окружения";
const ADD: &str = "Добавить в сцену";

pub(super) fn author(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio {
        workspace: Workspace::Create,
        welcome: Some(Page::Home),
        ..Studio::default()
    };
    app.desk.recovery_root = Some(output.join("recovery"));
    let scene = output.join("living-world.robot.json");
    let executable = output.join("LivingWorld.exe");
    let mut sink = Sink::None;
    let mut driver = Driver::new(app, &mut display, &mut sink, Some((&scene, &executable)));

    driver.settle(8)?;
    driver.click("Создать проект")?;
    driver.click("3D-сцена")?;
    driver.click("Окружение")?;
    driver.click(COMPOSE)?;
    driver.click(ADD)?;
    if driver.app.scene.world.models.len() != 8 {
        return Err(format!(
            "Луг не добавлен в пустую сцену. {}",
            driver.app.status
        ));
    }

    select(&mut driver, MEADOW, HOUSE)?;
    center(&mut driver, "-4.85", "-9.7")?;
    driver.click(ADD)?;
    select(&mut driver, HOUSE, ROAD)?;
    center(&mut driver, "3.5", "-3.4")?;
    driver.click(ADD)?;
    for name in [HOUSE, ROAD] {
        if !driver
            .app
            .scene
            .world
            .models
            .iter()
            .any(|model| model.label == name)
        {
            return Err(format!(
                "Объект «{name}» не добавлен. {}",
                driver.app.status
            ));
        }
    }

    select(&mut driver, ROAD, SKY)?;
    driver.click(ADD)?;
    if driver.app.scene.environment.is_none() {
        return Err(format!("Небо не добавлено. {}", driver.app.status));
    }
    driver.app.camera = Camera {
        target: [-0.45, 1.0, -6.0],
        yaw: 0.24,
        pitch: 0.37,
        distance: 13.0,
    };
    screenshot(&mut driver, &output.join("palette-editor.png"))?;

    select(&mut driver, SKY, FOX)?;
    center(&mut driver, "0.0", "-4.75")?;
    driver.click(ADD)?;
    if driver.app.editor_tab != EditorTab::Npcs {
        return Err(format!("Редактор не открыл маршрут. {}", driver.app.status));
    }
    let id = fox_id(&driver.app)?;
    let fox = driver
        .app
        .scene
        .npcs
        .iter()
        .find(|npc| npc.id == id)
        .unwrap();
    let route = fox
        .route
        .iter()
        .map(|waypoint| waypoint.position)
        .collect::<Vec<_>>();
    driver.app.camera = Camera {
        target: [-0.05, 0.35, -4.82],
        yaw: 1.53,
        pitch: 0.29,
        distance: 3.65,
    };
    screenshot(&mut driver, &output.join("fox-editor.png"))?;

    driver.click("Персонаж")?;
    driver.click("Собрать персонажа…")?;
    driver.click("Создать из деталей")?;
    if !driver.app.can_play() {
        return Err(format!("Нет управляемого персонажа. {}", driver.app.status));
    }

    panel_click(&mut driver, "Сохранить как…")?;
    if !scene.is_file() {
        return Err(format!("Сцена не сохранена. {}", driver.app.status));
    }
    reload(&mut driver)?;
    fox_id(&driver.app)?;
    panel_click(&mut driver, "Собрать игру для Windows")?;
    if !executable.is_file() {
        return Err(format!("Игра не экспортировалась. {}", driver.app.status));
    }
    write_report(
        output,
        "editor.json",
        json!({
            "input_actions": driver.input_actions,
            "placements": driver.app.scene.world.models.len(),
            "route": route,
            "sky": driver.app.scene.environment.is_some(),
            "walk_and_run_mapped": true,
            "reopened_project": true,
            "windows_export": executable.file_name().unwrap_or_default().to_string_lossy(),
        }),
    )
}

pub(super) fn player(package: GamePackage, output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::from_package(package, display.context());
    let id = fox_id(&app)?;
    if !app.game_running {
        return Err("Игра не запустила сцену из EXE.".into());
    }
    app.view = View::Three;
    app.scene.play_settings.camera.mode = crate::play_settings::CameraMode::Fixed;
    app.camera = Camera {
        target: [0.4, 0.36, -5.67],
        yaw: 1.48,
        pitch: 0.24,
        distance: 5.2,
    };
    let mut video = Video::start(&output.join("fox-route.mp4"), SIZE[0], SIZE[1], FPS)?;
    let mut sink = Sink::Video(&mut video);
    let mut driver = Driver::new(app, &mut display, &mut sink, None);
    driver.settle(9)?;
    let start = driver
        .app
        .npc_position(id)
        .ok_or("Нет начальной позиции лисёнка.")?;
    let mut max_displacement = 0.0_f32;
    let mut walked = false;
    let mut screenshot_taken = false;
    for _ in 0..245 {
        driver.frame(Vec::new())?;
        let state = driver
            .app
            .npc_simulation
            .state(id)
            .ok_or("Маршрут лисёнка не запущен.")?;
        let distance = Vec3::from_array(state.position()).distance(start);
        max_displacement = max_displacement.max(distance);
        walked |= matches!(state.motion, Motion::Walk | Motion::Run);
        if !screenshot_taken && walked && distance > 0.85 {
            screenshot(&mut driver, &output.join("fox-in-game.png"))?;
            screenshot_taken = true;
        }
    }
    if !walked || max_displacement < 1.35 || !screenshot_taken {
        return Err(format!(
            "Лисёнок не прошёл маршрут. Дистанция {max_displacement:.2} м."
        ));
    }
    video.finish()?;
    write_report(
        output,
        "game.json",
        json!({
            "launched_exported_exe": true,
            "pid": std::process::id(),
            "max_displacement_m": max_displacement,
            "walk_or_run_sampled": walked,
            "capture_pixels": SIZE,
        }),
    )
}

fn fox_id(app: &Studio) -> Result<u64, String> {
    let fox = app
        .scene
        .npcs
        .iter()
        .find(|npc| npc.avatar.name == "Лисёнок")
        .ok_or("В сцене нет лисёнка.")?;
    if fox.route.len() != 3
        || fox.avatar.walk.is_none()
        || fox.avatar.run.is_none()
        || app.assets.get(&fox.avatar.source).is_none()
    {
        return Err("Лисёнок загружен без маршрута или анимации.".into());
    }
    Ok(fox.id)
}

fn select(driver: &mut Driver<'_, '_>, current: &str, next: &str) -> Result<(), String> {
    driver.click(current)?;
    if driver.point(next, usize::MAX).is_err() {
        let menu_row = driver.point(ROAD, usize::MAX)?;
        driver.frame(vec![
            Event::PointerMoved(menu_row),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, -220.0),
                modifiers: Modifiers::NONE,
            },
        ])?;
        driver.settle(6)?;
    }
    driver.click_n(next, usize::MAX)
}

fn center(driver: &mut Driver<'_, '_>, x: &str, z: &str) -> Result<(), String> {
    number(driver, "X ", x)?;
    number(driver, "Z ", z)
}

fn screenshot(driver: &mut Driver<'_, '_>, output: &Path) -> Result<(), String> {
    driver.position = None;
    driver.frame(vec![Event::PointerGone])?;
    driver.settle(2)?;
    image::save_buffer(
        output,
        &driver.last,
        SIZE[0],
        SIZE[1],
        image::ColorType::Rgb8,
    )
    .map_err(|error| error.to_string())
}

fn write_report(output: &Path, file: &str, report: serde_json::Value) -> Result<(), String> {
    let report = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    fs::write(output.join(file), report).map_err(io_error)
}
