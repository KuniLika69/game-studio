use std::{
    ffi::OsStr,
    fs,
    io::{BufWriter, Write},
    path::Path,
};

use eframe::egui::Key;

use super::{
    Driver, FPS, SIZE, Sink, append_player, io_error, quarter_proof,
    staged_proof::{move_and_play, panel_click, play_linden, reload},
    tactics_proof, top_proof,
};
use crate::{
    Studio, View, configure_context, game_export::GamePackage, offscreen::Offscreen,
    proof_video::Video, rules, viewport3d,
};

const WATCHKEEPER_QUEST: u32 = 7;
const WATCHKEEPER_ZONE: u32 = 17;
const WATCHKEEPER_ACCESS: u32 = 41;
const WATCHKEEPER_DECOY: u32 = 42;
const WATCHKEEPER_DOOR: u64 = 4_611_686_018_427_388_348;

pub(super) fn author(scene_pack: &Path, output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let output = output.canonicalize().map_err(io_error)?;
    let scene_pack = scene_pack.canonicalize().map_err(io_error)?;
    let exe = output.join("Watchkeeper.exe");
    let project = output.join("watchkeeper.robot.json");
    let runtime = output.join("watchkeeper-runtime.json");
    if [&exe, &project, &runtime].iter().any(|path| path.exists()) {
        return Err("Файлы проверки пакета механик уже существуют.".into());
    }
    let mut video = Video::start(&output.join("after.mp4"), SIZE[0], SIZE[1], FPS)?;
    {
        let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
        configure_context(display.context());
        let mut app = Studio::default();
        app.desk.recovery_root = Some(output.join("recovery"));
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(app, &mut display, &mut sink, Some((&scene_pack, &exe)));
        d.settle(6)?;
        panel_click(&mut d, "Открыть…")?;
        if !d.app.status.starts_with("Пакет механик JSON открыт") {
            return Err(d.app.status.clone());
        }
        watchkeeper_targets(&d.app)?;
        d.hold(2 * FPS)?;
        panel_click(&mut d, "Правила")?;
        d.click("Получен допуск")?;
        d.click("Открыть хранилище по пропуску")?;
        d.point("Открыть хранилище по пропуску", 0)?;
        d.hold(2 * FPS)?;
        file(&mut d, &project);
        panel_click(&mut d, "Сохранить как…")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        if !exe.is_file() || !project.is_file() {
            return Err(format!("Проект или EXE не записан. {}", d.app.status));
        }
        d.hold(FPS)?;
    }
    let saved = crate::scene::Scene::load(&project)?;
    let exported = GamePackage::read(&exe)?.ok_or("Нет игрового пакета в EXE смотрителя.")?;
    if saved.rules != exported.scene.rules
        || saved.story != exported.scene.story
        || saved.world != exported.scene.world
    {
        return Err("Экспорт изменил правила, квест или карту из проекта JSON.".into());
    }
    append_player(
        &mut video,
        &exe,
        &[
            OsStr::new("--background-mechanics-player"),
            runtime.as_os_str(),
        ],
    )?;
    video.finish()
}

