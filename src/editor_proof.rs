use std::{ffi::OsStr, fs, path::Path, time::Instant};

use eframe::egui::{Event, Key, PointerButton};
use serde_json::json;

use super::{
    Driver, FPS, SIZE, Sink, io_error, map2d_tools_proof, object_portability_proof,
    staged_proof::{move_and_play, panel_click, reload},
    top_proof, visual_proof,
};
use crate::{
    EditorTab, Studio, configure_context, game_export, offscreen::Offscreen, proof_input,
    proof_video::Video, quality::Profile, viewport3d,
};

pub(super) fn record(
    output: &Path,
    releases: &Path,
    verification: &Path,
    atlas: &Path,
    chair: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let stage = output.join("export-stage");
    let recovery = output.join("recovery-test");
    fs::create_dir_all(&stage).map_err(io_error)?;
    let legacy_12 = verification.join("v0.12/room-key.robot.json");
    let legacy_13 = verification.join("v0.13/linden-lane.robot.json");
    let legacy_14 = verification.join("v0.14/linden-marks.robot.json");
    let legacy_15 = verification.join("v0.15/terrace.robot.json");
    let library = verification.join("v0.14/two-marks.rules.json");
    let source_scene = output.join("seat-source.robot.json");
    let seat_scene = output.join("seat-workshop.robot.json");
    let top_scene = output.join("terrace.robot.json");
    let object = output.join("workshop-bench.studio-object");
    let invalid_model = output.join("invalid-model.glb");
    let stage_seat = stage.join("SeatWorkshop.exe");
    let stage_terrace = stage.join("Terrace.exe");
    let seat_exe = releases.join("SeatWorkshop.exe");
    let top_exe = releases.join("Terrace.exe");
    let movie_path = output.join("stage-016.mp4");
    for path in [
        &source_scene,
        &seat_scene,
        &top_scene,
        &object,
        &invalid_model,
        &stage_seat,
        &stage_terrace,
        &seat_exe,
        &top_exe,
        &movie_path,
    ] {
        if path.exists() {
            return Err(format!("Путь занят. {}", path.display()));
        }
    }

    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(&movie_path, SIZE[0], SIZE[1], FPS)?;
    let mut app = Studio::default();
    app.desk.recovery_root = Some(recovery.clone());
    let seat_exercise;
    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((&legacy_12, &stage_seat)),
        );
        d.settle(5)?;
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene.world.keys.len() != 1 || d.app.scene.checkpoint.is_none() {
            return Err("Сцена 0.12 не загрузилась.".into());
        }
        set_file(&mut d, &legacy_13);
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene.rules.rules.len() != 5 || d.app.scene.world.buildings.len() != 12 {
            return Err("Сцена 0.13 не загрузилась.".into());
        }
        set_file(&mut d, &legacy_15);
        panel_click(&mut d, "Открыть…")?;
        if d.app
            .scene
            .map_2d
            .as_ref()
            .is_none_or(|map| map.atlases.len() < 3)
        {
            return Err("Карта 0.15 не загрузилась.".into());
        }
        set_file(&mut d, &legacy_14);
        panel_click(&mut d, "Открыть…")?;
        visual_proof::author(&mut d, chair)?;
        let find_seat = |app: &Studio| {
            app.scene
                .world
                .models
                .iter()
                .find(|item| item.label == "Скамья для проверки")
                .cloned()
        };
        let updated = find_seat(&d.app).ok_or("Нет заменённой скамьи.")?;
        fs::write(&invalid_model, b"invalid GLB").map_err(io_error)?;
        d.app.proof.as_mut().unwrap().import_glb = Some(invalid_model);
        panel_click(&mut d, "Заменить модель GLB…")?;
        d.expect_hud("Не удалось заменить GLB")?;
        if find_seat(&d.app).as_ref() != Some(&updated) {
            return Err("Повреждённый GLB затронул действующую модель.".into());
        }
        d.click("Отменить")?;
        let restored = find_seat(&d.app).ok_or("Отмена замены удалила скамью.")?;
        if restored.source == updated.source
            || restored.seat != updated.seat
            || restored.collider != updated.collider
            || d.app.assets.get(&restored.source).is_none()
        {
            return Err("Отмена замены не восстановила GLB и функции скамьи.".into());
        }
        d.click("Повторить")?;
        if find_seat(&d.app).as_ref() != Some(&updated)
            || d.app.assets.get(&updated.source).is_none()
        {
            return Err("Повтор замены не восстановил другой GLB.".into());
        }
        object_portability_proof::save_selected(&mut d, &object)?;
        set_file(&mut d, &source_scene);
        panel_click(&mut d, "Сохранить как…")?;
        let started = Instant::now();
        let start_actions = d.input_actions;
        object_portability_proof::reinsert(&mut d, &object)?;
        d.click("Персонаж")?;
        d.click("Рыцарь")?;
        if !d.app.can_play() {
            return Err("Герой не готов к запуску.".into());
        }
        set_file(&mut d, &seat_scene);
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        d.click("Поиск…")?;
        d.settle(6)?;
        d.edit("Имя объекта, правила или ресурса", 0, "скамья для проверки")?;
        d.click("Скамья для проверки")?;
        if d.app.editor_tab != EditorTab::World
            || d.app.world_selection != Some(d.app.scene.world.models[0].id)
        {
            return Err("Результат поиска не открыл инспектор скамьи.".into());
        }
        gizmos(&mut d)?;
        panel_click(&mut d, "▶ Играть")?;
        visual_proof::sit(&mut d)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        if !stage_seat.is_file() {
            return Err(d.app.status.clone());
        }
        seat_exercise = json!({"seconds": started.elapsed().as_secs_f64(), "input_presses": d.input_actions - start_actions});
        app = d.app;
    }
    move_and_play(
        &mut video,
        &mut display,
        &stage_seat,
        &seat_exe,
        "seat",
        &output.join("timings-seat.json"),
    )?;
    eprintln!("0.16: заменённый GLB, история, поиск и перенос скамьи пройдены.");

    let top_exercise;
    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((&top_scene, &stage_terrace)),
        );
        d.settle(5)?;
        map2d_tools_proof::author(&mut d)?;
        recover(&mut d, &recovery)?;
        diagnostics(&mut d)?;
        d.click("Новый…")?;
        d.click("Продолжить без сохранения")?;
        d.click("Демо-робот")?;
        d.settle(8)?;
        let started = Instant::now();
        let start_actions = d.input_actions;
        top_proof::author_top(&mut d, atlas, &library, true)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "▶ Играть")?;
        top_proof::play(&mut d, true)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        if !stage_terrace.is_file() {
            return Err(d.app.status.clone());
        }
        top_exercise = json!({"seconds": started.elapsed().as_secs_f64(), "input_presses": d.input_actions - start_actions});
    }
    move_and_play(
        &mut video,
        &mut display,
        &stage_terrace,
        &top_exe,
        "top",
        &output.join("timings-top.json"),
    )?;
    video.finish()?;
    write_json(
        &output.join("exercise.json"),
        json!({"method":"continuous automated UI exercise, wall time; native file dialogs use preselected paths", "seat_workshop":seat_exercise, "puzzle_top":top_exercise}),
    )?;
    eprintln!(
        "0.16: кисть и буфер, восстановление, диагностика, пакет правил и отдельная головоломка пройдены."
    );
    Ok(())
}

