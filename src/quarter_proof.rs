//! Background playback of the editorial change and both quarter scenarios.
use std::{
    f32::consts::{PI, TAU},
    ffi::OsStr,
    fs,
    io::{BufWriter, Write},
    path::Path,
};

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, RichText};
use serde_json::json;

use super::{COMPLETE, Driver, FPS, SIZE, Sink, append_player, io_error};
use crate::{
    Studio, View, configure_context, game_export,
    offscreen::Offscreen,
    proof_input,
    proof_video::Video,
    quarter,
    rules::{self, Condition, TraceOutcome},
    scene::Scene,
    viewport3d::{self, Camera},
    world::Interaction,
};

const OUTSIDE: [f32; 2] = [4.66, -15.70];
const BEHIND_WALL: [f32; 2] = [4.66, -17.46];
const INSIDE: [f32; 2] = [6.55, -15.70];
const GUARD_TALK: [f32; 2] = [3.58, -10.35];
const KEY_APPROACH: [f32; 2] = [-4.40, -10.75];
const WARMUP_TICKS: usize = 120;

pub fn record(
    scene: &Path,
    stage: &Path,
    moved: &Path,
    video: &Path,
    metrics: &Path,
    legacy: &Path,
) -> Result<(), String> {
    for path in [scene, stage, moved, video, metrics] {
        if path.exists() {
            return Err(format!("Путь уже занят. {}", path.display()));
        }
    }
    if stage.parent() == moved.parent() {
        return Err("Укажи две разные папки для проверки переноса.".into());
    }
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::default();
    {
        let mut sink = Sink::None;
        let mut d = Driver::new(app, &mut display, &mut sink, Some((legacy, stage)));
        d.settle(7)?;
        click(&mut d, "Открыть…")?;
        if d.app.scene.world.keys.len() != 1
            || d.app.scene.checkpoint.is_none()
            || !d.app.scene.rules.rules.is_empty()
        {
            return Err("Сцена «Ключ от комнаты» из 0.12 не восстановилась.".into());
        }
        d.app.proof.as_mut().expect("proof paths").scene = Some(scene.to_owned());
        click(&mut d, "Новый…")?;
        click(&mut d, quarter::NAME)?;
        check_condition(&d.app.scene, false)?;
        fresh(&mut d, View::Three)?;
        inspect_free_camera(&mut d, scene.parent().unwrap_or(Path::new(".")))?;
        play(&mut d, false, false)?;
        click(&mut d, "■ Стоп")?;
        click(&mut d, "Новый…")?;
        click(&mut d, quarter::NAME)?;
        app = d.app;
    }
    eprintln!(
        "Открыта сцена 0.12, исходный квест с ключом пройден в 3D, свободная камера проверена."
    );

    let mut recording = Video::start(video, SIZE[0], SIZE[1], FPS)?;
    {
        let mut sink = Sink::Video(&mut recording);
        let mut d = Driver::new(app, &mut display, &mut sink, Some((scene, stage)));
        d.settle(40)?;
        d.click("Правила")?;
        d.click("Поручение Марины")?;
        d.click_n(quarter::LOCK_RULE, usize::MAX)?;
        d.settle(55)?;
        d.click("Есть предмет")?;
        d.click_n("Значение флага", usize::MAX)?;
        d.click("Поручение Марины")?;
        d.click_n(quarter::GUARD_FLAG, usize::MAX)?;
        check_condition(&d.app.scene, true)?;
        d.settle(55)?;
        d.click("Сохранить")?;
        d.settle(48)?;
        d.click("Открыть…")?;
        check_condition(&Scene::load(scene)?, true)?;
        check_condition(&d.app.scene, true)?;
        fresh(&mut d, View::Three)?;
        play(&mut d, true, false)?;
        click(&mut d, "■ Стоп")?;
        d.click("Собрать игру для Windows")?;
        if !stage.is_file() {
            return Err(d.app.status.clone());
        }
        d.settle(90)?;
        app = d.app;
    }
    eprintln!("Условие изменено через интерфейс, открыто из сохранения и пройдено в редакторе.");

    fs::rename(stage, moved).map_err(io_error)?;
    let exe = moved.canonicalize().map_err(io_error)?;
    let metrics = metrics
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(io_error)?
        .join(metrics.file_name().ok_or("Нет имени отчёта.")?);
    let label = format!(
        "{} → {}",
        stage
            .parent()
            .and_then(Path::file_name)
            .unwrap_or_default()
            .to_string_lossy(),
        exe.parent()
            .and_then(Path::file_name)
            .unwrap_or_default()
            .to_string_lossy()
    );
    let art = display.frame(proof_input::Driver::new(SIZE).raw(vec![]), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(260.0);
                ui.heading(RichText::new("Запуск перенесённой игры").size(34.0));
                ui.add_space(15.0);
                ui.label(RichText::new(&label).size(21.0));
                ui.label(
                    RichText::new(exe.file_name().unwrap_or_default().to_string_lossy())
                        .color(crate::ACCENT)
                        .size(24.0),
                );
            });
        });
    })?;
    recording.push(&art, 72)?;
    append_player(
        &mut recording,
        &exe,
        &[
            OsStr::new("--background-proof-quarter-player"),
            metrics.as_os_str(),
        ],
    )?;
    recording.finish()?;
    eprintln!("Перенесённая игра прошла фоновой маршрут. Видео записано.");

    let mut sink = Sink::None;
    let mut d = Driver::new(app, &mut display, &mut sink, Some((scene, stage)));
    d.settle(4)?;
    click(&mut d, "Открыть…")?;
    fresh(&mut d, View::Two)?;
    play(&mut d, true, false)?;
    eprintln!("Вариант с охранником полностью пройден на 2D-плане.");
    Ok(())
}