pub(super) fn player(package: GamePackage, report: &Path) -> Result<(), String> {
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let app = Studio::from_package(package, display.context());
    let adapter = display.adapter_info().clone();
    let mut frames = BufWriter::new(std::io::stdout().lock());
    let mut sink = Sink::Stream(&mut frames);
    let mut d = Driver::new(app, &mut display, &mut sink, None);
    let [decoy, key, outside, checkpoint] = watchkeeper_targets(&d.app)?;
    d.settle(20)?;
    let before = d.phase;

    quarter_proof::walk(&mut d, decoy)?;
    d.key(Key::E)?;
    d.expect_hud("Подобран ключ «Пустой жетон».")?;
    if !d.app.interactions.has_key(WATCHKEEPER_DECOY)
        || d.app.interactions.has_key(WATCHKEEPER_ACCESS)
    {
        return Err("Неверный жетон не попал в инвентарь отдельным предметом.".into());
    }
    d.hold(FPS)?;
    quarter_proof::walk(&mut d, outside)?;
    d.key(Key::E)?;
    d.expect_hud("Дверь заперта. Нужен допуск.")?;
    watchkeeper_stage(&d.app, 0)?;
    watchkeeper_door(&d.app, false)?;
    d.hold(2 * FPS)?;

    quarter_proof::walk(&mut d, key)?;
    d.key(Key::E)?;
    d.expect_hud("Подобран ключ «Медный пропуск».")?;
    if !d.app.interactions.has_key(WATCHKEEPER_ACCESS) {
        return Err("Допуск не зарегистрирован как полученный из мира.".into());
    }
    watchkeeper_stage(&d.app, 1)?;
    d.hold(FPS)?;
    quarter_proof::walk(&mut d, outside)?;
    d.key(Key::E)?;
    d.settle(90)?;
    watchkeeper_stage(&d.app, 2)?;
    let open_fraction = watchkeeper_door(&d.app, true)?;
    d.hold(FPS)?;
    quarter_proof::walk(&mut d, checkpoint)?;
    watchkeeper_stage(&d.app, 3)?;
    let completed = "Выполнено · Маршрут смотрителя · Контрольная зона пройдена.";
    d.expect_hud(completed)?;
    let final_at = d.app.actor_position().to_array();
    d.hold(3 * FPS)?;
    let evidence = serde_json::json!({
        "route": "Маршрут смотрителя", "pid": std::process::id(), "gpu": adapter.name,
        "backend": format!("{:?}", adapter.backend), "input_actions":d.input_actions,
        "simulated_seconds": (d.phase - before) as f64 / 60.0,
        "states": [
            {"item_id": WATCHKEEPER_DECOY, "door_locked": true, "quest_stage": 0},
            {"item_id": WATCHKEEPER_ACCESS, "quest_stage": 1},
            {"door_id": WATCHKEEPER_DOOR, "open_fraction": open_fraction, "quest_stage": 2},
            {"actor_id": rules::PLAYER_ACTOR_ID, "zone_id": WATCHKEEPER_ZONE, "quest_stage": 3, "position": final_at, "hud": completed},
        ],
    });
    frames.flush().map_err(io_error)?;
    fs::write(
        report,
        serde_json::to_vec_pretty(&evidence).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

fn watchkeeper_targets(app: &Studio) -> Result<[[f32; 2]; 4], String> {
    let [decoy, pass] = [WATCHKEEPER_DECOY, WATCHKEEPER_ACCESS];
    let locate = |id| -> Result<[f32; 2], String> {
        let [x, _, z] = app
            .scene
            .world
            .key(id)
            .ok_or_else(|| format!("В пакете нет предмета {id}."))?
            .position;
        Ok([x, z + 0.27])
    };
    let outside = app
        .scene
        .world
        .door(WATCHKEEPER_DOOR)
        .ok_or("Нет двери хранилища в JSON-пакете.")?
        .interaction_position();
    let zone = app
        .scene
        .rules
        .zones
        .iter()
        .find(|zone| zone.id == WATCHKEEPER_ZONE)
        .ok_or("Нет контрольной зоны в JSON-пакете.")?;
    if app.scene.story.quest(WATCHKEEPER_QUEST).is_none() || app.scene.rules.rules.len() != 3 {
        return Err("Не загружены квест и три правила из JSON.".into());
    }
    Ok([
        locate(decoy)?,
        locate(pass)?,
        [outside[0], outside[2] + 0.70],
        [zone.position[0], zone.position[2]],
    ])
}

fn watchkeeper_stage(app: &Studio, expected: u16) -> Result<(), String> {
    let stage = app.rule_session.quest_stage(WATCHKEEPER_QUEST).unwrap_or(0);
    if stage != expected {
        return Err(format!("Этап квеста {stage}, ожидался {expected}."));
    }
    Ok(())
}

fn watchkeeper_door(app: &Studio, opened: bool) -> Result<f32, String> {
    let door = app
        .scene
        .world
        .door(WATCHKEEPER_DOOR)
        .ok_or("Нет двери хранилища.")?;
    let fraction = app.interactions.fraction(&door);
    let locked = app
        .scene
        .world
        .door_locked(&app.interactions, WATCHKEEPER_DOOR);
    if (opened && (locked || fraction < 0.98)) || (!opened && (!locked || fraction > 0.01)) {
        return Err(format!(
            "Состояние двери: заперта = {locked}, открыта на {fraction:.3}. Ожидалось открытие = {opened}."
        ));
    }
    Ok(fraction)
}

pub(super) fn record(
    output: &Path,
    releases: &Path,
    verification: &Path,
    atlas: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let stage = output.join("export-stage");
    fs::create_dir_all(&stage).map_err(io_error)?;
    let source_12 = verification.join("v0.12/room-key.robot.json");
    let source_13 = verification.join("v0.13/linden-lane.robot.json");
    let source_15 = verification.join("v0.15/linden-marks.robot.json");
    let library = verification.join("v0.14/two-marks.rules.json");
    let top_scene = output.join("terrace.robot.json");
    let linden_scene = output.join("linden-marks.robot.json");
    let tactical = output.join("Мост и дозор.robot.json");
    let gateway = output.join("gateway.robot.json");
    let check = output.join("terrace.check.json");
    let stage_linden = stage.join("LindenMarks.exe");
    let stage_top = stage.join("Terrace.exe");
    let stage_tactics = stage.join("BridgeWatch.exe");
    let stage_gateway = stage.join("Gateway.exe");
    let game_linden = releases.join("LindenMarks.exe");
    let game_top = releases.join("Terrace.exe");
    let game_tactics = releases.join("BridgeWatch.exe");
    let game_gateway = releases.join("Gateway.exe");
    let movie = output.join("stage-017.mp4");
    for path in [
        &top_scene,
        &linden_scene,
        &tactical,
        &gateway,
        &check,
        &stage_top,
        &stage_linden,
        &stage_tactics,
        &stage_gateway,
        &game_top,
        &game_linden,
        &game_tactics,
        &game_gateway,
        &movie,
    ] {
        if path.exists() {
            return Err(format!("Путь занят. {}", path.display()));
        }
    }

    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(&movie, SIZE[0], SIZE[1], FPS)?;
    let mut studio = Studio::default();
    studio.desk.recovery_root = Some(output.join("recovery-test"));
    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(
            studio,
            &mut display,
            &mut sink,
            Some((&source_12, &stage_linden)),
        );
        d.settle(6)?;
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene.world.keys.len() != 1 || !d.app.can_play() {
            return Err("Не открылась сцена 0.12.".into());
        }
        file(&mut d, &source_13);
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene.rules.rules.len() != 5 || !d.app.can_play() {
            return Err("Не открылась сцена 0.13.".into());
        }
        file(&mut d, &source_15);
        panel_click(&mut d, "Открыть…")?;
        file(&mut d, &linden_scene);
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        quarter_proof::fresh(&mut d, View::Three)?;
        play_linden(&mut d, true)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        studio = d.app;
    }
    move_and_play(
        &mut video,
        &mut display,
        &stage_linden,
        &game_linden,
        "linden",
        &output.join("timing-linden.json"),
    )?;
    eprintln!("0.17: исходные сцены открылись, 3D-квест прошёл в редакторе и EXE.");

    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(
            studio,
            &mut display,
            &mut sink,
            Some((&top_scene, &stage_top)),
        );
        d.settle(6)?;
        top_proof::author_top(&mut d, atlas, &library, true)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        regression(&mut d, &check)?;
        panel_click(&mut d, "Сохранить")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        studio = d.app;
    }
    move_and_play(
        &mut video,
        &mut display,
        &stage_top,
        &game_top,
        "top",
        &output.join("timing-top.json"),
    )?;
    eprintln!(
        "0.17: запись проверки, первое расхождение после правки и успешный повтор подтверждены."
    );

    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(
            studio,
            &mut display,
            &mut sink,
            Some((&tactical, &stage_tactics)),
        );
        d.settle(6)?;
        tactics_proof::author(&mut d, atlas)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "▶ Играть")?;
        tactics_proof::play(&mut d, true)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        studio = d.app;
    }
    move_and_play(
        &mut video,
        &mut display,
        &stage_tactics,
        &game_tactics,
        "tactics",
        &output.join("timing-tactics.json"),
    )?;
    eprintln!("0.17: тактика из пустой карты пройдена в редакторе и перенесённой сборке.");

    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(
            studio,
            &mut display,
            &mut sink,
            Some((&gateway, &stage_gateway)),
        );
        prove_gateway(&mut d, &tactical)?;
    }
    move_and_play(
        &mut video,
        &mut display,
        &stage_gateway,
        &game_gateway,
        "gateway",
        &output.join("timing-gateway.json"),
    )?;
    video.finish()?;
    eprintln!("0.17: переход 3D → тактика, сохранение перехода и автономный экспорт подтверждены.");
    Ok(())
}