fn set_file(d: &mut Driver<'_, '_>, scene: &Path) {
    d.app.proof.as_mut().unwrap().scene = Some(scene.into());
}

fn gizmos(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let original = d.app.scene.world.clone();
    for (mode, axis) in [("Переместить", 0), ("Вращать", 1), ("Масштабировать", 3)]
    {
        d.click(mode)?;
        drag_gizmo(d, axis)?;
        let changed = d.app.scene.world.clone();
        let (a, b) = (&original.models[0], &changed.models[0]);
        let edited = match axis {
            0 => {
                a.position != b.position
                    && (b.position[0] * 2.0 - (b.position[0] * 2.0).round()).abs() < 0.001
            }
            1 => a.rotation != b.rotation,
            _ => a.scale != b.scale,
        };
        if !edited {
            return Err(format!("Манипулятор «{mode}» не применил жест."));
        }
        d.click("Отменить")?;
        if d.app.scene.world != original {
            return Err(format!("Одно действие не отменило весь жест «{mode}»."));
        }
        d.click("Повторить")?;
        if d.app.scene.world != changed {
            return Err(format!("Повтор не вернул жест «{mode}»."));
        }
        d.click("Отменить")?;
    }
    d.click("Переместить")?;
    d.click("Дублировать")?;
    let world = &d.app.scene.world;
    if world.models.len() != 2
        || world.models[0].id == world.models[1].id
        || world.models[0].seat != world.models[1].seat
    {
        return Err("Дублирование потеряло модель или её точку посадки.".into());
    }
    drag_gizmo(d, 0)?;
    drag_gizmo(d, 0)?;
    let before_group = d.app.scene.world.clone();
    let bench = &before_group.models[0];
    let copy = &before_group.models[1];
    let point = d.point(&bench.label, 0)?;
    let mods = eframe::egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    d.frame(proof_input::hover(point))?;
    d.frame(
        [true, false]
            .map(|pressed| Event::PointerButton {
                pos: point,
                button: PointerButton::Primary,
                pressed,
                modifiers: mods,
            })
            .into(),
    )?;
    d.settle(4)?;
    if d.app.gizmo.selection() != [copy.id, bench.id] {
        return Err(format!(
            "Ctrl+клик не добавил вторую модель к выделению: {:?}.",
            d.app.gizmo.selection()
        ));
    }
    drag_gizmo(d, 0)?;
    let after_group = &d.app.scene.world.models;
    let dx = after_group[0].position[0] - bench.position[0];
    let other_dx = after_group[1].position[0] - copy.position[0];
    if dx.abs() < 0.1 || (dx - other_dx).abs() > 0.001 {
        return Err("Модели выделенной группы не переместились вместе.".into());
    }
    d.click("Отменить")?;
    if d.app.scene.world != before_group {
        return Err("Отмена группы не восстановила обе модели.".into());
    }
    d.click("Отменить")?;
    d.click("Отменить")?;
    d.click("Отменить")?;
    if d.app.scene.world != original {
        return Err("Одна отмена не сняла дублирование.".into());
    }
    Ok(())
}

