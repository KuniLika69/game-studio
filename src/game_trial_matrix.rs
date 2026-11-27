use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use serde_json::{Value as Json, json};

use super::{game_trial_proof as fixture, io_error};
use crate::{
    Scene, ai,
    assets::Library,
    editor_history::Desk,
    game_export,
    game_trial::{self, Case, Suite},
    npcs::FIRST_NPC_ID,
    playback::{Check, Entry, Intent, Probe, Value},
    rules::{Action, Comparison, Condition, Event, GameAction},
    scene_patch::{self, Base, Proposal},
};

const TASKS: [(&str, &str); 5] = [
    (
        "second-route",
        "После диалога включи правило 3 «Второй маршрут». Сохрани правило 2 и старые сохранения.",
    ),
    (
        "move-gate",
        "Перенеси «Дальний знак», зона 2, на X=8.5, Y=0, Z=10.5. Остальные правила оставь прежними.",
    ),
    (
        "threshold",
        "Измени только условие счётчика 1 «Сигналы» в правиле 3 «Второй маршрут» с >=0 на >=1. Другие условия и действия сохрани.",
    ),
    (
        "reward",
        "В правиле 4 «Сигнал у стойки» поменяй действие прибавления к счётчику 1 с +1 на +2. Остальное сохрани.",
    ),
    (
        "caption",
        "В реплике 1 диспетчера Даны измени только текст ответа 1 на «Отметить разрешение». Не меняй его ID и связанные правила.",
    ),
];

pub(super) fn run(output: &Path, endpoint: &str, model: &str) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let output = output.canonicalize().map_err(io_error)?;
    let mut library = Library::default();
    let base = fixture::fixture(&mut library)?;
    let source = output.join("baseline-for-save.exe");
    game_export::export(&source, &base, &library, false)?;
    fixture::make_checks(&output, &source)?;
    let mut rows = Vec::new();
    for (task_id, (name, prompt)) in TASKS.into_iter().enumerate() {
        let dir = output.join(name);
        fs::create_dir(&dir).map_err(io_error)?;
        let (scenario, manual, tests) = setup(base.clone(), task_id)?;
        scenario.save(&dir.join("original.robot.json"))?;
        write(
            &dir.join("task.json"),
            &json!({"prompt": prompt, "manual": manual}),
        )?;
        let path = suite(&dir, tests)?;

        let start = Instant::now();
        let (manual_scene, manual_undo) = stage(&scenario, &manual.to_string(), &library)?;
        let manual_exe = dir.join("Manual.exe");
        game_export::export(&manual_exe, &manual_scene, &library, false)?;
        let manual_ms = start.elapsed().as_millis();
        let manual_audit =
            game_trial::audit_package(&manual_exe, &path, &dir.join("manual-audit.json"))?;
        if manual_audit.iter().any(|row| !row.outcome.passed) {
            return Err(format!(
                "Заранее заданный ручной вариант «{name}» не прошёл: {manual_audit:?}"
            ));
        }

        let start = Instant::now();
        let answer = ai::complete(
            endpoint,
            model,
            "",
            crate::ai_scene::SYSTEM_PROMPT,
            &format!(
                "Запрос автора\n{prompt}\nКаталог сцены JSON\n{}",
                scene_patch::catalogue(&scenario)
            ),
        );
        let inference_ms = start.elapsed().as_millis();
        if let Ok(answer) = &answer {
            fs::write(dir.join("model-response.txt"), answer).map_err(io_error)?;
        }
        let prefix = Instant::now();
        let built = answer.and_then(|reply| stage(&scenario, ai::response_json(&reply), &library));
        let mut verified_ms = inference_ms + prefix.elapsed().as_millis();
        let mut verified_undo = manual_undo;
        let (without, corrected) = match built {
            Ok((new_scene, undo)) => {
                let detached = dir.join("Unchecked.exe");
                game_export::export(&detached, &new_scene, &library, false)?;
                let elapsed = inference_ms + prefix.elapsed().as_millis();
                // Independent post-acceptance audit for the path without preflight.
                let late =
                    game_trial::audit_package(&detached, &path, &dir.join("late-audit.json"))?;
                let misses = late.iter().filter(|row| !row.outcome.passed).count();
                let check = game_trial::compare(&scenario, &new_scene, &library, false, &path)?;
                verified_ms += check.elapsed_ms;
                if check.approved() {
                    verified_undo = undo;
                }
                write(
                    &dir.join("model-preflight.json"),
                    &serde_json::to_value(&check).map_err(|e| e.to_string())?,
                )?;
                (
                    json!({"candidate_ms":elapsed,"undo_us":undo,"missed_cases":misses}),
                    !check.approved(),
                )
            }
            Err(error) => (json!({"error":error,"missed_cases":null}), true),
        };
        if corrected {
            let start = Instant::now();
            let fixed = game_trial::compare(&scenario, &manual_scene, &library, false, &path)?;
            verified_ms += start.elapsed().as_millis();
            if !fixed.approved() {
                return Err(format!("Исправление «{name}» не прошло двойную проверку."));
            }
            write(
                &dir.join("corrected-preflight.json"),
                &serde_json::to_value(fixed).map_err(|e| e.to_string())?,
            )?;
        }
        rows.push(json!({
            "task":name,"inference_ms":inference_ms,
            "manual_pre_authored": {"candidate_ms":manual_ms,"undo_us":manual_undo,"missed_cases":0},
            "model_without_preflight": without,
            "model_with_preflight": {
                "accepted_ms":verified_ms, "undo_us":verified_undo,
                "missed_cases":0, "fallback_patch":corrected,
            },
        }));
        write(
            &output.join("matrix.json"),
            &json!({
                "method":"Scripted authoring benchmark. Manual mode starts with a predetermined edit. Both model modes use the same live response. Post-acceptance audit excluded from unchecked author time.",
                "model":model, "results":rows,
            }),
        )?;
    }
    Ok(())
}