pub fn player(package: game_export::GamePackage, metrics: &Path) -> Result<(), String> {
    check_condition(&package.scene, true)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let app = Studio::from_package(package, display.context());
    if app.view != View::Three || app.free_camera {
        return Err("Экспорт должен начинаться с обычной 3D-камерой.".into());
    }
    let adapter = display.adapter_info().clone();
    let output = std::io::stdout();
    let mut frames = BufWriter::new(output.lock());
    let mut sink = Sink::Stream(&mut frames);
    let mut d = Driver::new(app, &mut display, &mut sink, None);
    d.settle(WARMUP_TICKS)?;
    d.display.start_timing();
    play(&mut d, true, true)?;
    let mut samples = d.display.stop_timing();
    let game_frames = d.phase;
    if game_frames < 60 * 60 {
        return Err(format!("В игре записано только {game_frames} шагов."));
    }
    if samples.is_empty() {
        return Err("Маршрут не дал замеров кадра.".into());
    }
    frames.flush().map_err(io_error)?;
    let samples_in_order = samples.clone();
    samples.sort_by(f32::total_cmp);
    let percentile = |p: f32| samples[((samples.len() - 1) as f32 * p).ceil() as usize];
    let report = json!({
        "mode": "standalone Windows EXE, 3D regular gameplay camera",
        "route": "Липовый проезд: Марина, северная часть улицы, лавка с ключом, мастерская, Илья, вход",
        "adapter": adapter.name,
        "driver": adapter.driver_info,
        "backend": format!("{:?}", adapter.backend),
        "resolution": SIZE,
        "settings": { "msaa_samples": viewport3d::VIEWPORT_MSAA, "shadow_map": viewport3d::shadow_size(viewport3d::VIEWPORT_MSAA), "preset": "normal", "vsync": "no screen surface" },
        "method": "Wall time of full offscreen frame including egui, GPU completion and RGB readback, excluding video encoding and pipe writes. No presentation rate measurement.",
        "simulation_hz": 60, "video_fps": FPS, "gameplay_seconds": (game_frames / 2) as f32 / FPS as f32,
        "warmup_simulation_frames": WARMUP_TICKS,
        "count": samples.len(),
        "mean_ms": samples.iter().sum::<f32>() / samples.len() as f32,
        "p50_ms": percentile(0.50), "p95_ms": percentile(0.95), "p99_ms": percentile(0.99),
        "min_ms": samples[0], "max_ms": samples[samples.len()-1],
        "frames_ms": samples_in_order,
    });
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(metrics)
        .map_err(io_error)?;
    serde_json::to_writer_pretty(file, &report).map_err(|e| e.to_string())
}