fn drag_gizmo(d: &mut Driver<'_, '_>, axis: usize) -> Result<(), String> {
    let Some(super::Projection::World(camera, rect)) =
        d.app.proof.as_ref().and_then(|env| env.projection.as_ref())
    else {
        return Err("Нет 3D-проекции для проверки манипулятора.".into());
    };
    let (from, direction) = d
        .app
        .gizmo
        .axis_handle(&d.app.scene.world, *camera, *rect, axis)
        .ok_or_else(|| format!("Ось {axis} манипулятора вне кадра."))?;
    d.frame(proof_input::hover(from))?;
    d.frame(vec![proof_input::pointer_button(
        from,
        PointerButton::Primary,
        true,
    )])?;
    for step in 1..=6 {
        d.frame(vec![Event::PointerMoved(
            from + direction * (8.0 * step as f32),
        )])?;
    }
    let to = from + direction * 48.0;
    d.frame(vec![proof_input::pointer_button(
        to,
        PointerButton::Primary,
        false,
    )])?;
    d.settle(5)
}

fn recover(d: &mut Driver<'_, '_>, root: &Path) -> Result<(), String> {
    let recorded = d.app.scene.clone();
    if !d.app.desk.dirty() {
        return Err("Нет индикатора правок после работы кистью.".into());
    }
    d.close_request = true;
    d.frame(vec![])?;
    if !d.display.last_output().is_some_and(|output| {
        output.viewport_output.values().any(|viewport| {
            viewport
                .commands
                .contains(&eframe::egui::ViewportCommand::CancelClose)
        })
    }) {
        return Err("Редактор не остановил закрытие окна с правками.".into());
    }
    d.settle(5)?;
    d.click("Отмена")?;
    if d.app.desk.replace.is_some() || d.app.scene != recorded {
        return Err("Отмена закрытия потеряла правки.".into());
    }
    if let Some(after) = d.app.desk.remaining() {
        std::thread::sleep(after + std::time::Duration::from_millis(20));
    }
    d.settle(2)?;
    let context = d.app.proof.take();
    d.app = Studio::default();
    d.app.proof = context;
    d.app.desk.recovery_root = Some(root.to_path_buf());
    d.settle(8)?;
    d.click("Проект")?;
    d.click("Восстановить автосохранение…")?;
    d.click("Восстановить")?;
    if d.app.scene != recorded || !d.app.desk.dirty() {
        return Err("После восстановления загружена другая карта.".into());
    }
    Ok(())
}

