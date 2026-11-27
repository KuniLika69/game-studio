use std::{borrow::Cow, fs, path::Path, sync::Arc};

use eframe::{egui, egui_wgpu};
use glam::Vec3;
use serde_json::json;
use studio_core::render_settings::{BlendMode, PostProcess};

use super::{Driver, FPS, SIZE, Sink, io_error, proof_input};
use crate::{
    EditorTab, Studio,
    assets::materials::{Lighting, Tuning},
    configure_context,
    effects::{Kind, MAX_PARTICLES, Particle},
    environment::{Atmosphere, Environment, WaterSurface},
    material_graph, model,
    offscreen::Offscreen,
    proof_video::Video,
    scene::{Part, Shape},
    viewport3d::{
        self, Camera, ModelDraw, NaturalScene, SortSample, TransparentLayer, ViewportScene,
    },
};

const LEFT: f32 = -0.88;
const RIGHT: f32 = 0.88;
const GLASS_CENTER: [f32; 3] = [0.78, 0.35, -0.12];
const CHECK_SIZE: [u32; 2] = [64, 32];
const GLASS: TransparentLayer = TransparentLayer::Model {
    model: 0,
    primitive: 0,
};
const WATER: TransparentLayer = TransparentLayer::Water(0);
const ORBIT_FRAMES: u32 = 8 * FPS / 2;
const STRESS_FRAMES: u32 = 32;