fn check_condition(scene: &Scene, guard: bool) -> Result<(), String> {
    let rule = scene
        .rules
        .rules
        .iter()
        .find(|r| r.name == quarter::LOCK_RULE)
        .ok_or("Не найдено правило входа.")?;
    let access_flag = scene
        .rules
        .flags
        .iter()
        .find(|f| f.name == quarter::GUARD_FLAG)
        .ok_or("Не найден флаг охранника.")?
        .id;
    let expected = if guard {
        Condition::Flag {
            flag_id: access_flag,
            value: true,
        }
    } else {
        Condition::HasItem {
            item_id: scene.world.keys[0].id,
        }
    };
    if rule.conditions.first() != Some(&expected)
        || !matches!(rule.event, Some(rules::Event::Door { door_id })
            if rule.actions.iter().any(|a| *a == rules::Action::UnlockDoor { door_id }))
        || !scene
            .rules
            .zones
            .iter()
            .any(|zone| zone.building_id.is_some())
    {
        return Err(format!(
            "После редактирования или открытия ожидалось условие {expected:?}. Правило {rule:?}."
        ));
    }
    scene.validate()
}

pub(super) fn fresh(d: &mut Driver<'_, '_>, view: View) -> Result<(), String> {
    if view == View::Two {
        click(d, "2D")?;
        click(d, "План")?;
    } else {
        click(d, "3D")?;
        click(d, "Сброс камеры")?;
    }
    click(d, "▶ Играть")?;
    d.settle(15)
}

fn inspect_free_camera(d: &mut Driver<'_, '_>, folder: &Path) -> Result<(), String> {
    click(d, "Свободная камера")?;
    if !d.app.free_camera {
        return Err("Свободная камера не включилась.".into());
    }
    let before = d.app.camera.target;
    d.frame(vec![proof_input::key_event(
        Key::PageUp,
        Modifiers::NONE,
        true,
    )])?;
    d.settle(38)?;
    d.frame(vec![proof_input::key_event(
        Key::PageUp,
        Modifiers::NONE,
        false,
    )])?;
    if d.app.camera.target[1] < before[1] + 1.0 || d.app.game_position().length() > 0.01 {
        return Err("Свободная камера сместила героя или осталась на месте.".into());
    }
    d.settle(10)?;
    image::save_buffer(
        folder.join("free-camera.png"),
        &d.last,
        SIZE[0],
        SIZE[1],
        image::ColorType::Rgb8,
    )
    .map_err(|error| error.to_string())?;
    click(d, "Свободная камера")?;
    click(d, "Сброс камеры")
}

