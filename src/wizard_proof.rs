use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use eframe::egui::Key;
use serde_json::{Value, json};

use super::{
    Driver, FPS, SIZE, Sink, io_error, proof_chat,
    staged_proof::{move_and_play, number, panel_click, reload},
    top_proof,
};
use crate::{
    Studio, configure_context,
    game_plan::{self, Format, Plan},
    offscreen::Offscreen,
    proof_video::Video,
    rules::{Comparison, Condition},
    studio_welcome::Page,
    viewport3d,
};

const AUTHOR: &str = "Хочу короткую игру про обход сигнальных маяков.";
const ANSWER: &str =
    "2D сверху, WASD, герой Смотрительница, два сигнала за 14 секунд. R начинает новую попытку.";
const CHANGE: &str = "Измени условие победы у выходного маяка. Теперь достаточно одного сигнала.";
const WORKSHOP: &str = "Мастерская после дождя, холодная улица и тёплый интерьер. Сделай короткий одиночный 3D-квест. Управление WASD, герой Алекс, собеседник мастер Лида. Хочу поговорить с Лидой, открыть дверь и занести инструмент. Покажи три варианта света и расстановки.";
const WORKSHOP_CHANGE: &str = "Отключи ровно одно существующее правило «Отметка на верстаке», чтобы проверка заметила исчезновение финиша. Верни только операцию set_rule_enabled.";
const INPUT: &str = "Опиши игру или ответь на вопрос";
const CHECK: &str = "Файл проверки .check.json";
const MODEL: &str = "wizard-fixture";

pub(super) fn record(output: &Path, releases: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let staging = output.join("export-stage");
    fs::create_dir_all(&staging).map_err(io_error)?;
    let stage = staging.join("SignalTrail.exe");
    let target = releases.join("SignalTrail.exe");
    let scene = output.join("signal-trail.robot.json");
    let check = output.join("signal-trail.check.json");
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(&output.join("wizard-022.mp4"), SIZE[0], SIZE[1], FPS)?;
    {
        let mut sink = Sink::Video(&mut video);
        let app = Studio {
            welcome: Some(Page::Home),
            ..Studio::default()
        };
        let mut d = Driver::new(app, &mut display, &mut sink, Some((&scene, &stage)));
        d.app.desk.recovery_root = Some(output.join("wizard-recovery"));
        d.settle(6)?;
        d.click("Создать проект")?;
        d.click("2D сверху")?;
        let blank = d.app.scene.clone();
        let (endpoint, server) = proof_chat::serve(3, wizard_reply)?;
        d.click("Мастер игры")?;
        connect(&mut d, &endpoint, MODEL)?;
        ask(&mut d, AUTHOR)?;
        d.settle(12)?;
        d.expect_hud("Какой вид камеры выбираешь")?;
        d.hold(24)?;
        ask(&mut d, ANSWER)?;
        await_plan(&mut d, false)?;
        right_click(&mut d, "Собрать по плану")?;
        if d.app
            .scene
            .map_2d
            .as_ref()
            .is_none_or(|map| map.mode != crate::map2d::Mode::Top)
            || threshold(&d.app) != Some(2)
            || d.app.scene == blank
        {
            return Err(format!(
                "Не собран проект с двумя сигналами. {}",
                d.app.status
            ));
        }
        d.click("Отменить")?;
        if d.app.scene != blank {
            return Err("Мастер не вернул пустую сцену одним шагом отмены.".into());
        }
        d.click("Повторить")?;
        panel_click(&mut d, "▶ Играть")?;
        timeout(&mut d)?;
        panel_click(&mut d, "■ Стоп")?;
        record_check(&mut d, &check)?;
        author_change(&mut d)?;
        server
            .join()
            .map_err(|_| "Не завершён проверочный API.")??;
        if threshold(&d.app) != Some(1) {
            return Err("Принятая правка не изменила порог.".into());
        }
        d.click("Отменить")?;
        if threshold(&d.app) != Some(2) {
            return Err("Отмена не вернула два сигнала.".into());
        }
        d.click("Повторить")?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        d.click("Собрать игру для Windows")?;
    }
    move_and_play(
        &mut video,
        &mut display,
        &stage,
        &target,
        "signal-trail",
        &output.join("timing-signal-video.json"),
    )?;
    video.finish()?;
    eprintln!(
        "Мастер игры: два хода диалога, редактор плана, отмена сборки, поражение и повтор. Ранняя победа по принятой правке проверена в перенесённом EXE."
    );
    Ok(())
}

