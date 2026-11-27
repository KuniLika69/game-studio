use std::{fs, path::Path};

use eframe::egui::{Key, Modifiers};
use serde_json::json;

use super::{
    Driver, FPS, SIZE, Sink, io_error, quarter_proof,
    staged_proof::{move_and_play, panel_click, reload},
    top_proof,
};
use crate::{
    Studio, configure_context, map2d::Mode, offscreen::Offscreen, proof_input, proof_video::Video,
    viewport3d,
};

#[path = "games23_projects.rs"]
pub(super) mod projects;

const TEMPLATE: &str = "Две калибровки";
const COUNTER: &str = "Калибровки";

pub(super) fn record(output: &Path, releases: &Path, atlas: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let staged = output.join("staging");
    fs::create_dir_all(&staged).map_err(io_error)?;
    let staging_delta = staged.join("TradeWind.exe");
    let staging_lens = staged.join("FishSpitLights.exe");
    let source_delta = staged.join("expedition-seed.robot.json");
    let source_lens = staged.join("lens-seed.robot.json");
    let delta = output.join("trade-wind.robot.json");
    let lens = output.join("fish-spit-lights.robot.json");
    let library = output.join("two-calibrations.rules.json");

    let mut assets = crate::assets::Library::default();
    let fixture = projects::expedition(&mut assets)?;
    assets.save_files(crate::asset_sources(&fixture), &source_delta)?;
    fixture.save(&source_delta)?;
    projects::lens(atlas)?.save(&source_lens)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(
        &output.join("shared-rules-games.mp4"),
        SIZE[0],
        SIZE[1],
        FPS,
    )?;
    {
        let mut sink = Sink::Video(&mut video);
        let mut app = Studio::default();
        app.desk.recovery_root = Some(output.join("recovery-expedition"));
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((&source_delta, &staging_delta)),
        );
        d.settle(5)?;
        panel_click(&mut d, "Открыть…")?;
        d.click("Правила")?;
        d.click("Библиотека")?;
        d.click("Сохранить шаблон")?;
        d.edit("Две разные отметки", usize::MAX, TEMPLATE)?;
        d.edit("Отметки", usize::MAX, COUNTER)?;
        set_path(&mut d, &library)?;
        d.click("Сохранить библиотеку")?;
        if !library.is_file() {
            return Err("Общий пакет не сохранён.".into());
        }
        bind(
            &mut d,
            projects::EXPEDITION_PADS.map(|(name, _)| name),
            projects::EXPEDITION,
        )?;
        complete_author(&mut d, &delta)?;
        panel_click(&mut d, "▶ Играть")?;
        play_delta(&mut d)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
    }
    move_and_play(
        &mut video,
        &mut display,
        &staging_delta,
        &releases.join("TradeWind.exe"),
        "delta23",
        &output.join("delta-capture-timing.json"),
    )?;
    {
        let mut sink = Sink::Video(&mut video);
        let mut app = Studio::default();
        app.desk.recovery_root = Some(output.join("recovery-lens"));
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((&source_lens, &staging_lens)),
        );
        d.settle(5)?;
        panel_click(&mut d, "Открыть…")?;
        d.expect_hud("2D сверху")?;
        d.click("Правила")?;
        d.click("Библиотека")?;
        set_path(&mut d, &library)?;
        d.click("Открыть библиотеку")?;
        if !d
            .app
            .scene
            .rules
            .templates
            .iter()
            .any(|row| row.name == TEMPLATE && row.counter_name == COUNTER)
        {
            return Err("Перенесённый пакет не появился в 2D-проекте.".into());
        }
        bind(
            &mut d,
            projects::LENS_PADS.map(|(name, _)| name),
            projects::LENS,
        )?;
        complete_author(&mut d, &lens)?;
        panel_click(&mut d, "▶ Играть")?;
        play_lens(&mut d)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
    }
    move_and_play(
        &mut video,
        &mut display,
        &staging_lens,
        &releases.join("FishSpitLights.exe"),
        "lens23",
        &output.join("lens-capture-timing.json"),
    )?;
    video.finish()
}

fn complete_author(d: &mut Driver<'_, '_>, path: &Path) -> Result<(), String> {
    d.app.proof.as_mut().unwrap().scene = Some(path.to_owned());
    panel_click(d, "Сохранить как…")?;
    reload(d)
}

