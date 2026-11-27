//! Background runs of the final editor and its self-contained exported game.
use std::{fs, path::Path, time::Instant};

use eframe::egui::{Key, Modifiers};
use serde_json::{Value, json};

use super::{Driver, FPS, SIZE, Sink, io_error, proof_input, quarter_proof, staged_proof};
use crate::{
    Studio, configure_context, flagship,
    game_export::GamePackage,
    offscreen::Offscreen,
    proof_video::Video,
    quality::{self, Profile},
    rain_workshop, viewport3d,
};

const PREVIEW_SLOT: &str = "flagship-before-choice.checkpoint.json";
const COMPLETE: &str = "Решение внесено в журнал";

#[derive(Clone, Copy)]
enum Branch {
    East,
    West,
}

impl Branch {
    fn answer(self) -> &'static str {
        match self {
            Self::East => flagship::EAST_ANSWER,
            Self::West => flagship::WEST_ANSWER,
        }
    }

    fn result(self) -> i32 {
        match self {
            Self::East => 1,
            Self::West => 2,
        }
    }

    fn destination(self) -> [f32; 2] {
        match self {
            Self::East => flagship::EAST_STATION,
            Self::West => flagship::WEST_STATION,
        }
    }

    fn water(self) -> f32 {
        match self {
            Self::East => 0.10,
            Self::West => -0.12,
        }
    }
}

#[derive(Default)]
struct Trace {
    path_m: f32,
    foot_frames: u32,
    max_set: Option<usize>,
    contact: Option<crate::studio_door_hand::ContactMeasure>,
    step_height_m: Option<f32>,
    marks: Vec<(&'static str, f64)>,
}

impl Trace {
    fn mark(&mut self, driver: &Driver<'_, '_>, name: &'static str) {
        self.marks.push((name, driver.phase as f64 / 60.0));
    }

    fn memory(&mut self) {
        if let Some((_, working_set)) = quality::process_sample() {
            self.max_set = Some(self.max_set.unwrap_or(0).max(working_set));
        }
    }
}

pub(super) fn author(directory: &Path, releases: &Path) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let staging = directory.join("flagship-export-stage");
    fs::create_dir_all(&staging).map_err(io_error)?;
    let scene = directory.join("last-light.robot.json");
    let staged_exe = staging.join("LastLight.exe");
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::default();
    app.desk.recovery_root = Some(directory.join("flagship-recovery"));
    let mut video = Video::start(
        &directory.join("flagship-editor.mp4"),
        SIZE[0],
        SIZE[1],
        FPS,
    )?;
    {
        let mut sink = Sink::Video(&mut video);
        let mut driver = Driver::new(app, &mut display, &mut sink, Some((&scene, &staged_exe)));
        driver.settle(6)?;
        if driver.point("Попробовать пример", 0).is_ok() {
            driver.click("Попробовать пример")?;
        } else {
            driver.click("Новый…")?;
        }
        driver.click(flagship::NAME)?;
        confirm_scene(&driver.app)?;
        driver.settle(28)?;
        screenshot(&driver, directory, "flagship-editor.png", SIZE)?;
        staged_proof::panel_click(&mut driver, "Сохранить как…")?;
        staged_proof::reload(&mut driver)?;
        confirm_scene(&driver.app)?;
        staged_proof::panel_click(&mut driver, "Собрать игру для Windows")?;
    }
    video.finish()?;
    let target = releases.join("LastLight.exe");
    if target.exists() {
        return Err("Сборка LastLight.exe уже есть в папке назначения.".into());
    }
    fs::rename(staged_exe, target).map_err(io_error)
}

fn confirm_scene(app: &Studio) -> Result<(), String> {
    let scene = &app.scene;
    if scene
        .rules
        .counters
        .iter()
        .any(|counter| counter.name == flagship::OUTCOME)
    {
        scene.validate()
    } else {
        Err(format!("Не открылась сцена набережной. {}", app.status))
    }
}

