use std::{fs, path::Path};

use eframe::egui;
use glam::Vec3;
use serde_json::json;
use studio_core::render_settings::{PostProcess, ShadowQuality};

use super::{Driver, FPS, SIZE, Sink, io_error, proof_input};
use crate::{
    EditorTab, Studio,
    assets::materials::Lighting,
    configure_context,
    environment::Environment,
    offscreen::Offscreen,
    proof_video::Video,
    scene::{Part, Shape},
    viewport3d::{self, Camera, NaturalScene, ViewportScene},
};

const RESOLUTION: [u32; 2] = [2560, 1440];
const CAMERA: Camera = Camera {
    yaw: 0.08,
    pitch: 0.83,
    distance: 10.8,
    target: [-1.55, 0.72, 0.0],
};
const ZENITH: Vec3 = Vec3::new(0.22, 0.971, 0.0);
const HORIZON: Vec3 = Vec3::new(0.968, 0.25, 0.0);
const SMALL_MOVE: Camera = Camera {
    yaw: 0.082,
    ..CAMERA
};
const FAR_CAMERA: Camera = Camera {
    distance: 21.0,
    ..CAMERA
};
const MASK_SIZE: [u32; 2] = [720, 180];
const CLOSE_GROUND: [f32; 4] = [-1.12, -0.32, 0.04, 0.68];
const EXTENDED_GROUND: [f32; 4] = [-6.55, -1.35, -0.38, 0.38];