pub(super) fn play(d: &mut Driver<'_, '_>, guard: bool, tour: bool) -> Result<(), String> {
    let quest = d.app.scene.story.quests[0].id;
    d.key(Key::E)?;
    click(d, "Может, позже")?;
    expect_stage(d, quest, None)?;
    if tour {
        d.settle(42)?;
    }
    d.key(Key::E)?;
    click(d, quarter::ACCEPT)?;
    expect_stage(d, quest, Some(1))?;

    if tour {
        for z in [-5.0, -10.0, -15.0, -20.0, -25.0, -30.0, -35.0, -40.0, -43.0] {
            walk(d, [0.0, z])?;
        }
        close_up(d)?;
        for z in [-38.0, -33.0, -28.0, -23.0, -18.0, -13.0, -10.75] {
            walk(d, [0.0, z])?;
        }
    }

    if !guard {
        approach_workshop(d)?;
        d.key(Key::E)?;
        expect_locked(d)?;
        d.expect_hud("нужен предмет")?;
        expect_stage(d, quest, Some(1))?;
        walk(d, [0.0, -13.0])?;
    }
    walk(d, KEY_APPROACH)?;
    d.key(Key::E)?;
    d.expect_hud("Подобран ключ")?;
    expect_stage(d, quest, Some(2))?;
    if tour {
        d.settle(120)?;
    }
    approach_workshop(d)?;
    if guard {
        d.key(Key::E)?;
        expect_locked(d)?;
        expect_stage(d, quest, Some(2))?;
        if !d.app.standalone {
            d.expect_hud("условие не выполнено")?;
        }
        d.settle(if tour { 165 } else { 38 })?;
        walk(d, GUARD_TALK)?;
        d.key(Key::E)?;
        if tour {
            d.settle(165)?;
        }
        click(d, quarter::WRONG_REPLY)?;
        let access = d
            .app
            .scene
            .rules
            .flags
            .iter()
            .find(|f| f.name == quarter::GUARD_FLAG)
            .ok_or("Нет флага доступа.")?
            .id;
        if d.app.rule_session.flag(&d.app.scene.rules, access) != Some(false) {
            return Err("Посторонний ответ выдал допуск.".into());
        }
        if tour {
            d.settle(90)?;
        }
        d.key(Key::E)?;
        if tour {
            d.settle(150)?;
        }
        click(d, quarter::GUARD_REPLY)?;
        if d.app.rule_session.flag(&d.app.scene.rules, access) != Some(true) {
            return Err("Ответ Илье не дал допуск.".into());
        }
        if tour {
            d.settle(112)?;
        }
        walk(d, [3.63, -14.45])?;
        walk(d, OUTSIDE)?;
    }
    d.key(Key::E)?;
    d.settle(44)?;
    expect_stage(d, quest, Some(3))?;
    if !matches!(near(d), Some(Interaction::Door { opening: false, .. })) {
        return Err("После допуска створка не открылась.".into());
    }
    if !d.app.standalone {
        let trace = d
            .app
            .rule_session
            .trace()
            .next_back()
            .ok_or("Нет журнала правил.")?;
        if !matches!(trace.outcome, TraceOutcome::Fired { .. }) {
            return Err("В журнале отсутствует сработавшее правило открытия.".into());
        }
    }
    walk(d, BEHIND_WALL)?;
    d.settle(if tour { 120 } else { 20 })?;
    expect_stage(d, quest, Some(3))?;
    walk(d, OUTSIDE)?;
    walk(d, INSIDE)?;
    expect_stage(d, quest, Some(4))?;
    d.expect_hud(COMPLETE)?;
    d.expect_hud("Внутри")?;
    if tour {
        turn(d, -1.3, 0.10)?;
        d.settle(300)?;
    } else {
        d.settle(50)?;
    }
    Ok(())
}

fn approach_workshop(d: &mut Driver<'_, '_>) -> Result<(), String> {
    walk(d, [0.0, -13.45])?;
    walk(d, [3.63, -15.60])?;
    walk(d, OUTSIDE)
}

fn near(d: &Driver<'_, '_>) -> Option<Interaction> {
    d.app
        .scene
        .world
        .interaction_near(&d.app.interactions, d.app.game_position().to_array())
}

fn expect_locked(d: &Driver<'_, '_>) -> Result<(), String> {
    d.expect_hud("Дверь заперта")?;
    if matches!(near(d), Some(Interaction::RuleLockedDoor { .. })) {
        Ok(())
    } else {
        Err("Дверь должна оставаться запертой.".into())
    }
}

fn expect_stage(d: &Driver<'_, '_>, quest: u32, expected: Option<u16>) -> Result<(), String> {
    let stage = d.app.rule_session.quest_stage(quest);
    if stage == expected {
        Ok(())
    } else {
        Err(format!(
            "Ожидался этап {expected:?}, получен {stage:?}. Позиция {:?}.",
            d.app.game_position()
        ))
    }
}

