use std::{collections::HashSet, fs, path::Path};

use eframe::egui::{self, Event, Key, Modifiers};
use serde_json::json;

use super::{Driver, FPS, SIZE, Sink, io_error, proof_input};
use crate::{
    EditorTab, ImportError, Studio, configure_context, game_export, model, offscreen::Offscreen,
    proof_video::Video, rules, scene_patch, viewport3d,
};

const MODELS: &[&str] = &[
    "a1_model_header.glb",
    "a1_model_truncated_json.glb",
    "a1_model_external_texture.glb",
    "a1_model_nan_position.glb",
    "a1_model_position_reference.glb",
    "a1_model_texture_descriptor.glb",
    "a1_model_index_format.glb",
    "a1_model_animation_target.glb",
];
const SCENES: &[&str] = &[
    "a1_scene/truncated.robot.json",
    "a1_scene/unknown-version.robot.json",
    "a1_scene/foreign-world-id.robot.json",
    "a1_scene/invalid-weather.robot.json",
    "a1_rules_story_missing_entry.robot.json",
    "a1_rules_story_bad_radius.robot.json",
];
const PACKAGES: &[&str] = &[
    "a1_scene/unsupported-version.package",
    "a1_scene/checksum-mismatch.package",
];
const RULES: &[&str] = &[
    "a1_rules_library_truncated.rules.json",
    "a1_rules_library_bad_version.rules.json",
    "a1_rules_library_foreign_namespace.rules.json",
];
const PATCHES: &[&str] = &[
    "a1_rules_patch_truncated.json",
    "a1_rules_patch_invalid_base.json",
];
const HUMAN: &str = "a1_model_human_style.json";

pub(super) fn record(output: &Path, fixtures: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let adapter = display.adapter_info().name.clone();
    let mut video = Video::start(&output.join("after.mp4"), SIZE[0], SIZE[1], FPS)?;
    let evidence;
    {
        let mut app = Studio::default();
        app.courtyard_demo();
        app.editor_tab = EditorTab::World;
        app.desk.recovery_root = Some(output.join("recovery"));
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(app, &mut display, &mut sink, None);
        let untouched = d.app.scene.clone();
        d.settle(5)?;
        d.app
            .proof
            .as_mut()
            .ok_or("Нет контекста проверки.")?
            .import_glb = Some(fixtures.join(MODELS[0]));
        d.click("Импорт GLB…")?;
        d.point("Проблемы импорта · 1", 0)?;
        d.click("Проблемы импорта · 1")?;
        d.click("Показать")?;
        if d.app.import_problems.selected != Some(0) {
            return Err("Кнопка не выделила строку импорта.".into());
        }
        d.point("Выбрано", 0)?;
        d.hold(4 * FPS)?;
        d.click("Проблемы импорта · 1")?;
        d.click("▶ Играть")?;
        let start = d.app.actor_position();
        for name in &MODELS[1..] {
            d.app.import_model(&fixtures.join(name));
        }
        for name in SCENES {
            d.app.open_scene_path(&fixtures.join(name));
        }
        for name in PACKAGES {
            log_rejected(
                &mut d.app,
                name,
                game_export::GamePackage::read(&fixtures.join(name)),
            )?;
        }
        let mut rule_set = rules::RuleSet::default();
        for name in RULES {
            log_rejected(
                &mut d.app,
                name,
                rule_set.load_library(&fixtures.join(name)),
            )?;
        }
        for name in PATCHES {
            log_rejected(
                &mut d.app,
                name,
                scene_patch::Proposal::load(&fixtures.join(name)),
            )?;
        }
        let path = fixtures.join(HUMAN);
        let bytes = fs::read(&path).map_err(io_error)?;
        let human: model::HumanStyle =
            serde_json::from_slice(&bytes).map_err(|error| ImportError::file(&path, error))?;
        log_rejected(
            &mut d.app,
            HUMAN,
            human.validate().map_err(|error| error.with_file(&path)),
        )?;
        if !d.app.game_running || d.app.scene != untouched {
            return Err("Отказ в импорте изменил активную сцену или остановил игру.".into());
        }
        let expected = MODELS
            .iter()
            .chain(SCENES)
            .chain(PACKAGES)
            .chain(RULES)
            .chain(PATCHES)
            .copied()
            .chain([HUMAN])
            .collect::<Vec<_>>();
        let count = d.app.import_problems.entries.len();
        if count != expected.len() {
            return Err(format!(
                "В панели {count} ошибок, ожидалось {}.",
                expected.len()
            ));
        }
        d.frame(vec![proof_input::key_event(Key::W, Modifiers::NONE, true)])?;
        d.settle(95)?;
        d.frame(vec![proof_input::key_event(Key::W, Modifiers::NONE, false)])?;
        let movement = d.app.actor_position().distance(start);
        if movement < 0.2 {
            return Err(format!(
                "Игрок не продолжил движение после импорта, пройдено {movement:.3} м."
            ));
        }
        d.hold(3 * FPS)?;
        d.click(&format!("Проблемы импорта · {count}"))?;
        let point = d.point("Показать", 0)?;
        let mut seen = HashSet::new();
        for page in 0..7 {
            let frame = d.display.last_output().ok_or("Пустой кадр панели.")?;
            for label in proof_input::labels(frame) {
                for name in &expected {
                    let basename = name.rsplit('/').next().unwrap_or(name);
                    if label.text.contains(basename) {
                        seen.insert(*name);
                    }
                }
            }
            if seen.len() == count {
                d.hold(3 * FPS)?;
                break;
            }
            d.hold(3 * FPS)?;
            if page < 6 {
                d.frame(vec![
                    Event::PointerMoved(point),
                    Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -350.0),
                        modifiers: Modifiers::NONE,
                    },
                ])?;
                d.settle(12)?;
            }
        }
        if seen.len() != count {
            let missing = expected
                .iter()
                .filter(|name| !seen.contains(*name))
                .copied()
                .collect::<Vec<_>>();
            return Err(format!("Прокрутка не показала {}.", missing.join(", ")));
        }
        d.hold(2 * FPS)?;
        evidence = json!({
            "adapter": adapter, "fixtures": expected, "typed_errors": count,
            "visible_rows": seen.len(), "highlight_confirmed": true,
            "game_kept_running": d.app.game_running, "movement_m": movement,
            "scene_unchanged": d.app.scene == untouched, "ui_actions": d.input_actions,
        });
    }
    video.finish()?;
    fs::write(output.join("proof.json"), evidence.to_string()).map_err(io_error)
}

fn log_rejected<T>(
    app: &mut Studio,
    label: &str,
    result: Result<T, ImportError>,
) -> Result<(), String> {
    match result {
        Err(error) => {
            app.record_import_problem(label, error);
            Ok(())
        }
        Ok(_) => Err(format!("Повреждённый файл принят. {label}")),
    }
}