pub(super) fn record(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new_with_gpu_timing(RESOLUTION, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let adapter = display.adapter_info().clone();
    let mut fixture = Studio::default();
    fixture.courtyard_demo();
    let environment = stage();
    let mut input = proof_input::Driver::new(RESOLUTION);
    let post = PostProcess {
        shadow_quality: ShadowQuality::Cascaded,
        ..PostProcess::default()
    };
    let benchmark = PostProcess {
        bloom: true,
        ..post
    };
    let view = View {
        studio: &fixture,
        environment: &environment,
    };
    for _ in 0..24 {
        view.frame(&mut display, &mut input, CAMERA, ZENITH, true, benchmark)?;
    }
    display.take_gpu_frame_times();
    display.start_timing();
    for frame in 0..120 {
        let direction = if frame < 60 { ZENITH } else { HORIZON };
        view.frame(&mut display, &mut input, CAMERA, direction, true, benchmark)?;
    }
    let wall = timing_summary(display.stop_timing())?;
    let gpu = display.take_gpu_frame_times();
    let shadows = timing_summary(gpu.iter().map(|frame| frame.shadow_ms).collect())?;
    let combined = timing_summary(
        gpu.iter()
            .map(|frame| frame.shadow_ms + frame.bloom_ms + frame.composite_ms)
            .collect(),
    )?;
    if shadows.max_ms > 4.0 || combined.max_ms > 8.0 || wall.fps < 60.0 {
        return Err(format!(
            "Превышен бюджет A5: тени {:.3} мс, тени+пост {:.3} мс, полный кадр {:.1} FPS.",
            shadows.max_ms, combined.max_ms, wall.fps
        ));
    }
    let mut capture =
        |camera, sun, knight| view.frame(&mut display, &mut input, camera, sun, knight, post);
    let zenith = capture(CAMERA, ZENITH, true)?;
    let zenith_ground = capture(CAMERA, ZENITH, false)?;
    let horizon = capture(CAMERA, HORIZON, true)?;
    let horizon_ground = capture(CAMERA, HORIZON, false)?;
    let moved = capture(SMALL_MOVE, HORIZON, true)?;
    let moved_ground = capture(SMALL_MOVE, HORIZON, false)?;
    let far = capture(FAR_CAMERA, HORIZON, true)?;
    let far_ground = capture(FAR_CAMERA, HORIZON, false)?;
    let zenith_near = ground_mask(&zenith, &zenith_ground, CAMERA, CLOSE_GROUND)?;
    let zenith_far = ground_mask(&zenith, &zenith_ground, CAMERA, EXTENDED_GROUND)?;
    let horizon_long = ground_mask(&horizon, &horizon_ground, CAMERA, EXTENDED_GROUND)?;
    let moved_long = ground_mask(&moved, &moved_ground, SMALL_MOVE, EXTENDED_GROUND)?;
    let far_long = ground_mask(&far, &far_ground, FAR_CAMERA, EXTENDED_GROUND)?;
    let near_count = shadow_count(&zenith_near);
    let zenith_long_count = shadow_count(&zenith_far);
    let horizon_count = shadow_count(&horizon_long);
    let far_count = shadow_count(&far_long);
    if near_count < 100
        || horizon_count < 1000
        || far_count < 1000
        || zenith_long_count > horizon_count / 10
    {
        return Err(format!(
            "Не найдены проверяемые солнечные тени. Зенит близко/далеко {near_count}/{zenith_long_count}, горизонт {horizon_count}, дальний каскад {far_count}."
        ));
    }
    let stable = compare_masks(output, &horizon_long, &moved_long)?;
    for (name, rgb) in [
        ("after-zenith.png", &zenith),
        ("after-horizon.png", &horizon),
        ("after-horizon-moved.png", &moved),
        ("after-far-cascade.png", &far),
    ] {
        save_frame(&output.join(name), rgb, RESOLUTION)?;
    }
    let editor = editor_proof(output)?;

    let mut video = Video::start(&output.join("after.mp4"), RESOLUTION[0], RESOLUTION[1], FPS)?;
    video.push(&zenith, 4 * FPS)?;
    let step_count = 6 * FPS / 2;
    for index in 0..step_count {
        let t = index as f32 / (step_count - 1) as f32;
        let t = t * t * (3.0 - 2.0 * t);
        let rgb = view.frame(
            &mut display,
            &mut input,
            CAMERA,
            ZENITH.lerp(HORIZON, t),
            true,
            post,
        )?;
        video.push(&rgb, 2)?;
    }
    video.push(&horizon, 4 * FPS)?;
    for step in 0..(4 * FPS / 2) {
        let angle = std::f32::consts::TAU * step as f32 / (4 * FPS / 2) as f32;
        let camera = Camera {
            yaw: CAMERA.yaw + angle.sin() * 0.012,
            ..CAMERA
        };
        let rgb = view.frame(&mut display, &mut input, camera, HORIZON, true, post)?;
        video.push(&rgb, 2)?;
    }
    video.push(&far, 3 * FPS)?;
    let editor = image::RgbImage::from_raw(SIZE[0], SIZE[1], editor)
        .ok_or("Не удалось показать кадр редактора.")?;
    let editor = image::imageops::resize(
        &editor,
        RESOLUTION[0],
        RESOLUTION[1],
        image::imageops::FilterType::Triangle,
    );
    video.push(editor.as_raw(), 4 * FPS)?;
    video.finish()?;
    let evidence = json!({
        "adapter":adapter.name, "backend":format!("{:?}", adapter.backend),
        "resolution":RESOLUTION, "msaa":viewport3d::VIEWPORT_MSAA, "shadow":post.shadow_quality,
        "camera":{"yaw":CAMERA.yaw,"pitch":CAMERA.pitch,"distance":CAMERA.distance,"target":CAMERA.target},
        "sun_zenith":ZENITH.normalize().to_array(), "sun_horizon":HORIZON.normalize().to_array(),
        "source_model":"Knight", "fixed_idle_seconds":0.42,
        "timings_with_bloom": {"gpu_shadows": shadows, "gpu_shadows_and_post": combined, "frame_including_readback": wall},
        "pixel_checks": { "world_sample_grid":MASK_SIZE,"near_ground_world_xxzz":CLOSE_GROUND,"extended_ground_world_xxzz":EXTENDED_GROUND,
            "zenith_near":near_count,"zenith_extended":zenith_long_count,"horizon_extended":horizon_count,
            "far_camera_extended":far_count,"far_camera_distance":FAR_CAMERA.distance,"stability":stable },
    });
    fs::write(
        output.join("after.json"),
        serde_json::to_vec_pretty(&evidence).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

fn stage() -> Vec<Part> {
    [
        ([-4.55, 0.75, -2.75], [0.42, 1.50, 0.42], [0.62, 0.48, 0.36]),
        ([2.85, 0.56, -2.15], [0.62, 1.12, 0.62], [0.48, 0.56, 0.62]),
        ([-0.80, 0.07, -3.25], [8.40, 0.14, 0.14], [0.42, 0.42, 0.44]),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (position, size, color))| Part {
        id: index as u64 + 1,
        name: String::new(),
        shape: Shape::Box,
        position,
        rotation: [0.0; 3],
        size,
        color,
        motion: Default::default(),
    })
    .collect()
}

struct View<'a> {
    studio: &'a Studio,
    environment: &'a [Part],
}

impl View<'_> {
    fn frame(
        &self,
        display: &mut Offscreen,
        input: &mut proof_input::Driver,
        camera: Camera,
        direction: Vec3,
        render_knight: bool,
        post_process: PostProcess,
    ) -> Result<Vec<u8>, String> {
        let mut atmosphere = Environment::default().sample(0.0);
        atmosphere.sun_direction = direction.normalize();
        atmosphere.sun_strength = 0.9;
        atmosphere.sun_color = Vec3::new(1.0, 0.89, 0.75);
        atmosphere.ambient = Vec3::splat(0.22);
        atmosphere.fog_density = 0.0;
        atmosphere.cloud_cover = 0.0;
        atmosphere.surface_wet = 0.0;
        let knight = if render_knight {
            let mut draw = self
                .studio
                .avatar_draw_without_foot()
                .ok_or("Отсутствует встроенная модель рыцаря.")?;
            draw.elapsed = 0.42;
            Some(draw)
        } else {
            None
        };
        display.frame(input.raw(Vec::new()), |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    viewport3d::paint(
                        ui,
                        ui.available_rect_before_wrap(),
                        ViewportScene {
                            robot: &[],
                            environment: self.environment,
                            architecture: &[],
                            models: knight.as_slice(),
                            camera,
                            selected: None,
                            sparks: &[],
                            glow: 0.0,
                            playing: true,
                            lighting: Lighting {
                                ground_tint: Some([0.54, 0.56, 0.60]),
                                ..Lighting::default()
                            },
                            lights: &[],
                            natural: Some(NaturalScene {
                                atmosphere: &atmosphere,
                                water: &[],
                                elapsed: 0.0,
                                wind: Vec3::ZERO,
                                snow_tracks: &[],
                            }),
                            effects: &[],
                            post_process,
                        },
                    );
                });
        })
    }
}