pub(super) fn player(package: GamePackage, mode: &str, output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let record = matches!(mode, "east-video" | "west-video");
    let smoke = mode == "smoke";
    let alternate = mode == "west-video";
    let resume = mode == "resume";
    if !(record || smoke || resume || matches!(mode, "high" | "low" | "compare" | "neutral")) {
        return Err(
            "Выбери east-video, west-video, smoke, resume, high, low, compare или neutral.".into(),
        );
    }
    let profile = if matches!(mode, "low" | "compare" | "neutral") {
        Profile::Low
    } else {
        Profile::High
    };
    let size = if record { SIZE } else { [1920, 1080] };
    let mut display = Offscreen::new(size, profile.samples())?;
    configure_context(display.context());
    let opening = Instant::now();
    let mut app = Studio::from_package(package, display.context());
    app.low_fx = profile == Profile::Low;
    confirm_scene(&app)?;
    if mode == "neutral" {
        let environment = app
            .scene
            .environment
            .as_mut()
            .ok_or("Нет среды мастерской.")?;
        environment.weather.initial = crate::environment::WeatherPreset::Clear;
        environment.time.start_hour = 13.4;
        environment.ground_mist_height = 0.0;
        app.scene.lighting = Default::default();
        for model in &mut app.scene.world.models {
            model.light = None;
        }
        app.start_play(display.context(), false);
    }
    if matches!(mode, "compare" | "neutral") {
        // Use the baseline viewport bounds. The captured package retains the actual quest HUD.
        app.scene.rules.counters.clear();
    }
    let slot = output.join(if alternate {
        "last-light-west.checkpoint.json"
    } else {
        "last-light.checkpoint.json"
    });
    if alternate {
        fs::copy(output.join(PREVIEW_SLOT), &slot).map_err(io_error)?;
    }
    if record || smoke || resume {
        app.open_game_save(display.context(), Some(slot.clone()));
    }
    let startup_ms = opening.elapsed().as_secs_f64() * 1_000.0;
    let mut video = record
        .then(|| {
            Video::start(
                &output.join(format!("flagship-{mode}-silent.mp4")),
                size[0],
                size[1],
                FPS,
            )
        })
        .transpose()?;
    if record {
        app.set_audio_recording(output.join(format!("flagship-{mode}.wav")))?;
    }
    let mut sink = video.as_mut().map_or(Sink::None, Sink::Video);
    let mut driver = Driver::new(app, &mut display, &mut sink, None);
    driver.input = proof_input::Driver::new(size);
    driver.settle(80)?;
    if mode == "compare" {
        return screenshot(&driver, output, "flagship-final-1080.png", size);
    }
    if mode == "neutral" {
        return screenshot(&driver, output, "flagship-neutral-1080.png", size);
    }
    if resume {
        require_stage(&driver, 7)?;
        require_result(&driver, Branch::East)?;
        require_water(&driver, Branch::East)?;
        driver.expect_hud("Игра загружена.")?;
        return write_report(
            output,
            mode,
            json!({
                "pid":std::process::id(), "stage":7, "outcome":1, "bytes":fs::metadata(slot).map_err(io_error)?.len(),
                "startup_and_restore_ms":startup_ms, "position":driver.app.game_position().to_array(),
            }),
        );
    }

    let mut trace = Trace::default();
    trace.memory();
    let start_tick = driver.phase;
    let shots = record.then_some((output, size));
    let run = !record || alternate;
    driver.display.start_timing();
    if alternate {
        require_stage(&driver, 4)?;
        driver.expect_hud("Игра загружена.")?;
    } else {
        opening_route(&mut driver, run, &mut trace, shots)?;
        if record || smoke {
            driver.key(Key::F5)?;
            driver.expect_hud("Игра сохранена.")?;
            fs::copy(&slot, output.join(PREVIEW_SLOT)).map_err(io_error)?;
        }
    }
    let branch = if alternate {
        Branch::West
    } else {
        Branch::East
    };
    choice_route(&mut driver, branch, run, !smoke, &mut trace, shots)?;
    if record || smoke {
        driver.key(Key::F5)?;
        driver.expect_hud("Игра сохранена.")?;
        if !alternate {
            quarter_proof::walk(&mut driver, [0.02, -10.50])?;
            driver.key(Key::F9)?;
            driver.expect_hud("Игра загружена.")?;
            require_stage(&driver, 7)?;
            require_result(&driver, branch)?;
        }
    }
    let recorded = driver.app.audio_recording_stats();
    if let Some(audio) = recorded {
        if audio.dialogue_player == 0
            || (!alternate && audio.dialogue_npc == 0)
            || audio.footsteps[0] == 0
            || audio.planted_footsteps == 0
            || audio.rain_mix_frames == 0
            || audio.inside_mix_frames == 0
        {
            return Err(format!(
                "В миксе неполный набор реплик, шагов и среды. {audio:?}"
            ));
        }
        driver.app.write_recorded_audio()?;
    }
    let seconds = (driver.phase - start_tick) as f64 / 60.0;
    if mode == "east-video" && !(180.0..=300.0).contains(&seconds) {
        return Err(format!(
            "Непрерывный маршрут занял {seconds:.2} с, нужно 180–300 с."
        ));
    }
    let adapter = driver.display.adapter_info().clone();
    let raw = driver.display.stop_timing();
    let mut timings = raw.clone();
    timings.sort_by(f32::total_cmp);
    let percentile = |p: f32| timings[((timings.len() - 1) as f32 * p).ceil() as usize];
    trace.memory();
    write_report(
        output,
        mode,
        json!({
            "pid":std::process::id(), "gpu":adapter.name, "driver":adapter.driver_info,
            "backend":format!("{:?}",adapter.backend), "profile":profile.name(), "frame_ms": raw,
            "msaa":profile.samples(), "shadow_map":viewport3d::shadow_size(profile.samples()),
            "internal_resolution":size, "output_resolution":size, "frames":timings.len(),
            "p50_ms":percentile(0.5), "p95_ms":percentile(0.95), "p99_ms":percentile(0.99),
            "mean_ms":timings.iter().sum::<f32>() / timings.len() as f32, "seconds":seconds,
            "long_leg_displacement_m":trace.path_m, "wet_frames_during_run":trace.foot_frames,
            "peak_sampled_working_set_bytes":trace.max_set, "door_hand":trace.contact, "video_marks":trace.marks,
            "step_height_m":trace.step_height_m,
            "outcome":branch.result(), "stage":7, "audio":recorded, "startup_and_restore_ms":startup_ms,
            "basin_level_m":require_water(&driver, branch)?,
            "method":"whole offscreen frame through WGPU sync and RGB readback; present/encoding excluded",
        }),
    )?;
    if let Some(video) = video {
        video.finish()?;
    }
    Ok(())
}