pub(super) fn smoke_gateway(output: &Path, tactical: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut sink = Sink::None;
    let mut studio = Studio::default();
    studio.desk.recovery_root = Some(output.join("recovery-test"));
    let mut d = Driver::new(
        studio,
        &mut display,
        &mut sink,
        Some((
            &output.join("gateway.robot.json"),
            &output.join("Gateway.exe"),
        )),
    );
    prove_gateway(&mut d, tactical)
}

fn prove_gateway(d: &mut Driver<'_, '_>, tactical: &Path) -> Result<(), String> {
    d.settle(6)?;
    author_gateway(d, tactical)?;
    panel_click(d, "Сохранить как…")?;
    reload(d)?;
    panel_click(d, "▶ Играть")?;
    enter_gateway(d)?;
    panel_click(d, "■ Стоп")?;
    if d.app.scene.map_2d.is_some()
        || d.app
            .scene
            .checkpoint
            .as_ref()
            .and_then(|checkpoint| checkpoint.scene_id)
            != Some(2)
    {
        return Err(
            "После выхода не восстановлен редактируемый корень с контрольной точкой перехода."
                .into(),
        );
    }
    panel_click(d, "Сохранить")?;
    reload(d)?;
    panel_click(d, "Продолжить")?;
    if d.app.tactics_battle.is_none() {
        return Err("Продолжение не открыло связанную сцену.".into());
    }
    panel_click(d, "■ Стоп")?;
    panel_click(d, "Собрать игру для Windows")
}

