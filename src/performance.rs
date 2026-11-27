use std::{
    ffi::OsString,
    ops::Range,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use eframe::{egui, egui_wgpu};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::assets::Source;

#[path = "performance_telemetry.rs"]
mod telemetry;
pub(crate) use telemetry::{Snapshot, Telemetry, percentile_sorted};

const SURFACE: [u32; 2] = [1920, 1080];
const SIMULATION_HZ: u32 = 60;
pub const STEP_SECONDS: f32 = 1.0 / SIMULATION_HZ as f32;

pub struct Options {
    pub overhead: bool,
    pub capture: bool,
    pub scene: Option<PathBuf>,
    pub actor: Option<Source>,
    report: PathBuf,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Self>, &'static str> {
        let mut args = args.into_iter();
        let Some(mode) = args.next() else {
            return Ok(None);
        };
        let capture = match mode.to_str() {
            Some("--benchmark") => false,
            Some("--capture-motion") => true,
            _ => return Ok(None),
        };
        let overhead = match args.next().as_deref().and_then(|arg| arg.to_str()) {
            Some("2d") => true,
            Some("3d") => false,
            _ => return Err("Укажи режим 3d или 2d"),
        };
        let report = args.next().ok_or("Укажи путь к JSON-отчёту")?.into();
        let mut scene = None;
        let mut actor = None;
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--scene") if scene.is_none() => {
                    scene = Some(args.next().ok_or("Укажи путь после --scene")?.into());
                }
                Some("--actor") if capture && actor.is_none() => {
                    actor = Some(match args.next().as_deref().and_then(|arg| arg.to_str()) {
                        Some("emma") => Source::Emma,
                        Some("ryan") => Source::Ryan,
                        _ => return Err("Укажи emma или ryan после --actor"),
                    });
                }
                _ => return Err("Неизвестный или повторный параметр замера"),
            }
        }
        Ok(Some(Self {
            overhead,
            capture,
            scene,
            actor,
            report,
        }))
    }

    pub fn window(&self) -> egui::ViewportBuilder {
        egui::ViewportBuilder::default()
            .with_app_id("robot-studio-performance")
            .with_inner_size([768.0, 432.0])
            .with_decorations(false)
            .with_resizable(false)
            .with_active(true)
            .with_position([40.0, 40.0])
    }

    pub fn title(&self) -> &'static str {
        match (self.capture, self.overhead) {
            (false, false) => "Robot Studio benchmark 3D",
            (false, true) => "Robot Studio benchmark 2D",
            (true, false) => "Robot Studio motion 3D",
            (true, true) => "Robot Studio motion 2D",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Input {
    pub movement: [f32; 2],
    pub sprint: bool,
    pub jump: bool,
}

struct Stage {
    title: &'static str,
    ticks: u32,
    movement: [f32; 2],
    sprint: bool,
    jump: bool,
}

const ROUTE: [Stage; 8] = [
    Stage {
        title: "Начало шага",
        ticks: 54,
        movement: [0., -1.],
        sprint: false,
        jump: false,
    },
    Stage {
        title: "Шаг → бег",
        ticks: 33,
        movement: [0., -1.],
        sprint: true,
        jump: false,
    },
    Stage {
        title: "Остановка",
        ticks: 36,
        movement: [0., 0.],
        sprint: false,
        jump: false,
    },
    Stage {
        title: "Поворот на 90°",
        ticks: 54,
        movement: [1., 0.],
        sprint: false,
        jump: false,
    },
    Stage {
        title: "Разворот на 180°",
        ticks: 60,
        movement: [-1., 0.],
        sprint: false,
        jump: false,
    },
    Stage {
        title: "Шаг на бордюр",
        ticks: 84,
        movement: [0., -1.],
        sprint: false,
        jump: false,
    },
    Stage {
        title: "Прыжок",
        ticks: 63,
        movement: [0., -1.],
        sprint: false,
        jump: true,
    },
    Stage {
        title: "Приземление и остановка",
        ticks: 39,
        movement: [0., 0.],
        sprint: false,
        jump: false,
    },
];

fn route_ticks() -> u32 {
    ROUTE.iter().map(|stage| stage.ticks).sum()
}

fn stage_at(step: u32) -> (&'static Stage, u32) {
    let mut start = 0;
    for stage in &ROUTE {
        if step < start + stage.ticks {
            return (stage, start);
        }
        start += stage.ticks;
    }
    let last = ROUTE.last().expect("nonempty performance route");
    (last, start - last.ticks)
}

pub fn command(step: u32) -> Input {
    let (stage, start) = stage_at(step);
    Input {
        movement: stage.movement,
        sprint: stage.sprint,
        jump: stage.jump && step == start,
    }
}

#[derive(Serialize)]
struct Checkpoint {
    stage: &'static str,
    simulation_seconds: f64,
    position: [f32; 3],
    heading_degrees: f32,
    peak_height: f32,
}

#[derive(Default)]
struct FrameTimes {
    last: Option<Instant>,
    samples: Vec<f64>,
}

struct FrameProbe(Arc<Mutex<FrameTimes>>);

impl egui_wgpu::CallbackTrait for FrameProbe {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        _pass: &mut wgpu::RenderPass<'static>,
        _resources: &egui_wgpu::CallbackResources,
    ) {
        let now = Instant::now();
        let mut frames = self.0.lock().expect("performance frame observer");
        if let Some(last) = frames.last.replace(now) {
            frames
                .samples
                .push(now.duration_since(last).as_secs_f64() * 1_000.0);
        }
    }
}

