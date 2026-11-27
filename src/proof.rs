//! Records the editor and packaged game into a virtual viewport without desktop input.
use std::{
    ffi::OsString,
    fs,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{
    View, configure_context, game_export, offscreen::Offscreen, plan2d::Plan, proof_input,
    proof_video::Video, scene::Scene, story, viewport3d::Camera, world,
};
use eframe::egui::{self, Color32, Event, Key, Modifiers, Pos2, Rect, RichText};

#[path = "character_stage_proof.rs"]
mod character_stage_proof;
#[path = "chapter_proof.rs"]
mod chapter_proof;
#[path = "chapter_soak.rs"]
mod chapter_soak;
#[path = "games23_proof.rs"]
mod games23_proof;
#[path = "editor_proof.rs"]
mod editor_proof;
#[path = "import_proof.rs"]
mod import_proof;
#[path = "dangling_proof.rs"]
mod dangling_proof;
#[path = "hdr_proof.rs"]
mod hdr_proof;
#[path = "transparent_proof.rs"]
mod transparent_proof;
#[path = "vrm_decal_proof.rs"]
mod vrm_decal_proof;
#[path = "shadows_proof.rs"]
mod shadows_proof;
#[path = "telemetry_proof.rs"]
mod telemetry_proof;
#[path = "environment_proof.rs"]
mod environment_proof;
#[path = "final_proof.rs"]
mod final_proof;
#[path = "map2d_tools_proof.rs"]
mod map2d_tools_proof;
#[path = "mechanics_proof.rs"]
mod mechanics_proof;
#[path = "mill_walk_proof.rs"]
mod mill_walk_proof;
#[path = "object_portability_proof.rs"]
mod object_portability_proof;
#[path = "quarter_proof.rs"]
mod quarter_proof;
#[path = "side_proof.rs"]
mod side_proof;
#[path = "staged_proof.rs"]
mod staged_proof;
#[path = "tactics_proof.rs"]
mod tactics_proof;
#[path = "top_proof.rs"]
mod top_proof;
#[path = "visual_proof.rs"]
mod visual_proof;
#[path = "visual_stage_proof.rs"]
mod visual_stage_proof;
#[path = "audio_studio_proof.rs"]
mod audio_studio_proof;
#[path = "dungeon_proof.rs"]
mod dungeon_proof;
#[path = "fx_optimization_proof.rs"]
mod fx_optimization_proof;
#[path = "extension_proof.rs"]
mod extension_proof;
#[path = "review_stage_proof.rs"]
mod review_stage_proof;
#[path = "proof_chat.rs"]
mod proof_chat;
#[path = "wizard_proof.rs"]
mod wizard_proof;
#[path = "game_trial_proof.rs"]
mod game_trial_proof;
#[path = "game_trial_matrix.rs"]
mod game_trial_matrix;
#[path = "onboarding_proof.rs"]
mod onboarding_proof;
#[path = "rain_proof.rs"]
mod rain_proof;
#[path = "actor_proof.rs"]
mod actor_proof;
#[path = "flagship_proof.rs"]
mod flagship_proof;
#[path = "weather_proof.rs"]
mod weather_proof;
#[path = "workspace_proof.rs"]
mod workspace_proof;
#[path = "mira_proof.rs"]
mod mira_proof;
#[path = "world_palette_proof.rs"]
mod world_palette_proof;
#[path = "coast_proof.rs"]
mod coast_proof;
#[path = "weather_world_proof.rs"]
mod weather_world_proof;
#[path = "storybook_proof.rs"]
mod storybook_proof;

const SIZE: [u32; 2] = [1440, 810];
const FPS: u32 = 30;
const TITLE: &str = "Ключ от комнаты";
const KEY_NAME: &str = "Латунный ключ";
const ACCEPT: &str = "Найду ключ";
const DECLINE: &str = "Может, позже";
const COMPLETE: &str = "Выполнено";

pub struct Environment {
    pub scene: Option<PathBuf>,
    pub export: Option<PathBuf>,
    pub map_png: Option<PathBuf>,
    pub import_glb: Option<PathBuf>,
    pub character_file: Option<PathBuf>,
    pub effect_file: Option<PathBuf>,
    pub wav_file: Option<PathBuf>,
    pub sprite_canvas: Option<Rect>,
    pub projection: Option<Projection>,
}

pub enum Projection {
    World(Camera, Rect),
    Plan(Plan),
}

impl Projection {
    fn point(&self, at: [f32; 3]) -> Option<Pos2> {
        match self {
            Self::World(camera, rect) => camera.screen_point(*rect, at),
            Self::Plan(plan) => Some(plan.point(at[0], at[2])),
        }
    }
}

pub fn run(
    arguments: &[OsString],
    package: Option<game_export::GamePackage>,
) -> Result<(), String> {
    match arguments {
        [mode, output] if mode == "--background-frame-telemetry" && package.is_none() => {
            telemetry_proof::record(Path::new(output))
        }
        [mode, source, output] if mode == "--background-mechanics" && package.is_none() => {
            mechanics_proof::author(Path::new(source), Path::new(output))
        }
        [mode, output] if mode == "--background-mechanics-player" => mechanics_proof::player(
            package.ok_or("Нужен упакованный EXE маршрута.")?, Path::new(output),
        ),
        [mode, output] if mode == "--background-shadow-cascades" && package.is_none() => {
            shadows_proof::record(Path::new(output))
        }
        [mode, output, source] if mode == "--background-vrm-decal" && package.is_none() => {
            vrm_decal_proof::record(Path::new(output), Path::new(source))
        }
        [mode, output] if mode == "--background-transparency" && package.is_none() => {
            transparent_proof::record(Path::new(output))
        }
        [mode, output] if mode == "--background-hdr-post" && package.is_none() => {
            hdr_proof::record(Path::new(output))
        }
        [mode, output] if mode == "--background-dangling-links" && package.is_none() => {
            dangling_proof::record(Path::new(output))
        }
        [mode, output, fixtures] if mode == "--background-import-errors" && package.is_none() => {
            import_proof::record(Path::new(output), Path::new(fixtures))
        }
        [mode, folder] if mode == "--background-workspaces" && package.is_none() => {
            workspace_proof::record(Path::new(folder))
        }
        [mode, output] if mode == "--background-world-palette" && package.is_none() => {
            world_palette_proof::author(Path::new(output))
        }
        [mode, output] if mode == "--background-world-palette-player" => {
            world_palette_proof::player(package.ok_or("Нужен EXE палитры окружения.")?, Path::new(output))
        }
        [mode, output, exe] if mode == "--background-coast-author" && package.is_none() => {
            coast_proof::author(Path::new(output), Path::new(exe))
        }
        [mode, output] if mode == "--background-coast-player" => {
            coast_proof::player(package.ok_or("Нужен EXE прибрежной деревни.")?, Path::new(output))
        }
        [mode, output, exe] if mode == "--background-weather-world-author" && package.is_none() => {
            weather_world_proof::author(Path::new(output), Path::new(exe))
        }
        [mode, output] if mode == "--background-weather-world-player" => {
            weather_world_proof::player(package.ok_or("Нужен EXE живого побережья.")?, Path::new(output))
        }
        [mode, output, exe] if mode == "--background-storybook-author" && package.is_none() => {
            storybook_proof::author(Path::new(output), Path::new(exe))
        }
        [mode, output] if mode == "--background-storybook-player" => {
            storybook_proof::player(package.ok_or("Нужен EXE сказочной поляны.")?, Path::new(output))
        }
        [mode, output, releases, model, expedition] if mode == "--background-mira-author" && package.is_none() => {
            mira_proof::author(Path::new(output), Path::new(releases), Path::new(model), Path::new(expedition))
        }
        [mode, game, output] if mode == "--background-mira-player" => {
            mira_proof::player(package.ok_or("Нужен игровой EXE с Мирой.")?, &game.to_string_lossy(), Path::new(output))
        }
        [mode, folder] if mode == "--background-actor-proof" => {
            actor_proof::record(package.ok_or("Нужен экспортированный EXE мастерской.")?, Path::new(folder))
        }
        [mode, output] if mode == "--background-chapter-render-smoke" =>
            chapter_soak::render_smoke(package.ok_or("Нужен EXE связанной проверки.")?, Path::new(output)),
        [mode, output, endpoint, model] if mode == "--background-v023-matrix" && package.is_none() => {
            game_trial_matrix::run(Path::new(output), &endpoint.to_string_lossy(), &model.to_string_lossy())
        }
        [mode, output, releases, atlas] if mode == "--background-games-023" && package.is_none() =>
            games23_proof::record(Path::new(output), Path::new(releases), Path::new(atlas)),
        [mode, output, source] if mode == "--background-legacy-maps-023" && package.is_none() =>
            games23_proof::legacy(Path::new(output), Path::new(source)),
        [mode, output] if mode == "--background-v023-weather" && package.is_none() => {
            weather_proof::record(Path::new(output))
        }
        [mode, output, releases] if mode == "--background-flagship-author" && package.is_none() => {
            flagship_proof::author(Path::new(output), Path::new(releases))
        }
        [mode, pass, output] if mode == "--background-flagship-player" => {
            flagship_proof::player(
                package.ok_or("Нужен EXE «Последний свет на набережной».")?,
                &pass.to_string_lossy(), Path::new(output),
            )
        }
        [mode, output, scale] if mode == "--background-v023-blank" && package.is_none() => {
            let scale: f32 = scale.to_string_lossy().parse().map_err(|_| "Укажи масштаб 1 или 1.25.")?;
            if !matches!(scale, 1.0 | 1.25) { return Err("Укажи масштаб 1 или 1.25.".into()); }
            wizard_proof::blank_manual(Path::new(output), scale)
        }
        [mode, output] if mode == "--background-v023-trial" && package.is_none() => {
            game_trial_proof::record(Path::new(output), None)
        }
        [mode, output, endpoint, model] if mode == "--background-v023-trial" && package.is_none() => {
            game_trial_proof::record(Path::new(output), Some((&endpoint.to_string_lossy(), &model.to_string_lossy())))
        }
        [mode, suite, revision, report] if mode == "--background-check-package" => {
            crate::game_trial::run(package.ok_or("Для прогона нужен игровой EXE.")?,
                Path::new(suite), &revision.to_string_lossy(), Path::new(report))
        }
        [mode, check, output] if mode == "--background-capture-trial-slot" => {
            crate::game_trial::capture(package.ok_or("Для записи слота нужен игровой EXE.")?,
                Path::new(check), Path::new(output))
        }
        [mode, scene, game] if mode == "--background-chapter-soak-author" && package.is_none() =>
            chapter_soak::author(Path::new(scene), Path::new(game)),
        [mode, action, output, seconds] if mode == "--background-chapter-soak" =>
            chapter_soak::run(package.ok_or("Нужен EXE связанной проверки.")?,
                &action.to_string_lossy(), Path::new(output),
                seconds.to_string_lossy().parse().map_err(|_| "Некорректная длительность проверки.")?),
        [mode, output] if mode == "--background-v022-onboarding" && package.is_none() => {
            onboarding_proof::record(Path::new(output))
        }
        [mode, output, releases] if mode == "--background-rain-author" && package.is_none() => {
            rain_proof::author(Path::new(output), Path::new(releases))
        }
        [mode, profile, output] if mode == "--background-rain-player" || mode == "--background-rain-measure" => {
            let profile = if profile == "high" { crate::quality::Profile::High } else { crate::quality::Profile::Low };
            rain_proof::player(package.ok_or("Нужен EXE мастерской.")?, Path::new(output), profile,
                mode == "--background-rain-player")
        }
        [mode, output, releases] if mode == "--background-v022-wizard" && package.is_none() => {
            wizard_proof::record(Path::new(output), Path::new(releases))
        }
        [mode, endpoint, model, output] if mode == "--background-v022-wizard-live" && package.is_none() => {
            wizard_proof::real_model(&endpoint.to_string_lossy(), &model.to_string_lossy(), Path::new(output))
        }
        [mode, output, releases] if mode == "--background-chapter" && package.is_none() => {
            chapter_proof::record(Path::new(output), Path::new(releases))
        }
        [mode, pass, save, report] if mode == "--background-chapter-player" => {
            chapter_proof::player(package.ok_or("Нужен EXE главы.")?, &pass.to_string_lossy(), Path::new(save), Path::new(report))
        }
        [mode, profile, report] if mode == "--background-chapter-measure" => {
            chapter_proof::measure(package.ok_or("Нужен EXE главы.")?, profile == "high", Path::new(report))
        }
        [mode, scene, check, endpoint, model, output]
            if mode == "--background-review-live" && package.is_none() => review_stage_proof::live(
                Path::new(scene), Path::new(check), &endpoint.to_string_lossy(),
                &model.to_string_lossy(), Path::new(output),
            ),
        [mode, scene, preset, target, output]
            if mode == "--background-fx-budget" && package.is_none() =>
        {
            let preset = preset.to_str().and_then(|value| value.parse().ok())
                .ok_or("Укажи числовой номер эффекта.")?;
            let target = target.to_str().and_then(|value| value.parse().ok())
                .ok_or("Укажи целевое время кадра в мс.")?;
            fx_optimization_proof::run(Path::new(scene), preset, target, Path::new(output))
        }
        [mode, output, releases, terrace, check, action, tool]
            if mode == "--background-v021" && package.is_none() => review_stage_proof::record(
                Path::new(output), Path::new(releases), Path::new(terrace), Path::new(check),
                Path::new(action), Path::new(tool),
            ),
        [mode, output] if mode == "--background-dungeon-author" && package.is_none() => {
            dungeon_proof::smoke(Path::new(output))
        }
        [mode, report] if mode == "--background-dungeon-player" => dungeon_proof::player(
            package.ok_or("Укажи упакованную игру.")?,
            Path::new(report),
        ),
        [mode, output] if mode == "--background-dungeon-metrics" && package.is_none() => {
            dungeon_proof::metrics(Path::new(output))
        }
        [mode, output, releases, previous, donor, wav] if mode == "--background-v020" && package.is_none() => {
            character_stage_proof::record(Path::new(output), Path::new(releases), Path::new(previous), Path::new(donor), Path::new(wav))
        }
        [mode, output, wav] if mode == "--background-audio-author" && package.is_none() => {
            audio_studio_proof::author(Path::new(output), Path::new(wav))
        }
        [mode, output] if mode == "--background-audio-player" => audio_studio_proof::player(
            package.ok_or("Укажи упакованную игру.")?,
            Path::new(output),
        ),
        [mode, output, releases, verification]
            if mode == "--background-v019" && package.is_none() =>
        {
            visual_stage_proof::record(
                Path::new(output),
                Path::new(releases),
                Path::new(verification),
            )
        }
        [mode, output, releases, previous] if mode == "--background-v018" && package.is_none() => {
            environment_proof::record(Path::new(output), Path::new(releases), Path::new(previous))
        }
        [mode, output, tactical] if mode == "--background-v017-gateway" && package.is_none() => {
            mechanics_proof::smoke_gateway(Path::new(output), Path::new(tactical))
        }
        [mode, output, releases, verification, atlas]
            if mode == "--background-v017" && package.is_none() =>
        {
            mechanics_proof::record(
                Path::new(output),
                Path::new(releases),
                Path::new(verification),
                Path::new(atlas),
            )
        }
        [mode, output, releases, verification, atlas, chair]
            if mode == "--background-v016" && package.is_none() =>
        {
            editor_proof::record(
                Path::new(output),
                Path::new(releases),
                Path::new(verification),
                Path::new(atlas),
                Path::new(chair),
            )
        }
        [mode, route, metrics] if mode == "--background-v016-measure" => editor_proof::measure(
            package.ok_or("Укажи упакованную игру.")?,
            route,
            Path::new(metrics),
        ),
        [mode, from, to] if mode == "--background-benchmark-export" && package.is_none() => {
            editor_proof::benchmark_export(Path::new(from), Path::new(to))
        }
        [mode, output, releases, previous, atlas, chair, compatible]
            if mode == "--background-v015" && package.is_none() =>
        {
            final_proof::record(
                Path::new(output),
                Path::new(releases),
                Path::new(previous),
                Path::new(atlas),
                Path::new(chair),
                Path::new(compatible),
            )
        }
        [mode, output, releases, legacy_13, legacy_12, atlas]
            if mode == "--background-v014" && package.is_none() =>
        {
            staged_proof::record_014(
                Path::new(output),
                Path::new(releases),
                Path::new(legacy_13),
                Path::new(legacy_12),
                Path::new(atlas),
            )
        }
        [mode, route, metrics] if mode == "--background-staged-player" => staged_proof::player(
            package.ok_or("Укажи упакованную игру.")?,
            route,
            Path::new(metrics),
        ),
        [mode, scene, stage, moved, video, metrics, legacy]
            if mode == "--background-proof-quarter" && package.is_none() =>
        {
            quarter_proof::record(
                Path::new(scene),
                Path::new(stage),
                Path::new(moved),
                Path::new(video),
                Path::new(metrics),
                Path::new(legacy),
            )
        }
        [mode, metrics] if mode == "--background-proof-quarter-player" => quarter_proof::player(
            package.ok_or("Для проверки нужен экспортированный EXE.")?,
            Path::new(metrics),
        ),
        [mode] if mode == "--background-proof-player" => {
            let package = package.ok_or("Для проверки нужен экспортированный EXE.")?;
            let mut display = Offscreen::new(SIZE, crate::viewport3d::VIEWPORT_MSAA)?;
            configure_context(display.context());
            let app = crate::Studio::from_package(package, display.context());
            let output = std::io::stdout();
            let mut stream = BufWriter::new(output.lock());
            let mut sink = Sink::Stream(&mut stream);
            let mut driver = Driver::new(app, &mut display, &mut sink, None);
            driver.settle(10)?;
            driver.play(Route::Plan, false)?;
            driver.hold(100)?;
            stream.flush().map_err(io_error)
        }
        [mode, scene, stage, moved, video] if mode == "--background-proof" && package.is_none() => {
            record(
                Path::new(scene),
                Path::new(stage),
                Path::new(moved),
                Path::new(video),
            )
        }
        _ => Err("Укажи пути к сцене, файлу экспорта, перенесённому EXE и записи.".into()),
    }
}

fn record(scene: &Path, stage: &Path, moved: &Path, video: &Path) -> Result<(), String> {
    for path in [scene, stage, moved, video] {
        if path.exists() {
            return Err(format!("Выходной путь уже занят. {}", path.display()));
        }
    }
    if stage.parent() == moved.parent() {
        return Err("Для переноса игры нужна отдельная папка.".into());
    }
    let mut display = Offscreen::new(SIZE, crate::viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(video, SIZE[0], SIZE[1], FPS)?;
    let mut app = crate::Studio::default();
    app.city_demo();
    {
        let mut sink = Sink::Video(&mut video);
        let mut driver = Driver::new(app, &mut display, &mut sink, Some((scene, stage)));
        driver.settle(12)?;
        driver.author()?;
        driver.play(Route::Mixed, true)?;
        driver.click("Сохранить")?;
        driver.click("■ Стоп")?;
        driver.open_saved(scene)?;
        driver.click("Продолжить")?;
        driver.expect_hud(COMPLETE)?;
        driver.expect_hud(KEY_NAME)?;
        driver.click("■ Стоп")?;
        driver.click("3D")?;
        driver.click("Собрать игру для Windows")?;
        if !stage.is_file() {
            return Err(driver.app.status.clone());
        }
        driver.hold(50)?;
        app = driver.app;
    }
    fs::rename(stage, moved).map_err(io_error)?;
    let moved = moved.canonicalize().map_err(io_error)?;
    let stage_name = stage.parent().and_then(Path::file_name).unwrap_or_default();
    let moved_name = moved.parent().and_then(Path::file_name).unwrap_or_default();
    let file_name = moved.file_name().unwrap_or_default();
    let moved_frame = display.frame(proof_input::Driver::new(SIZE).raw(Vec::new()), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(230.0);
                ui.heading(RichText::new("Запуск перенесённой игры").size(32.0));
                ui.add_space(18.0);
                ui.label(
                    RichText::new(format!(
                        "{} → {}",
                        stage_name.to_string_lossy(),
                        moved_name.to_string_lossy(),
                    ))
                    .size(22.0),
                );
                ui.label(
                    RichText::new(file_name.to_string_lossy())
                        .color(crate::ACCENT)
                        .size(25.0),
                );
            });
        });
    })?;
    video.push(&moved_frame, 75)?;
    append_player(&mut video, &moved, &["--background-proof-player".as_ref()])?;
    video.finish()?;

    let mut sink = Sink::None;
    let mut driver = Driver::new(app, &mut display, &mut sink, Some((scene, stage)));
    driver.settle(3)?;
    for route in [Route::Three, Route::Plan] {
        driver.open_saved(scene)?;
        driver.play(route, true)?;
        driver.click("■ Стоп")?;
    }
    eprintln!(
        "Проверка завершена. Сцена собрана через UI, пройдена в 3D, 2D-плане, со сменой вида и в перенесённом EXE."
    );
    Ok(())
}