fn file(d: &mut Driver<'_, '_>, path: &Path) {
    d.app.proof.as_mut().unwrap().scene = Some(path.to_owned());
}

fn regression(d: &mut Driver<'_, '_>, path: &Path) -> Result<(), String> {
    d.click("Проверки…")?;
    d.edit("Имя проверки", 0, "Две отметки, таймер и финиш")?;
    d.click("Записать проверку")?;
    top_proof::play(d, true)?;
    panel_click(d, "■ Стоп")?;
    d.expect_hud("Записано 2 ожиданий")?;
    d.edit("Файл проверки .check.json", 0, &path.to_string_lossy())?;
    d.click("Сохранить проверку")?;
    d.expect_hud("Проверка сохранена")?;
    d.click("Повторить проверку с начала")?;
    wait_for_result(d, "Проверка пройдена")?;
    d.click("Проверки…")?;

    d.click("Поиск…")?;
    d.edit("Имя объекта, правила или ресурса", 0, "Первая отметка")?;
    d.click_n("Первая отметка", usize::MAX)?;
    d.click("Включено")?;
    if d.app
        .scene
        .rules
        .rules
        .iter()
        .find(|rule| rule.name == "Первая отметка")
        .is_none_or(|rule| rule.enabled)
    {
        return Err("Пробная правка не отключила правило первой отметки.".into());
    }
    d.click("Проверки…")?;
    d.click("Повторить проверку с начала")?;
    wait_for_result(d, "Первое расхождение · Отметка 1")?;
    if !d
        .app
        .playback
        .outcomes()
        .iter()
        .any(|line| line.contains("ожидалось 1, получено 0"))
    {
        return Err(format!(
            "Отчёт не показал счётчик 1→0. {:?}",
            d.app.playback.outcomes()
        ));
    }
    d.hold(42)?;
    d.click("Проверки…")?;
    d.click("Отменить")?;
    d.click("Проверки…")?;
    d.click("Открыть проверку")?;
    d.click("Повторить проверку с начала")?;
    wait_for_result(d, "Проверка пройдена")?;
    d.hold(32)?;
    d.click("Проверки…")
}

