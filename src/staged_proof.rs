//! Background runs for the independent game modes and the cross-project libraries.
use std::{
    ffi::OsStr,
    fs,
    io::{BufWriter, Write},
    path::Path,
};

use eframe::egui::{self, Key, RichText};
use serde_json::json;

use super::{Driver, FPS, SIZE, Sink, append_player, io_error, quarter_proof, top_proof};
use crate::{
    Studio, View, configure_context, game_export, map2d, offscreen::Offscreen, proof_input,
    proof_video::Video, quarter, rules, viewport3d, world::Interaction,
};

const FIRST: &str = "Западная отметка";
const SECOND: &str = "Восточная отметка";
const KEY_CELL: [f32; 2] = [-4.40, -10.75];
const EAST_CELL: [f32; 2] = [3.48, -10.35];
const DOOR: [f32; 2] = [4.66, -15.70];

pub(super) fn record_014(
    output: &Path,
    releases: &Path,
    legacy_13: &Path,
    legacy_12: &Path,
    atlas: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let stage = output.join("export-stage");
    fs::create_dir_all(&stage).map_err(io_error)?;
    let linden = output.join("linden-marks.robot.json");
    let top = output.join("terrace.robot.json");
    let library = output.join("two-marks.rules.json");
    let metrics = output.join("timings-3d.json");
    let top_metrics = output.join("timings-top.json");
    let linden_exe = releases.join("LindenMarks.exe");
    let top_exe = releases.join("Terrace.exe");
    let stage_linden = stage.join("LindenMarks.exe");
    let stage_top = stage.join("Terrace.exe");
    for path in [
        &linden,
        &top,
        &library,
        &metrics,
        &top_metrics,
        &linden_exe,
        &top_exe,
        &stage_linden,
        &stage_top,
    ] {
        if path.exists() {
            return Err(format!("Выходной путь уже занят. {}", path.display()));
        }
    }
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::default();
    {
        let mut sink = Sink::None;
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((legacy_12, &stage_linden)),
        );
        d.settle(4)?;
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene.world.keys.len() != 1 || d.app.scene.checkpoint.is_none() {
            return Err("Проект 0.12 не открылся.".into());
        }
        d.app.proof.as_mut().unwrap().scene = Some(legacy_13.into());
        panel_click(&mut d, "Открыть…")?;
        quarter_proof::fresh(&mut d, View::Three)?;
        quarter_proof::play(&mut d, true, false)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Открыть…")?;
        app = d.app;
    }
    eprintln!("0.12 открылся, исходный Липовый проезд 0.13 пройден.");

    let mut video = Video::start(&output.join("stage-014.mp4"), SIZE[0], SIZE[1], FPS)?;
    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(app, &mut display, &mut sink, Some((&linden, &stage_linden)));
        d.settle(5)?;
        author_linden(&mut d, &library)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        quarter_proof::fresh(&mut d, View::Three)?;
        play_linden(&mut d, true)?;
        panel_click(&mut d, "■ Стоп")?;
        d.click("Собрать игру для Windows")?;
        app = d.app;
    }
    move_and_play(
        &mut video,
        &mut display,
        &stage_linden,
        &linden_exe,
        "linden",
        &metrics,
    )?;
    eprintln!(
        "Механика из библиотеки открывает мастерскую, третий вход в зоны не добавляет отметок."
    );

    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(app, &mut display, &mut sink, Some((&top, &stage_top)));
        d.settle(4)?;
        top_proof::author(&mut d, atlas, &library)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "▶ Играть")?;
        top_proof::play(&mut d, true)?;
        panel_click(&mut d, "■ Стоп")?;
        d.click("Собрать игру для Windows")?;
    }
    move_and_play(
        &mut video,
        &mut display,
        &stage_top,
        &top_exe,
        "top",
        &top_metrics,
    )?;
    video.finish()?;
    eprintln!("2D сверху собран и пройден в редакторе и перенесённом EXE.");
    Ok(())
}