fn diagnostics(d: &mut Driver<'_, '_>) -> Result<(), String> {
    d.click("Правила")?;
    d.click("+ Правило")?;
    let last = d
        .app
        .scene
        .rules
        .rules
        .last()
        .ok_or("Правило не создано.")?;
    let name = last.name.clone();
    d.click("Персонаж")?;
    d.click("Поиск…")?;
    d.settle(4)?;
    d.click("Только ошибки (1)")?;
    d.click(&name)?;
    if d.app.editor_tab != EditorTab::Rules || d.point("Когда", 0).is_err() {
        return Err("Диагностика не открыла правило.".into());
    }
    d.click("Отменить")?;
    if !d.app.scene.rules.rules.is_empty() {
        return Err("Временная ошибка осталась в сцене.".into());
    }
    d.key(Key::F10)?;
    d.point("Графика и производительность", 0)?;
    d.key(Key::F10)
}

pub(super) fn measure(
    game: game_export::GamePackage,
    route: &OsStr,
    report: &Path,
) -> Result<(), String> {
    let name = route.to_str().ok_or("Маршрут замера указан некорректно.")?;
    let (name, profile, size) = name
        .strip_suffix("-high")
        .map_or((name, Profile::Low, [1920, 1080]), |name| {
            (name, Profile::High, SIZE)
        });
    let samples = profile.samples();
    let mut display = Offscreen::new(size, samples)?;
    configure_context(display.context());
    let mut studio = Studio::from_package(game, display.context());
    studio.low_fx = profile == Profile::Low;
    let mut sink = Sink::None;
    let mut d = Driver::new(studio, &mut display, &mut sink, None);
    d.input = proof_input::Driver::new(size);
    d.settle(80)?;
    d.display.start_timing();
    match name {
        "delta23" => super::games23_proof::play_delta(&mut d)?,
        "lens23" => super::games23_proof::play_lens(&mut d)?,
        "signal-trail" => super::wizard_proof::play(&mut d)?,
        "review" => super::review_stage_proof::play_review(&mut d)?,
        "dungeon" => { super::dungeon_proof::play(&mut d)?; }
        "seat" => visual_proof::sit(&mut d)?,
        "top" => top_proof::play(&mut d, false)?,
        "linden" => {
            if d.app
                .scene
                .world
                .models
                .iter()
                .any(|item| item.seat.is_some())
            {
                visual_proof::sit(&mut d)?;
            }
            super::staged_proof::play_linden(&mut d, false)?;
        }
        "side" => super::side_proof::play(&mut d, false)?,
        "tactics" => super::tactics_proof::play(&mut d, false)?,
        "mill" => super::mill_walk_proof::play(&mut d, false)?,
        "river" => super::environment_proof::play_river(&mut d)?,
        "visual" => super::visual_stage_proof::play_city(&mut d, false, None)?,
        "cast" => super::character_stage_proof::play_cast(&mut d, false)?,
        "fx2d" => super::visual_stage_proof::play_map(&mut d)?,
        _ => return Err("Неизвестный маршрут замера.".into()),
    }
    let adapter = d.display.adapter_info().clone();
    let mut values = d.display.stop_timing();
    if values.is_empty() {
        return Err("Нет данных о времени кадра.".into());
    }
    values.sort_by(f32::total_cmp);
    let q = |p: f32| values[((values.len() - 1) as f32 * p).ceil() as usize];
    write_json(
        report,
        json!({"route":name, "gpu":adapter.name, "backend":format!("{:?}", adapter.backend), "driver":adapter.driver_info,
        "resolution":size, "msaa":samples, "shadow_map":viewport3d::shadow_size(samples), "profile":profile.name(), "frames":values.len(),
        "mean_ms":values.iter().sum::<f32>() / values.len() as f32, "p50_ms":q(0.50), "p95_ms":q(0.95), "p99_ms":q(0.99),
        "method":"offscreen frame through GPU sync and RGB readback; encoder and pipe excluded"}),
    )
}

pub(super) fn benchmark_export(from: &Path, to: &Path) -> Result<(), String> {
    let scene = crate::scene::Scene::load(from)?;
    let mut assets = crate::assets::Library::default();
    for (source, _) in crate::asset_sources(&scene) {
        assets.ensure(source, Some(from))?;
    }
    game_export::export(to, &scene, &assets, false)
}

fn write_json(path: &Path, value: serde_json::Value) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(&value).map_err(|err| err.to_string())?,
    )
    .map_err(io_error)
}