fn wait_for_result(d: &mut Driver<'_, '_>, expected: &str) -> Result<(), String> {
    for _ in 0..460 {
        if !d.app.game_running {
            break;
        }
        d.settle(1)?;
    }
    if !d
        .app
        .playback
        .outcomes()
        .iter()
        .any(|line| line.contains(expected))
    {
        return Err(format!(
            "Ожидался результат «{expected}». {:?}",
            d.app.playback.outcomes()
        ));
    }
    d.hold(45)
}

fn author_gateway(d: &mut Driver<'_, '_>, tactical: &Path) -> Result<(), String> {
    d.click("Новый…")?;
    d.click("Пустая сцена")?;
    d.click("Рыцарь")?;
    d.click("Проект")?;
    d.click("Управление и движение…")?;
    if d.point("F", 0).is_err() {
        d.click("Клавиши")?;
    }
    if d.point("F", 0).is_err() {
        let at = d
            .point("Влево", 0)
            .or_else(|_| d.point("Взаимодействие", 0))?;
        top_proof::wheel(d, at, -255.0)?;
    }
    d.click("F")?;
    d.key(Key::G)?;
    if crate::play_settings::label(&d.app.scene.play_settings.controls, crate::play_settings::Action::Mark) != "G" {
        return Err("Редактор не применил клавишу G для игровой отметки.".into());
    }
    d.key(Key::Escape)?;
    d.click("Правила")?;
    d.click("Связанные сцены")?;
    d.edit("Путь к .robot.json", 0, &tactical.to_string_lossy())?;
    d.click("Присоединить сцену")?;
    d.click("Связанные сцены")?;
    top_proof::new_rule(d, "Выйти к мосту", "Игровое действие")?;
    d.click("+ Действие")?;
    d.click_n("Перезапустить попытку", usize::MAX)?;
    top_proof::click_action_kind(d, "Перейти в сцену")?;
    if d.point("Выбрать", 0).is_ok() {
        d.click("Выбрать")?;
    } else {
        d.click_n("Исходная сцена", usize::MAX)?;
    }
    d.click_n("Мост и дозор", usize::MAX)?;
    if !d
        .app
        .scene
        .rules
        .rules
        .last()
        .is_some_and(|rule| rule.actions == [rules::Action::Transition { scene_id: 2 }])
    {
        return Err("В редакторе не назначен точный переход на тактическую сцену.".into());
    }
    Ok(())
}

pub(super) fn enter_gateway(d: &mut Driver<'_, '_>) -> Result<(), String> {
    if d.app.scene.map_2d.is_some() || !d.app.game_running {
        return Err("Переход ожидается из работающей исходной 3D-сцены.".into());
    }
    d.hold(30)?;
    d.key(Key::F)?;
    if d.app.scene.map_2d.is_some() {
        return Err("Старая клавиша F осталась привязана к переходу.".into());
    }
    d.key(Key::G)?;
    d.settle(12)?;
    if d.app.tactics_battle.is_none()
        || d.app.link_playthrough.as_ref().map(|play| play.active) != Some(2)
    {
        return Err("Переназначенная G не открыла связанный тактический уровень.".into());
    }
    d.expect_hud("Раунд 1")
}
