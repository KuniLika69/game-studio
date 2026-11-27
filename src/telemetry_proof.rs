use std::{fs, path::Path, time::Instant};

use eframe::egui::Key;
use serde_json::{Value, json};

use super::{Driver, FPS, SIZE, Sink, io_error};
use crate::{Studio, configure_context, offscreen::Offscreen, performance::Telemetry, proof_video::Video, viewport3d};

pub(super) fn record(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let log = output.join("spike.log");
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let adapter = display.adapter_info().name.clone();
    let mut studio = Studio::default();
    studio.courtyard_demo();
    studio.telemetry = Telemetry::new(Some(log.clone()));
    studio.desk.recovery_root = Some(output.join("recovery"));
    let mut video = Video::start(&output.join("after.mp4"), SIZE[0], SIZE[1], FPS)?;
    let evidence;
    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(studio, &mut display, &mut sink, None);
        d.settle(12)?;
        let scene_id = d.app.telemetry_scene_id();
        d.display.start_timing();
        d.settle(60)?;
        let real = d.display.stop_timing();
        for &frame in &real {
            d.app.telemetry.record(Instant::now(), scene_id, f64::from(frame));
        }
        let clean = d.app.telemetry.snapshot(Instant::now());
        if clean.count != real.len() || !clean.spikes.is_empty() || log.exists() {
            return Err("Проверочные кадры двора вышли за 100 мс.".into());
        }
        d.key(Key::F10)?;
        d.expect_hud("Текущий кадр")?;
        d.expect_hud("Последние 60 секунд")?;
        for label in ["p50", "p95", "p99"] {
            d.expect_hud(label)?;
        }
        save(output.join("telemetry.png").as_path(), &d.last)?;
        d.hold(7 * FPS)?;

        // Контрольные значения проверяют журнал и порядок трёх записей в UI.
        for (id, span) in [(1, 103.0), (2, 146.0), (3, 119.0), (4, 134.0)] {
            d.app.telemetry.record(Instant::now(), id, span);
        }
        let with_spikes = d.app.telemetry.snapshot(Instant::now());
        let order = with_spikes.spikes.iter().map(|sample| sample.scene_id).collect::<Vec<_>>();
        if order != [2, 4, 3] || with_spikes.log_error.is_some() {
            return Err("Три самых долгих задержки не отсортировались или не записались.".into());
        }
        d.settle(6)?;
        d.point("Самые долгие задержки", 0)?;
        for millis in ["146.00 мс", "134.00 мс", "119.00 мс"] {
            d.expect_hud(millis)?;
        }
        let raw = fs::read_to_string(log).map_err(io_error)?;
        let rows = raw.lines().map(serde_json::from_str::<Value>).collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        if rows.iter().map(|entry| entry["scene_id"].as_u64()).collect::<Vec<_>>()
            != [Some(1), Some(2), Some(3), Some(4)] {
            return Err("Журнал не содержит четыре контрольных ID сцены.".into());
        }
        save(output.join("telemetry-spikes.png").as_path(), &d.last)?;
        d.hold(14 * FPS)?;
        evidence = json!({"gpu": adapter, "real_frames": clean, "injected_spikes": with_spikes.spikes,
            "log_rows": rows, "injected": true, "log_file": "spike.log"});
    }
    video.finish()?;
    fs::write(output.join("ui-proof.json"), serde_json::to_vec_pretty(&evidence).map_err(|error| error.to_string())?).map_err(io_error)
}

fn save(path: &Path, rgb: &[u8]) -> Result<(), String> {
    image::save_buffer(path, rgb, SIZE[0], SIZE[1], image::ColorType::Rgb8)
        .map_err(|error| error.to_string())
}
