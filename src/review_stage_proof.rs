//! Exercises AI suggestions through the editor, then runs the moved Windows exports.
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use serde_json::json;

use super::{
    Driver, FPS, SIZE, Sink, dungeon_proof, io_error,
    staged_proof::{move_and_play, panel_click, reload},
    top_proof,
};
use crate::{Studio, configure_context, offscreen::Offscreen, proof_video::Video, viewport3d};

const REQUEST: &str = "Предложи изменить начисление первой отметки.";
const BAD: &str = r#"{"ops":[{"op":"set_rule_counter","rule_id":1,"counter_id":1,"value":0}]}"#;
const GOOD: &str = r#"{"ops":[{"op":"set_rule_enabled","id":2,"enabled":false},{"op":"set_rule_enabled","id":7,"enabled":false}]}"#;
const LIVE_REQUEST: &str = "Измени ровно одно существующее действие AddCounter в правиле 1 для счётчика 1. Новое значение прибавления 0. Верни эту операцию в массиве ops.";
const CHECK_PATH: &str = "Файл проверки .check.json или текущая запись";

pub(super) fn record(
    output: &Path,
    releases: &Path,
    terrace: &Path,
    check: &Path,
    action: &Path,
    tool: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let staging = output.join("export-stage");
    fs::create_dir_all(&staging).map_err(io_error)?;
    let source_game = staging.join("TerraceReview.exe");
    let reviewed = output.join("terrace-review.robot.json");
    let dungeon_name = "ClosedLabyrinth.exe";
    let dungeon_scene = output.join("closed-labyrinth.robot.json");
    let dungeon_game = staging.join(dungeon_name);
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(&output.join("stage-021.mp4"), SIZE[0], SIZE[1], FPS)?;
    {
        let mut sink = Sink::Video(&mut video);
        let mut app = Studio::default();
        app.desk.recovery_root = Some(output.join("recovery-test"));
        let mut d = Driver::new(app, &mut display, &mut sink, Some((terrace, &source_game)));
        d.settle(6)?;
        panel_click(&mut d, "Открыть…")?;
        author_review(&mut d, check, output)?;
        let counter = super::extension_proof::author(&mut d, action, tool)?;
        d.app.proof.as_mut().unwrap().scene = Some(reviewed);
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "▶ Играть")?;
        top_proof::play(&mut d, true)?;
        super::extension_proof::play(&mut d, counter)?;
        show_manual_reset_removed(&mut d)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
    }
    move_and_play(
        &mut video,
        &mut display,
        &source_game,
        &releases.join("TerraceReview.exe"),
        "review",
        &output.join("timing-review.json"),
    )?;
    {
        let mut sink = Sink::Video(&mut video);
        let mut app = Studio::default();
        app.desk.recovery_root = Some(output.join("recovery-dungeon"));
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((&dungeon_scene, &dungeon_game)),
        );
        d.settle(6)?;
        dungeon_proof::author(&mut d)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        let steps = dungeon_proof::play(&mut d)?;
        fs::write(
            output.join("dungeon-editor.json"),
            format!("{{\"route_steps\":{steps}}}\n"),
        )
        .map_err(io_error)?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
    }
    move_and_play(
        &mut video,
        &mut display,
        &dungeon_game,
        &releases.join(dungeon_name),
        "dungeon",
        &output.join("timing-dungeon.json"),
    )?;
    video.finish()?;
    eprintln!(
        "0.21: два варианта прошли через редактор ИИ. Принятый вариант и генератор карты проверены в перенесённых EXE."
    );
    Ok(())
}

pub(super) fn play_review(d: &mut Driver<'_, '_>) -> Result<(), String> {
    if !accepted_rules(&d.app.scene) {
        return Err("Сборка потеряла принятую правку проекта.".into());
    }
    let counter = d
        .app
        .scene
        .rules
        .counters
        .iter()
        .find(|counter| counter.name == "Бонус расширения")
        .ok_or("В сборке нет счётчика расширения.")?
        .id;
    top_proof::play(d, false)?;
    super::extension_proof::play(d, counter)?;
    show_manual_reset_removed(d)
}

fn accepted_rules(scene: &crate::Scene) -> bool {
    [2, 7].into_iter().all(|id| {
        scene
            .rules
            .rules
            .iter()
            .any(|rule| rule.id == id && !rule.enabled)
    })
}

fn show_manual_reset_removed(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let before = d.app.game_position();
    d.key(egui::Key::R)?;
    if d.app.game_position().distance(before) > 0.2
        || d.app.rule_session.counter(&d.app.scene.rules, 1) != Some(2)
    {
        return Err("Отключённое правило R всё ещё сбрасывает прохождение.".into());
    }
    d.hold(18)
}

