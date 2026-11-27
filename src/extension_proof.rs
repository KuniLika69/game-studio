use std::path::Path;

use super::{Driver, staged_proof::number, top_proof};
use crate::rules::{Action, TraceOutcome};

const LAST: usize = usize::MAX;
const COUNTER: &str = "Бонус расширения";
const RULE: &str = "Удвоить бонус при отметке";
const PATH: &str = "Путь к файлу .studio-extension.json";

pub(super) fn author(d: &mut Driver<'_, '_>, action: &Path, tool: &Path) -> Result<u32, String> {
    d.click("Правила")?;
    reveal(d, "Расширения проекта", 0)?;
    d.click("Расширения проекта")?;
    let action = action.to_string_lossy();
    let tool = tool.to_string_lossy();
    import(d, PATH, &action)?;
    import(d, &action, &tool)?;
    if d.app.scene.rules.extensions.manifests().len() < 2 {
        return Err(d.app.status.clone());
    }

    let before = d.app.scene.lighting;
    d.click("Применить вечерний свет")?;
    let dimmed = d.app.scene.lighting;
    if (dimmed.daylight, dimmed.interior_gain) != (0.58, 1.65) {
        return Err("Инструмент не изменил освещение сцены.".into());
    }
    d.click("Отменить")?;
    if d.app.scene.lighting != before {
        return Err("Отмена не восстановила исходный свет.".into());
    }
    d.click("Повторить")?;
    if d.app.scene.lighting != dimmed {
        return Err("Повтор не восстановил применённый свет.".into());
    }
    if d.point("Счётчики", 0).is_err() {
        d.click("Правила")?;
    }
    reveal(d, "Счётчики", 0)?;
    d.click("Счётчики")?;
    reveal(d, "+ Счётчик", 0)?;
    d.click("+ Счётчик")?;
    let counter = d
        .app
        .scene
        .rules
        .counters
        .last()
        .ok_or("Счётчик не создан.")?;
    let (id, original_name) = (counter.id, counter.name.clone());
    d.edit(&original_name, LAST, COUNTER)?;
    number(d, "Начальное значение ", "3")?;

    reveal(d, "Сценарии", 0)?;
    d.click("Сценарии")?;
    top_proof::new_rule(d, RULE, "Игровое действие")?;
    reveal(d, "+ Действие", 0)?;
    d.click("+ Действие")?;
    let created = d
        .app
        .scene
        .rules
        .rules
        .last()
        .and_then(|rule| rule.actions.first())
        .copied();
    let first = match created {
        Some(Action::SetFlag { .. }) => "Установить флаг",
        Some(Action::UnlockDoor { .. }) => "Отпереть дверь",
        Some(Action::SetQuestStage { .. }) => "Задать этап задания",
        Some(Action::SetCounter { .. }) => "Задать счётчик",
        _ => return Err("Не найдено меню нового действия.".into()),
    };
    reveal(d, first, LAST)?;
    d.click_n(first, LAST)?;
    top_proof::click_action_kind(d, "Удвоить счётчик")?;
    let call = match d
        .app
        .scene
        .rules
        .rules
        .last()
        .and_then(|rule| rule.actions.first())
    {
        Some(Action::Extension(call)) => *call,
        _ => return Err("Меню не поставило действие из расширения.".into()),
    };
    if call.counter_id != id {
        let default_name = d
            .app
            .scene
            .rules
            .counter(call.counter_id)
            .ok_or("Нет предыдущего счётчика.")?
            .name
            .clone();
        reveal(d, &default_name, LAST)?;
        d.click_n(&default_name, LAST)?;
        d.click_n(COUNTER, LAST)?;
    }
    if !d.app.scene.rules.rules.last().is_some_and(|rule| {
        rule.name == RULE
            && rule.once
            && matches!(rule.actions.as_slice(), [Action::Extension(call)] if call.counter_id == id)
    }) || !d
        .app
        .scene
        .rules
        .counter(id)
        .is_some_and(|counter| counter.initial == 3)
    {
        return Err("Расширение не привязано к отдельному счётчику со значением 3.".into());
    }
    Ok(id)
}

fn import(d: &mut Driver<'_, '_>, field: &str, path: &str) -> Result<(), String> {
    d.edit(field, 0, path)?;
    d.click("Импортировать расширение")
}

fn reveal(d: &mut Driver<'_, '_>, label: &str, occurrence: usize) -> Result<(), String> {
    for amount in [1150.0, -260.0, -260.0] {
        if d.point(label, occurrence).is_ok() {
            return Ok(());
        }
        top_proof::scroll_panel(d, amount)?;
    }
    d.point(label, occurrence).map(|_| ())
}

pub(super) fn play(d: &Driver<'_, '_>, counter: u32) -> Result<(), String> {
    let rule = d
        .app
        .scene
        .rules
        .rules
        .iter()
        .find(|rule| rule.name == RULE)
        .ok_or("В игре отсутствует правило расширения.")?;
    if d.app.rule_session.counter(&d.app.scene.rules, counter) != Some(6)
        || !d.app.rule_session.trace().any(|trace| {
            trace.rule_id == rule.id && matches!(trace.outcome, TraceOutcome::Fired { actions: 1 })
        })
    {
        return Err("Правило расширения не удвоило отдельный счётчик в игре.".into());
    }
    d.expect_hud(&format!("{COUNTER} · 6"))
}
