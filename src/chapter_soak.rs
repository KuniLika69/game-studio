use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{self, Key, Modifiers};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Driver, FPS, SIZE, Sink, io_error};
use crate::{
    Studio,
    assets::Library,
    game_export::{self, GamePackage},
    game_save,
    linked_scenes::ROOT_SCENE_ID,
    map2d::{self, Mode},
    offscreen::Offscreen,
    performance::{Telemetry, percentile_sorted},
    playback::Intent,
    proof_input, quality,
    rules::{Action, Event, GameAction},
    scene::Scene,
};

const LOCATIONS: usize = 10;
const STEP: f32 = 1.0 / 60.0;
const COUNT: u32 = 1;
const TELEMETRY_SIZE: [u32; 2] = [2560, 1440];
const TELEMETRY_FPS: usize = 30;

pub(super) fn author(scene: &Path, game: &Path) -> Result<(), String> {
    let mut root = location(1);
    for number in 2..=LOCATIONS as u32 {
        let id = root
            .linked_scenes
            .attach(format!("Глава испытания {number:02}"), location(number))?;
        if id != number {
            return Err("Сменился порядок связанных глав.".into());
        }
    }
    transitions(&mut root, 1)?;
    for child in &mut root.linked_scenes.entries {
        transitions(&mut child.scene, child.id)?;
    }
    root.save(scene)?;
    let reopened = Scene::load(scene)?;
    if root != reopened {
        return Err("Сценарий изменился после повторного открытия.".into());
    }
    game_export::export(game, &reopened, &Library::default(), false)
}

fn location(number: u32) -> Scene {
    let mut scene = match number % 3 {
        0 => map2d::scene(Mode::Side, true),
        1 => Scene::default(),
        _ => map2d::scene(Mode::Top, true),
    };
    let counter = scene.rules.add_counter().expect("counter in empty chapter");
    assert_eq!(counter, COUNT);
    scene.rules.counters.last_mut().unwrap().name = format!("Выходы из главы {number:02}");
    scene
}

fn transitions(scene: &mut Scene, number: u32) -> Result<(), String> {
    for (action, label, target) in [
        (GameAction::Mark, "Далее", destination(number, true)),
        (GameAction::Retry, "Назад", destination(number, false)),
    ] {
        scene
            .rules
            .add_rule()
            .ok_or("Нет места для правила перехода.")?;
        let rule = scene.rules.rules.last_mut().unwrap();
        rule.name = label.into();
        rule.event = Some(Event::GameAction { action });
        rule.once = false;
        rule.actions = vec![
            Action::AddCounter {
                counter_id: COUNT,
                delta: 1,
            },
            Action::Transition { scene_id: target },
        ];
    }
    Ok(())
}

fn destination(number: u32, next: bool) -> u32 {
    if next {
        number % LOCATIONS as u32 + 1
    } else {
        (number + LOCATIONS as u32 - 2) % LOCATIONS as u32 + 1
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Ledger {
    scene: u32,
    counters: [i32; LOCATIONS],
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            scene: ROOT_SCENE_ID,
            counters: [0; LOCATIONS],
        }
    }
}

impl Ledger {
    fn verify(&self, app: &Studio) -> Result<(), String> {
        let location = app
            .link_playthrough
            .as_ref()
            .map(|play| play.active)
            .unwrap_or(0);
        let counter = app.rule_session.counter(&app.scene.rules, COUNT);
        let expected = self.counters[self.scene as usize - 1];
        if location != self.scene || counter != Some(expected) {
            return Err(format!(
                "Глава {} и счётчик {} ожидались. Глава {location}, счётчик {counter:?}.",
                self.scene, expected
            ));
        }
        Ok(())
    }

