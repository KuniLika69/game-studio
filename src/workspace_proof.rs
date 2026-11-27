use std::{fs, path::Path};

use eframe::egui::{Event, Key};
use serde_json::json;

use super::{
    Driver, SIZE, Sink, io_error, proof_input,
    staged_proof::{panel_click, reload},
};
use crate::{
    EditorTab, Studio, configure_context, editor_workspace::Workspace, flagship,
    offscreen::Offscreen, studio_welcome::Page, viewport3d,
};

pub(super) fn record(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio {
        workspace: Workspace::Create,
        welcome: Some(Page::Home),
        ..Studio::default()
    };
    app.desk.recovery_root = Some(output.join("recovery"));
    let scene_path = output.join("last-light.robot.json");
    let game_path = output.join("LastLight.exe");
    let mut sink = Sink::None;
    let mut driver = Driver::new(
        app,
        &mut display,
        &mut sink,
        Some((&scene_path, &game_path)),
    );
    driver.input = proof_input::Driver::with_scale(SIZE, 1.25);
    driver.settle(8)?;
    if (driver.display.context().pixels_per_point() - 1.25).abs() > 0.005 {
        return Err("Редактор не получил масштаб 125%.".into());
    }
    for label in ["Создать проект", "Открыть проект", "Попробовать пример"]
    {
        driver.point(label, 0)?;
    }
    screenshot(&mut driver, output, "welcome-125.png")?;

    driver.click("Попробовать пример")?;
    driver.click(flagship::NAME)?;
    if driver.app.welcome.is_some()
        || driver.app.scene.avatar.is_none()
        || driver.app.workspace != Workspace::Create
    {
        return Err("Пример не открылся в пространстве создания.".into());
    }
    for label in ["Сохранить", "▶ Играть", "Собрать игру для Windows"]
    {
        driver.point(label, 0)?;
    }
    driver.point("Клипы, дыхание и волосы", 0)?;
    if driver.point("Правки ИИ…", 0).is_ok() {
        return Err("Команда разработчика видна в основных инструментах создания.".into());
    }
    let scene = driver.app.scene.clone();
    let was_dirty = driver.app.desk.dirty();
    screenshot(&mut driver, output, "creation-125.png")?;
    driver.click("Первые шаги…")?;
    driver.point("Первые шаги", 0)?;
    screenshot(&mut driver, output, "first-steps-125.png")?;
    driver.key(Key::F1)?;
    if driver.app.workspace_help_open {
        return Err("F1 не закрыл справку.".into());
    }
    driver.key(Key::F1)?;
    if !driver.app.workspace_help_open {
        return Err("F1 не открыл справку.".into());
    }

    driver.click("Разработка")?;
    if driver.app.workspace != Workspace::Develop {
        return Err("Переключатель не открыл пространство разработки.".into());
    }
    for label in ["Проверки…", "Правки ИИ…", "Справка разработчика…"]
    {
        driver.point(label, 0)?;
    }
    driver.click("Справка разработчика…")?;
    driver.point("Справка разработчика", 0)?;
    screenshot(&mut driver, output, "developer-125.png")?;
    driver.click("Открыть производительность")?;
    if !driver.app.quality.open {
        return Err("Ссылка из справки не открыла замеры производительности.".into());
    }
    driver.app.quality.open = false;
    driver.settle(4)?;
    driver.click("Открыть правила")?;
    if driver.app.editor_tab != EditorTab::Rules {
        return Err("Ссылка из справки не переключила редактор на правила.".into());
    }
    driver.click("Открыть проверки")?;
    driver.point("Проверка прохождения", 0)?;

    // Reset the auxiliary verification window to test the remaining help link.
    // Its position may overlap the guide at 125%.
    driver.app.playback = Default::default();
    driver.settle(4)?;
    driver.click("Открыть правки ИИ")?;
    driver.point("Предложения ИИ для сцены", 0)?;
    // The review window can overlap the project toolbar at this scale.
    driver.app.ai_review = Default::default();
    driver.settle(4)?;
    driver.click("Создание")?;
    if driver.app.workspace != Workspace::Create
        || driver.app.scene != scene
        || driver.app.desk.dirty() != was_dirty
    {
        return Err(
            "Переход между пространствами изменил проект или не вернул режим создания.".into(),
        );
    }
    panel_click(&mut driver, "▶ Играть")?;
    if !driver.app.game_running {
        return Err("Игровая проверка не запустилась из пространства создания.".into());
    }
    panel_click(&mut driver, "■ Стоп")?;
    if driver.app.game_running {
        return Err("Игровая проверка не остановилась.".into());
    }
    panel_click(&mut driver, "Сохранить как…")?;
    if !scene_path.is_file() {
        return Err(format!("Проект не сохранился. {}", driver.app.status));
    }
    reload(&mut driver)?;
    panel_click(&mut driver, "Собрать игру для Windows")?;
    if !game_path.is_file() {
        return Err(format!("Игра не экспортировалась. {}", driver.app.status));
    }
    let report = serde_json::to_vec_pretty(&json!({
        "ui_scale_percent": 125,
        "capture_pixels": SIZE,
        "input_actions": driver.input_actions,
        "welcome_actions_visible": true,
        "create_to_develop_and_back": true,
        "f1_toggles_help": true,
        "developer_links_open_matching_tools": true,
        "project_unchanged_between_workspaces": true,
        "creation_play_starts_and_stops": true,
        "save_reopen_and_export_in_creation": true,
        "human_tester": false
    }))
    .map_err(|error| error.to_string())?;
    fs::write(output.join("workspaces.json"), report).map_err(io_error)
}

fn screenshot(driver: &mut Driver<'_, '_>, output: &Path, file: &str) -> Result<(), String> {
    driver.position = None;
    driver.frame(vec![Event::PointerGone])?;
    driver.settle(3)?;
    image::save_buffer(
        output.join(file),
        &driver.last,
        SIZE[0],
        SIZE[1],
        image::ColorType::Rgb8,
    )
    .map_err(|error| error.to_string())
}
