use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use serde_json::json;

use super::{Driver, FPS, SIZE, Sink, io_error, staged_proof::panel_click, top_proof};
use crate::{
    Avatar, Scene, Studio,
    assets::{Library, Source},
    configure_context,
    game_trial::{self, Case, Suite},
    linked_scenes::ROOT_SCENE_ID,
    map2d::{self, Mode},
    npcs::{FIRST_NPC_ID, Npc, Waypoint},
    offscreen::Offscreen,
    playback::{Check, Choice, Entry, Intent, Probe, Value},
    proof_video::Video,
    rules::{Action, Condition, Event, GameAction, RuleSet},
    story::Story,
    viewport3d,
};

const STUDY: &str = "Пункт развилки";
const BAD: &str = r#"{"ops":[{"op":"set_rule_enabled","id":2,"enabled":false},{"op":"set_rule_enabled","id":3,"enabled":true}]}"#;
const GOOD: &str = r#"{"ops":[{"op":"set_rule_enabled","id":3,"enabled":true}]}"#;
const QUESTION: &str = "После разговора открой второй маршрут. Включи только существующее правило «Второй маршрут» (ID 3). Старый маршрут (ID 2) и сохранения должны работать. Верни одну операцию set_rule_enabled.";
const SUITE_FIELD: &str = "Набор проверок .suite.json для двух EXE";
const COMPARE: &str = "Проверить в двух EXE";

pub(super) fn record(output: &Path, connection: Option<(&str, &str)>) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let output = output.canonicalize().map_err(io_error)?;
    let mut library = Library::default();
    let original = fixture(&mut library)?;
    let project = output.join("junction.robot.json");
    original.save(&project)?;
    let seed_exe = output.join("BaselineSlot.exe");
    crate::game_export::export(&seed_exe, &original, &library, false)?;
    let suite = make_checks(&output, &seed_exe)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut movie = Video::start(&output.join("two-exe-review.mp4"), SIZE[0], SIZE[1], FPS)?;
    {
        let mut sink = Sink::Video(&mut movie);
        let mut app = Studio::default();
        app.desk.recovery_root = Some(output.join("trial-recovery"));
        let export = output.join("Accepted.exe");
        let mut d = Driver::new(app, &mut display, &mut sink, Some((&project, &export)));
        d.settle(7)?;
        panel_click(&mut d, "Открыть…")?;
        if d.app.scene != original {
            return Err("Исходная сцена проверки не открылась.".into());
        }
        d.click("Правки ИИ…")?;
        d.edit(SUITE_FIELD, 0, &suite.to_string_lossy())?;
        d.edit(r#"{"ops":[…]}"#, 0, BAD)?;
        d.click("Проверить ссылки и показать правки")?;
        reveal(&mut d, COMPARE)?;
        d.click(COMPARE)?;
        wait_report(&mut d)?;
        let rejected = d
            .app
            .ai_review
            .game_comparison()
            .ok_or("Нет отчёта об отказе.")?;
        if rejected.approved()
            || !rejected.original.iter().all(|row| row.outcome.passed)
            || rejected
                .edited
                .first()
                .is_none_or(|row| row.outcome.passed || !row.restored_slot)
            || d.app.scene != original
        {
            return Err(format!(
                "Вариант не воспроизвёл регрессию старого слота. {rejected:?}"
            ));
        }
        copy_run(rejected, &output.join("rejected"))?;
        capture_label(
            &mut d,
            "Применение заблокировано",
            &output.join("rejected.png"),
        )?;
        d.hold(44)?;
        if let Some((endpoint, model)) = connection {
            show_top(&mut d)?;
            d.click("Подключение к модели")?;
            d.edit("http://127.0.0.1:8082/v1", 0, endpoint)?;
            d.edit("qwen3.8-flash-next-iq2_xs", 0, model)?;
            d.click("Подключение к модели")?;
            d.edit("Опиши изменения сцены или механики", 0, QUESTION)?;
            d.click("Запросить правки")?;
            let start = Instant::now();
            while d.app.ai_review.awaiting_model() && start.elapsed() < Duration::from_secs(155) {
                thread::sleep(Duration::from_millis(250));
                d.settle(1)?;
            }
            d.expect_hud("Ответ получен")?;
            fs::write(
                output.join("local-model-patch.json"),
                d.app.ai_review.draft_text(),
            )
            .map_err(io_error)?;
        } else {
            show_top(&mut d)?;
            d.edit(BAD, 0, GOOD)?;
        }
        d.click("Проверить ссылки и показать правки")?;
        reveal(&mut d, COMPARE)?;
        d.click(COMPARE)?;
        wait_report(&mut d)?;
        let accepted = d
            .app
            .ai_review
            .game_comparison()
            .ok_or("Нет отчёта о повторе.")?;
        if !accepted.approved() {
            return Err(format!("Исправленная правка не прошла. {accepted:?}"));
        }
        copy_run(accepted, &output.join("accepted-check"))?;
        capture_label(
            &mut d,
            "Два EXE прошли проверку",
            &output.join("verified.png"),
        )?;
        d.hold(46)?;
        reveal(&mut d, "Принять правки")?;
        d.click("Принять правки")?;
        let applied = d.app.scene.clone();
        if !applied.rules.rules[1].enabled || !applied.rules.rules[2].enabled {
            return Err("Принятая правка не оставила оба маршрута.".into());
        }
        d.click("Отменить")?;
        if d.app.scene != original {
            return Err("Откат не вернул исходные правила.".into());
        }
        d.click("Повторить")?;
        if d.app.scene != applied {
            return Err("Повтор не вернул принятое состояние.".into());
        }
        d.app.proof.as_mut().unwrap().scene = Some(output.join("junction-edited.robot.json"));
        panel_click(&mut d, "Сохранить как…")?;
        super::staged_proof::reload(&mut d)?;
        panel_click(&mut d, "Собрать игру для Windows")?;
    }
    movie.finish()
}