fn opening_route(
    driver: &mut Driver<'_, '_>,
    run: bool,
    trace: &mut Trace,
    shots: Option<(&Path, [u32; 2])>,
) -> Result<(), String> {
    trace.mark(driver, "старт к западу");
    travel(driver, [-2.38, -0.25], run, trace)?;
    require_stage(driver, 1)?;
    travel(driver, flagship::WEST_SURVEY, run, trace)?;
    driver.key(Key::F)?;
    require_stage(driver, 2)?;
    if let Some((output, size)) = shots {
        driver.settle(22)?;
        screenshot(driver, output, "flagship-west.png", size)?;
        inspect_street(driver, output, size)?;
    }
    trace.mark(driver, "разворот на восток");
    travel(driver, flagship::EAST_SURVEY, run, trace)?;
    driver.key(Key::F)?;
    require_stage(driver, 3)?;
    if let Some((output, size)) = shots {
        screenshot(driver, output, "flagship-east.png", size)?;
    }
    travel(driver, [0.0, -3.42], run, trace)?;
    quarter_proof::walk(driver, [0.0, -2.60])?;
    quarter_proof::turn(driver, 0.0, 0.24)?;
    trace.mark(driver, "разгон у лужи");
    sprint_puddle(driver, trace, shots)?;
    enter_workshop(driver, trace, shots)?;
    require_stage(driver, 4)?;
    quarter_proof::walk(driver, flagship::INTERVIEW)
}

fn inspect_street(
    driver: &mut Driver<'_, '_>,
    output: &Path,
    size: [u32; 2],
) -> Result<(), String> {
    let player = driver.app.game_position();
    let target = driver.app.camera.target;
    click(driver, "Свободная камера")?;
    driver.frame(vec![proof_input::key_event(
        Key::PageUp,
        Modifiers::NONE,
        true,
    )])?;
    driver.settle(40)?;
    driver.frame(vec![proof_input::key_event(
        Key::PageUp,
        Modifiers::NONE,
        false,
    )])?;
    if !driver.app.free_camera
        || driver.app.camera.target[1] < target[1] + 1.0
        || driver.app.game_position().distance(player) > 0.02
    {
        return Err("Свободная камера не поднялась над улицей или сместила героя.".into());
    }
    quarter_proof::turn(driver, -0.78, 0.40)?;
    screenshot(driver, output, "flagship-free-camera.png", size)?;
    click(driver, "Свободная камера")?;
    driver.settle(15)
}

