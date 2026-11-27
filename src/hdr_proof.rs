use std::{fs, ops::Range, path::Path};

use eframe::egui;
use serde_json::json;
use studio_core::render_settings::PostProcess;

use super::{Driver, FPS, SIZE, Sink, io_error, proof_input};
use crate::{
    EditorTab, Studio, configure_context, material_graph, model,
    offscreen::Offscreen,
    proof_video::Video,
    viewport3d::{self, Camera, ModelDraw, ViewportScene},
};

const RESOLUTION: [u32; 2] = [2560, 1440];
const WARMUP: usize = 24;
const SAMPLES: usize = 150;

pub(super) fn record(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new_with_gpu_timing(RESOLUTION, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let adapter = display.adapter_info().clone();
    let mut app = fixture()?;
    app.scene.post_process.bloom = true;
    app.scene.save(&output.join("hdr-reference.robot.json"))?;
    let scene = Reference::new(&app)?;
    let mut input = proof_input::Driver::new(RESOLUTION);
    let camera = app.camera;
    let bloom = app.scene.post_process;
    for _ in 0..WARMUP {
        scene.frame(&mut display, &mut input, camera, bloom)?;
    }
    display.take_gpu_frame_times();
    display.start_timing();
    let mut on = Vec::new();
    for _ in 0..SAMPLES {
        on = scene.frame(&mut display, &mut input, camera, bloom)?;
    }
    let wall = timings(&display.stop_timing())?;
    let gpu = display.take_gpu_frame_times();
    if gpu.len() != SAMPLES {
        return Err(format!("GPU вернул {} из {SAMPLES} замеров.", gpu.len()));
    }
    let phases = |read: fn(&crate::offscreen::GpuFrameTimes) -> f32| -> Result<Timings, String> {
        timings(&gpu.iter().map(read).collect::<Vec<_>>())
    };
    let geometry = phases(|frame| frame.geometry_ms)?;
    let blur = phases(|frame| frame.bloom_ms)?;
    let composite = phases(|frame| frame.composite_ms)?;
    let post = phases(|frame| frame.bloom_ms + frame.composite_ms)?;
    let total = phases(|frame| frame.geometry_ms + frame.bloom_ms + frame.composite_ms)?;
    if post.p95 > 3.0 || total.fps < 60.0 {
        return Err(format!(
            "GPU 1440p: пост p95 = {:.3} мс, скорость {:.1} FPS.",
            post.p95, total.fps
        ));
    }

    let off = scene.frame(&mut display, &mut input, camera, PostProcess::default())?;
    let higher_threshold = PostProcess {
        bloom_threshold: 8.0,
        ..bloom
    };
    let high = scene.frame(&mut display, &mut input, camera, higher_threshold)?;
    let brighter = PostProcess {
        exposure_ev: 1.0,
        ..PostProcess::default()
    };
    let exposed = scene.frame(&mut display, &mut input, camera, brighter)?;
    let pixels = check_pixels(&off, &on, &high, &exposed)?;
    for (name, data) in [
        ("bloom-off.png", &off),
        ("bloom-on.png", &on),
        ("bloom-threshold8.png", &high),
        ("exposure-plus1.png", &exposed),
    ] {
        save_frame(&output.join(name), data, RESOLUTION)?;
    }
    let metrics = json!({
        "gpu": adapter.name, "backend": format!("{:?}", adapter.backend),
        "resolution": RESOLUTION, "msaa": viewport3d::VIEWPORT_MSAA,
        "camera": { "yaw": camera.yaw, "pitch": camera.pitch, "target": camera.target, "distance": camera.distance },
        "post_process": bloom,
        "frame_including_readback": wall, "gpu_geometry": geometry, "gpu_bloom": blur,
        "gpu_composite": composite, "gpu_post_total": post, "gpu_frame_total": total,
        "pixel_checks": pixels,
    });
    fs::write(
        output.join("after.json"),
        serde_json::to_vec_pretty(&metrics).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)?;
    editor_proof(output, fixture()?)?;
    let mut video = Video::start(&output.join("after.mp4"), RESOLUTION[0], RESOLUTION[1], FPS)?;
    video.push(&off, 4 * FPS)?;
    video.push(&on, 5 * FPS)?;
    for step in 0..(8 * FPS / 2) {
        let phase = std::f32::consts::TAU * step as f32 / (8.0 * FPS as f32 / 2.0);
        let orbit = Camera {
            yaw: camera.yaw + phase.sin() * 0.22,
            ..camera
        };
        let frame = scene.frame(&mut display, &mut input, orbit, bloom)?;
        video.push(&frame, 2)?;
    }
    video.push(&high, 3 * FPS)?;
    video.push(&exposed, 3 * FPS)?;
    video.push(&on, 3 * FPS)?;
    video.finish()
}

fn fixture() -> Result<Studio, String> {
    let mut app = Studio::default();
    app.courtyard_demo();
    app.scene.lighting.daylight = 0.38;
    for (name, color, strength) in [
        ("Ящик во дворе", [0.14, 0.7, 1.0], 6.0),
        ("Ящик у двери", [1.0, 0.32, 0.05], 4.0),
    ] {
        let item = app
            .scene
            .world
            .models
            .iter_mut()
            .find(|item| item.label == name)
            .ok_or_else(|| format!("Не найдена модель «{name}»."))?;
        item.material.emissive_color = Some(color);
        item.material.emissive_strength = strength;
    }
    app.scene.avatar = None;
    Ok(app)
}

struct Reference<'a> {
    editor: &'a Studio,
    environment: Vec<crate::scene::Part>,
    architecture: Vec<crate::scene::Part>,
    models: Vec<ModelDraw<'a>>,
}