type Pair = (Check, Check);

fn setup(mut scene: Scene, task: usize) -> Result<(Scene, Json, Vec<Pair>), String> {
    if task != 0 {
        scene.rules.rules[2].enabled = true;
    }
    let mut tests = Vec::new();
    let manual = match task {
        0 => {
            tests.push((travel(64, true, 1)?, travel(64, true, 3)?));
            tests.push((travel(64, false, 1)?, travel(64, false, 1)?));
            json!({"op":"set_rule_enabled","id":3,"enabled":true})
        }
        1 => {
            tests.push((travel(90, true, 1)?, travel(90, true, 3)?));
            tests.push((travel(90, false, 1)?, travel(90, false, 1)?));
            json!({"op":"set_zone_position","id":2,"position":[8.5,0,10.5]})
        }
        2 | 3 => {
            let counter = scene.rules.add_counter().ok_or("Нет счётчика.")?;
            scene.rules.counters[0].name = "Сигналы".into();
            let zone = scene
                .rules
                .add_zone([5.45, 0.0, 10.5])
                .ok_or("Нет стойки.")?;
            scene.rules.zones.last_mut().unwrap().name = "Стойка сигналов".into();
            scene.rules.zones.last_mut().unwrap().size[0] = 0.9;
            scene.rules.rules[2].conditions.push(Condition::Counter {
                counter_id: counter,
                comparison: Comparison::AtLeast,
                value: 0,
            });
            scene.rules.add_rule().ok_or("Нет правила стойки.")?;
            let rule = scene.rules.rules.last_mut().unwrap();
            rule.name = "Сигнал у стойки".into();
            rule.event = Some(Event::GameAction {
                action: GameAction::Mark,
            });
            rule.conditions = vec![Condition::InZone { zone_id: zone }];
            rule.actions = vec![Action::AddCounter {
                counter_id: counter,
                delta: 1,
            }];
            if task == 2 {
                tests.push((travel(64, true, 3)?, travel(64, true, 1)?));
                let sent = terminal_route(true, 3)?;
                tests.push((sent.clone(), sent));
                json!({"op":"set_rule_condition_counter","rule_id":3,"counter_id":counter,"value":1})
            } else {
                tests.push((terminal_route(false, 1)?, terminal_route(false, 2)?));
                json!({"op":"set_rule_counter","rule_id":4,"counter_id":counter,"value":2})
            }
        }
        _ => {
            let check = |text: &str| -> Result<Check, String> {
                let mut check = Check::new("Подпись ответа".into());
                fixture::dialogue(&mut check)?;
                check.expect(
                    "После подтверждения".into(),
                    vec![
                        Entry {
                            probe: Probe::AnswerText {
                                npc: FIRST_NPC_ID,
                                node: 1,
                                option: 1,
                            },
                            label: "Ответ 1".into(),
                            value: Value::Text(text.into()),
                        },
                        Entry {
                            probe: Probe::Flag(1),
                            label: "Допуск".into(),
                            value: Value::Switch(true),
                        },
                    ],
                )?;
                Ok(check)
            };
            tests.push((check("Получить допуск")?, check("Отметить разрешение")?));
            json!({"op":"set_reply_text","npc_id":FIRST_NPC_ID.to_string(),"node_id":1,"choice_id":1,"text":"Отметить разрешение"})
        }
    };
    scene.validate()?;
    Ok((scene, json!({"ops":[manual]}), tests))
}