fn sprint_puddle(
    driver: &mut Driver<'_, '_>,
    trace: &mut Trace,
    shots: Option<(&Path, [u32; 2])>,
) -> Result<(), String> {
    let from = driver.app.game_position();
    let before = trace.foot_frames;
    let mut top_speed = 0.0_f32;
    driver.input.set_modifiers(Modifiers::SHIFT);
    driver.frame(vec![proof_input::key_event(Key::W, Modifiers::SHIFT, true)])?;
    for _ in 0..108 {
        driver.frame(vec![])?;
        top_speed = top_speed.max(driver.app.game.horizontal_speed());
        if !driver.app.environment_splashes.is_empty() {
            trace.foot_frames += 1;
            if let Some((output, size)) = shots
                && trace.foot_frames == before + 5
            {
                screenshot(driver, output, "flagship-splash.png", size)?;
            }
        }
        if driver.app.game_position().z <= -5.67 {
            break;
        }
    }
    driver.input.set_modifiers(Modifiers::NONE);
    driver.frame(vec![proof_input::key_event(Key::W, Modifiers::NONE, false)])?;
    trace.path_m += from.distance(driver.app.game_position());
    if trace.foot_frames == before || top_speed < 2.4 {
        return Err(format!(
            "На пробеге по луже нет брызг от стопы. Старт {from:?}, финиш {:?}, скорость {top_speed:.2} м/с, yaw {:.2}.",
            driver.app.game_position(),
            driver.app.camera.yaw
        ));
    }
    driver.settle(24)
}

fn choice_route(
    driver: &mut Driver<'_, '_>,
    branch: Branch,
    run: bool,
    animate: bool,
    trace: &mut Trace,
    shots: Option<(&Path, [u32; 2])>,
) -> Result<(), String> {
    driver.key(Key::E)?;
    require_speaker(driver, 1)?;
    trace.mark(driver, "реплика Лиды");
    if animate {
        driver.settle(105)?;
        driver.key(Key::F8)?;
        driver.settle(92)?;
        if let Some((output, size)) = shots {
            screenshot(driver, output, "flagship-dialogue-lida.png", size)?;
        }
        wait_line(driver, 1, 206)?;
        driver.key(Key::F8)?;
    }
    click(driver, branch.answer())?;
    require_stage(driver, 5)?;
    require_speaker(driver, if matches!(branch, Branch::East) { 2 } else { 3 })?;
    trace.mark(driver, "реплика Арсена");
    if animate {
        driver.settle(68)?;
        driver.key(Key::F8)?;
        driver.settle(54)?;
        if let Some((output, size)) = shots {
            screenshot(driver, output, "flagship-dialogue-reply.png", size)?;
        }
        wait_line(
            driver,
            if matches!(branch, Branch::East) { 2 } else { 3 },
            126,
        )?;
    }
    click(driver, "Завершить")?;
    quarter_proof::walk(driver, [0.03, -7.96])?;
    quarter_proof::walk(driver, [0.0, -5.39])?;
    quarter_proof::walk(driver, [0.0, -3.42])?;
    travel(driver, branch.destination(), run, trace)?;
    driver.key(Key::F)?;
    trace.mark(
        driver,
        if matches!(branch, Branch::East) {
            "пуск на востоке"
        } else {
            "сброс на западе"
        },
    );
    require_stage(driver, 6)?;
    require_result(driver, branch)?;
    if let Some((output, size)) = shots {
        driver.settle(60)?;
        screenshot(driver, output, "flagship-outcome.png", size)?;
    }
    travel(driver, [0.0, -3.42], run, trace)?;
    enter_workshop(driver, trace, shots)?;
    quarter_proof::walk(driver, flagship::LEDGER)?;
    driver.key(Key::F)?;
    trace.mark(driver, "запись в журнал");
    require_stage(driver, 7)?;
    require_result(driver, branch)?;
    driver.expect_hud(COMPLETE)?;
    if let Some((output, size)) = shots {
        screenshot(driver, output, "flagship-completed.png", size)?;
    }
    Ok(())
}