    fn go(&mut self, app: &mut Studio, ctx: &egui::Context, next: bool) -> Result<f64, String> {
        let started = Instant::now();
        app.advance_intent(
            ctx,
            Intent {
                dt: STEP,
                mark: next,
                retry: !next,
                ..Default::default()
            },
        );
        app.check_scene_transition(ctx);
        let millis = started.elapsed().as_secs_f64() * 1_000.0;
        self.advance(app, next)?;
        Ok(millis)
    }

    fn advance(&mut self, app: &Studio, next: bool) -> Result<(), String> {
        let active = self.scene as usize - 1;
        self.counters[active] += 1;
        self.scene = destination(self.scene, next);
        self.verify(app)
    }
}

pub(super) fn run(
    game: GamePackage,
    kind: &str,
    output: &Path,
    seconds: u64,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    if kind == "telemetry" {
        return telemetry(game, output, seconds);
    }
    let ctx = egui::Context::default();
    let mut app = Studio::from_package(game, &ctx);
    match kind {
        "seed" => seed(&mut app, &ctx, output),
        "run" => soak(&mut app, &ctx, output, seconds),
        "verify" => {
            let expected = load_ledger(&output.join("final-ledger.json"))?;
            app.open_game_save(&ctx, Some(output.join("stress-latest.checkpoint.json")));
            expected.verify(&app)?;
            let checkpoint = app
                .full_checkpoint()
                .ok_or("Нет восстановленного состояния.")?;
            if checkpoint.visited.len() != LOCATIONS - 1 {
                return Err("При новом процессе загрузились не все посещённые главы.".into());
            }
            let mut revisited = expected.clone();
            for _ in 0..LOCATIONS {
                revisited.go(&mut app, &ctx, true)?;
            }
            write(
                output.join("restart-verified.json"),
                json!({"pid":std::process::id(),"state":expected,"visited":checkpoint.visited.len(),
                    "verified_counters_after_restart":LOCATIONS}),
            )
        }
        _ => Err("Укажи seed, run, verify или telemetry.".into()),
    }
}

pub(super) fn render_smoke(game: GamePackage, output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = crate::offscreen::Offscreen::new(SIZE, crate::viewport3d::VIEWPORT_MSAA)?;
    crate::configure_context(display.context());
    let mut app = Studio::from_package(game, display.context());
    let path = output.join("render-smoke.checkpoint.json");
    if path.exists() {
        return Err("Для визуальной проверки нужен пустой слот.".into());
    }
    app.open_game_save(display.context(), Some(path));
    let mut video = crate::proof_video::Video::start(
        &output.join("chapter-render-smoke.mp4"),
        SIZE[0],
        SIZE[1],
        FPS,
    )?;
    let mut expected = Ledger::default();
    let mut archive = None;
    let mut timings = Vec::with_capacity(LOCATIONS + 3);
    {
        let mut sink = Sink::Video(&mut video);
        let mut driver = Driver::new(app, &mut display, &mut sink, None);
        driver.settle(10)?;
        for (index, next) in [true; LOCATIONS].into_iter().chain([false; 3]).enumerate() {
            let from = expected.scene;
            let key = if next { Key::F } else { Key::R };
            driver.display.start_timing();
            driver.frame(crate::proof_input::key(key, Modifiers::NONE))?;
            let frame_ms = driver.display.stop_timing().into_iter().next();
            expected.advance(&driver.app, next)?;
            timings.push(
                json!({"from":from,"to":expected.scene,"input_to_rgb_ms":frame_ms,
                "working_set_bytes":quality::process_sample().map(|(_, bytes)| bytes)}),
            );
            driver.settle(3)?;
            if index == 2 {
                driver.key(Key::F5)?;
                driver.expect_hud("Игра сохранена.")?;
                archive = Some(expected.clone());
            }
            driver.hold(14)?;
        }
        driver.key(Key::F9)?;
        driver.expect_hud("Игра загружена.")?;
        let archive = archive.ok_or("Не была записана промежуточная сцена.")?;
        archive.verify(&driver.app)?;
        driver.hold(28)?;
    }
    video.finish()?;
    write(
        output.join("render-smoke.json"),
        json!({"forward": LOCATIONS,"backward":3,"saved_chapter":4,"loaded_chapter":4,
            "method":"One offscreen frame with real key input, WGPU rendering, synchronization and RGB readback.",
            "transition_frames":timings,"resolution":SIZE}),
    )
}