fn stage(scene: &Scene, json: &str, library: &Library) -> Result<(Scene, u128), String> {
    let proposal = Proposal::decode_ops(
        json.as_bytes(),
        Base::capture(scene).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut next = scene.clone();
    proposal
        .stage(scene)
        .map_err(|e| e.to_string())?
        .apply(&mut next)
        .map_err(|e| e.to_string())?;
    let mut desk = Desk::default();
    desk.initialize(scene, library);
    desk.observe(&next, library);
    let start = Instant::now();
    let restored = desk.travel(false).ok_or("Нет действия для отката.")?;
    let rollback = (*restored.scene).clone();
    let undo_us = start.elapsed().as_micros();
    if rollback != *scene {
        return Err("Откат не восстановил исходный проект.".into());
    }
    Ok((next, undo_us))
}

fn move_x(check: &mut Check, ticks: usize) -> Result<(), String> {
    for _ in 0..ticks {
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
    })
}
fn travel(ticks: usize, ask: bool, location: u32) -> Result<Check, String> {
    let mut check = Check::new("Дальний знак".into());
    if ask {
        fixture::dialogue(&mut check)?;
    }
    move_x(&mut check, ticks)?;
    check.expect(
        "После взаимодействия у знака".into(),
        vec![fixture::position(location)],
    )?;
    Ok(check)
}
fn terminal_route(travel: bool, expected: i32) -> Result<Check, String> {
    let mut check = Check::new("Сигнал у стойки".into());
    fixture::dialogue(&mut check)?;
    move_x(&mut check, 33)?;
    let entry = if travel {
        move_x(&mut check, 30)?;
        fixture::position(expected as u32)
    } else {
        Entry {
            probe: Probe::Counter(1),
            label: "Сигналы".into(),
            value: Value::Number(expected),
        }
    };
    check.expect("После нажатия F".into(), vec![entry])?;
    Ok(check)
}
fn suite(dir: &Path, tests: Vec<Pair>) -> Result<PathBuf, String> {
    let mut cases = vec![Case {
        name: "Поддержка старого слота".into(),
        original: PathBuf::from("../old-route.check.json"),
        edited: None,
        slot: Some(PathBuf::from("../old.checkpoint.json")),
    }];
    for (index, (old, new)) in tests.into_iter().enumerate() {
        let original = PathBuf::from(format!("original-{index}.check.json"));
        let edited = PathBuf::from(format!("edited-{index}.check.json"));
        old.save(&dir.join(&original))?;
        new.save(&dir.join(&edited))?;
        cases.push(Case {
            name: new.title,
            original,
            edited: Some(edited),
            slot: None,
        });
    }
    let file = dir.join("task.suite.json");
    write(
        &file,
        &serde_json::to_value(Suite { version: 1, cases }).map_err(|e| e.to_string())?,
    )?;
    Ok(file)
}
fn write(path: &Path, value: &Json) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(io_error)
}