pub(super) fn fixture(library: &mut Library) -> Result<Scene, String> {
    let mut scene = map2d::scene(Mode::Top, true);
    scene.rules = RuleSet::default();
    scene.world = Default::default();
    scene.story = Story::demo(FIRST_NPC_ID, [23.0, 10.5]);
    let guide = &mut scene.story.conversations[0];
    guide.name = "Дана · диспетчер".into();
    guide.nodes[0].text = "Я отмечу допуск. Ближний знак ведёт на старый маршрут, дальний — на второй, если он открыт.".into();
    guide.nodes[0].choices[0].text = "Получить допуск".into();
    guide.nodes[0].choices[0].target = None;
    guide.nodes[0].choices[0].starts_quest = None;
    library.ensure(&Source::Emma, None)?;
    let actor = Avatar::new(
        Source::Emma,
        guide.name.clone(),
        &library.get(&Source::Emma).unwrap().model,
    );
    scene.npcs.push(Npc::new(
        FIRST_NPC_ID,
        actor,
        vec![Waypoint::at([3.8, 0.0, 10.5])],
    ));
    let old_id = scene
        .linked_scenes
        .attach("Старый маршрут".into(), map2d::scene(Mode::Top, true))?;
    let new_id = scene
        .linked_scenes
        .attach("Второй маршрут".into(), map2d::scene(Mode::Side, true))?;
    let rules = &mut scene.rules;
    let permit = rules.add_flag().ok_or("Нет флага допуска.")?;
    rules.flags[0].name = "Допуск Даны".into();
    let near = rules
        .add_zone([3.5, 0.0, 10.5])
        .ok_or("Нет исходной зоны.")?;
    let far = rules.add_zone([7.5, 0.0, 10.5]).ok_or("Нет новой зоны.")?;
    rules.zones[0].name = "Ближний знак".into();
    rules.zones[1].name = "Дальний знак".into();
    let event = Some(Event::GameAction {
        action: GameAction::Mark,
    });
    let test = Condition::Flag {
        flag_id: permit,
        value: true,
    };
    for (name, enabled, event, conditions, actions) in [
        (
            "Допуск после ответа",
            true,
            Some(Event::Dialogue {
                npc_id: FIRST_NPC_ID,
                node_id: 1,
                choice_id: 1,
            }),
            vec![],
            vec![Action::SetFlag {
                flag_id: permit,
                value: true,
            }],
        ),
        (
            "Старый маршрут",
            true,
            event,
            vec![test, Condition::InZone { zone_id: near }],
            vec![Action::Transition { scene_id: old_id }],
        ),
        (
            "Второй маршрут",
            false,
            event,
            vec![test, Condition::InZone { zone_id: far }],
            vec![Action::Transition { scene_id: new_id }],
        ),
    ] {
        rules.add_rule().ok_or("Нет места для правила.")?;
        let rule = rules.rules.last_mut().unwrap();
        rule.name = name.into();
        rule.enabled = enabled;
        rule.event = event;
        rule.conditions = conditions;
        rule.actions = actions;
    }
    scene.validate()?;
    Ok(scene)
}

