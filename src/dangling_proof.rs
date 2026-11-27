use std::{fs, path::Path};

use serde_json::json;

use super::{Driver, FPS, SIZE, Sink, io_error};
use crate::{
    EditorTab, Studio, configure_context,
    offscreen::Offscreen,
    proof_video::Video,
    rules::{Action, Event, GameAction},
    viewport3d,
};

const MODEL: &str = "Ящик во дворе";
const RULE: &str = "Включить ящик во дворе";

pub(super) fn record(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::default();
    app.courtyard_demo();
    app.editor_tab = EditorTab::World;
    app.desk.recovery_root = Some(output.join("recovery"));
    let object_id = app
        .scene
        .world
        .models
        .iter()
        .find(|model| model.label == MODEL)
        .ok_or("Во дворе нет контрольного ящика.")?
        .id;
    let counter_id = app
        .scene
        .rules
        .add_counter()
        .ok_or("Не добавился контрольный счётчик.")?;
    let rule_id = app
        .scene
        .rules
        .add_rule()
        .ok_or("Не добавилось контрольное правило.")?;
    let rule = app
        .scene
        .rules
        .rules
        .last_mut()
        .ok_or("Не найдено контрольное правило.")?;
    rule.name = RULE.into();
    rule.enabled = true;
    rule.event = Some(Event::GameAction {
        action: GameAction::Mark,
    });
    rule.actions = vec![Action::SetObjectEnabled {
        object_id,
        enabled: true,
    }];
    let mut video = Video::start(&output.join("after.mp4"), SIZE[0], SIZE[1], FPS)?;
    let actions;
    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(app, &mut display, &mut sink, None);
        d.settle(5)?;
        if !d.app.import_problems.references.is_empty() {
            return Err("Ссылка отмечена ошибкой до удаления ящика.".into());
        }
        d.click(MODEL)?;
        d.hold(3 * FPS)?;
        d.click("Удалить модель")?;
        if d.app
            .scene
            .world
            .models
            .iter()
            .any(|model| model.id == object_id)
            || d.app.scene.rules.rules[0].actions
                != [Action::SetObjectEnabled {
                    object_id,
                    enabled: true,
                }]
        {
            return Err("Удаление ящика не сохранило проверяемый сценарий.".into());
        }
        let banner = "⚠ Висячие ссылки · 1";
        d.point(banner, 0)?;
        d.point("Проблемы импорта · 1", 0)?;
        d.hold(4 * FPS)?;
        d.click(banner)?;
        d.point(&format!("Правило «{RULE}» #{rule_id}"), 0)?;
        d.point(&format!("Проблемная ссылка · Объект #{object_id}"), 0)?;
        d.click("Показать")?;
        d.point("Выбрано", 0)?;
        d.hold(6 * FPS)?;
        d.click("Проблемы импорта · 1")?;

        let untouched = d.app.scene.clone();
        d.click("Правки ИИ…")?;
        let draft = json!({"ops": [
            {"op": "set_counter_initial", "id": counter_id, "value": 7},
            {"op": "set_rule_enabled", "id": rule_id, "enabled": true},
        ]});
        d.edit(r#"{"ops":[…]}"#, 0, &draft.to_string())?;
        d.hold(4 * FPS)?;
        d.click("Проверить ссылки и показать правки")?;
        d.expect_hud(&format!(
            "У правила #{rule_id} отсутствует событие или цель действия."
        ))?;
        if d.point("Принять правки", 0).is_ok() || d.app.scene != untouched {
            return Err("Отклонённые ИИ-правки затронули сцену.".into());
        }
        d.hold(8 * FPS)?;
        actions = d.input_actions;
    }
    video.finish()?;
    let proof = json!({"inspector_warning": true, "panel_owner_and_target": true,
        "rejected_patch": true, "scene_unchanged": true, "ui_actions": actions,
        "rule_id": rule_id, "missing_object_id": object_id, "counter_id": counter_id});
    fs::write(
        output.join("proof.json"),
        serde_json::to_vec_pretty(&proof).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}
