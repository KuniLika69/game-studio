use std::{fs, path::Path};

use super::{
    Driver, FPS, SIZE, Sink, io_error, quarter_proof, side_proof,
    staged_proof::{move_and_play, panel_click, play_linden, reload},
    top_proof, visual_proof,
};
use crate::{
    Studio, View, configure_context, offscreen::Offscreen, proof_input, proof_video::Video,
    scene::Scene, viewport3d,
};

pub(super) fn record(
    output: &Path,
    releases: &Path,
    previous: &Path,
    atlas: &Path,
    chair: &Path,
    compatible: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let stage = output.join("export-stage");
    fs::create_dir_all(&stage).map_err(io_error)?;
    let character = output.join("vera.character.json");
    let verification = previous
        .parent()
        .ok_or("Нужна папка с проектами проверки.")?;
    let legacy_12 = verification.join("v0.12/room-key.robot.json");
    let legacy_13 = verification.join("v0.13/linden-lane.robot.json");
    let source = previous.join("linden-marks.robot.json");
    let library = previous.join("two-marks.rules.json");
    let project_3d = output.join("linden-marks.robot.json");
    let project_top = output.join("terrace.robot.json");
    let project_side = output.join("bell-tower.robot.json");
    let stage_3d = stage.join("LindenMarks.exe");
    let stage_top = stage.join("Terrace.exe");
    let stage_side = stage.join("BellTower.exe");
    let game_3d = releases.join("LindenMarks.exe");
    let game_top = releases.join("Terrace.exe");
    let game_side = releases.join("BellTower.exe");
    let timings_3d = output.join("timings-3d.json");
    let timings_top = output.join("timings-top.json");
    let timings_side = output.join("timings-side.json");
    let movie = output.join("three-games.mp4");
    for path in [
        &character,
        &project_3d,
        &project_top,
        &project_side,
        &stage_3d,
        &stage_top,
        &stage_side,
        &game_3d,
        &game_top,
        &game_side,
        &timings_3d,
        &timings_top,
        &timings_side,
        &movie,
    ] {
        if path.exists() {
            return Err(format!("Путь занят. {}", path.display()));
        }
    }
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut movie = Video::start(&movie, SIZE[0], SIZE[1], FPS)?;
    let mut app = Studio::default();
    {
        let mut sink = Sink::Video(&mut movie);
        let mut d = Driver::new(app, &mut display, &mut sink, Some((&legacy_12, &stage_3d)));
        d.settle(6)?;
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene.world.keys.len() != 1
            || d.app.scene.checkpoint.is_none()
            || !d.app.can_play()
        {
            return Err("Проект 0.12 не открылся в 0.15.".into());
        }
        d.app.proof.as_mut().unwrap().scene = Some(legacy_13);
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene.rules.rules.len() != 5
            || d.app.scene.world.buildings.len() != 12
            || !d.app.can_play()
        {
            return Err("Квартал 0.13 не открылся в 0.15.".into());
        }
        d.app.proof.as_mut().unwrap().scene = Some(source);
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene.rules.counters.is_empty() {
            return Err("Квартал 0.14 не загрузился.".into());
        }
        d.app.proof.as_mut().unwrap().scene = Some(project_3d.clone());
        visual_proof::author(&mut d, chair)?;
        save_hero(&mut d, chair, compatible, &character)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        if !d
            .app
            .scene
            .world
            .models
            .iter()
            .any(|item| item.seat.is_some() && item.collider.is_some())
        {
            return Err("После открытия потерялись настройки стула.".into());
        }
        quarter_proof::fresh(&mut d, View::Three)?;
        visual_proof::sit(&mut d)?;
        play_linden(&mut d, true)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        app = d.app;
    }
    move_and_play(
        &mut movie,
        &mut display,
        &stage_3d,
        &game_3d,
        "linden",
        &timings_3d,
    )?;
    eprintln!("3D и посадка проверены в редакторе и перенесённой сборке 0.15.");

    {
        let mut sink = Sink::Video(&mut movie);
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((&project_top, &stage_top)),
        );
        d.settle(4)?;
        top_proof::author(&mut d, atlas, &library)?;
        place_hero(&mut d, &character, true)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "▶ Играть")?;
        top_proof::play(&mut d, true)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        app = d.app;
    }
    move_and_play(
        &mut movie,
        &mut display,
        &stage_top,
        &game_top,
        "top",
        &timings_top,
    )?;
    eprintln!("Самостоятельное 2D сверху прошло с перенесённым персонажем и правилами.");

    {
        let mut sink = Sink::Video(&mut movie);
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((&project_side, &stage_side)),
        );
        d.settle(4)?;
        side_proof::author(&mut d, atlas)?;
        place_hero(&mut d, &character, false)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "▶ Играть")?;
        side_proof::play(&mut d, true)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
    }
    move_and_play(
        &mut movie,
        &mut display,
        &stage_side,
        &game_side,
        "side",
        &timings_side,
    )?;
    movie.finish()?;
    eprintln!("Платформер прошёл в отдельном EXE. Итоговая запись готова.");
    Ok(())
}

