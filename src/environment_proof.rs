use std::{fs, path::Path};

use eframe::egui::Key;

use super::{
    Driver, FPS, SIZE, Sink, io_error, mill_walk_proof,
    staged_proof::{move_and_play, number, panel_click, reload},
};
use crate::{
    Studio, configure_context, mill, offscreen::Offscreen, proof_video::Video, viewport3d,
};

pub(super) fn record(output: &Path, releases: &Path, previous: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let staging = output.join("export-stage");
    fs::create_dir_all(&staging).map_err(io_error)?;
    let mill_scene = output.join("river-mill.robot.json");
    let map_scene = output.join("river-map.robot.json");
    let staged_mill = staging.join("RiverMill.exe");
    let staged_map = staging.join("RiverMap.exe");
    let mill_game = releases.join("RiverMill.exe");
    let map_game = releases.join("RiverMap.exe");
    let video_file = output.join("stage-018.mp4");
    for path in [
        &mill_scene,
        &map_scene,
        &staged_mill,
        &staged_map,
        &mill_game,
        &map_game,
        &video_file,
    ] {
        if path.exists() {
            return Err(format!("Путь занят. {}", path.display()));
        }
    }

    let mut video = Video::start(&video_file, SIZE[0], SIZE[1], FPS)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::default();
    app.desk.recovery_root = Some(output.join("recovery-test"));
    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(app, &mut display, &mut sink, Some((previous, &staged_mill)));
        d.settle(6)?;
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene.linked_scenes.entries.len() != 1 {
            return Err("Не открылась связанная сцена 0.17.".into());
        }
        d.click("Новый…")?;
        d.click(mill::NAME)?;
        d.app.scene.validate()?;
        d.click("Окружение")?;
        d.click("Физика")?;
        d.hold(22)?;
        d.click("Среда")?;
        d.hold(22)?;
        d.click("Правила")?;
        d.click("Поиск…")?;
        d.edit("Имя объекта, правила или ресурса", 0, mill::PLATE_RULE)?;
        d.click_n(mill::PLATE_RULE, usize::MAX)?;
        if !d.display.last_output().is_some_and(|output| {
            crate::proof_input::find_label_containing(output, "Уровень, см", 0).is_some()
        }) {
            super::top_proof::scroll_panel(&mut d, -315.0)?;
        }
        number(&mut d, "Уровень, см", "-20")?;
        preview_wrong_level(&mut d)?;
        d.click("Отменить")?;
        d.app.proof.as_mut().unwrap().scene = Some(mill_scene.clone());
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "▶ Играть")?;
        mill_walk_proof::play(&mut d, true)?;
        let final_level = level(&d)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Сохранить")?;
        reload(&mut d)?;
        panel_click(&mut d, "Продолжить")?;
        if (level(&d)? - final_level).abs() > 0.001 || d.app.rule_session.quest_stage(1) != Some(3)
        {
            return Err("Копия сцены потеряла уровень воды или завершённую цель.".into());
        }
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        app = d.app;
    }
    move_and_play(
        &mut video,
        &mut display,
        &staged_mill,
        &mill_game,
        "mill",
        &output.join("timing-mill.json"),
    )?;
    eprintln!(
        "0.18: физическая цепочка мельницы, ошибка уровня, восстановление и перенесённый EXE проверены."
    );

    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((&map_scene, &staged_map)),
        );
        d.settle(6)?;
        d.click("Новый…")?;
        d.click(mill::MAP_NAME)?;
        d.app.scene.validate()?;
        d.click("Среда")?;
        d.hold(28)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "▶ Играть")?;
        play_river(&mut d)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
    }
    move_and_play(
        &mut video,
        &mut display,
        &staged_map,
        &map_game,
        "river",
        &output.join("timing-river.json"),
    )?;
    video.finish()?;
    eprintln!(
        "0.18: тот же объём воды и событие уровня проверены на самостоятельной карте 2D и в её EXE."
    );
    Ok(())
}

fn preview_wrong_level(d: &mut Driver<'_, '_>) -> Result<(), String> {
    panel_click(d, "▶ Играть")?;
    mill_walk_proof::push_until_pressed(d)?;
    d.settle(100)?;
    if d.app.rule_session.quest_stage(1) == Some(3) || (level(d)? + 0.2).abs() > 0.001 {
        return Err("Ошибочная настройка уровня неожиданно завершила механическую цепочку.".into());
    }
    d.hold(25)?;
    panel_click(d, "■ Стоп")
}

pub(super) fn play_river(d: &mut Driver<'_, '_>) -> Result<(), String> {
    if (level(d)? + 0.2).abs() > 0.001 || d.app.rule_session.quest_stage(1).is_some() {
        return Err("Канал не стартовал с исходного уровня.".into());
    }
    d.hold(24)?;
    d.key(Key::F)?;
    d.settle(90)?;
    if (level(d)? - 0.42).abs() > 0.002 || d.app.rule_session.quest_stage(1) != Some(2) {
        return Err("Игровое действие в 2D не подняло воду до 42 см.".into());
    }
    d.expect_hud("Выполнено · Река сверху")?;
    d.hold(52)
}

fn level(d: &Driver<'_, '_>) -> Result<f32, String> {
    let config = d
        .app
        .scene
        .environment
        .as_ref()
        .ok_or("Среда отсутствует.")?;
    let zone = config.waters.first().ok_or("Водный объём отсутствует.")?;
    d.app
        .environment_session
        .as_ref()
        .and_then(|runtime| runtime.level(config, zone.id))
        .ok_or_else(|| "Уровень воды недоступен.".into())
}