fn telemetry(game: GamePackage, output: &Path, seconds: u64) -> Result<(), String> {
    if !(1..=7_200).contains(&seconds) {
        return Err("Длительность от 1 до 7200 секунд.".into());
    }
    let log_path = output.join("spike.log");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&log_path)
        .map_err(io_error)?;
    let mut progress =
        BufWriter::new(File::create(output.join("telemetry-progress.ndjson")).map_err(io_error)?);
    let mut telemetry = Telemetry::new(Some(log_path.clone()));
    let mut display = Offscreen::new(TELEMETRY_SIZE, crate::viewport3d::VIEWPORT_MSAA)?;
    crate::configure_context(display.context());
    let adapter = display.adapter_info().clone();
    let mut app = Studio::from_package(game, display.context());
    let mut input = proof_input::Driver::new(TELEMETRY_SIZE);
    let mut expected = Ledger::default();
    expected.verify(&app)?;
    // Инициализация окна и первый вывод начальной главы выполняются до измеряемого интервала.
    display.frame(input.raw(Vec::new()), |ctx| app.run_frame(ctx))?;
    let mut elapsed_samples = Vec::with_capacity(seconds as usize * TELEMETRY_FPS);
    let mut transitions = 0;
    let mut forward = 0;
    let mut backward = 0;
    let mut next_progress = Duration::ZERO;
    let cadence = Duration::from_secs_f64(1.0 / TELEMETRY_FPS as f64);
    let duration = Duration::from_secs(seconds);
    let cycle = 2 * LOCATIONS;
    let started = Instant::now();
    let mut next_frame = started;

    loop {
        if let Some(wait) = next_frame.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        let next =
            (elapsed_samples.len() % TELEMETRY_FPS == 0).then_some(transitions % cycle < LOCATIONS);
        let events = next.map_or_else(Vec::new, |forward| {
            proof_input::key(if forward { Key::F } else { Key::R }, Modifiers::NONE)
        });
        // Шаг виртуального ввода совпадает с частотой прогона. Ожидание не входит в замер кадра.
        input.skip_frames(1);
        let raw = input.raw(events);
        let frame_started = Instant::now();
        display.frame(raw, |ctx| app.run_frame(ctx))?;
        let rendered = Instant::now();
        let millis = rendered.duration_since(frame_started).as_secs_f64() * 1_000.0;
        let scene = app
            .link_playthrough
            .as_ref()
            .map_or(ROOT_SCENE_ID, |play| play.active);
        telemetry.record(rendered, scene, millis);
        elapsed_samples.push(millis);
        if let Some(next) = next {
            expected.advance(&app, next)?;
            transitions += 1;
            forward += usize::from(next);
            backward += usize::from(!next);
        }
        let elapsed = started.elapsed();
        if elapsed >= next_progress {
            writeln!(
                progress,
                "{}",
                json!({
                    "elapsed_wall_seconds": elapsed.as_secs_f64(), "samples": elapsed_samples.len(),
                    "transitions": transitions, "scene_id": scene,
                })
            )
            .map_err(io_error)?;
            progress.flush().map_err(io_error)?;
            next_progress += Duration::from_secs(30);
        }
        if elapsed >= duration && transitions >= cycle && transitions % cycle == 0 {
            break;
        }
        next_frame += cadence;
    }
    let elapsed = started.elapsed();
    let spike_bytes = fs::metadata(log_path).map_err(io_error)?.len();
    let spikes = elapsed_samples.iter().filter(|&&ms| ms >= 100.0).count();
    elapsed_samples.sort_by(f64::total_cmp);
    let p99 = percentile_sorted(&elapsed_samples, 0.99).ok_or("Нет замеров времени кадров.")?;
    write(
        output.join("REPORT.json"),
        json!({
            "pid": std::process::id(), "gpu": adapter.name, "backend": format!("{:?}", adapter.backend),
            "resolution": TELEMETRY_SIZE, "msaa": crate::viewport3d::VIEWPORT_MSAA,
            "requested_seconds": seconds, "elapsed_wall_seconds": elapsed.as_secs_f64(),
            "samples": elapsed_samples.len(), "chapters": LOCATIONS, "loops": transitions / cycle,
            "transitions": transitions, "forward_transitions": forward, "backward_transitions": backward,
            "p50_ms": percentile_sorted(&elapsed_samples, 0.50),
            "p95_ms": percentile_sorted(&elapsed_samples, 0.95), "p99_ms": p99,
            "max_ms": elapsed_samples.last(), "spikes_ge_100_ms": spikes, "spike_log_bytes": spike_bytes,
            "verified_ledger": expected, "frame_cadence_fps": TELEMETRY_FPS,
            "measurement": "Offscreen::frame, CPU + GPU + RGB readback, включая кадры переходов. Ожидание и запись отчёта вне интервала кадра.",
        }),
    )?;
    if p99 >= 100.0 || spikes > 0 || spike_bytes > 0 {
        return Err(format!(
            "Телеметрия не прошла бюджет. p99 {p99:.2} мс, зависаний {spikes}, журнал {spike_bytes} байт."
        ));
    }
    Ok(())
}