fn append_player(
    video: &mut Video,
    exe: &Path,
    arguments: &[&std::ffi::OsStr],
) -> Result<(), String> {
    let mut command = Command::new(exe);
    command
        .args(arguments)
        .current_dir(exe.parent().ok_or("Папка игры недоступна.")?)
        .env_remove("CARGO_HOME")
        .env_remove("RUSTUP_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped());
    if let Some(windows) = std::env::var_os("SystemRoot") {
        command.env("PATH", PathBuf::from(windows).join("System32"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut game = command.spawn().map_err(io_error)?;
    let mut frames = BufReader::new(game.stdout.take().ok_or("Нет кадров игры.")?);
    let result: Result<(), String> = (|| {
        let mut rgb = vec![0; (SIZE[0] * SIZE[1] * 3) as usize];
        let mut total = 0;
        loop {
            let mut repeats = [0; 4];
            if frames.read(&mut repeats[..1]).map_err(io_error)? == 0 {
                break;
            }
            frames.read_exact(&mut repeats[1..]).map_err(io_error)?;
            let repeats = u32::from_le_bytes(repeats);
            if repeats == 0 || repeats > 120 || total + repeats > 5_400 {
                return Err("Неожиданное количество кадров из игры.".into());
            }
            frames.read_exact(&mut rgb).map_err(io_error)?;
            video.push(&rgb, repeats)?;
            total += repeats;
        }
        if total == 0 {
            return Err("Игра завершилась без кадров.".into());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = game.kill();
    }
    let status = game.wait().map_err(io_error)?;
    result?;
    if !status.success() {
        return Err(format!(
            "Проверка перенесённой игры завершилась с кодом {status}."
        ));
    }
    Ok(())
}

enum Sink<'a> {
    None,
    Video(&'a mut Video),
    Stream(&'a mut dyn Write),
}

impl Sink<'_> {
    fn push(&mut self, rgb: &[u8], count: u32) -> Result<(), String> {
        match self {
            Self::None => Ok(()),
            Self::Video(video) => video.push(rgb, count),
            Self::Stream(stream) => stream
                .write_all(&count.to_le_bytes())
                .and_then(|()| stream.write_all(rgb))
                .map_err(io_error),
        }
    }
}

#[derive(Clone, Copy)]
enum Route {
    Three,
    Plan,
    Mixed,
}

struct Driver<'a, 'stream> {
    app: crate::Studio,
    display: &'a mut Offscreen,
    input: proof_input::Driver,
    sink: &'a mut Sink<'stream>,
    last: Vec<u8>,
    position: Option<Pos2>,
    phase: usize,
    input_actions: usize,
    close_request: bool,
}

impl<'a, 'stream> Driver<'a, 'stream> {
    fn new(
        mut app: crate::Studio,
        display: &'a mut Offscreen,
        sink: &'a mut Sink<'stream>,
        paths: Option<(&Path, &Path)>,
    ) -> Self {
        app.proof = Some(Environment {
            scene: paths.map(|(scene, _)| scene.to_owned()),
            export: paths.map(|(_, export)| export.to_owned()),
            map_png: None,
            import_glb: None,
            character_file: None,
            effect_file: None,
            wav_file: None,
            sprite_canvas: None,
            projection: None,
        });
        Self {
            app,
            display,
            input: proof_input::Driver::new(SIZE),
            sink,
            last: Vec::new(),
            position: None,
            phase: 0,
            input_actions: 0,
            close_request: false,
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> Result<(), String> {
        if let Some(pointer) = events.iter().find_map(|event| match event {
            Event::PointerMoved(point) => Some(*point),
            _ => None,
        }) {
            self.position = Some(pointer);
        }
        let is_click = events
            .iter()
            .any(|event| matches!(event, Event::PointerButton { pressed: true, .. }));
        self.input_actions += events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    Event::PointerButton { pressed: true, .. }
                        | Event::Key {
                            pressed: true,
                            repeat: false,
                            ..
                        }
                )
            })
            .count();
        let mut raw = self.input.raw(events);
        if std::mem::take(&mut self.close_request) {
            raw.viewports
                .get_mut(&raw.viewport_id)
                .unwrap()
                .events
                .push(egui::ViewportEvent::Close);
        }
        self.last = self.display.frame(raw, |ctx| {
            self.app.run_frame(ctx);
            if let Some(pos) = self.position {
                let overlay = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("proof cursor"),
                ));
                let radius = if is_click { 9.0 } else { 4.5 };
                overlay.circle_stroke(pos, radius, egui::Stroke::new(1.75_f32, Color32::WHITE));
                overlay.circle_filled(pos, 2.0, crate::ACCENT);
            }
        })?;
        self.phase += 1;
        if self.phase.is_multiple_of(2) {
            self.sink.push(&self.last, 1)?;
        }
        Ok(())
    }

    fn settle(&mut self, count: usize) -> Result<(), String> {
        for _ in 0..count {
            self.frame(Vec::new())?;
        }
        Ok(())
    }

    fn hold(&mut self, count: u32) -> Result<(), String> {
        self.sink.push(&self.last, count)
    }

    fn point(&self, text: &str, occurrence: usize) -> Result<Pos2, String> {
        let output = self.display.last_output().ok_or("Пустой интерфейс.")?;
        let point = if occurrence == usize::MAX {
            proof_input::labels(output)
                .into_iter()
                .rfind(|label| label.text == text)
                .map(|label| label.center())
        } else {
            proof_input::find_label(output, text, occurrence)
        };
        point.ok_or_else(|| {
            let available = proof_input::labels(output)
                .into_iter()
                .map(|label| label.text)
                .collect::<Vec<_>>();
            format!(
                "Элемент «{text}» ({occurrence}) не найден. Видны {}",
                available.join(" | ")
            )
        })
    }

    fn click(&mut self, text: &str) -> Result<(), String> {
        self.click_n(text, 0)
    }

    fn click_n(&mut self, text: &str, occurrence: usize) -> Result<(), String> {
        let point = self.point(text, occurrence)?;
        self.frame(proof_input::hover(point))?;
        self.hold(7)?;
        self.frame(proof_input::click(point))?;
        self.settle(4)?;
        self.hold(8)
    }

    fn edit(&mut self, field: &str, occurrence: usize, text: &str) -> Result<(), String> {
        self.input.skip_frames(42);
        self.click_n(field, occurrence)?;
        self.frame(proof_input::replace_text(text))?;
        self.settle(3)?;
        self.hold(10)
    }

    fn key(&mut self, key: Key) -> Result<(), String> {
        self.frame(proof_input::key(key, Modifiers::NONE))?;
        self.settle(3)
    }

    fn at_world(&mut self, at: [f32; 3]) -> Result<(), String> {
        let point = self
            .app
            .proof
            .as_ref()
            .and_then(|env| env.projection.as_ref())
            .and_then(|projection| projection.point(at))
            .ok_or_else(|| format!("Точка вне текущего вида {at:?}."))?;
        self.frame(proof_input::hover(point))?;
        self.hold(5)?;
        self.frame(proof_input::click(point))
    }

    fn walk_to(&mut self, target: [f32; 3]) -> Result<(), String> {
        self.at_world(target)?;
        for _ in 0..30 {
            self.settle(6)?;
            if self.app.walk_target.is_none() {
                break;
            }
        }
        let player = self.app.game_position().to_array();
        if (player[0] - target[0]).hypot(player[2] - target[2]) > 0.32 {
            return Err(format!("Игрок {player:?} не подошёл к {target:?}."));
        }
        self.settle(6)
    }

    fn author(&mut self) -> Result<(), String> {
        self.click("Новый…")?;
        self.click("Пустая сцена")?;
        self.click("Создать персонажа…")?;
        self.click("Применить внешность")?;
        // Close the character window through its title-bar control.
        let close = self.point("Редактор персонажа", 0)?;
        let close = Pos2::new((close.x + 155.0).min(SIZE[0] as f32 - 19.0), close.y);
        self.frame(proof_input::click(close))?;
        self.settle(4)?;
        if self.app.human_editor_open {
            return Err("Не удалось закрыть настройки героя.".into());
        }
        self.click("NPC")?;
        self.click("+ Создать NPC")?;
        self.click("Мужчина — Райан")?;
        self.edit("NPC 1", 1, "Райан")?;
        self.edit("X 2.00", 0, "1.28")?;
        self.key(Key::Enter)?;
        self.click("2")?;
        self.click("Удалить точку")?;

        self.click("Окружение")?;
        self.click("Построить дом")?;
        self.edit("X 7.20", 0, "0")?;
        self.key(Key::Enter)?;
        self.edit("Z -3.10", 0, "-5")?;
        self.key(Key::Enter)?;
        self.click("Дверная створка")?;
        self.click("+ Ключ")?;
        self.edit("Ключ 1", 1, KEY_NAME)?;
        self.click("Поставить на 2D-плане")?;
        self.at_world([-2.15, 0.0, -1.15])?;
        self.settle(4)?;
        self.click("Городской дом")?;
        self.click("Без замка")?;
        self.click_n(KEY_NAME, usize::MAX)?;

        self.click("NPC")?;
        self.click("Задания")?;
        self.click("+ Задание")?;
        self.edit("Новое задание", 1, TITLE)?;
        self.edit(
            "Дойди до указанного места.",
            0,
            "Найди латунный ключ и войди в комнату.",
        )?;
        self.click("Войти через дверь")?;
        self.click("Городской дом")?;
        self.click_n("Городской дом", usize::MAX)?;

        self.click("Диалог")?;
        self.click("Создать диалог")?;
        self.edit(
            "Привет!",
            usize::MAX,
            "Поможешь найти ключ? Он слева от дома. Возьми его и зайди в комнату.",
        )?;
        self.click("+ Ответ игрока")?;
        self.edit("Ответить", 0, ACCEPT)?;
        self.click("Без задания")?;
        self.click_n(TITLE, usize::MAX)?;
        self.click("+ Ответ игрока")?;
        self.edit("Ответить", 0, DECLINE)?;
        self.click("Сохранить")?;
        self.check_scene(&self.app.scene)?;
        self.hold(26)
    }

    fn check_scene(&self, scene: &Scene) -> Result<(), String> {
        let [key] = scene.world.keys.as_slice() else {
            return Err("Нужен один созданный ключ.".into());
        };
        let [house] = scene.world.buildings.as_slice() else {
            return Err("Нужен один созданный дом.".into());
        };
        let [ryan] = scene.npcs.as_slice() else {
            return Err("Нужен один созданный NPC.".into());
        };
        let [quest] = scene.story.quests.as_slice() else {
            return Err("Нужно одно созданное задание.".into());
        };
        let dialogue = scene
            .story
            .conversation(ryan.id)
            .ok_or("Диалог Райана отсутствует.")?;
        let choices = &dialogue.nodes[0].choices;
        if ryan.avatar.name != "Райан"
            || ryan.route[0].position[0] > 1.49
            || quest.title != TITLE
            || key.name != KEY_NAME
            || key.position[0] > -1.5
            || house.position[0].abs() > 0.02
            || (house.position[2] + 5.0).abs() > 0.02
            || house.door.and_then(|door| door.required_key) != Some(key.id)
            || quest.objective
                != (story::Objective::Enter {
                    building_id: house.id,
                })
            || choices.len() != 2
            || choices[0].starts_quest != Some(quest.id)
            || choices[1].starts_quest.is_some()
        {
            return Err(format!(
                "Связи квеста не совпали. Райан {:?}, ключ {key:?}, дверь {:?}, дом {:?}, цель {quest:?}, ответы {choices:?}.",
                ryan.route[0].position, house.door, house.position,
            ));
        }
        Ok(())
    }

    fn open_saved(&mut self, scene: &Path) -> Result<(), String> {
        self.click("Открыть…")?;
        self.check_scene(&Scene::load(scene)?)?;
        self.check_scene(&self.app.scene)
    }

    fn expect_hud(&self, text: &str) -> Result<(), String> {
        if self
            .display
            .last_output()
            .is_some_and(|out| proof_input::find_label_containing(out, text, 0).is_some())
        {
            Ok(())
        } else {
            Err(format!("На экране не появилось «{text}»."))
        }
    }

    fn quest(&self) -> Option<String> {
        self.app.story_session.objective(&self.app.scene.story)
    }

    fn play(&mut self, route: Route, start: bool) -> Result<(), String> {
        if matches!(route, Route::Plan) {
            self.click("2D")?;
            self.click("План")?;
        } else {
            self.click("3D")?;
            self.click("Сброс камеры")?;
        }
        if start {
            self.click("▶ Играть")?;
            self.settle(5)?;
        }
        if self.quest().is_some() {
            return Err("Новое прохождение запущено с сохранённым заданием.".into());
        }
        self.key(Key::E)?;
        self.click(DECLINE)?;
        if self.quest().is_some() {
            return Err("Отказ запустил задание.".into());
        }
        self.key(Key::E)?;
        self.click(ACCEPT)?;
        self.expect_hud("Найди латунный ключ")?;
        let house = &self.app.scene.world.buildings[0];
        let wall = house.door_bounds();
        let entrance = house.entrance_position();
        let outside = [(wall.min[0] + wall.max[0]) * 0.5, 0.0, wall.max[2] + 0.68];
        let key = self.app.scene.world.keys[0].clone();
        self.walk_to(outside)?;
        self.key(Key::E)?;
        self.expect_hud("Дверь заперта")?;
        self.hold(28)?;
        self.walk_to(key.position)?;
        self.key(Key::E)?;
        self.expect_hud("Подобран ключ")?;
        if !self.app.interactions.has_key(key.id) {
            return Err("Ключ не появился в инвентаре.".into());
        }
        if matches!(route, Route::Mixed) {
            let before = self.app.game_position();
            self.click("2D")?;
            self.click("План")?;
            self.expect_hud(KEY_NAME)?;
            self.expect_hud("Найди латунный ключ")?;
            if self.app.view != View::Two
                || !self.app.overhead
                || !self.app.interactions.has_key(key.id)
                || before.distance(self.app.game_position()) > 0.08
            {
                return Err("Инвентарь или позиция сбросились при смене вида.".into());
            }
        }
        self.walk_to(outside)?;
        self.key(Key::E)?;
        self.settle(37)?;
        if self
            .quest()
            .is_none_or(|status| status.starts_with(COMPLETE))
            || !matches!(
                self.app
                    .scene
                    .world
                    .interaction_near(&self.app.interactions, self.app.game_position().to_array()),
                Some(world::Interaction::Door { opening: false, .. })
            )
        {
            return Err("Проверка открытия двери до входа не прошла.".into());
        }
        self.hold(18)?;
        self.walk_to(entrance)?;
        self.expect_hud(COMPLETE)?;
        self.expect_hud("Внутри")?;
        self.hold(60)
    }
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}
