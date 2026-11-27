use std::{
    ffi::OsStr,
    fs,
    io::{BufWriter, Write},
    path::Path,
    time::Instant,
};

use eframe::egui::{Key, Modifiers};
use serde_json::{Value, json};

use super::{
    Driver, FPS, SIZE, Sink, append_player, io_error, quarter_proof,
    staged_proof::{panel_click, reload},
};
use crate::{
    Studio, configure_context, game_export::GamePackage, linked_scenes::ROOT_SCENE_ID, meridian,
    offscreen::Offscreen, proof_input, proof_video::Video, quality, viewport3d,
};

pub(super) fn record(output: &Path, release: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(release).map_err(io_error)?;
    let staged = output.join("Meridian-staged.exe");
    let scene = output.join("pine-meridian.robot.json");
    let game = release.join("PineMeridian.exe");
    let save = output.join("meridian-checkpoint.json");
    if [&game, &staged, &save].iter().any(|path| path.exists()) {
        return Err("Путь проверки меридиана уже занят.".into());
    }
    let mut video = Video::start(&output.join("pine-meridian.mp4"), SIZE[0], SIZE[1], FPS)?;
    {
        let mut display = display_high()?;
        let mut app = Studio {
            welcome: Some(crate::studio_welcome::Page::Home),
            ..Studio::default()
        };
        app.desk.recovery_root = Some(output.join("recovery-meridian"));
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(app, &mut display, &mut sink, Some((&scene, &staged)));
        d.settle(5)?;
        panel_click(&mut d, "Попробовать пример")?;
        panel_click(&mut d, meridian::NAME)?;
        if d.app.scene.linked_scenes.entries.len() != 2 {
            return Err(format!("Меридиан не открыт. {}", d.app.status));
        }
        d.hold(50)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "Собрать игру для Windows")?;
    }
    fs::rename(&staged, &game).map_err(io_error)?;
    let game = game.canonicalize().map_err(io_error)?;
    let output = output.canonicalize().map_err(io_error)?;
    let save = output.join(save.file_name().unwrap());
    for pass in ["outbound", "resume"] {
        let report = output.join(format!("meridian-{pass}.json"));
        append_player(
            &mut video,
            &game,
            &[
                OsStr::new("--background-chapter-player"),
                OsStr::new(pass),
                save.as_os_str(),
                report.as_os_str(),
            ],
        )?;
    }
    video.finish()?;
    Ok(())
}

pub(super) fn player(
    game: GamePackage,
    pass: &str,
    save: &Path,
    report: &Path,
) -> Result<(), String> {
    let mut display = display_high()?;
    let start = Instant::now();
    let mut app = Studio::from_package(game, display.context());
    app.open_game_save(display.context(), Some(save.to_owned()));
    let load_ms = start.elapsed().as_secs_f64() * 1_000.0;
    let mut frames = BufWriter::new(std::io::stdout().lock());
    let mut sink = Sink::Stream(&mut frames);
    let mut d = Driver::new(app, &mut display, &mut sink, None);
    d.settle(30)?;
    let start = d.phase;
    let mut transitions = Vec::new();
    match pass {
        "outbound" => {
            if save.exists() {
                return Err("Проверке выхода нужна чистая игра.".into());
            }
            begin(&mut d, &mut transitions)?;
            quarter_proof::walk(&mut d, [0.08, -5.35])?;
            d.key(Key::F5)?;
            d.expect_hud("Игра сохранена.")?;
            if !save.is_file() {
                return Err("Клавиша F5 не записала сохранение.".into());
            }
            d.hold(60)?;
        }
        "resume" => {
            assert_location(&d, 3)?;
            if (d.app.game_position().z + 5.35).abs() > 0.37 {
                return Err("Игра не восстановила точку у гряды после перезапуска.".into());
            }
            d.expect_hud("Игра загружена.")?;
            d.hold(46)?;
            return_to_a(&mut d, &mut transitions)?;
            d.hold(70)?;
        }
        _ => return Err("Выбери исходящий маршрут или продолжение.".into()),
    }
    let simulated_seconds = (d.phase - start) as f64 / 60.0;
    let position = d.app.game_position().to_array();
    frames.flush().map_err(io_error)?;
    write_json(
        report,
        json!({"pass":pass, "pid":std::process::id(), "restore_and_start_ms":load_ms,
        "simulated_seconds":simulated_seconds, "position":position, "transitions":transitions}),
    )
}