fn ground_mask(
    frame: &[u8],
    empty: &[u8],
    camera: Camera,
    [x0, x1, z0, z1]: [f32; 4],
) -> Result<Vec<bool>, String> {
    // Обе камеры проецируются на один и тот же участок плоскости, чтобы сдвиг кадра не менял маску.
    let rect = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(RESOLUTION[0] as f32, RESOLUTION[1] as f32),
    );
    let mut mask = Vec::with_capacity((MASK_SIZE[0] * MASK_SIZE[1]) as usize);
    for z in 0..MASK_SIZE[1] {
        let z = z0 + (z1 - z0) * (z as f32 + 0.5) / MASK_SIZE[1] as f32;
        for x in 0..MASK_SIZE[0] {
            let x = x0 + (x1 - x0) * (x as f32 + 0.5) / MASK_SIZE[0] as f32;
            let pixel = camera
                .screen_point(rect, [x, -0.002, z])
                .ok_or("Участок земли не попал в кадр.")?;
            mask.push(luminance(empty, pixel) - luminance(frame, pixel) >= 12.0);
        }
    }
    Ok(mask)
}

fn luminance(rgb: &[u8], pixel: egui::Pos2) -> f32 {
    let locate = |q: f32, limit| (q - 0.5).clamp(0.0, (limit - 2) as f32);
    let x = locate(pixel.x, RESOLUTION[0]);
    let y = locate(pixel.y, RESOLUTION[1]);
    let sample = |x: usize, y: usize| {
        let index = (y * RESOLUTION[0] as usize + x) * 3;
        0.2126 * f32::from(rgb[index])
            + 0.7152 * f32::from(rgb[index + 1])
            + 0.0722 * f32::from(rgb[index + 2])
    };
    let lerp = |from: f32, to: f32, t: f32| from + (to - from) * t;
    let ix = x as usize;
    let iy = y as usize;
    let a = lerp(sample(ix, iy), sample(ix + 1, iy), x.fract());
    let b = lerp(sample(ix, iy + 1), sample(ix + 1, iy + 1), x.fract());
    lerp(a, b, y.fract())
}