pub(super) fn reload(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let saved = d.app.scene.clone();
    panel_click(d, "Открыть…")?;
    if d.app.scene != saved {
        let after = serde_json::to_value(&d.app.scene).map_err(|error| error.to_string())?;
        let before = serde_json::to_value(saved).map_err(|error| error.to_string())?;
        let changes = before
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(name, value)| (after.get(name) != Some(value)).then_some(name.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "Сохранение не совпало с открытым проектом. {} Изменились поля {changes}.",
            d.app.status
        ));
    }
    Ok(())
}

fn author_linden(d: &mut Driver<'_, '_>, library: &Path) -> Result<(), String> {
    d.click("Правила")?;
    d.click("Зоны")?;
    add_zone(d, FIRST, [KEY_CELL[0], 0.0, KEY_CELL[1]])?;
    add_zone(d, SECOND, [EAST_CELL[0], 0.0, EAST_CELL[1]])?;
    d.click("Библиотека")?;
    d.edit("Путь к файлу .rules.json", 0, &library.to_string_lossy())?;
    d.click("Сохранить шаблон")?;
    d.click("Сохранить библиотеку")?;
    d.click("Выбери источник")?;
    d.click_n(&format!("Вход · {FIRST}"), usize::MAX)?;
    d.click("Выбери источник")?;
    d.click_n(&format!("Вход · {SECOND}"), usize::MAX)?;
    d.click("Выбери цель")?;
    d.click_n("Отпереть · Мастерская 04 · Входная дверь", usize::MAX)?;
    d.click("Применить шаблон")?;
    if !library.exists() || d.app.scene.rules.counters.len() != 1 {
        return Err("Шаблон не сохранён или не применился к кварталу.".into());
    }
    d.click("Сценарии")?;
    d.click("Поручение Марины")?;
    d.click_n(quarter::LOCK_RULE, usize::MAX)?;
    d.click("Значение флага")?;
    d.click_n("Сравнение счётчика", usize::MAX)?;
    number(d, "Значение", "2")?;
    let id = d.app.scene.rules.counters[0].id;
    let rule = d
        .app
        .scene
        .rules
        .rules
        .iter()
        .find(|rule| rule.name == quarter::LOCK_RULE)
        .ok_or("Не найдено правило допуска.")?;
    if rule.conditions.first()
        != Some(&rules::Condition::Counter {
            counter_id: id,
            comparison: rules::Comparison::AtLeast,
            value: 2,
        })
    {
        return Err(format!(
            "Правило мастерской не использует обе отметки. {:?}",
            rule.conditions
        ));
    }
    Ok(())
}

pub(super) fn add_zone(d: &mut Driver<'_, '_>, label: &str, pos: [f32; 3]) -> Result<(), String> {
    d.click("+ Зона")?;
    let default = d
        .app
        .scene
        .rules
        .zones
        .last()
        .ok_or("Редактор не добавил зону.")?
        .name
        .clone();
    d.edit(&default, 1, label)?;
    for (axis, value) in ["X", "Y", "Z"].into_iter().zip(pos) {
        if value != 0.0 {
            number(d, axis, &value.to_string())?;
        }
    }
    Ok(())
}

pub(super) fn number(d: &mut Driver<'_, '_>, prefix: &str, value: &str) -> Result<(), String> {
    let output = d.display.last_output().ok_or("Пустой интерфейс.")?;
    let label = proof_input::labels(output)
        .into_iter()
        .find(|label| {
            label.text.strip_prefix(prefix).is_some_and(|value| {
                let value = value.trim();
                value
                    .strip_suffix(" с")
                    .unwrap_or(value)
                    .trim()
                    .parse::<f32>()
                    .is_ok()
            })
        })
        .ok_or_else(|| format!("Нет поля «{prefix}»."))?
        .text;
    d.edit(&label, 0, value)?;
    d.key(Key::Enter)
}

pub(super) fn panel_click(d: &mut Driver<'_, '_>, label: &str) -> Result<(), String> {
    let pos = d.point(label, 0)?;
    d.frame(proof_input::hover(pos))?;
    d.frame(proof_input::click(pos))?;
    d.settle(7)
}