fn enter_workshop(
    driver: &mut Driver<'_, '_>,
    trace: &mut Trace,
    shots: Option<(&Path, [u32; 2])>,
) -> Result<(), String> {
    quarter_proof::walk(driver, flagship::WORKSHOP_DOOR)?;
    let height = driver.app.game_position().y;
    if !(0.055..=0.105).contains(&height) {
        return Err(format!(
            "Герой не поднялся на гранитную ступень. Высота {height:.3} м."
        ));
    }
    trace.step_height_m = Some(height);
    let door = driver.app.scene.world.buildings[0]
        .doors()
        .next()
        .ok_or("Нет двери.")?;
    let fraction = driver.app.interactions.fraction(&door);
    if fraction < 0.90 {
        quarter_proof::turn(driver, 0.68, 0.19)?;
        trace.mark(driver, "дотянуться до ручки");
        driver.key(Key::E)?;
        for _ in 0..240 {
            driver.frame(vec![])?;
            if driver.app.door_contact.is_some() {
                break;
            }
        }
        driver.settle(6)?;
        let contact = driver
            .app
            .door_contact
            .ok_or("Не измерено касание ручки.")?;
        if contact.gap_m > 0.020 || contact.leaf_fraction_at_contact > 0.001 {
            return Err(format!("Рука отстала от ручки. {contact:?}"));
        }
        trace.contact = Some(contact);
        trace.mark(driver, "касание ручки");
        if let Some((output, size)) = shots {
            screenshot(driver, output, "flagship-door.png", size)?;
        }
        driver.settle(46)?;
    }
    quarter_proof::turn(driver, 0.0, 0.24)?;
    quarter_proof::walk(driver, [0.08, -8.24])
}

fn wait_line(driver: &mut Driver<'_, '_>, id: u32, seen_ticks: usize) -> Result<(), String> {
    let scene = &driver.app.scene;
    let node = scene.story.conversations[0]
        .nodes
        .iter()
        .find(|node| node.id == id)
        .ok_or("Нет реплики.")?;
    let clip = scene
        .audio
        .sound(node.voice.ok_or("Нет озвучки.")?)
        .ok_or("Нет звуковой дорожки.")?
        .clip()?;
    let duration_ticks = (clip.duration() * 60.0).ceil() as usize;
    driver.settle(duration_ticks.saturating_add(12).saturating_sub(seen_ticks))
}

fn travel(
    driver: &mut Driver<'_, '_>,
    target: [f32; 2],
    run: bool,
    trace: &mut Trace,
) -> Result<(), String> {
    let destination = glam::Vec2::from(target);
    for _ in 0..9 {
        let from = driver.app.game_position();
        let at = glam::Vec2::new(from.x, from.z);
        let distance = at.distance(destination);
        if distance < 0.41 {
            return Ok(());
        }
        travel_segment(driver, destination, run, trace)?;
        let moved = driver.app.game_position();
        let left = glam::Vec2::new(moved.x, moved.z).distance(destination);
        trace.path_m += moved.distance(from);
        if left <= 4.70 {
            if left > 0.41 {
                quarter_proof::walk(driver, target)?;
                trace.path_m += driver.app.game_position().distance(moved);
            }
            return Ok(());
        }
        if left >= distance - 0.65 {
            return Err(format!("Участок до {target:?} перекрыт в точке {moved:?}."));
        }
    }
    Err(format!(
        "Не удалось выйти к {target:?} от {:?}",
        driver.app.game_position()
    ))
}

fn travel_segment(
    driver: &mut Driver<'_, '_>,
    goal: glam::Vec2,
    run: bool,
    trace: &mut Trace,
) -> Result<(), String> {
    let from = driver.app.game_position();
    let start = glam::Vec2::new(from.x, from.z);
    let delta = goal - start;
    let length = delta.length();
    let direction = delta / length;
    let yaw = (-direction.x).atan2(-direction.y) - driver.app.camera_shoulder;
    quarter_proof::turn(driver, yaw, 0.23)?;
    let modifiers = if run {
        Modifiers::SHIFT
    } else {
        Modifiers::NONE
    };
    driver.input.set_modifiers(modifiers);
    driver.frame(vec![proof_input::key_event(Key::W, modifiers, true)])?;
    let max = (length * if run { 25.0 } else { 52.0 }).ceil() as usize + 180;
    for tick in 0..max {
        driver.frame(vec![])?;
        let at = driver.app.game_position();
        let offset = goal - glam::Vec2::new(at.x, at.z);
        let remaining = offset.dot(direction);
        if !driver.app.environment_splashes.is_empty() {
            trace.foot_frames += 1;
        }
        if tick % 210 == 0 {
            trace.memory();
        }
        if remaining < if run { 0.63 } else { 0.28 }
            || (offset - direction * remaining).length() > 1.02
        {
            break;
        }
    }
    driver.input.set_modifiers(Modifiers::NONE);
    driver.frame(vec![proof_input::key_event(Key::W, Modifiers::NONE, false)])?;
    driver.settle(24)?;
    Ok(())
}