pub(super) fn play(d: &mut Driver<'_, '_>) -> Result<(), String> {
    if threshold(&d.app) != Some(1) {
        return Err("Для отдельной игры ожидался принятый порог в один сигнал.".into());
    }
    mark(d, 7.5)?;
    mark(d, 7.5)?;
    if count(&d.app, game_plan::SIGNALS) != Some(1) {
        return Err("Один сигнал можно начислить повторно.".into());
    }
    mark(d, 22.5)?;
    require_victory(d, true)?;
    d.hold(55)
}

pub(super) fn blank_manual(output: &Path, scale: f32) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let stage = output.join("stage");
    let moved = output.join("moved");
    for path in [&stage, &moved] {
        fs::create_dir_all(path).map_err(io_error)?;
    }
    let scene = output.join("manual-trail.robot.json");
    let export = stage.join("ManualTrail.exe");
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut movie = Video::start(&output.join("blank-to-exe.mp4"), SIZE[0], SIZE[1], FPS)?;
    {
        let mut sink = Sink::Video(&mut movie);
        let app = Studio {
            welcome: Some(Page::Home),
            ..Studio::default()
        };
        let mut d = Driver::new(app, &mut display, &mut sink, Some((&scene, &export)));
        d.input = crate::proof_input::Driver::with_scale(SIZE, scale);
        d.app.desk.recovery_root = Some(output.join("manual-recovery"));
        d.settle(6)?;
        if (d.display.context().pixels_per_point() - scale).abs() > 0.005 {
            return Err("Не применился запрошенный масштаб.".into());
        }
        d.click("Создать проект")?;
        d.click("2D сверху")?;
        let blank = d.app.scene.clone();
        d.click("Мастер игры")?;
        d.click("Настроить план вручную")?;
        number(&mut d, "Сигналов для победы ", "1")?;
        right_click(&mut d, "Собрать по плану")?;
        if threshold(&d.app) != Some(1) || d.app.scene == blank {
            return Err("Ручная сборка не создала настроенную игру.".into());
        }
        d.click("Отменить")?;
        if d.app.scene != blank {
            return Err("Undo не вернул пустой проект.".into());
        }
        d.click("Повторить")?;
        panel_click(&mut d, "▶ Играть")?;
        play(&mut d)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        fs::write(
            output.join("interface.json"),
            serde_json::to_vec_pretty(&json!({
                "scale": scale, "automated": true, "input_actions": d.input_actions,
                "network_model_calls": 0, "undo_redo": true, "save_reload": true
            }))
            .map_err(|error| error.to_string())?,
        )
        .map_err(io_error)?;
    }
    move_and_play(
        &mut movie,
        &mut display,
        &export,
        &moved.join("ManualTrail.exe"),
        "signal-trail",
        &output.join("timing.json"),
    )?;
    movie.finish()
}