fn save_hero(
    d: &mut Driver<'_, '_>,
    incompatible: &Path,
    compatible: &Path,
    file: &Path,
) -> Result<(), String> {
    d.click("Персонаж")?;
    d.click("Сменные детали GLB")?;
    d.app.proof.as_mut().unwrap().import_glb = Some(incompatible.to_owned());
    d.click("Импорт совместимой детали…")?;
    if !d.app.status.contains("Скелет детали несовместим") {
        return Err(format!(
            "Несовместимая деталь не отклонена. {}",
            d.app.status
        ));
    }
    d.app.proof.as_mut().unwrap().import_glb = Some(compatible.to_owned());
    d.click("Импорт совместимой детали…")?;
    if d.app
        .scene
        .avatar
        .as_ref()
        .is_none_or(|actor| actor.modular_parts.len() != 1)
    {
        return Err(format!("Совместимая деталь не добавлена. {}", d.app.status));
    }
    d.click("Библиотека персонажей")?;
    d.app.proof.as_mut().unwrap().character_file = Some(file.to_owned());
    d.click("Сохранить персонажа в библиотеку…")?;
    let saved = Scene::load(file)?;
    if saved.avatar != d.app.scene.avatar || !saved.npcs.is_empty() {
        return Err("Библиотека персонажа сохранила роль или потеряла внешность.".into());
    }
    Ok(())
}

fn place_hero(d: &mut Driver<'_, '_>, file: &Path, paint: bool) -> Result<(), String> {
    d.click("Персонаж")?;
    if d.point("Разместить из библиотеки…", 0).is_err() {
        d.click("Библиотека персонажей")?;
    }
    d.app.proof.as_mut().unwrap().character_file = Some(file.to_owned());
    d.click("Разместить из библиотеки…")?;
    if d.app.scene.avatar != Scene::load(file)?.avatar {
        return Err(format!(
            "Персонаж из библиотеки не размещён. {}",
            d.app.status
        ));
    }
    d.click("Рисовать спрайт")?;
    d.click("Клип в 8 кадров")?;
    if paint {
        let rect = d
            .app
            .proof
            .as_ref()
            .and_then(|p| p.sprite_canvas)
            .ok_or("Не найден холст спрайта.")?;
        let point = rect.left_top() + eframe::egui::vec2(rect.width() * 0.48, rect.height() * 0.35);
        d.frame(proof_input::click(point))?;
        d.settle(5)?;
        let manual = d
            .app
            .scene
            .sprite_pixels
            .iter()
            .find(|pixel| !pixel.baked)
            .cloned()
            .ok_or("Ручной пиксель не добавлен.")?;
        d.click("Клип в 8 кадров")?;
        if !d.app.scene.sprite_pixels.contains(&manual) {
            return Err("После переноса кадров пропал пиксель ручной дорисовки.".into());
        }
    }
    d.click("Вернуться к карте")
}