fn seed(app: &mut Studio, ctx: &egui::Context, output: &Path) -> Result<(), String> {
    let mut ledger = Ledger::default();
    for (name, steps) in [("archive", 3), ("incoming", 5)] {
        for _ in 0..steps {
            ledger.go(app, ctx, true)?;
        }
        let path = output.join(format!("{name}.checkpoint.json"));
        if path.exists() {
            return Err("Посеву нужны пустые слоты.".into());
        }
        app.open_game_save(ctx, Some(path.clone()));
        app.write_game_save();
        if !path.is_file() {
            return Err("Слот не создан.".into());
        }
        write(output.join(format!("{name}-ledger.json")), json!(&ledger))?;
    }
    Ok(())
}

fn soak(app: &mut Studio, ctx: &egui::Context, output: &Path, seconds: u64) -> Result<(), String> {
    if !(1..=7_200).contains(&seconds) {
        return Err("Длительность от 1 до 7200 секунд.".into());
    }
    let started = Instant::now();
    let mut log =
        BufWriter::new(File::create(output.join("stress-progress.ndjson")).map_err(io_error)?);
    let archived = output.join("archive.checkpoint.json");
    let archived_bytes = fs::read(&archived).map_err(io_error)?;
    let old = load_ledger(&output.join("archive-ledger.json"))?;
    let mut now = load_ledger(&output.join("incoming-ledger.json"))?;
    app.open_game_save(ctx, Some(output.join("incoming.checkpoint.json")));
    now.verify(app)?;
    app.open_game_save(ctx, Some(archived.clone()));
    old.verify(app)?;
    let incoming = game_save::restore(
        &output.join("incoming.checkpoint.json"),
        &app.chapter_root().linked_scenes,
    )?;
    app.load_external_checkpoint(incoming, ctx);
    now.verify(app)?;
    let live = output.join("stress-latest.checkpoint.json");
    if live.exists() {
        return Err("Часовому прогону нужен пустой рабочий слот.".into());
    }
    app.open_game_save(ctx, Some(live.clone()));

    let mut transitions = Vec::new();
    let mut checkpoints = Vec::new();
    let mut working_sets = Vec::new();
    let mut next_report = Duration::ZERO;
    let mut cycles = 0_u64;
    while started.elapsed() < Duration::from_secs(seconds) || cycles == 0 {
        for next in [true; LOCATIONS]
            .into_iter()
            .chain([false; 4])
            .chain([true; 4])
        {
            transitions.push(now.go(app, ctx, next)?);
        }
        cycles += 1;
        if started.elapsed() >= next_report {
            let restored = save_and_restore(app, ctx, &live, &now)?;
            checkpoints.push(restored);
            working_sets.extend(quality::process_sample().map(|(_, bytes)| bytes));
            writeln!(log, "{}", json!({
                "elapsed_seconds": started.elapsed().as_secs_f64(), "cycles": cycles,
                "transitions": transitions.len(), "working_set":working_sets.last(), "save_bytes":restored.0,
                "save_ms":restored.1, "restore_ms":restored.2, "chapter":now.scene,
            })).map_err(io_error)?;
            log.flush().map_err(io_error)?;
            next_report += Duration::from_secs(60);
        }
        thread::sleep(Duration::from_millis(950));
    }
    checkpoints.push(save_and_restore(app, ctx, &live, &now)?);
    working_sets.extend(quality::process_sample().map(|(_, bytes)| bytes));
    if fs::read(archived).map_err(io_error)? != archived_bytes {
        return Err("Старый слот изменился в ходе прогона.".into());
    }
    let snapshot = now.clone();
    write(output.join("final-ledger.json"), json!(&snapshot))?;
    transitions.sort_by(f64::total_cmp);
    let pct = |q: f64| percentile_sorted(&transitions, q);
    write(
        output.join("stress-summary.json"),
        json!({
            "pid":std::process::id(), "elapsed_wall_seconds":started.elapsed().as_secs_f64(),
            "chapters":LOCATIONS, "cycles":cycles, "transitions":transitions.len(),
            "mean_transition_ms":transitions.iter().sum::<f64>() / transitions.len() as f64,
            "p50_transition_ms":pct(0.50), "p95_transition_ms":pct(0.95), "p99_transition_ms":pct(0.99), "max_transition_ms":transitions.last(),
            "initial_working_set":working_sets.first(), "final_working_set":working_sets.last(),
            "peak_sampled_working_set":working_sets.iter().max(),
            "checkpoint_sample_columns":["bytes","write_ms","reload_ms"], "checkpoint_samples":checkpoints,
            "archive_unchanged":true,"loaded_older_chapter_from_second_process":true,
            "method":"Walls clocks around real Studio gameplay intent and linked-scene transition, CPU only. Each cycle visits ten chapters and reverses direction. Checks room-local counter on every entry.",
        }),
    )
}