pub(super) fn real_model(endpoint: &str, model: &str, output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut sink = Sink::None;
    let mut app = Studio::default();
    app.set_scene(crate::Scene::empty(), None, "");
    app.desk.recovery_root = Some(output.join("model-recovery"));
    let file = output.join("workshop-amber.robot.json");
    let unused = output.join("unused.exe");
    let mut d = Driver::new(app, &mut display, &mut sink, Some((&file, &unused)));
    d.settle(6)?;
    d.click("Мастер игры")?;
    connect(&mut d, endpoint, model)?;
    ask(&mut d, WORKSHOP)?;
    wait_turn(&mut d, true)?;
    if d.app.wizard.ready_plan().is_none() && d.app.wizard.notice().is_empty() {
        ask(
            &mut d,
            "Выбираю янтарный навес. Оставь исходную задачу и WASD. Подготовь итоговый план workshop3d.",
        )?;
    }
    await_plan(&mut d, true)?;
    let plan = d.app.wizard.ready_plan().ok_or("Модель не вернула план.")?;
    if plan.format != Format::Workshop3d || plan.title.is_empty() {
        return Err(format!("Модель выбрала другой формат. {plan:?}"));
    }
    fs::write(
        output.join("workshop-model-plan.json"),
        serde_json::to_vec_pretty(plan).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)?;
    for label in [
        "Синий час · Маршрут вдоль лужи",
        "Янтарный навес · Подход сбоку",
        "Утренний туман · Открытый проход",
    ] {
        if d.point(label, 0).is_err() {
            top_proof::scroll_panel(&mut d, -220.0)?;
        }
        d.point(label, 0)?;
    }
    capture_png(&d, &output.join("workshop-model-plan.png"))?;
    right_click(&mut d, "Янтарный навес · Подход сбоку")?;
    right_click(&mut d, "Собрать по плану")?;
    if d.app.scene.avatar.is_none()
        || d.app.scene.npcs.len() != 1
        || d.app.scene.world.buildings.is_empty()
    {
        return Err("Выбранный план не создал редактируемую 3D сцену.".into());
    }
    capture_png(&d, &output.join("workshop-amber-editor.png"))?;
    change_workshop_material(&mut d, output)?;
    check_workshop_rule(&mut d, output)?;
    panel_click(&mut d, "Сохранить как…")?;
    reload(&mut d)?;
    eprintln!(
        "Модель предложила план 3D и правку правила. Собран янтарный вариант, материал изменён вручную. Изменение проверено на маршруте, принято и отменено."
    );
    Ok(())
}

fn change_workshop_material(d: &mut Driver<'_, '_>, output: &Path) -> Result<(), String> {
    let lamp = "Фонарь у входа";
    d.click("Поиск…")?;
    d.edit("Имя объекта, правила или ресурса", 0, lamp)?;
    d.click_n(lamp, usize::MAX)?;
    right_click(d, "Материалы GLB")?;
    if d.display
        .last_output()
        .is_some_and(|out| crate::proof_input::find_label_containing(out, "Свечение ", 0).is_none())
    {
        top_proof::scroll_panel(d, -220.0)?;
    }
    number(d, "Свечение ", "2.6")?;
    if d.app
        .scene
        .world
        .models
        .iter()
        .find(|item| item.label == lamp)
        .is_none_or(|item| item.material.emissive_strength != 2.6)
    {
        return Err("Значение материала не изменилось.".into());
    }
    capture_png(d, &output.join("workshop-material-editor.png"))
}

fn check_workshop_rule(d: &mut Driver<'_, '_>, output: &Path) -> Result<(), String> {
    let check = output.join("workshop-model-route.check.json");
    d.click("Проверки…")?;
    d.click("Записать проверку")?;
    super::rain_proof::play_short(d)?;
    d.key(Key::F6)?;
    panel_click(d, "■ Стоп")?;
    d.edit(CHECK, 0, &check.to_string_lossy())?;
    d.click("Сохранить проверку")?;
    d.click("Проверки…")?;
    d.click("Правки ИИ…")?;
    d.edit("Опиши изменения сцены или механики", 0, WORKSHOP_CHANGE)?;
    d.click("Запросить правки")?;
    let start = Instant::now();
    while d.app.ai_review.awaiting_model() && start.elapsed() < Duration::from_secs(155) {
        thread::sleep(Duration::from_millis(400));
        d.settle(1)?;
    }
    fs::write(
        output.join("workshop-model-patch.json"),
        d.app.ai_review.draft_text(),
    )
    .map_err(io_error)?;
    d.expect_hud("Ответ получен")?;
    d.click("Проверить ссылки и показать правки")?;
    d.expect_hud("Связи проверены · правок 1")?;
    d.click("Сравнить по записанной проверке")?;
    d.settle(85)?;
    let (baseline, candidate) = d
        .app
        .ai_review
        .comparison_summary()
        .ok_or("Сравнение двух вариантов мастерской не завершилось.")?;
    if !baseline.passed || candidate.passed {
        return Err(format!(
            "Варианты мастерской дали неожиданный результат. {:?} / {:?}",
            baseline.messages, candidate.messages
        ));
    }
    capture_png(d, &output.join("workshop-model-review.png"))?;
    d.click("Принять правки")?;
    if d.app
        .scene
        .rules
        .rules
        .iter()
        .find(|rule| rule.name == "Отметка на верстаке")
        .is_none_or(|rule| rule.enabled)
    {
        return Err("В мастерской не отключилось точное правило.".into());
    }
    d.click("Отменить")?;
    if d.app
        .scene
        .rules
        .rules
        .iter()
        .find(|rule| rule.name == "Отметка на верстаке")
        .is_none_or(|rule| !rule.enabled)
    {
        return Err("Отмена не восстановила правило мастерской.".into());
    }
    Ok(())
}