pub(super) fn measure(game: GamePackage, high: bool, output: &Path) -> Result<(), String> {
    let profile = if high {
        quality::Profile::High
    } else {
        quality::Profile::Low
    };
    let size = if high { SIZE } else { [1920, 1080] };
    let mut display = Offscreen::new(size, profile.samples())?;
    configure_context(display.context());
    let mut app = Studio::from_package(game, display.context());
    app.low_fx = !high;
    let mut sink = Sink::None;
    let mut d = Driver::new(app, &mut display, &mut sink, None);
    d.input = proof_input::Driver::new(size);
    d.settle(30)?;
    let start = d.phase;
    let mut transitions = Vec::new();
    d.display.start_timing();
    begin(&mut d, &mut transitions)?;
    return_to_a(&mut d, &mut transitions)?;
    let mut times = d.display.stop_timing();
    times.sort_by(f32::total_cmp);
    let q = |p: f32| times[((times.len() - 1) as f32 * p).ceil() as usize];
    let frames = times.len();
    let mean = times.iter().sum::<f32>() / frames as f32;
    let runtime_seconds = (d.phase - start) as f64 / 60.0;
    let adapter = d.display.adapter_info().clone();
    for _ in 0..2 {
        quarter_proof::walk(&mut d, meridian::DOOR_AT)?;
        quarter_proof::walk(&mut d, [0.2, -2.05])?;
        travel(&mut d, &mut transitions, meridian::FIRST_EXIT, 2)?;
        travel(&mut d, &mut transitions, meridian::SECOND_EXIT, 3)?;
        return_to_a(&mut d, &mut transitions)?;
    }
    write_json(
        output,
        json!({
            "route":"meridian", "profile": profile.name(), "resolution":size, "gpu":adapter.name,
            "backend":format!("{:?}",adapter.backend), "driver":adapter.driver_info,
            "msaa":profile.samples(), "shadow_map":viewport3d::shadow_size(profile.samples()),
            "mean_ms":mean, "p95_ms":q(0.95), "frames":frames, "simulated_seconds":runtime_seconds,
            "transitions":transitions, "method":"offscreen WGPU sync and RGB readback; ms on transition input frame; Windows working set at start and after a frame",
        }),
    )
}

fn display_high() -> Result<Offscreen, String> {
    let display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    Ok(display)
}

fn begin(d: &mut Driver<'_, '_>, transitions: &mut Vec<Value>) -> Result<(), String> {
    assert_location(d, ROOT_SCENE_ID)?;
    d.key(Key::E)?;
    d.click(meridian::PICK)?;
    d.expect_hud("Выбранный маршрут · 1")?;
    let [x, _, z] = meridian::KEY_AT;
    quarter_proof::walk(d, [x - 0.48, z])?;
    d.key(Key::E)?;
    d.expect_hud("Подобран ключ")?;
    quarter_proof::walk(d, meridian::DOOR_AT)?;
    d.key(Key::E)?;
    d.settle(76)?;
    check_station(d)?;
    quarter_proof::walk(d, [0.2, -2.05])?;
    travel(d, transitions, meridian::FIRST_EXIT, 2)?;
    d.key(Key::F)?;
    d.expect_hud("Золотой знак")?;
    quarter_proof::walk(d, [-1.84, -5.25])?;
    d.key(Key::F)?;
    assert_location(d, 2)?;
    travel(d, transitions, meridian::SECOND_EXIT, 3)?;
    d.key(Key::F)?;
    d.expect_hud("Башня ветров")?;
    Ok(())
}

fn return_to_a(d: &mut Driver<'_, '_>, transitions: &mut Vec<Value>) -> Result<(), String> {
    travel(d, transitions, meridian::LAST_EXIT, ROOT_SCENE_ID)?;
    check_station(d)?;
    quarter_proof::walk(d, meridian::DOOR_AT)?;
    quarter_proof::walk(d, [meridian::DOOR_AT[0], -4.25])
}

fn travel(
    d: &mut Driver<'_, '_>,
    timings: &mut Vec<Value>,
    at: [f32; 2],
    expected: u32,
) -> Result<(), String> {
    let from = location(d).ok_or("Нет исходной локации перехода.")?;
    quarter_proof::walk(d, at)?;
    let before = quality::process_sample().map(|(_, bytes)| bytes);
    let started = Instant::now();
    d.frame(proof_input::key(Key::F, Modifiers::NONE))?;
    let frame_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let after = quality::process_sample().map(|(_, bytes)| bytes);
    assert_location(d, expected)?;
    timings.push(
        json!({"from":from, "to":expected, "input_to_frame_ms":frame_ms,
                       "working_set_before_bytes":before, "working_set_after_bytes":after}),
    );
    d.settle(16)
}

fn location(d: &Driver<'_, '_>) -> Option<u32> {
    d.app.link_playthrough.as_ref().map(|play| play.active)
}

fn assert_location(d: &Driver<'_, '_>, expected: u32) -> Result<(), String> {
    if location(d) != Some(expected) {
        return Err(format!(
            "Ожидалась локация {expected}, получено {:?}. {}",
            location(d),
            d.app.status
        ));
    }
    Ok(())
}

fn check_station(d: &Driver<'_, '_>) -> Result<(), String> {
    let scene = &d.app.scene;
    let door = scene
        .world
        .buildings
        .first()
        .and_then(|house| house.doors().next())
        .ok_or("Нет двери дозорной станции.")?;
    let token = scene.world.keys.first().ok_or("Нет жетона станции.")?;
    let choice = scene
        .rules
        .counters
        .iter()
        .find(|value| value.name == meridian::CHOICE)
        .ok_or("Нет сохранённого выбора.")?;
    if d.app.interactions.fraction(&door) < 0.99
        || !d.app.interactions.has_key(token.id)
        || d.app.rule_session.counter(&scene.rules, choice.id) != Some(1)
    {
        return Err(format!(
            "Дверь, жетон или выбор не восстановлены. Дверь = {:.3}, жетон = {}, выбор = {:?}, положение {:?}.",
            d.app.interactions.fraction(&door),
            d.app.interactions.has_key(token.id),
            d.app.rule_session.counter(&scene.rules, choice.id),
            d.app.game_position()
        ));
    }
    d.expect_hud("Выбранный маршрут · 1")
}

fn write_json(path: &Path, value: Value) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}