fn shadow_count(mask: &[bool]) -> usize {
    mask.iter().filter(|&&shadow| shadow).count()
}

fn compare_masks(
    output: &Path,
    first: &[bool],
    second: &[bool],
) -> Result<serde_json::Value, String> {
    let mut changed = 0;
    let mut shadow = 0;
    let mut rgb = Vec::with_capacity(first.len() * 3);
    for (&a, &b) in first.iter().zip(second) {
        changed += usize::from(a != b);
        shadow += usize::from(a || b);
        rgb.extend_from_slice(match (a, b) {
            (true, true) => &[190, 202, 223],
            (false, false) => &[17, 24, 35],
            _ => &[255, 124, 82],
        });
    }
    let changed_pct = changed as f64 * 100.0 / first.len() as f64;
    let shadow_changed_pct = changed as f64 * 100.0 / shadow as f64;
    save_frame(&output.join("stability-mask.png"), &rgb, MASK_SIZE)?;
    if shadow_changed_pct > 1.0 {
        return Err(format!(
            "При повороте камеры изменилось {shadow_changed_pct:.3}% пикселей тени (бюджет 1%)."
        ));
    }
    Ok(
        json!({"camera_yaw_before":CAMERA.yaw,"camera_yaw_after":SMALL_MOVE.yaw,
        "world_samples":first.len(),"shadow_union_pixels":shadow,"changed_pixels":changed,
        "changed_pct":changed_pct,"shadow_changed_pct":shadow_changed_pct}),
    )
}

#[derive(serde::Serialize)]
struct Timing {
    frames: usize,
    mean_ms: f32,
    p95_ms: f32,
    max_ms: f32,
    fps: f32,
}

fn timing_summary(mut values: Vec<f32>) -> Result<Timing, String> {
    if values.len() != 120
        || values
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err(format!(
            "Неполный замер A5, ожидалось 120 положительных отсчётов. Получено {}.",
            values.len()
        ));
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    values.sort_by(f32::total_cmp);
    Ok(Timing {
        frames: values.len(),
        mean_ms: mean,
        p95_ms: values[113],
        max_ms: values[119],
        fps: 1000.0 / mean,
    })
}

fn editor_proof(output: &Path) -> Result<Vec<u8>, String> {
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut studio = Studio::default();
    studio.courtyard_demo();
    studio.desk.recovery_root = Some(output.join("recovery"));
    let mut sink = Sink::None;
    let mut d = Driver::new(studio, &mut display, &mut sink, None);
    d.settle(5)?;
    if d.app.editor_tab != EditorTab::Lighting {
        d.click("Свет")?;
    }
    d.point("Обычные тени", 0)?;
    save_frame(&output.join("editor-shadow-default.png"), &d.last, SIZE)?;
    let frame = d.display.last_output().ok_or("Нет кадра настроек света.")?;
    let label = proof_input::labels(frame)
        .into_iter()
        .find(|label| label.text == "Качество теней")
        .ok_or("Ползунок качества теней не виден в редакторе.")?;
    let left = egui::pos2(label.bounds.min.x - 90.0, label.center().y);
    let right = egui::pos2(label.bounds.min.x - 23.0, label.center().y);
    d.frame(vec![
        egui::Event::PointerMoved(left),
        proof_input::pointer_button(left, egui::PointerButton::Primary, true),
    ])?;
    d.frame(proof_input::hover(right))?;
    d.frame(vec![proof_input::pointer_button(
        right,
        egui::PointerButton::Primary,
        false,
    )])?;
    d.settle(4)?;
    d.point("2 каскада", 0)?;
    if d.app.scene.post_process.shadow_quality != ShadowQuality::Cascaded {
        return Err("Ползунок не включил два каскада теней.".into());
    }
    save_frame(&output.join("editor-shadow-quality.png"), &d.last, SIZE)?;
    Ok(d.last)
}

fn save_frame(path: &Path, rgb: &[u8], [width, height]: [u32; 2]) -> Result<(), String> {
    image::save_buffer(path, rgb, width, height, image::ColorType::Rgb8)
        .map_err(|error| error.to_string())
}