fn click(d: &mut Driver<'_, '_>, label: &str) -> Result<(), String> {
    let position = d.point(label, 0)?;
    d.frame(proof_input::hover(position))?;
    d.frame(proof_input::click(position))?;
    d.settle(6)
}

pub(super) fn walk(d: &mut Driver<'_, '_>, [x, z]: [f32; 2]) -> Result<(), String> {
    let from = d.app.game_position();
    let steps = ((x - from.x).hypot(z - from.z) / 5.5).ceil().max(1.0) as u32;
    for step in 1..=steps {
        let amount = step as f32 / steps as f32;
        walk_segment(
            d,
            [
                from.x + (x - from.x) * amount,
                from.z + (z - from.z) * amount,
            ],
        )?;
    }
    Ok(())
}

fn walk_segment(d: &mut Driver<'_, '_>, [x, z]: [f32; 2]) -> Result<(), String> {
    let position = d.app.game_position();
    if (x - position.x).hypot(z - position.z) < 0.13 {
        return Ok(());
    }
    let at = [x, position.y, z];
    let try_point = |d: &Driver<'_, '_>| d.app.proof.as_ref()?.projection.as_ref()?.point(at);
    if d.app.view == View::Three && try_point(d).is_none() {
        turn(d, (position.x - x).atan2(position.z - z), 0.27)?;
    }
    let point =
        try_point(d).ok_or_else(|| format!("Цель движения вне кадра {at:?} из {position:?}."))?;
    d.frame(proof_input::click(point))?;
    for _ in 0..50 {
        d.settle(6)?;
        if d.app.walk_target.is_none() {
            break;
        }
    }
    let foot = d.app.game_position();
    if (foot.x - x).hypot(foot.z - z) > 0.33 {
        return Err(format!(
            "Герой не дошёл до ({x}, {z}). Текущая позиция {foot:?}."
        ));
    }
    d.settle(8)
}

pub(super) fn turn(d: &mut Driver<'_, '_>, yaw: f32, pitch: f32) -> Result<(), String> {
    if d.app.view != View::Three {
        return Ok(());
    }
    let rect = match d.app.proof.as_ref().and_then(|p| p.projection.as_ref()) {
        Some(super::Projection::World(_, rect)) => *rect,
        _ => return Err("Не найден вид 3D.".into()),
    };
    let diff = (yaw - d.app.camera.yaw + PI).rem_euclid(TAU) - PI;
    let offset = egui::vec2(-diff / 0.009, (pitch - d.app.camera.pitch) / 0.008);
    if offset.length() < 3.0 {
        return Ok(());
    }
    let from = rect.center() - offset * 0.5;
    d.frame(vec![
        Event::PointerMoved(from),
        proof_input::pointer_button(from, PointerButton::Primary, true),
    ])?;
    const STEPS: usize = 26;
    for step in 1..=STEPS {
        let at = from + offset * (step as f32 / STEPS as f32);
        d.frame(vec![Event::PointerMoved(at)])?;
    }
    d.frame(vec![proof_input::pointer_button(
        from + offset,
        PointerButton::Primary,
        false,
    )])?;
    d.settle(12)
}

fn close_up(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let yaw = d.app.avatar_heading.to_radians();
    turn(d, yaw, 0.19)?;
    zoom(d, 1.55)?;
    d.settle(240)?;
    turn(d, yaw + 0.72, 0.15)?;
    d.settle(105)?;
    zoom(d, Camera::default().distance)?;
    d.settle(100)
}

fn zoom(d: &mut Driver<'_, '_>, distance: f32) -> Result<(), String> {
    let rect = match d.app.proof.as_ref().and_then(|p| p.projection.as_ref()) {
        Some(super::Projection::World(_, rect)) => *rect,
        _ => return Err("Для крупного плана нужен вид 3D.".into()),
    };
    let delta = (d.app.camera.distance / distance).ln() / 0.003;
    d.frame(vec![
        Event::PointerMoved(rect.center()),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, delta),
            modifiers: Modifiers::NONE,
        },
    ])?;
    d.settle(50)
}