pub(super) fn record(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let adapter = display.adapter_info().clone();
    let mut input = proof_input::Driver::new(SIZE);
    let mut sample = Fixture::new()?;
    let glb = output.join("amber-glass.glb");
    fs::write(&glb, pane_glb()?).map_err(io_error)?;
    for _ in 0..3 {
        sample.frame(&mut display, &mut input, LEFT, 0.0)?;
    }
    display.measure_transparency();
    let left = sample.frame(&mut display, &mut input, LEFT, 0.0)?;
    let left_order = measured_order(&display.take_transparency_samples(), [GLASS, WATER])?;
    save_frame(&output.join("after-left.png"), &left, SIZE)?;
    let right = sample.frame(&mut display, &mut input, RIGHT, 0.5)?;
    let right_order = measured_order(&display.take_transparency_samples(), [WATER, GLASS])?;
    save_frame(&output.join("after-right.png"), &right, SIZE)?;
    let (sample_rgb, pixels) = checkerboard_test()?;
    save_frame(
        &output.join("additive-checker.png"),
        &sample_rgb,
        CHECK_SIZE,
    )?;
    editor_proof(output, &glb)?;
    let mut video = Video::start(&output.join("after.mp4"), SIZE[0], SIZE[1], FPS)?;
    video.push(&left, 5 * FPS)?;
    for index in 0..ORBIT_FRAMES {
        let t = index as f32 / (ORBIT_FRAMES - 1) as f32;
        let ease = t * t * (3.0 - 2.0 * t);
        let rgb = sample.frame(
            &mut display,
            &mut input,
            LEFT + (RIGHT - LEFT) * ease,
            f64::from(t) * 2.0,
        )?;
        video.push(&rgb, 2)?;
    }
    video.push(&right, 5 * FPS)?;
    let checker = image::RgbImage::from_raw(CHECK_SIZE[0], CHECK_SIZE[1], sample_rgb)
        .ok_or("Неверный размер результата пиксельной проверки.")?;
    let enlarged = image::imageops::resize(
        &checker,
        SIZE[0],
        SIZE[1],
        image::imageops::FilterType::Nearest,
    );
    video.push(enlarged.as_raw(), 4 * FPS)?;
    video.finish()?;
    let sorting = sort_stats(
        display.take_transparency_samples(),
        ORBIT_FRAMES as usize,
        2,
    )?;
    sample.particles = stress_particles();
    for frame in 0..STRESS_FRAMES {
        let yaw = LEFT + (RIGHT - LEFT) * frame as f32 / (STRESS_FRAMES - 1) as f32;
        sample.frame(&mut display, &mut input, yaw, f64::from(frame) / 30.0)?;
    }
    let stress = sort_stats(
        display.take_transparency_samples(),
        STRESS_FRAMES as usize,
        MAX_PARTICLES + 2,
    )?;
    let evidence = json!({
        "adapter": adapter.name, "backend": format!("{:?}", adapter.backend),
        "resolution": SIZE, "msaa": viewport3d::VIEWPORT_MSAA,
        "glass": { "center": GLASS_CENTER, "material": "glTF alphaMode BLEND", "alpha": 0.38 },
        "water": { "center": sample.water.center, "height": sample.water.height, "blend": "Add" },
        "camera_yaw": [LEFT, RIGHT], "left_order": left_order, "right_order": right_order,
        "sorting": sorting, "sorting_stress": stress,
        "checkerboard_linear_rgb8": pixels,
    });
    fs::write(
        output.join("after.json"),
        serde_json::to_vec_pretty(&evidence).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

fn sort_stats(
    samples: Vec<SortSample>,
    frames: usize,
    layers: usize,
) -> Result<serde_json::Value, String> {
    if samples.len() != frames
        || samples
            .iter()
            .any(|frame| frame.order.len() != layers || !frame.ms.is_finite() || frame.ms < 0.0)
    {
        return Err(format!(
            "Замер сортировки неполный. Ожидалось {frames} кадров по {layers} слоёв."
        ));
    }
    let mut timings: Vec<_> = samples.into_iter().map(|sample| sample.ms).collect();
    timings.sort_by(f32::total_cmp);
    let p95 = timings[(timings.len() * 95).div_ceil(100) - 1];
    let maximum = timings[timings.len() - 1];
    if maximum > 1.0 {
        return Err(format!(
            "Сортировка {layers} слоёв заняла до {maximum:.4} мс при бюджете 1 мс."
        ));
    }
    Ok(json!({"layers": layers, "frames": frames, "p95_ms": p95, "max_ms": maximum}))
}

fn stress_particles() -> Vec<Particle> {
    (0..MAX_PARTICLES)
        .map(|index| Particle {
            kind: Kind::Smoke,
            position: Vec3::new(
                (index * 37 % 241) as f32 * 0.025 - 3.0,
                (index * 47 % 127) as f32 * 0.012,
                (index * 83 % 211) as f32 * 0.023 - 2.4,
            ),
            velocity: Vec3::ZERO,
            radius: 0.04,
            color: [0.62, 0.66, 0.75],
            opacity: 0.24,
        })
        .collect()
}

fn measured_order(
    samples: &[SortSample],
    expected: [TransparentLayer; 2],
) -> Result<Vec<String>, String> {
    let actual = samples.last().ok_or("Вьюпорт не записал порядок слоёв.")?;
    if actual.order != expected {
        return Err(format!(
            "Неверный порядок слоёв {:?}. Ожидалось {expected:?}.",
            actual.order
        ));
    }
    Ok(actual
        .order
        .iter()
        .map(|layer| format!("{layer:?}"))
        .collect())
}

struct Fixture {
    checker: Vec<Part>,
    atmosphere: Atmosphere,
    water: WaterSurface,
    pane: Arc<model::Model>,
    particles: Vec<Particle>,
}

impl Fixture {
    fn new() -> Result<Self, String> {
        let checker = (0..8)
            .flat_map(|z| {
                (0..10).map(move |x| {
                    let light = (x + z) % 2 == 0;
                    Part {
                        id: (10 * z + x + 1) as u64,
                        name: String::new(),
                        shape: Shape::Box,
                        position: [(x as f32 - 4.5) * 0.66, 0.012, (z as f32 - 3.5) * 0.66],
                        rotation: [0.0; 3],
                        size: [0.66, 0.024, 0.66],
                        color: if light {
                            [0.72, 0.75, 0.82]
                        } else {
                            [0.055, 0.065, 0.095]
                        },
                        motion: Default::default(),
                    }
                })
            })
            .collect();
        let mut atmosphere = Environment::default().sample(0.0);
        atmosphere.sun_strength = 0.66;
        atmosphere.fog_density = 0.0;
        atmosphere.surface_wet = 0.0;
        let water = WaterSurface {
            id: 1,
            center: [-0.75, 0.10],
            size: [4.10, 3.10],
            rounded_edge: false,
            shore: None,
            height: 0.12,
            floor_y: -0.14,
            current: [0.02, 0.03],
            tint: [0.02, 0.24, 0.62],
            wave_height: 0.057,
            wave_hz: 0.65,
            blend_mode: BlendMode::Add,
        };
        Ok(Self {
            checker,
            atmosphere,
            water,
            pane: Arc::new(model::decode_glb(&pane_glb()?)?),
            particles: Vec::new(),
        })
    }

    fn frame(
        &self,
        display: &mut Offscreen,
        input: &mut proof_input::Driver,
        yaw: f32,
        elapsed: f64,
    ) -> Result<Vec<u8>, String> {
        let camera = Camera {
            yaw,
            pitch: 0.91,
            distance: 7.9,
            target: [0.0, 0.20, 0.0],
        };
        let glass = ModelDraw {
            token: 426,
            model: &self.pane,
            position: GLASS_CENTER,
            rotation: [0.0; 3],
            scale: [2.05, 1.0, 1.55],
            selected: false,
            tuning: Tuning {
                blend_mode: BlendMode::Add,
                ..Tuning::default()
            },
            vegetation: [0.0; 4],
            receives_weather: false,
            graph: None,
            material_preview: material_graph::PreviewLight::Scene,
            clip: None,
            elapsed: 0.0,
            blend: None,
            bone_offsets: Vec::new(),
            bone_translations: Vec::new(),
            morph_weights: Vec::new(),
            secondary: model::SecondaryPose::default(),
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
                            environment: &self.checker,
                            architecture: &[],
                            models: std::slice::from_ref(&glass),
                            camera,
                            selected: None,
                            sparks: &[],
                            glow: 0.0,
                            playing: false,
                            lighting: Lighting {
                                ground_tint: Some([0.095, 0.12, 0.16]),
                                ..Lighting::default()
                            },
                            lights: &[],
                            natural: Some(NaturalScene {
                                atmosphere: &self.atmosphere,
                                water: std::slice::from_ref(&self.water),
                                elapsed,
                                wind: Vec3::ZERO,
                                snow_tracks: &[],
                            }),
                            effects: &self.particles,
                            post_process: PostProcess::default(),
                        },
                    );
                });
        })
    }
}