pub struct Runner {
    options: Options,
    launched: Instant,
    ready: Option<Instant>,
    started: Option<Instant>,
    adjusted: bool,
    done: bool,
    size: [u32; 2],
    cursor: u32,
    frames: Arc<Mutex<FrameTimes>>,
    checkpoints: Vec<Checkpoint>,
    peak_height: f32,
    adapter: Option<wgpu::AdapterInfo>,
    scene_checksum: Option<String>,
}

impl Runner {
    pub fn new(options: Options, adapter: Option<wgpu::AdapterInfo>) -> Self {
        let scene_checksum = options
            .scene
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
        Self {
            options,
            launched: Instant::now(),
            ready: None,
            started: None,
            adjusted: false,
            done: false,
            size: [0; 2],
            cursor: 0,
            frames: Arc::new(Mutex::new(FrameTimes {
                samples: Vec::with_capacity(10_000),
                ..Default::default()
            })),
            checkpoints: Vec::new(),
            peak_height: 0.0,
            adapter,
            scene_checksum,
        }
    }

    pub fn tick(&mut self, ctx: &egui::Context) -> Range<u32> {
        let empty = self.cursor..self.cursor;
        if self.done {
            return empty;
        }
        let ppp = ctx.pixels_per_point();
        if let Some(rect) = ctx.input(|input| input.viewport().inner_rect) {
            self.size = [rect.width(), rect.height()].map(|value| (value * ppp).round() as u32);
        }
        if self.size != SURFACE {
            if self.started.is_some() || self.launched.elapsed() > Duration::from_secs(8) {
                self.fail(
                    ctx,
                    &format!("Surface is {:?}, expected {:?}", self.size, SURFACE),
                );
                return empty;
            }
            if !self.adjusted {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                    egui::vec2(SURFACE[0] as f32, SURFACE[1] as f32) / ppp,
                ));
                self.adjusted = true;
            }
            self.ready = None;
            return empty;
        }

        let now = Instant::now();
        let ready = *self.ready.get_or_insert(now);
        let warmup = if self.options.capture { 5 } else { 2 };
        if now.duration_since(ready) < Duration::from_secs(warmup) {
            return empty;
        }
        let start = *self.started.get_or_insert(now);
        let next = (now.duration_since(start).as_secs_f64() * f64::from(SIMULATION_HZ)) as u32;
        let next = next.min(route_ticks());
        let steps = self.cursor..next;
        self.cursor = next;
        steps
    }

    pub fn observe_frame(&self, ui: &egui::Ui) {
        if self.started.is_some() && !self.done {
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                ui.max_rect(),
                FrameProbe(Arc::clone(&self.frames)),
            ));
        }
    }

    pub fn checkpoint(&mut self, step: u32, position: [f32; 3], heading_degrees: f32) {
        let (stage, start) = stage_at(step);
        self.peak_height = self.peak_height.max(position[1]);
        if step + 1 == start + stage.ticks {
            self.checkpoints.push(Checkpoint {
                stage: stage.title,
                simulation_seconds: f64::from(step + 1) / f64::from(SIMULATION_HZ),
                position,
                heading_degrees,
                peak_height: self.peak_height,
            });
            self.peak_height = 0.0;
        }
    }

    pub fn after_update(&mut self, ctx: &egui::Context) {
        if self.done {
            return;
        }
        if self.cursor == route_ticks() {
            self.write_report(ctx);
        } else if self.options.capture {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else {
            ctx.request_repaint();
        }
    }

    pub fn overlay(&self, ctx: &egui::Context) {
        if !self.options.capture {
            return;
        }
        let stage = if self.started.is_some() {
            stage_at(self.cursor).0.title
        } else {
            "Подготовка маршрута"
        };
        let stage = if stage == "Шаг → бег" {
            "Переход на бег"
        } else {
            stage
        };
        egui::Area::new(egui::Id::new("motion_capture_stage"))
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(12.0, -12.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(stage).strong().size(16.0))
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
            });
    }

    fn write_report(&mut self, ctx: &egui::Context) {
        let frame_times = self
            .frames
            .lock()
            .expect("performance frame observer")
            .samples
            .clone();
        let mut sorted = frame_times.clone();
        if sorted.is_empty() {
            self.fail(ctx, "Too few frames measured");
            return;
        }
        sorted.sort_unstable_by(f64::total_cmp);
        let count = sorted.len();
        let total_ms = sorted.iter().sum::<f64>();
        let percentile = |p: f64| sorted[((p * count as f64).ceil() as usize).saturating_sub(1)];
        let adapter = self.adapter.as_ref().map(|adapter| {
            serde_json::json!({
                "name": adapter.name, "backend": format!("{:?}", adapter.backend),
                "driver": adapter.driver, "driver_info": adapter.driver_info,
            })
        });
        let samples = if self.options.capture {
            crate::viewport3d::VIEWPORT_MSAA
        } else {
            1
        };
        let report = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "mode": if self.options.overhead { "2D plan" } else { "3D" },
            "run": if self.options.capture { "motion capture" } else { "benchmark" },
            "quality": if self.options.capture { "default" } else { "low" },
            "msaa_samples": samples,
            "shadow_map_px": crate::viewport3d::shadow_size(samples),
            "surface_px": self.size,
            "vsync_requested": false,
            "timing": "Wall-clock intervals between successive acquired WGPU surface render passes after warm-up",
            "scene": self.options.scene,
            "scene_sha256": self.scene_checksum,
            "actor_override": self.options.actor,
            "adapter": adapter,
            "frames": count,
            "elapsed_s": total_ms / 1_000.0,
            "mean_fps": count as f64 * 1_000.0 / total_ms,
            "mean_frame_ms": total_ms / count as f64,
            "p50_frame_ms": percentile(0.50),
            "p95_frame_ms": percentile(0.95),
            "p99_frame_ms": percentile(0.99),
            "max_frame_ms": sorted[count - 1],
            "simulation_hz": SIMULATION_HZ,
            "checkpoints": self.checkpoints,
            "frame_times_ms": frame_times,
        });
        self.finish(ctx, report);
    }

    fn fail(&mut self, ctx: &egui::Context, error: &str) {
        self.finish(
            ctx,
            serde_json::json!({"error": error, "surface_px": self.size}),
        );
    }

    fn finish(&mut self, ctx: &egui::Context, report: serde_json::Value) {
        if let Err(error) = std::fs::write(&self.options.report, format!("{report:#}\n")) {
            eprintln!("Не удалось сохранить отчёт замера: {error}");
        }
        self.done = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