fn mark(d: &mut Driver<'_, '_>, x: f32) -> Result<(), String> {
    top_proof::move_axis(d, 0, x)?;
    d.key(Key::F)
}

fn threshold(app: &Studio) -> Option<i32> {
    let signals = app
        .scene
        .rules
        .counters
        .iter()
        .find(|row| row.name == game_plan::SIGNALS)?
        .id;
    app.scene
        .rules
        .rules
        .iter()
        .find(|row| row.name == game_plan::FINISH_RULE)?
        .conditions
        .iter()
        .find_map(|condition| match *condition {
            Condition::Counter {
                counter_id,
                comparison: Comparison::AtLeast,
                value,
            } if counter_id == signals => Some(value),
            _ => None,
        })
}

fn count(app: &Studio, name: &str) -> Option<i32> {
    let current = app
        .scene
        .rules
        .counters
        .iter()
        .find(|row| row.name == name)?;
    app.rule_session.counter(&app.scene.rules, current.id)
}

fn timeout(d: &mut Driver<'_, '_>) -> Result<(), String> {
    top_proof::move_axis(d, 0, 4.75)?;
    let timer = d
        .app
        .scene
        .rules
        .timers
        .first()
        .ok_or("План не создал таймер.")?
        .seconds;
    let limit = (timer * 60.0).ceil() as usize + 45;
    for _ in (0..limit).step_by(24) {
        if count(&d.app, game_plan::TIMEOUT) == Some(1) {
            break;
        }
        d.settle(24)?;
    }
    if count(&d.app, game_plan::TIMEOUT) != Some(1) {
        return Err("При истечении таймера не зафиксировано поражение.".into());
    }
    d.expect_hud(&format!("{} · 1", game_plan::TIMEOUT))?;
    d.hold(20)?;
    d.key(Key::R)?;
    if count(&d.app, game_plan::TIMEOUT) != Some(0) || count(&d.app, game_plan::SIGNALS) != Some(0)
    {
        return Err("Клавиша R не начала новую попытку.".into());
    }
    Ok(())
}

fn record_check(d: &mut Driver<'_, '_>, check: &Path) -> Result<(), String> {
    d.click("Проверки…")?;
    d.click("Записать проверку")?;
    mark(d, 7.5)?;
    mark(d, 22.5)?;
    require_victory(d, false)?;
    d.key(Key::F6)?;
    mark(d, 14.5)?;
    mark(d, 22.5)?;
    require_victory(d, true)?;
    d.key(Key::F6)?;
    panel_click(d, "■ Стоп")?;
    d.edit(CHECK, 0, &check.to_string_lossy())?;
    d.click("Сохранить проверку")?;
    if !check.is_file() {
        return Err("Не записана проверка прохождения.".into());
    }
    d.click("Проверки…")?;
    Ok(())
}

fn require_victory(d: &Driver<'_, '_>, expected: bool) -> Result<(), String> {
    let quest = d.app.scene.story.quests.first().ok_or("Пустая цель.")?.id;
    let done = d.app.rule_session.quest_stage(quest) == Some(2);
    if done != expected {
        return Err(format!(
            "Цель ожидалась {expected}, получено {done}. Сигналы {:?}, таймаут {:?}.",
            count(&d.app, game_plan::SIGNALS),
            count(&d.app, game_plan::TIMEOUT)
        ));
    }
    if expected {
        d.expect_hud("Выполнено · Сигнальная тропа")?;
    }
    Ok(())
}