pub(super) fn play_linden(d: &mut Driver<'_, '_>, preview: bool) -> Result<(), String> {
    let quest = d.app.scene.story.quests[0].id;
    let counter = d
        .app
        .scene
        .rules
        .counters
        .first()
        .ok_or("В квартале отсутствует счётчик отметок.")?
        .id;
    d.key(Key::E)?;
    panel_click(d, quarter::ACCEPT)?;
    quarter_proof::walk(d, KEY_CELL)?;
    expect_counter(d, counter, 1)?;
    d.key(Key::E)?;
    quarter_proof::walk(d, [-4.40, -8.70])?;
    quarter_proof::walk(d, KEY_CELL)?;
    expect_counter(d, counter, 1)?;
    if preview {
        let repeated = d
            .app
            .rule_session
            .trace()
            .any(|trace| trace.outcome == rules::TraceOutcome::AlreadyFired);
        if !repeated {
            return Err("Журнал не показал повтор первой отметки.".into());
        }
        d.hold(45)?;
    }
    for waypoint in [[0.0, -13.45], [3.63, -15.60], DOOR] {
        quarter_proof::walk(d, waypoint)?;
    }
    d.key(Key::E)?;
    if !matches!(
        d.app
            .scene
            .world
            .interaction_near(&d.app.interactions, d.app.game_position().to_array()),
        Some(Interaction::RuleLockedDoor { .. })
    ) {
        return Err("Мастерская открылась после одной отметки.".into());
    }
    d.expect_hud("Дверь заперта")?;
    if preview {
        d.hold(48)?;
    } else {
        d.settle(65)?;
    }
    quarter_proof::walk(d, [3.63, -14.45])?;
    quarter_proof::walk(d, EAST_CELL)?;
    expect_counter(d, counter, 2)?;
    quarter_proof::walk(d, [3.63, -14.45])?;
    quarter_proof::walk(d, DOOR)?;
    d.key(Key::E)?;
    d.settle(48)?;
    quarter_proof::walk(d, [6.55, -15.70])?;
    if d.app.rule_session.quest_stage(quest) != Some(4) {
        return Err("Вход после двух отметок не завершил квест.".into());
    }
    d.expect_hud("Выполнено")?;
    d.settle(90)
}

fn expect_counter(d: &Driver<'_, '_>, id: u32, expected: i32) -> Result<(), String> {
    let actual = d.app.rule_session.counter(&d.app.scene.rules, id);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "Позиция {:?}. Ожидалось {expected} отметок, получено {actual:?}.",
            d.app.game_position()
        ))
    }
}

pub(super) fn move_and_play(
    capture: &mut Video,
    display: &mut Offscreen,
    source: &Path,
    target: &Path,
    route: &str,
    metrics: &Path,
) -> Result<(), String> {
    if target.exists() {
        return Err("Файл игры в папке назначения уже существует.".into());
    }
    fs::rename(source, target).map_err(io_error)?;
    let target = target.canonicalize().map_err(io_error)?;
    let report = metrics
        .parent()
        .ok_or("У отчёта нет папки.")?
        .canonicalize()
        .map_err(io_error)?
        .join(metrics.file_name().ok_or("У отчёта нет имени.")?);
    let art = display.frame(proof_input::Driver::new(SIZE).raw(vec![]), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(250.0);
                ui.heading(RichText::new("Запуск отдельной игры").size(31.0));
                ui.label(
                    RichText::new(target.file_name().unwrap_or_default().to_string_lossy())
                        .color(crate::ACCENT)
                        .size(25.0),
                );
                ui.label(format!(
                    "Из папки {}",
                    target
                        .parent()
                        .unwrap()
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ));
            });
        });
    })?;
    capture.push(&art, 66)?;
    append_player(
        capture,
        &target,
        &[
            OsStr::new("--background-staged-player"),
            OsStr::new(route),
            report.as_os_str(),
        ],
    )
}