fn set_path(d: &mut Driver<'_, '_>, path: &Path) -> Result<(), String> {
    if d.point("Путь к файлу .rules.json", 0).is_err() {
        top_proof::scroll_panel(d, 850.0)?;
    }
    d.edit(
        "Путь к файлу .rules.json",
        usize::MAX,
        &path.to_string_lossy(),
    )
}

fn bind(d: &mut Driver<'_, '_>, zones: [&str; 2], quest: &str) -> Result<(), String> {
    for zone in zones {
        if d.point("Выбери источник", 0).is_err() {
            top_proof::scroll_panel(d, -335.0)?;
        }
        d.click("Выбери источник")?;
        d.click_n(&format!("F в зоне · {zone}"), usize::MAX)?;
    }
    d.click("Выбери цель")?;
    let target = format!("Этап · {quest}");
    if d.point(&target, usize::MAX).is_err() {
        let popup_entry = d.point("Включить · Ива у дельты", usize::MAX)?;
        top_proof::wheel(d, popup_entry, -1_380.0)?;
    }
    d.click_n(&target, usize::MAX)?;
    if d.point("Применить шаблон", 0).is_err() {
        top_proof::scroll_panel(d, -290.0)?;
    }
    d.click("Применить шаблон")?;
    if d.app.scene.rules.rules.len() != 4 || d.app.scene.rules.counters.len() != 1 {
        return Err("Библиотека не создала четыре обычных правила и один счётчик.".into());
    }
    d.hold(22)
}

pub(super) fn play_delta(d: &mut Driver<'_, '_>) -> Result<(), String> {
    if d.app
        .scene
        .environment
        .as_ref()
        .and_then(|data| data.waters.first())
        .is_none()
        || d.app.scene.wind_rotor.is_none()
    {
        return Err("Редактируемая вода или ротор не загрузился.".into());
    }
    d.settle(10)?;
    let rotor = d.app.rotor_angle;
    quarter_proof::walk(d, projects::EXPEDITION_PADS[0].1)?;
    d.key(Key::F)?;
    verify(d, 1, false, projects::EXPEDITION)?;
    d.key(Key::F)?;
    verify(d, 1, false, projects::EXPEDITION)?;
    quarter_proof::turn(d, 0.02, 0.25)?;
    quarter_proof::walk(d, [0.08, -9.8])?;
    quarter_proof::walk(d, projects::EXPEDITION_PADS[1].1)?;
    d.key(Key::F)?;
    verify(d, 2, true, projects::EXPEDITION)?;
    if (d.app.rotor_angle - rotor).abs() <= 0.001 {
        return Err("Ротор не отозвался на ветер.".into());
    }
    d.hold(28)
}

pub(super) fn play_lens(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let map = d.app.scene.map_2d.as_ref().ok_or("Карта 2D отсутствует.")?;
    let hero = map
        .hero
        .and_then(|tile| map.atlas(tile.atlas))
        .ok_or("Собственный спрайт героя не загрузился.")?;
    if hero.name != "Мозаика Рыбной косы" || map.mode != Mode::Top {
        return Err("Самостоятельная карта со своим атласом не восстановилась.".into());
    }
    top_proof::move_axis(d, 0, 8.5)?;
    d.key(Key::F)?;
    verify(d, 1, false, projects::LENS)?;
    d.key(Key::F)?;
    verify(d, 1, false, projects::LENS)?;
    // The obvious short path crosses a solid rock. The route continues via the bridge.
    d.frame(vec![proof_input::key_event(Key::D, Modifiers::NONE, true)])?;
    d.settle(48)?;
    d.frame(vec![proof_input::key_event(Key::D, Modifiers::NONE, false)])?;
    d.settle(8)?;
    let blocked_at = d.app.game_position();
    if !(9.25..9.99).contains(&blocked_at.x) {
        return Err(format!(
            "Скалы должны остановить героя перед клеткой 10, текущая позиция {blocked_at:?}."
        ));
    }
    top_proof::move_axis(d, 2, 12.5)?;
    top_proof::move_axis(d, 0, 21.5)?;
    d.key(Key::F)?;
    verify(d, 2, true, projects::LENS)?;
    d.hold(28)
}