impl<'a> Reference<'a> {
    fn new(editor: &'a Studio) -> Result<Self, String> {
        let models = editor
            .scene
            .world
            .models
            .iter()
            .map(|item| {
                let asset = editor
                    .assets
                    .get(&item.source)
                    .ok_or_else(|| format!("Не загружена модель «{}».", item.label))?;
                Ok(ModelDraw {
                    token: asset.token,
                    model: &asset.model,
                    position: item.visual_position(),
                    rotation: item.rotation,
                    scale: item.scale,
                    selected: false,
                    tuning: item.material,
                    vegetation: [0.0; 4],
                    receives_weather: true,
                    graph: None,
                    material_preview: material_graph::PreviewLight::Scene,
                    clip: None,
                    elapsed: 0.0,
                    blend: None,
                    bone_offsets: Vec::new(),
                    bone_translations: Vec::new(),
                    morph_weights: Vec::new(),
                    secondary: model::SecondaryPose::default(),
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            editor,
            environment: editor.environment_parts(),
            architecture: editor.scene.world.building_parts(false),
            models,
        })
    }

    fn frame(
        &self,
        display: &mut Offscreen,
        input: &mut proof_input::Driver,
        camera: Camera,
        post_process: PostProcess,
    ) -> Result<Vec<u8>, String> {
        display.frame(input.raw(Vec::new()), |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    viewport3d::paint(
                        ui,
                        ui.available_rect_before_wrap(),
                        ViewportScene {
                            robot: &[],
                            environment: &self.environment,
                            architecture: &self.architecture,
                            models: &self.models,
                            camera,
                            selected: None,
                            sparks: &[],
                            glow: 0.0,
                            playing: false,
                            lighting: self.editor.scene.lighting,
                            lights: &[],
                            natural: None,
                            effects: &[],
                            post_process,
                        },
                    );
                });
        })
    }
}

fn editor_proof(output: &Path, mut app: Studio) -> Result<(), String> {
    app.editor_tab = EditorTab::World;
    app.desk.recovery_root = Some(output.join("recovery"));
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut sink = Sink::None;
    let mut d = Driver::new(app, &mut display, &mut sink, None);
    d.settle(5)?;
    d.click("Постобработка")?;
    d.point("Экспозиция", 0)?;
    d.click("Свечение (Bloom)")?;
    if !d.app.scene.post_process.bloom {
        return Err("Переключатель редактора не применил Bloom.".into());
    }
    if d.point("Порог свечения", 0).is_err() {
        d.click("Постобработка")?;
    }
    d.point("Порог свечения", 0)?;
    save_frame(&output.join("editor-controls.png"), &d.last, SIZE)
}

fn check_pixels(
    off: &[u8],
    on: &[u8],
    high: &[u8],
    exposed: &[u8],
) -> Result<serde_json::Value, String> {
    let halo = |image| luminance(image, 2055..2195, 714..724);
    let ground = |image| luminance(image, 820..955, 940..1000);
    let off_halo = halo(off);
    let on_halo = halo(on);
    let high_halo = halo(high);
    let off_ground = ground(off);
    let exposed_ground = ground(exposed);
    if on_halo - off_halo < 2.0 || on_halo - high_halo < 2.0 || exposed_ground - off_ground < 8.0 {
        return Err(format!(
            "Пиксельная проверка не пройдена. Ореол выкл. {off_halo:.2}, вкл. {on_halo:.2}, порог 8 {high_halo:.2}. Земля EV 0 {off_ground:.2}, EV +1 {exposed_ground:.2}."
        ));
    }
    Ok(json!({
        "halo_rect_xyxy": [2055, 714, 2195, 724],
        "halo_luma_rgb8": {"bloom_off":off_halo, "bloom_on":on_halo, "threshold_8":high_halo},
        "ground_rect_xyxy": [820, 940, 955, 1000],
        "ground_luma_rgb8": {"exposure_0":off_ground, "exposure_plus_1":exposed_ground},
    }))
}

fn luminance(rgb: &[u8], xs: Range<usize>, ys: Range<usize>) -> f32 {
    let count = xs.len() * ys.len();
    let mut total = 0.0;
    for y in ys {
        let row = y * RESOLUTION[0] as usize * 3;
        for x in xs.clone() {
            let index = row + x * 3;
            total += 0.2126 * f32::from(rgb[index])
                + 0.7152 * f32::from(rgb[index + 1])
                + 0.0722 * f32::from(rgb[index + 2]);
        }
    }
    total / count as f32
}

#[derive(serde::Serialize)]
struct Timings {
    samples: usize,
    mean_ms: f32,
    p50_ms: f32,
    #[serde(rename = "p95_ms")]
    p95: f32,
    fps: f32,
}

fn timings(samples: &[f32]) -> Result<Timings, String> {
    if samples.is_empty()
        || samples
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err("Неверные результаты замера кадров.".into());
    }
    let mut ordered = samples.to_vec();
    ordered.sort_by(f32::total_cmp);
    let mean_ms = samples.iter().sum::<f32>() / samples.len() as f32;
    let percentile = |percent: usize| ordered[(ordered.len() * percent).div_ceil(100) - 1];
    Ok(Timings {
        samples: samples.len(),
        mean_ms,
        p50_ms: percentile(50),
        p95: percentile(95),
        fps: if mean_ms > 0.0 { 1000.0 / mean_ms } else { 0.0 },
    })
}

fn save_frame(path: &Path, rgb: &[u8], [width, height]: [u32; 2]) -> Result<(), String> {
    image::save_buffer(path, rgb, width, height, image::ColorType::Rgb8)
        .map_err(|error| error.to_string())
}