fn pane_glb() -> Result<Vec<u8>, String> {
    let coordinates: [[f32; 3]; 6] = [
        [-1.0, 0.0, -1.0],
        [-1.0, 0.0, 1.0],
        [1.0, 0.0, -1.0],
        [1.0, 0.0, -1.0],
        [-1.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
    ];
    let bin = coordinates
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let model = json!({
        "asset": {"version": "2.0", "generator":"A4 transparency proof"},
        "scenes":[{"nodes":[0]}], "scene":0, "nodes":[{"mesh":0}],
        "buffers":[{"byteLength":bin.len()}],
        "bufferViews":[{"buffer":0,"byteLength":bin.len()}],
        "accessors":[{"bufferView":0,"count":6,"componentType":5126,"type":"VEC3", "min":[-1,0,-1], "max":[1,0,1]}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0}, "material":0}]}],
        "materials":[{"name":"Amber glass", "alphaMode":"BLEND", "doubleSided":true,
            "emissiveFactor":[0.22,0.08,0.02], "pbrMetallicRoughness":{"baseColorFactor":[0.48,0.12,0.03,0.38],
            "metallicFactor":0.0,"roughnessFactor":0.28}}],
    });
    let mut document = serde_json::to_vec(&model).map_err(|error| error.to_string())?;
    let padding = (4 - document.len() % 4) % 4;
    document.extend(std::iter::repeat_n(b' ', padding));
    gltf::binary::Glb {
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: 0,
        },
        json: Cow::Owned(document),
        bin: Some(Cow::Owned(bin)),
    }
    .to_vec()
    .map_err(|error| error.to_string())
}

fn editor_proof(output: &Path, glb: &Path) -> Result<(), String> {
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::default();
    app.courtyard_demo();
    app.scene
        .world
        .models
        .retain(|item| item.source != crate::assets::Source::Rock);
    app.editor_tab = EditorTab::World;
    app.desk.recovery_root = Some(output.join("recovery"));
    let mut sink = Sink::None;
    let mut d = Driver::new(app, &mut display, &mut sink, None);
    d.app
        .proof
        .as_mut()
        .ok_or("Нет контекста импорта тестового стекла.")?
        .import_glb = Some(glb.to_path_buf());
    d.settle(5)?;
    d.click("Импорт GLB…")?;
    d.click("Материалы GLB")?;
    d.click("Обычный (Normal)")?;
    d.click("Сложение (Add)")?;
    if !d
        .app
        .scene
        .world
        .models
        .last()
        .is_some_and(|item| item.material.blend_mode == BlendMode::Add)
    {
        return Err("Режим смешивания материала не применился в редакторе.".into());
    }
    save_frame(&output.join("editor-blend.png"), &d.last, SIZE)
}