fn require_water(driver: &Driver<'_, '_>, branch: Branch) -> Result<f32, String> {
    let config = driver.app.scene.environment.as_ref().ok_or("Нет среды.")?;
    let basin = config
        .waters
        .iter()
        .find(|water| water.name == "Западный отводной лоток")
        .ok_or("Нет западного отводного лотка.")?;
    let actual = driver
        .app
        .environment_session
        .as_ref()
        .and_then(|session| session.level(config, basin.id))
        .ok_or("Уровень лотка недоступен.")?;
    if (actual - branch.water()).abs() > 0.002 {
        return Err(format!(
            "Уровень воды {actual:.3} м, ожидалось {:.3} м.",
            branch.water()
        ));
    }
    Ok(actual)
}

fn require_speaker(driver: &Driver<'_, '_>, id: u32) -> Result<(), String> {
    if driver.app.story_session.active_npc() != Some(rain_workshop::NPC_ID)
        || driver
            .app
            .story_session
            .current(&driver.app.scene.story)
            .is_none_or(|(_, node)| node.id != id)
    {
        return Err(format!(
            "Не открыта реплика {id}. Герой {:?}, NPC {:?}, ближайший собеседник {:?}, статус {}",
            driver.app.game_position(),
            driver.app.npc_position(rain_workshop::NPC_ID),
            driver.app.nearby_talk_npc(),
            driver.app.status,
        ));
    }
    Ok(())
}

fn require_stage(driver: &Driver<'_, '_>, expected: u16) -> Result<(), String> {
    let stage = driver
        .app
        .rule_session
        .quest_stage(driver.app.scene.story.quests[0].id);
    if stage != Some(expected) {
        return Err(format!(
            "Ожидался этап {expected}, получен {stage:?}, позиция {:?}. Последние правила: {:?}.",
            driver.app.game_position(),
            driver
                .app
                .rule_session
                .trace()
                .rev()
                .take(4)
                .collect::<Vec<_>>(),
        ));
    }
    Ok(())
}

fn require_result(driver: &Driver<'_, '_>, branch: Branch) -> Result<(), String> {
    let rules = &driver.app.scene.rules;
    let action = rules
        .counters
        .iter()
        .find(|value| value.name == flagship::OUTCOME)
        .ok_or("Нет результата.")?;
    if driver.app.rule_session.counter(rules, action.id) != Some(branch.result()) {
        return Err("Игра раскрыла неверный исход.".into());
    }
    let has_light = |text: &str| {
        driver
            .app
            .scene
            .world
            .models
            .iter()
            .find(|model| model.label == text)
            .is_some_and(|model| driver.app.interactions.object_active(model.id))
    };
    let east = has_light("Янтарный маяк восточного шлюза");
    let west = has_light("Голубой маяк западного лотка");
    if (east, west)
        != (
            matches!(branch, Branch::East),
            matches!(branch, Branch::West),
        )
    {
        return Err(format!(
            "Свет исходов перепутан. Восток {east}, запад {west}."
        ));
    }
    Ok(())
}

fn click(driver: &mut Driver<'_, '_>, label: &str) -> Result<(), String> {
    let position = driver.point(label, 0)?;
    driver.frame(proof_input::click(position))?;
    driver.settle(5)
}

fn screenshot(
    driver: &Driver<'_, '_>,
    output: &Path,
    filename: &str,
    [width, height]: [u32; 2],
) -> Result<(), String> {
    image::save_buffer(
        output.join(filename),
        &driver.last,
        width,
        height,
        image::ColorType::Rgb8,
    )
    .map_err(|error| error.to_string())
}

fn write_report(output: &Path, mode: &str, report: Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    fs::write(output.join(format!("flagship-{mode}.json")), bytes).map_err(io_error)
}