fn save_and_restore(
    app: &mut Studio,
    ctx: &egui::Context,
    file: &Path,
    now: &Ledger,
) -> Result<(u64, f64, f64), String> {
    let start = Instant::now();
    app.write_game_save();
    let write_ms = start.elapsed().as_secs_f64() * 1_000.0;
    let bytes = fs::metadata(file).map_err(io_error)?.len();
    let state = game_save::restore(file, &app.chapter_root().linked_scenes)?;
    let expected = app.full_checkpoint().ok_or("Нет текущего состояния.")?;
    if expected != state {
        return Err("Рабочий слот отличается от состояния игры.".into());
    }
    let start = Instant::now();
    app.load_external_checkpoint(state, ctx);
    let restore_ms = start.elapsed().as_secs_f64() * 1_000.0;
    now.verify(app)?;
    Ok((bytes, write_ms, restore_ms))
}

fn load_ledger(path: &Path) -> Result<Ledger, String> {
    let state: Ledger = serde_json::from_slice(&fs::read(path).map_err(io_error)?)
        .map_err(|error| error.to_string())?;
    if !(1..=LOCATIONS as u32).contains(&state.scene) {
        return Err("В журнале указан номер главы вне диапазона проверки.".into());
    }
    Ok(state)
}

fn write(path: impl AsRef<Path>, value: Value) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}