pub(super) fn restore_winning_save(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let quest = if d.app.scene.map_2d.is_some() {
        projects::LENS
    } else {
        projects::EXPEDITION
    };
    let end = d.app.game_position();
    d.key(Key::F5)?;
    d.expect_hud("Игра сохранена.")?;
    d.hold(20)?;
    d.click("Начать заново")?;
    verify(d, 0, false, quest)?;
    d.key(Key::F9)?;
    d.expect_hud("Игра загружена.")?;
    verify(d, 2, true, quest)?;
    if d.app.game_position().distance(end) > 0.08 {
        return Err("Загрузка вернула другую позицию героя.".into());
    }
    d.hold(24)
}

fn verify(d: &Driver<'_, '_>, total: i32, complete: bool, quest: &str) -> Result<(), String> {
    let counter = d
        .app
        .scene
        .rules
        .counters
        .iter()
        .find(|row| row.name == COUNTER)
        .ok_or("Счётчик не найден.")?;
    let actual = d.app.rule_session.counter(&d.app.scene.rules, counter.id);
    let finished = d.app.rule_session.quest_stage(1) == Some(1);
    if actual != Some(total) || finished != complete {
        return Err(format!(
            "Счётчик {actual:?}, ожидалось {total}. Финиш {finished}, нужен {complete}."
        ));
    }
    d.expect_hud(&format!("{COUNTER} · {total}"))?;
    if complete {
        d.expect_hud(&format!("Выполнено · {quest}"))?;
    }
    Ok(())
}

pub(super) fn legacy(output: &Path, inputs: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let staging = output.join("staging");
    let moved = output.join("moved");
    fs::create_dir_all(&staging).map_err(io_error)?;
    fs::create_dir_all(&moved).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(
        &output.join("legacy-map-reexport.mp4"),
        SIZE[0],
        SIZE[1],
        FPS,
    )?;
    for (name, route, expected_mode) in [
        ("terrace", "top", Mode::Top),
        ("bell-tower", "side", Mode::Side),
    ] {
        let source = inputs.join(format!("{name}.robot.json"));
        let target = output.join(format!("{name}-imported.robot.json"));
        let exe_name = format!("{name}-imported.exe");
        let export = staging.join(&exe_name);
        {
            let mut app = Studio::default();
            app.desk.recovery_root = Some(output.join(format!("recovery-{name}")));
            let mut sink = Sink::Video(&mut video);
            let mut d = Driver::new(app, &mut display, &mut sink, Some((&source, &export)));
            d.settle(5)?;
            panel_click(&mut d, "Открыть…")?;
            if d.app
                .scene
                .map_2d
                .as_ref()
                .is_none_or(|map| map.mode != expected_mode)
            {
                return Err(format!("Старая сцена {name} не загрузилась."));
            }
            paint_legacy_tile(&mut d)?;
            complete_author(&mut d, &target)?;
            panel_click(&mut d, "Собрать игру для Windows")?;
        }
        move_and_play(
            &mut video,
            &mut display,
            &export,
            &moved.join(&exe_name),
            route,
            &output.join(format!("timing-{name}.json")),
        )?;
    }
    video.finish()?;
    let report = json!({"source":"0.15","opened_edited_saved_and_reopened":["terrace","bell-tower"],
        "played_moved_exe":["terrace-imported.exe","bell-tower-imported.exe"],
        "edit":"ground tile 0,0 changed using brush; existing collision kept"});
    fs::write(
        output.join("legacy-verified.json"),
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

fn paint_legacy_tile(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let before = d.app.scene.map_2d.as_ref().unwrap().clone();
    let stage = if before.layers[0][0].is_some_and(|tile| tile.atlas == 1 && tile.index == 1) {
        "0"
    } else {
        "1"
    };
    if stage == "1" {
        d.edit("Тайл 0", 0, "1")?;
        d.key(Key::Enter)?;
    }
    let view = d
        .app
        .map_editor
        .transform()
        .ok_or("Не найдена проекция карты.")?;
    let point = view.at_cell([0, 0]);
    d.frame(proof_input::hover(point))?;
    d.frame(proof_input::click(point))?;
    d.settle(4)?;
    let after = d.app.scene.map_2d.as_ref().unwrap();
    if after.layers[0][0] == before.layers[0][0] || after.colliders != before.colliders {
        return Err("Кисть не изменила старую карту без изменения коллизий.".into());
    }
    Ok(())
}
