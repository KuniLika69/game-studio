use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::json;

use crate::{
    Studio,
    assets::Library,
    configure_context,
    game_export::{self, GamePackage},
    offscreen::Offscreen,
    proof_input,
    proof_video::Video,
    scene::Scene,
};

const SIZE: [u32; 2] = [960, 540];
const FRAMES: usize = 120;
const TIMEOUT: Duration = Duration::from_secs(75);

pub(crate) struct Job(mpsc::Receiver<Result<PathBuf, String>>);

impl Job {
    pub fn start(
        exe: PathBuf,
        scene: Scene,
        assets: Library,
        start_in_2d: bool,
        reference: Option<PathBuf>,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("verify-export".into())
            .spawn(move || {
                let result = game_export::export(&exe, &scene, &assets, start_in_2d)
                    .and_then(|()| exe.canonicalize().map_err(|error| error.to_string()))
                    .and_then(|exe| verify(&exe));
                let _ = tx.send(result.inspect(|folder| {
                    if let Some(reference) = reference {
                        let _ = compare(&reference, folder);
                    }
                }));
            })
            .map_err(|error| error.to_string())?;
        Ok(Self(rx))
    }

    pub fn poll(&mut self, ctx: &eframe::egui::Context) -> Option<Result<PathBuf, String>> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Проверка EXE прервана.".into())),
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(250));
                None
            }
        }
    }
}

fn compare(reference: &Path, current: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let read = |directory: &Path| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        Ok(serde_json::from_slice(&fs::read(
            directory.join("проверка.json"),
        )?)?)
    };
    let prior = read(reference)?;
    let latest = read(current)?;
    if prior["mode"] != latest["mode"] || prior["size"] != latest["size"] {
        return Ok(());
    }
    let before = image::open(reference.join("конец.png"))?.into_rgb8();
    let after = image::open(current.join("конец.png"))?.into_rgb8();
    if before.dimensions() != after.dimensions() || before.dimensions() != (SIZE[0], SIZE[1]) {
        return Ok(());
    }
    let mut side_by_side = image::RgbImage::new(SIZE[0] * 2, SIZE[1]);
    image::imageops::replace(&mut side_by_side, &before, 0, 0);
    image::imageops::replace(&mut side_by_side, &after, i64::from(SIZE[0]), 0);
    side_by_side.save(current.join("до-и-после.png"))?;
    let same_device = prior["adapter"] == latest["adapter"];
    let delta = same_device
        .then(|| {
            Some(
                latest["p95_frame_with_readback_ms"].as_f64()?
                    - prior["p95_frame_with_readback_ms"].as_f64()?,
            )
        })
        .flatten();
    let difference = before
        .as_raw()
        .iter()
        .zip(after.as_raw())
        .map(|(&a, &b)| u64::from(a.abs_diff(b)))
        .sum::<u64>() as f64
        / (before.as_raw().len() as f64 * 255.0);
    let report = json!({"left":reference,"right":current,"same_adapter":same_device,"mean_rgb_absolute_difference":difference,"p95_delta_with_readback_ms":delta});
    fs::write(
        current.join("сравнение.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}

fn verify(exe: &Path) -> Result<PathBuf, String> {
    let folder = create_report_dir(exe)?;
    let log_path = folder.join("запуск.log");
    let log = fs::File::create(&log_path).map_err(|error| error.to_string())?;
    let mut command = Command::new(exe);
    command
        .args([
            std::ffi::OsStr::new("--verify-exported-game"),
            folder.as_os_str(),
        ])
        .current_dir(&folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut process = command.spawn().map_err(|error| error.to_string())?;
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = process.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = process.kill();
            let _ = process.wait();
            return Err(format!(
                "Запуск не закончил проверку за {} с. {}",
                TIMEOUT.as_secs(),
                log_path.display()
            ));
        }
        thread::sleep(Duration::from_millis(150));
    };
    if status.success() && folder.join("проверка.json").is_file() {
        return Ok(folder);
    }
    let details = fs::read_to_string(&log_path).unwrap_or_default();
    let message: String = details
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(220)
        .collect();
    Err(format!("{status}. {message} {}", log_path.display()))
}

fn create_report_dir(exe: &Path) -> Result<PathBuf, String> {
    let parent = exe.parent().ok_or("Нет папки экспорта.")?;
    let stem = exe.file_stem().ok_or("Нет имени EXE.")?.to_string_lossy();
    for suffix in 1..=500 {
        let name = if suffix == 1 {
            format!("{stem}.проверка")
        } else {
            format!("{stem}.проверка-{suffix}")
        };
        let path = parent.join(name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Не удалось выбрать свободную папку проверки рядом с EXE.".into())
}

pub(crate) fn record(package: GamePackage, output: &Path) -> Result<(), String> {
    let started = Instant::now();
    let mode = if package.start_in_2d { "2D" } else { "3D" };
    let mut display = Offscreen::new(SIZE, 1)?;
    configure_context(display.context());
    let mut studio = Studio::from_package(package, display.context());
    if !studio.game_running {
        return Err(studio.status);
    }
    let mut input = proof_input::Driver::new(SIZE);
    let mut draw =
        |display: &mut Offscreen| display.frame(input.raw(Vec::new()), |ctx| studio.run_frame(ctx));
    let first = draw(&mut display)?;
    let startup_ms = started.elapsed().as_secs_f64() * 1000.0;
    save(output, "старт.png", &first)?;
    let mut video = Video::start(&output.join("запуск.mp4"), SIZE[0], SIZE[1], 30).ok();
    let mut last = first;
    let mut timings = Vec::with_capacity(FRAMES);
    for index in 0..FRAMES {
        let start = Instant::now();
        last = draw(&mut display)?;
        timings.push(start.elapsed().as_secs_f64() * 1000.0);
        if index % 2 == 0
            && video
                .as_mut()
                .is_some_and(|stream| stream.push(&last, 1).is_err())
        {
            video = None;
        }
    }
    let has_video = video.is_some_and(|stream| stream.finish().is_ok());
    save(output, "конец.png", &last)?;
    timings.sort_unstable_by(f64::total_cmp);
    let index = (0.95 * (timings.len() - 1) as f64).round() as usize;
    let report = json!({
        "status":"passed", "mode":mode, "size":SIZE, "frames":FRAMES,
        "adapter":display.adapter_info().name, "startup_through_first_render_ms":startup_ms,
        "p95_frame_with_readback_ms":timings[index], "video":has_video,
    });
    fs::write(
        output.join("проверка.json"),
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn save(output: &Path, name: &str, image: &[u8]) -> Result<(), String> {
    image::save_buffer(
        output.join(name),
        image,
        SIZE[0],
        SIZE[1],
        image::ColorType::Rgb8,
    )
    .map_err(|error| error.to_string())
}