pub(super) fn make_checks(output: &Path, source_exe: &Path) -> Result<std::path::PathBuf, String> {
    let mut saved = Check::new("Слот с допуском".into());
    dialogue(&mut saved)?;
    saved.expect(
        "После ответа".into(),
        vec![
            position(ROOT_SCENE_ID),
            Entry {
                probe: Probe::Flag(1),
                label: "Допуск Даны".into(),
                value: Value::Switch(true),
            },
        ],
    )?;
    let capture = output.join("capture.check.json");
    saved.save(&capture)?;
    let slot = output.join("old.checkpoint.json");
    game_trial::capture_slot(source_exe, &capture, &slot)?;

    let mut legacy = Check::new("Старое сохранение открывает ближний путь".into());
    legacy.record(Intent {
        dt: 1.0 / 60.0,
        mark: true,
        ..Intent::default()
    })?;
    legacy.expect("Ближний знак после загрузки".into(), vec![position(2)])?;
    legacy.save(&output.join("old-route.check.json"))?;

    route(false, ROOT_SCENE_ID)?.save(&output.join("before-dialogue.check.json"))?;
    route(true, ROOT_SCENE_ID)?.save(&output.join("original-far.check.json"))?;
    route(true, 3)?.save(&output.join("edited-far.check.json"))?;
    let suite = Suite {
        version: 1,
        cases: vec![
            Case {
                name: "Старый слот · ближний знак".into(),
                original: "old-route.check.json".into(),
                edited: None,
                slot: Some("old.checkpoint.json".into()),
            },
            Case {
                name: "Дальний знак без допуска".into(),
                original: "before-dialogue.check.json".into(),
                edited: None,
                slot: None,
            },
            Case {
                name: "Дальний знак после разговора".into(),
                original: "original-far.check.json".into(),
                edited: Some("edited-far.check.json".into()),
                slot: None,
            },
        ],
    };
    let path = output.join("junction.suite.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&suite).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)?;
    Ok(path)
}

pub(super) fn dialogue(check: &mut Check) -> Result<(), String> {
    check.record(Intent {
        dt: 1.0 / 60.0,
        interact: true,
        ..Intent::default()
    })?;
    check.answer(Choice {
        npc: FIRST_NPC_ID,
        node: 1,
        option: 1,
    });
    Ok(())
}

fn route(talked: bool, target: u32) -> Result<Check, String> {
    let mut check = Check::new(STUDY.into());
    if talked {
        dialogue(&mut check)?;
    }
    for _ in 0..64 {
        check.record(Intent {
            dt: 1.0 / 60.0,
            movement: [1.0, 0.0],
            ..Intent::default()
        })?;
    }
    check.record(Intent {
        dt: 1.0 / 60.0,
        mark: true,
        ..Intent::default()
    })?;
    check.expect("У дальнего знака".into(), vec![position(target)])?;
    Ok(check)
}

pub(super) fn position(id: u32) -> Entry {
    Entry {
        probe: Probe::Location,
        label: "Локация".into(),
        value: Value::Number(id as i32),
    }
}

fn wait_report(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let time = Instant::now();
    while time.elapsed() < Duration::from_secs(90) {
        d.settle(1)?;
        if d.app.ai_review.game_comparison().is_some() {
            return Ok(());
        }
        if let Some(error) = d.app.ai_review.game_comparison_error() {
            return Err(error.to_owned());
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(format!("Нет сравнения EXE. {}", d.app.status))
}

fn capture_label(d: &mut Driver<'_, '_>, label: &str, file: &Path) -> Result<(), String> {
    if d.expect_hud(label).is_err() {
        top_proof::wheel(d, egui::pos2(565.0, 540.0), -250.0)?;
    }
    d.expect_hud(label)?;
    image::save_buffer(file, &d.last, SIZE[0], SIZE[1], image::ColorType::Rgb8)
        .map_err(|error| error.to_string())
}

fn reveal(d: &mut Driver<'_, '_>, label: &str) -> Result<(), String> {
    for amount in [-190.0, 180.0, -280.0, 900.0] {
        if d.point(label, 0).is_ok() {
            return Ok(());
        }
        top_proof::wheel(d, egui::pos2(565.0, 540.0), amount)?;
    }
    d.point(label, 0).map(|_| ())
}

fn show_top(d: &mut Driver<'_, '_>) -> Result<(), String> {
    top_proof::wheel(d, egui::pos2(565.0, 545.0), 1300.0)
}

fn copy_run(result: &game_trial::Comparison, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(io_error)?;
    for item in [
        "Original.exe",
        "Edited.exe",
        "Original.json",
        "Edited.json",
        "comparison.json",
    ] {
        fs::copy(result.folder.join(item), dir.join(item)).map_err(io_error)?;
    }
    fs::write(
        dir.join("summary.json"),
        serde_json::to_vec_pretty(&json!({
            "approved":result.approved(),"elapsed_ms":result.elapsed_ms,
            "original":result.original,"edited":result.edited
        }))
        .map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}