fn author_change(d: &mut Driver<'_, '_>) -> Result<(), String> {
    d.click("Правки ИИ…")?;
    d.edit("Опиши изменения сцены или механики", 0, CHANGE)?;
    d.click("Запросить правки")?;
    d.settle(12)?;
    d.expect_hud("Ответ получен")?;
    d.click("Проверить ссылки и показать правки")?;
    d.expect_hud("Связи проверены · правок 1")?;
    d.click("Сравнить по записанной проверке")?;
    d.settle(55)?;
    let (baseline, candidate) = d
        .app
        .ai_review
        .comparison_summary()
        .ok_or("Сравнение условий победы не завершилось.")?;
    if !baseline.passed || candidate.passed || threshold(&d.app) != Some(2) {
        return Err(format!(
            "Проверка не показала раннюю победу. {:?} / {:?}",
            baseline.messages, candidate.messages
        ));
    }
    d.expect_hud("Первое расхождение")?;
    d.hold(55)?;
    d.click("Принять правки")
}

fn connect(d: &mut Driver<'_, '_>, endpoint: &str, model: &str) -> Result<(), String> {
    d.click("Модель и адрес API")?;
    d.edit("http://127.0.0.1:8082/v1", 0, endpoint)?;
    d.edit("qwen3.8-flash-next-iq2_xs", 0, model)?;
    d.click("Модель и адрес API")
}

fn ask(d: &mut Driver<'_, '_>, text: &str) -> Result<(), String> {
    if d.point(INPUT, 0).is_err() {
        top_proof::scroll_panel(d, 850.0)?;
    }
    d.edit(INPUT, 0, text)?;
    right_click(d, "Обсудить с моделью")
}

fn right_click(d: &mut Driver<'_, '_>, label: &str) -> Result<(), String> {
    for amount in [-310.0, -380.0, 900.0] {
        if d.point(label, 0).is_ok() {
            break;
        }
        top_proof::scroll_panel(d, amount)?;
    }
    d.click(label)
}

fn await_plan(d: &mut Driver<'_, '_>, slow: bool) -> Result<(), String> {
    wait_turn(d, slow)?;
    if d.app.wizard.ready_plan().is_some() {
        return Ok(());
    }
    Err(format!(
        "Не получен согласованный план. {}",
        d.app.wizard.notice()
    ))
}

fn wait_turn(d: &mut Driver<'_, '_>, slow: bool) -> Result<(), String> {
    let start = Instant::now();
    let maximum = Duration::from_secs(if slow { 155 } else { 3 });
    while d.app.wizard.awaiting_model() && start.elapsed() < maximum {
        if slow {
            thread::sleep(Duration::from_millis(400));
        }
        d.settle(1)?;
    }
    Ok(())
}

fn capture_png(d: &Driver<'_, '_>, path: &Path) -> Result<(), String> {
    image::save_buffer(path, &d.last, SIZE[0], SIZE[1], image::ColorType::Rgb8)
        .map_err(|error| error.to_string())
}

fn wizard_reply(turn: usize, wire: Value) -> Result<String, String> {
    let prompt = wire["messages"][1]["content"].as_str().unwrap_or("");
    if wire["model"] != MODEL {
        return Err("Неверное имя проверочной модели.".into());
    }
    let value = match turn {
        0 if prompt.contains(AUTHOR) => json!({
            "message": "Какой вид камеры выбираешь, 2D сверху или платформер? Сколько сигналов нужно для победы?",
            "plan": null, "unavailable": []
        }),
        1 if prompt.contains(AUTHOR) && prompt.contains(ANSWER) => json!({
            "message": "Подойдёт короткий маршрут по трём маякам. Для победы нужны два сигнала до истечения 14 секунд.",
            "plan": Plan { seconds: 14.0, ..Plan::default() }, "unavailable": []
        }),
        2 if prompt.contains(CHANGE)
            && prompt.contains(game_plan::FINISH_RULE)
            && prompt.contains("counter_conditions") =>
        {
            json!({"ops":[
                {"op":"set_rule_condition_counter","rule_id":5,"counter_id":1,"value":1}
            ]})
        }
        _ => return Err("Запрос модели потерял контекст диалога или каталог условий.".into()),
    };
    Ok(value.to_string())
}