pub(super) fn player(
    game: game_export::GamePackage,
    route: &OsStr,
    metrics: &Path,
) -> Result<(), String> {
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::from_package(game, display.context());
    if route == "lens23" || route == "delta23" {
        app.open_game_save(display.context(), Some(metrics.with_extension("checkpoint.json")));
    }
    let mode = app.scene.map_2d.as_ref().map(|mode| mode.mode);
    let output = std::io::stdout();
    let mut frames = BufWriter::new(output.lock());
    let mut sink = Sink::Stream(&mut frames);
    let mut d = Driver::new(app, &mut display, &mut sink, None);
    d.settle(80)?;
    d.display.start_timing();
    match (route.to_str(), mode) {
        (Some("delta23"), None) => {
            super::games23_proof::play_delta(&mut d)?;
            super::games23_proof::restore_winning_save(&mut d)?;
        }
        (Some("lens23"), Some(map2d::Mode::Top)) => {
            super::games23_proof::play_lens(&mut d)?;
            super::games23_proof::restore_winning_save(&mut d)?;
        }
        (Some("signal-trail"), Some(map2d::Mode::Top)) => super::wizard_proof::play(&mut d)?,
        (Some("review"), Some(map2d::Mode::Top)) => super::review_stage_proof::play_review(&mut d)?,
        (Some("dungeon"), Some(map2d::Mode::Top)) => { super::dungeon_proof::play(&mut d)?; }
        (Some("cast"), None) => super::character_stage_proof::play_cast(&mut d, false)?,
        (Some("visual"), None) => super::visual_stage_proof::play_city(&mut d, false, None)?,
        (Some("fx2d"), Some(map2d::Mode::Top)) => super::visual_stage_proof::play_map(&mut d)?,
        (Some("mill"), None) => super::mill_walk_proof::play(&mut d, false)?,
        (Some("river"), Some(map2d::Mode::Top)) => super::environment_proof::play_river(&mut d)?,
        (Some("gateway"), None) => {
            super::mechanics_proof::enter_gateway(&mut d)?;
            super::tactics_proof::play(&mut d, false)?;
        }
        (Some("tactics"), Some(map2d::Mode::Top)) => super::tactics_proof::play(&mut d, false)?,
        (Some("seat"), None) => super::visual_proof::sit(&mut d)?,
        (Some("linden"), None) => {
            if d.app
                .scene
                .world
                .models
                .iter()
                .any(|item| item.seat.is_some())
            {
                super::visual_proof::sit(&mut d)?;
            }
            play_linden(&mut d, false)?;
        }
        (Some("top"), Some(map2d::Mode::Top)) => top_proof::play(&mut d, false)?,
        (Some("side"), Some(map2d::Mode::Side)) => super::side_proof::play(&mut d, false)?,
        _ => return Err("EXE запустился с неожиданным типом сцены.".into()),
    }
    let adapter = d.display.adapter_info().clone();
    let mut samples = d.display.stop_timing();
    frames.flush().map_err(io_error)?;
    if samples.is_empty() {
        return Err("Пропущен замер кадров.".into());
    }
    samples.sort_by(f32::total_cmp);
    let percentile = |p: f32| samples[((samples.len() - 1) as f32 * p).ceil() as usize];
    let report = json!({
        "route": route.to_string_lossy(),
        "gpu": adapter.name, "backend": format!("{:?}", adapter.backend), "driver": adapter.driver_info,
        "resolution": SIZE, "msaa": viewport3d::VIEWPORT_MSAA,
        "shadow_map": viewport3d::shadow_size(viewport3d::VIEWPORT_MSAA),
        "method": "Offscreen wall frame through GPU synchronization and RGB readback; encoder and pipe excluded.",
        "frames": samples.len(), "mean_ms": samples.iter().sum::<f32>()/samples.len() as f32,
        "p50_ms":percentile(0.5), "p95_ms":percentile(0.95), "p99_ms":percentile(0.99),
        "max_ms": samples[samples.len()-1],
    });
    fs::write(
        metrics,
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}