pub(super) fn author_review(
    d: &mut Driver<'_, '_>,
    check: &Path,
    output: &Path,
) -> Result<(), String> {
    let original = d.app.scene.clone();
    let (endpoint, mock) = mock_chat()?;
    ask(d, &endpoint, "stage6-fixture", REQUEST)?;
    mock.join().map_err(|_| "Сбой проверочного сервера.")??;
    d.settle(14)?;
    d.expect_hud("Ответ получен")?;
    d.click("Проверить ссылки и показать правки")?;
    d.expect_hud("Связи проверены · правок 1")?;
    start_check(d, check)?;
    d.settle(25)?;
    png(d, &output.join("review-first-difference.png"))?;
    let result = d.app.ai_review.comparison_summary().ok_or_else(|| {
        let labels = d
            .display
            .last_output()
            .map(crate::proof_input::labels)
            .unwrap_or_default();
        format!(
            "Сравнение не завершилось. {}",
            labels
                .into_iter()
                .map(|row| row.text)
                .collect::<Vec<_>>()
                .join(" | ")
        )
    })?;
    if !result.0.passed
        || result.1.passed
        || !result
            .1
            .messages
            .iter()
            .any(|line| line.contains("ожидалось 1, получено 0"))
    {
        return Err(format!(
            "Ожидалось расхождение 1 → 0. {:?} / {:?}",
            result.0.messages, result.1.messages
        ));
    }
    show_result(d, "ожидалось 1, получено 0")?;
    png(d, &output.join("review-first-difference.png"))?;
    if d.app.scene != original {
        return Err("Первый предпросмотр изменил открытый проект.".into());
    }
    d.hold(36)?;
    if d.point(BAD, 0).is_err() {
        top_proof::wheel(d, egui::pos2(555.0, 440.0), 1100.0)?;
    }
    d.edit(BAD, 0, GOOD)?;
    d.click("Проверить ссылки и показать правки")?;
    d.expect_hud("Связи проверены · правок 2")?;
    d.click("Сравнить по записанной проверке")?;
    d.settle(25)?;
    show_result(d, "Предложенные правки")?;
    let passed_twice = d
        .app
        .ai_review
        .comparison_summary()
        .is_some_and(|(base, variant)| base.passed && variant.passed);
    if !passed_twice || d.app.scene != original {
        return Err("Оба варианта должны пройти проверку без правки открытой сцены.".into());
    }
    png(d, &output.join("review-passed.png"))?;
    d.hold(36)?;
    if d.point("Принять правки", 0).is_err() {
        top_proof::wheel(d, egui::pos2(555.0, 440.0), 260.0)?;
    }
    d.click("Принять правки")?;
    let accepted = d.app.scene.clone();
    if !accepted_rules(&accepted) {
        return Err("Редактор не принял оба изменения.".into());
    }
    d.click("Отменить")?;
    if d.app.scene != original {
        return Err("Одно отменяющее действие не восстановило исходную сцену.".into());
    }
    d.click("Повторить")?;
    if d.app.scene != accepted {
        return Err("Повтор не вернул принятую правку.".into());
    }
    fs::write(
        output.join("review-variants.json"),
        serde_json::to_vec_pretty(&json!({
            "check": "Две отметки, таймер и финиш", "ticks": 820,
            "source": "mock Chat Completions, loopback",
            "first_divergence": {"tick": 91, "probe": "Отметки", "expected": 1, "actual": 0},
            "second": {"baseline_passed": true, "edited_variant_passed": true},
            "undo_redo": true
        }))
        .map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

pub(super) fn live(
    scene: &Path,
    check: &Path,
    endpoint: &str,
    model: &str,
    output: &Path,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut sink = Sink::None;
    let app = Studio::default();
    let mut d = Driver::new(
        app,
        &mut display,
        &mut sink,
        Some((scene, &output.join("unused.exe"))),
    );
    d.settle(6)?;
    panel_click(&mut d, "Открыть…")?;
    let before = d.app.scene.clone();
    ask(&mut d, endpoint, model, LIVE_REQUEST)?;
    let started = Instant::now();
    while d.app.ai_review.awaiting_model() && started.elapsed() < Duration::from_secs(155) {
        thread::sleep(Duration::from_millis(400));
        d.settle(1)?;
    }
    fs::write(
        output.join("local-model-proposal.json"),
        d.app.ai_review.draft_text(),
    )
    .map_err(io_error)?;
    d.expect_hud("Ответ получен")?;
    d.click("Проверить ссылки и показать правки")?;
    d.expect_hud("Связи проверены · правок 1")?;
    start_check(&mut d, check)?;
    d.settle(28)?;
    let (baseline, variant) = d
        .app
        .ai_review
        .comparison_summary()
        .ok_or("Сравнение не завершилось.")?;
    if !baseline.passed || variant.passed || d.app.scene != before {
        return Err("Сравнение с реальной моделью не показало ожидаемое расхождение.".into());
    }
    show_result(&mut d, "ожидалось 1, получено 0")?;
    png(&d, &output.join("local-model-review.png"))?;
    eprintln!(
        "Модель {model} предложила изменение через Chat Completions. Проверка зафиксировала расхождение 1 → 0."
    );
    Ok(())
}

fn ask(d: &mut Driver<'_, '_>, endpoint: &str, model: &str, prompt: &str) -> Result<(), String> {
    d.click("Правки ИИ…")?;
    d.click("Подключение к модели")?;
    d.edit("http://127.0.0.1:8082/v1", 0, endpoint)?;
    d.edit("qwen3.8-flash-next-iq2_xs", 0, model)?;
    d.click("Подключение к модели")?;
    d.edit("Опиши изменения сцены или механики", 0, prompt)?;
    d.click("Запросить правки")
}

fn start_check(d: &mut Driver<'_, '_>, check: &Path) -> Result<(), String> {
    if d.point(CHECK_PATH, 0).is_err() {
        top_proof::wheel(d, egui::pos2(555.0, 490.0), -200.0)?;
    }
    let path = check.to_string_lossy();
    d.edit(CHECK_PATH, 0, &path)?;
    let compare = "Сравнить по записанной проверке";
    if d.point(compare, 0).is_err() {
        let anchor = d.point(&path, 0)?;
        top_proof::wheel(d, anchor, -180.0)?;
    }
    d.click(compare)
}

fn show_result(d: &mut Driver<'_, '_>, text: &str) -> Result<(), String> {
    for mouse in [egui::pos2(570.0, 445.0), egui::pos2(530.0, 390.0)] {
        if d.expect_hud(text).is_ok() {
            return Ok(());
        }
        top_proof::wheel(d, mouse, -350.0)?;
    }
    d.expect_hud(text).map_err(|error| {
        let labels = d
            .display
            .last_output()
            .map(crate::proof_input::labels)
            .unwrap_or_default();
        format!(
            "{error} Видны {}",
            labels
                .into_iter()
                .map(|row| row.text)
                .collect::<Vec<_>>()
                .join(" | ")
        )
    })
}

fn png(d: &Driver<'_, '_>, path: &Path) -> Result<(), String> {
    image::save_buffer(path, &d.last, SIZE[0], SIZE[1], image::ColorType::Rgb8)
        .map_err(|error| error.to_string())
}

type Mock = thread::JoinHandle<Result<(), String>>;

fn mock_chat() -> Result<(String, Mock), String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(io_error)?;
    listener.set_nonblocking(true).map_err(io_error)?;
    let address = listener.local_addr().map_err(io_error)?;
    let worker = thread::spawn(move || {
        let started = Instant::now();
        let mut socket = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && started.elapsed() < Duration::from_secs(15) =>
                {
                    thread::sleep(Duration::from_millis(15));
                }
                Err(error) => return Err(format!("Не получен запрос проверочного API. {error}")),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(io_error)?;
        let mut request = BufReader::new(&socket);
        let mut header = String::new();
        let mut length = 0;
        request.read_line(&mut header).map_err(io_error)?;
        if !header.starts_with("POST /v1/chat/completions ") {
            return Err("Вызван неожиданный путь сервера.".into());
        }
        loop {
            header.clear();
            if request.read_line(&mut header).map_err(io_error)? == 0 || header.trim().is_empty() {
                break;
            }
            if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| error.to_string())?;
            }
        }
        if length == 0 || length > 65_536 {
            return Err("Тело запроса отсутствует или слишком велико.".into());
        }
        let mut body = vec![0; length];
        request.read_exact(&mut body).map_err(io_error)?;
        let wire: serde_json::Value =
            serde_json::from_slice(&body).map_err(|error| error.to_string())?;
        let prompt = wire["messages"][1]["content"].as_str().unwrap_or("");
        if wire["model"] != "stage6-fixture"
            || !prompt.contains(REQUEST)
            || !prompt.contains("Первая отметка")
        {
            return Err("Редактор не передал описание и список правил модели.".into());
        }
        let reply =
            json!({"choices":[{"message":{"content": BAD},"finish_reason":"stop"}]}).to_string();
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).map_err(io_error)
    });
    Ok((format!("http://{address}/v1"), worker))
}