fn checkerboard_test() -> Result<(Vec<u8>, serde_json::Value), String> {
    let mut display = Offscreen::new(CHECK_SIZE, 1)?;
    let mut input = proof_input::Driver::new(CHECK_SIZE);
    let rgb = display.frame(input.raw(Vec::new()), |ctx| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                    ui.available_rect_before_wrap(),
                    Checkerboard,
                ));
            });
    })?;
    let points = [([20, 20], [24, 32, 40]), ([20, 7], [80, 84, 88])];
    let mut evidence = Vec::new();
    for ([x, y], checker) in points {
        let index = (y * CHECK_SIZE[0] as usize + x) * 3;
        let actual = [rgb[index], rgb[index + 1], rgb[index + 2]];
        // У обеих полупрозрачных поверхностей alpha 0.5. Add = цвет фона + alpha × цвет слоя.
        let water = [32_u8, 72, 112];
        let glass = [48_u8, 32, 16];
        let expected = std::array::from_fn::<_, 3, _>(|channel| {
            checker[channel] + water[channel] / 2 + glass[channel] / 2
        });
        if actual
            .into_iter()
            .zip(expected)
            .any(|(value, expected)| value.abs_diff(expected) > 1)
        {
            return Err(format!(
                "Пиксель шахматки ({x}, {y}): GPU {actual:?}, ожидалось {expected:?}."
            ));
        }
        evidence.push(json!({"xy":[x,y], "base":checker, "water":water, "glass":glass, "alpha":0.5, "expected":expected, "actual":actual}));
    }
    Ok((rgb, json!(evidence)))
}

struct Checkerboard;
struct CheckerPipelines([wgpu::RenderPipeline; 3]);

impl egui_wgpu::CallbackTrait for Checkerboard {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Шахматка для проверки сложения"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(CHECK_SHADER)),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor::default());
        let pipelines = ["fs_board", "fs_water", "fs_glass"].map(|entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Проверка сложения"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_screen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: viewport3d::DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: (entry != "fs_board")
                            .then(|| viewport3d::transparent_blend(BlendMode::Add)),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        });
        resources.insert(CheckerPipelines(pipelines));
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(pipelines) = resources.get::<CheckerPipelines>() {
            for pipeline in &pipelines.0 {
                pass.set_pipeline(pipeline);
                pass.draw(0..3, 0..1);
            }
        }
    }
}

const CHECK_SHADER: &str = r#"
@vertex
fn vs_screen(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4f {
    let x = f32((vertex << 1u) & 2u);
    let y = f32(vertex & 2u);
    return vec4f(x * 2.0 - 1.0, y * -2.0 + 1.0, 0.0, 1.0);
}

@fragment
fn fs_board(@builtin(position) p: vec4f) -> @location(0) vec4f {
    let light = ((u32(p.x) / 16u + u32(p.y) / 16u) & 1u) != 0u;
    return vec4f(select(vec3f(24.0, 32.0, 40.0), vec3f(80.0, 84.0, 88.0), light) / 255.0, 1.0);
}

@fragment
fn fs_water(@builtin(position) p: vec4f) -> @location(0) vec4f {
    if p.x >= 44.0 { discard; }
    return vec4f(vec3f(32.0, 72.0, 112.0) / 255.0, 0.5);
}

@fragment
fn fs_glass(@builtin(position) p: vec4f) -> @location(0) vec4f {
    if p.x < 16.0 { discard; }
    return vec4f(vec3f(48.0, 32.0, 16.0) / 255.0, 0.5);
}
"#;

fn save_frame(path: &Path, rgb: &[u8], [width, height]: [u32; 2]) -> Result<(), String> {
    image::save_buffer(path, rgb, width, height, image::ColorType::Rgb8)
        .map_err(|error| error.to_string())
}
