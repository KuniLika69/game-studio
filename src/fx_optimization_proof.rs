//! Timed offscreen A/B experiment using the game's real rendering and WASD.
use std::{fs, path::Path};

use eframe::egui::{Event, Key, Modifiers};
use serde::Serialize;

use super::{Driver, Sink, io_error, proof_input};
use crate::{
    Scene, Studio, View, configure_context,
    effects::PresetId,
    fx_optimization::{self, Measurement, Proposal},
    offscreen::Offscreen,
};

const SIZE: [u32; 2] = [1920, 1080];
const FRAMES: usize = 120;

#[derive(Serialize)]
struct Run {
    measure: Measurement,
    position_start: [f32; 3],
    position_end: [f32; 3],
}

#[derive(Serialize)]
struct Comparison {
    resolution: [u32; 2],
    profile: &'static str,
    adapter: String,
    measured: &'static str,
    frame_target_ms: f32,
    preset_id: PresetId,
    preset_name: String,
    original: Run,
    proposal: Option<Proposal>,
    candidate: Option<Run>,
    scene_restored_after_experiment: Option<bool>,
    measured_frame_time_improvement: Option<bool>,
}

pub(super) fn run(
    project: &Path,
    preset_id: PresetId,
    target_ms: f32,
    output: &Path,
) -> Result<(), String> {
    if !(1.0..=100.0).contains(&target_ms) {
        return Err("Целевое время кадра от 1 до 100 мс.".into());
    }
    if project == output || output.extension().is_none_or(|ext| ext != "json") {
        return Err("Укажи отдельный JSON-файл для результатов.".into());
    }
    let mut scene = Scene::load(project)?;
    if scene.map_2d.is_some() || scene.tactics.is_some() {
        return Err("Этот эксперимент измеряет трёхмерную сцену.".into());
    }
    let preset_name = scene
        .effects
        .preset(preset_id)
        .ok_or("Эффект отсутствует.")?
        .settings
        .name
        .clone();
    let mut display = Offscreen::new(SIZE, 1)?;
    configure_context(display.context());
    let adapter = display.adapter_info().name.clone();
    let original = measure(&mut display, &scene, project, preset_id)?;
    let proposal =
        fx_optimization::propose(&scene.effects, preset_id, target_ms, &original.measure);
    let mut result = Comparison {
        resolution: SIZE,
        profile: "Low",
        adapter,
        measured: "Whole offscreen frame including CPU submission, GPU rendering and RGB readback",
        frame_target_ms: target_ms,
        preset_id,
        preset_name,
        original,
        proposal,
        candidate: None,
        scene_restored_after_experiment: None,
        measured_frame_time_improvement: None,
    };
    if let Some(proposal) = proposal {
        let baseline_scene = scene.clone();
        proposal.apply(&mut scene.effects)?;
        let candidate = measure(&mut display, &scene, project, preset_id)?;
        if distance(candidate.position_start, result.original.position_start) > 0.001
            || distance(candidate.position_end, result.original.position_end) > 0.01
        {
            return Err("Кандидат прошёл другой маршрут. Кадры несопоставимы.".into());
        }
        proposal.undo(&mut scene.effects)?;
        if scene != baseline_scene {
            return Err("Исходная сцена не восстановилась после эксперимента.".into());
        }
        result.scene_restored_after_experiment = Some(true);
        result.measured_frame_time_improvement = Some(fx_optimization::measured_gain(
            &result.original.measure,
            &candidate.measure,
        ));
        result.candidate = Some(candidate);
    }
    let report = serde_json::to_vec_pretty(&result).map_err(|error| error.to_string())?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(io_error)?;
    use std::io::Write as _;
    file.write_all(&report).map_err(io_error)
}

fn measure(
    display: &mut Offscreen,
    scene: &Scene,
    project: &Path,
    preset: PresetId,
) -> Result<Run, String> {
    let mut app = Studio {
        standalone: true,
        low_fx: true,
        ..Default::default()
    };
    app.set_scene(scene.clone(), Some(project.to_owned()), "");
    app.view = View::Three;
    app.begin_play(display.context(), false);
    if !app.game_running {
        return Err("Сцена не запустилась.".into());
    }
    let mut sink = Sink::None;
    let mut d = Driver::new(app, display, &mut sink, None);
    d.settle(26)?;
    let position_start = d.app.game_position().to_array();
    d.app
        .play_effect(preset, position_start)
        .ok_or("Эффект не запустился.")?;
    let mut peak_particles = 0;
    d.display.start_timing();
    for index in 0..FRAMES {
        let events: Vec<Event> = match index {
            0 => vec![key(Key::W, true)],
            46 => vec![key(Key::W, false), key(Key::D, true)],
            92 => vec![key(Key::D, false)],
            _ => Vec::new(),
        };
        d.frame(events)?;
        peak_particles = peak_particles.max(d.app.effect_particles().len());
    }
    let measure = Measurement::new(d.display.stop_timing(), peak_particles)?;
    Ok(Run {
        measure,
        position_start,
        position_end: d.app.game_position().to_array(),
    })
}

fn key(button: Key, pressed: bool) -> Event {
    proof_input::key_event(button, Modifiers::NONE, pressed)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    glam::Vec3::from_array(a).distance(glam::Vec3::from_array(b))
}
