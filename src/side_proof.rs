//! Background pointer and keyboard input for an authored side-view game.
use std::path::Path;

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};

use super::{Driver, staged_proof};
use crate::{
    map2d::{Collision, Map, Mode, Tile},
    proof_input,
    rules::{Action, Event as RuleEvent, TraceOutcome},
};

const SPAWN: [u16; 2] = [3, 14];
const PLATFORM: [u16; 2] = [3, 13];
const WALL: [u16; 2] = [7, 14];
const RESPAWN: [u16; 2] = [9, 14];
const PIT: [u16; 2] = [11, 17];
const FINISH: [u16; 2] = [19, 15];
const FINISH_NAME: &str = "Вход в обсерваторию";
const QUEST: &str = "Путь к обсерватории";
const LAST: usize = usize::MAX;

pub(super) fn author(d: &mut Driver<'_, '_>, atlas: &Path) -> Result<(), String> {
    d.click("Новый…")?;
    d.click("2D сбоку")?;
    let blank = map(d)?;
    if blank.mode != Mode::Side || blank.layers.iter().flatten().any(Option::is_some) {
        return Err("Редактор не открыл пустую карту 2D сбоку.".into());
    }

    d.app
        .proof
        .as_mut()
        .ok_or("Нет окружения фоновой проверки.")?
        .map_png = Some(atlas.into());
    d.click("Импорт PNG…")?;
    let imported = map(d)?
        .atlases
        .last()
        .filter(|asset| asset.id > 2)
        .ok_or_else(|| d.app.status.clone())?
        .id;

    select_tile(d, 0, 13)?;
    floor(d, 15)?;
    select_tile(d, 13, 12)?;
    floor(d, 16)?;
    floor(d, 17)?;
    select_tile(d, 12, 14)?;
    stroke(d, 17, 11, 12)?;
    select_tile(d, 14, 9)?;
    at_cell(d, [7, 13])?;
    at_cell(d, WALL)?;
    select_tile(d, 9, 15)?;
    stroke(d, PLATFORM[1], PLATFORM[0], 4)?;

    let imported_name = map(d)?.atlas(imported).unwrap().name.clone();
    d.click(&imported_name)?;
    d.click_n("Деревья и фонари", LAST)?;
    d.click("Высокие объекты")?;
    for cell in [[1, 14], [16, 14], [24, 14]] {
        at_cell(d, cell)?;
    }
    select_tile(d, 0, 1)?;
    for cell in [[6, 14], [9, 14], [19, 14]] {
        at_cell(d, cell)?;
    }

    d.click("Столкновения")?;
    d.click("Стена")?;
    floor(d, 15)?;
    at_cell(d, [7, 13])?;
    at_cell(d, WALL)?;
    d.click("Односторонняя платформа")?;
    stroke(d, PLATFORM[1], PLATFORM[0], 4)?;
    d.click("Старт")?;
    at_cell(d, SPAWN)?;
    d.click("Точка возрождения")?;
    at_cell(d, RESPAWN)?;

    d.click("Зоны")?;
    d.click("+ Зона")?;
    edit_zone_name(d)?;
    at_cell(d, FINISH)?;

    d.click("NPC")?;
    d.click("Задания")?;
    d.click("+ Задание")?;
    d.edit("Новое задание", LAST, QUEST)?;
    d.edit(
        "Дойди до указанного места.",
        LAST,
        "Перепрыгни провал и дойди до обсерватории.",
    )?;
    d.edit("Цель достигнута.", LAST, "Маршрут пройден.")?;
    d.click("По правилам")?;

    d.click("Правила")?;
    d.click("Сценарии")?;
    d.click("+ Правило")?;
    let name = d
        .app
        .scene
        .rules
        .rules
        .last()
        .ok_or("Нет правила.")?
        .name
        .clone();
    d.edit(&name, LAST, "Добраться до обсерватории")?;
    d.click("Выбрать событие")?;
    d.click_n("Вход в зону", LAST)?;
    d.click("+ Действие")?;
    if let Some(Action::SetFlag { .. }) = d.app.scene.rules.rules[0].actions.first() {
        d.click_n("Установить флаг", LAST)?;
        d.click_n("Задать этап задания", LAST)?;
    }
    d.edit("Этап 1", LAST, "4")?;
    d.key(Key::Enter)?;
    check_scene(d, Some(imported))?;
    d.hold(28)
}

pub(super) fn play(d: &mut Driver<'_, '_>, preview: bool) -> Result<(), String> {
    check_scene(d, None)?;
    d.settle(18)?;
    expect_position(d, [3.5, 3.0], 0.09)?;
    if respawn(d).is_some() {
        return Err("Точка возрождения активировалась до прохода.".into());
    }

    d.key(Key::Space)?;
    let mut apex = position(d)?[1];
    for _ in 0..62 {
        d.frame(Vec::new())?;
        apex = apex.max(position(d)?[1]);
    }
    if apex <= 5.12 {
        return Err(format!(
            "Прыжок снизу остановлен платформой. Высота {apex:.3}."
        ));
    }
    expect_position(d, [3.5, 5.0], 0.03)?;
    d.hold(20)?;
    if preview {
        staged_proof::panel_click(d, "■ Стоп")?;
        let saved = d
            .app
            .scene
            .checkpoint
            .as_ref()
            .ok_or("Позиция не сохранена.")?;
        if (saved.position[1] - 5.0).abs() > 0.04 {
            return Err(format!(
                "Сохранена неверная высота платформы: {:?}.",
                saved.position
            ));
        }
        staged_proof::panel_click(d, "Продолжить")?;
        d.settle(5)?;
        expect_position(d, [3.5, 5.0], 0.04)?;
    }

    held(d, Key::D, true)?;
    d.settle(78)?;
    held(d, Key::D, false)?;
    expect_position(d, [6.76, 3.0], 0.05)?;
    d.hold(16)?;

    jump_right(d)?;
    stop_at(d, 9.5)?;
    d.settle(20)?;
    expect_position(d, [9.5, 3.0], 0.31)?;
    if respawn(d) != Some(RESPAWN) {
        return Err("Проход по земле не активировал точку возрождения.".into());
    }
    d.expect_hud("Точка возрождения")?;
    d.hold(30)?;
    if preview {
        staged_proof::panel_click(d, "■ Стоп")?;
        if d.app
            .scene
            .checkpoint
            .as_ref()
            .and_then(|state| state.map_respawn)
            != Some(RESPAWN)
        {
            return Err("Сохранение игры не запомнило активную точку возрождения.".into());
        }
        staged_proof::panel_click(d, "Сохранить")?;
        staged_proof::reload(d)?;
        staged_proof::panel_click(d, "Продолжить")?;
        if respawn(d) != Some(RESPAWN) {
            return Err("Продолжение потеряло активную точку возрождения.".into());
        }
    }

    held(d, Key::D, true)?;
    let mut lowest = position(d)?[1];
    let mut fell = false;
    for _ in 0..150 {
        d.frame(Vec::new())?;
        let [x, y, _] = position(d)?;
        lowest = lowest.min(y);
        if lowest < 1.0 && (x - 9.5).abs() < 0.08 {
            fell = true;
            break;
        }
    }
    held(d, Key::D, false)?;
    d.settle(16)?;
    if !fell || respawn(d) != Some(RESPAWN) {
        return Err(format!(
            "Падение не вернуло к контрольной точке. Позиция {:?}.",
            position(d)?
        ));
    }
    expect_position(d, [9.5, 3.0], 0.14)?;
    d.expect_hud("Возвращение к контрольной точке")?;
    d.hold(25)?;

    held(d, Key::D, true)?;
    stop_at(d, 10.63)?;
    expect_position(d, [10.63, 3.0], 0.23)?;
    jump_right(d)?;
    stop_at(d, 15.0)?;
    d.settle(16)?;
    expect_position(d, [15.0, 3.0], 0.31)?;
    let quest = d.app.scene.story.quests.first().ok_or("Нет задания.")?.id;
    if d.app.rule_session.quest_stage(quest).is_some() {
        return Err("Финиш засчитан перед входом в зону.".into());
    }
    held(d, Key::D, true)?;
    stop_at(d, 19.5)?;
    if d.app.rule_session.quest_stage(quest) != Some(4)
        || !d.app.rule_session.trace().any(|entry| {
            matches!(entry.event, RuleEvent::EnterZone { .. })
                && matches!(entry.outcome, TraceOutcome::Fired { .. })
        })
    {
        return Err(format!("Финиш не сработал. Позиция {:?}.", position(d)?));
    }
    d.expect_hud(&format!("Выполнено · {QUEST}"))?;
    d.hold(55)
}

fn map<'a>(d: &'a Driver<'_, '_>) -> Result<&'a Map, String> {
    d.app
        .scene
        .map_2d
        .as_ref()
        .ok_or_else(|| "Карта отсутствует.".into())
}

fn check_scene(d: &Driver<'_, '_>, imported: Option<u32>) -> Result<(), String> {
    let map = map(d)?;
    let tile = |[col, row]: [u16; 2]| {
        map.layers[0][usize::from(row) * usize::from(map.size[0]) + usize::from(col)]
    };
    let art = tile(PLATFORM).ok_or("Платформа не закрашена.")?;
    if map.mode != Mode::Side
        || map.spawn != [3.5, 14.5]
        || map.respawn != Some(RESPAWN)
        || map.collision_at(PLATFORM.map(i32::from)) != Collision::Platform
        || map.collision_at(WALL.map(i32::from)) != Collision::Solid
        || map.collision_at(PIT.map(i32::from)) != Collision::Empty
        || tile(PIT)
            != Some(Tile {
                atlas: art.atlas,
                index: 14,
            })
        || art.index != 15
        || art.atlas != imported.unwrap_or(3)
        || !map.layers[2].iter().any(Option::is_some)
    {
        return Err("Сохранённые тайлы, слои, коллизии или позиции карты не совпали.".into());
    }
    let zone = d.app.scene.rules.zones.first().ok_or("Нет зоны финиша.")?;
    let rule = d
        .app
        .scene
        .rules
        .rules
        .first()
        .ok_or("Нет правила финиша.")?;
    let quest = d
        .app
        .scene
        .story
        .quests
        .first()
        .ok_or("Нет задания финиша.")?;
    if zone.name != FINISH_NAME
        || !zone.contains([19.5, 3.0, 0.0])
        || rule.event != Some(RuleEvent::EnterZone { zone_id: zone.id })
        || rule.actions
            != [Action::SetQuestStage {
                quest_id: quest.id,
                stage: 4,
            }]
    {
        return Err("Зона и правило задания не совпали.".into());
    }
    Ok(())
}

fn point(d: &Driver<'_, '_>, cell: [u16; 2]) -> Result<Pos2, String> {
    d.app
        .map_editor
        .transform()
        .map(|view| view.at_cell(cell))
        .ok_or_else(|| "Карта не отрисована.".into())
}

fn at_cell(d: &mut Driver<'_, '_>, cell: [u16; 2]) -> Result<(), String> {
    let point = point(d, cell)?;
    d.frame(proof_input::hover(point))?;
    d.frame(proof_input::click(point))?;
    d.settle(2)
}

fn stroke(d: &mut Driver<'_, '_>, row: u16, start: u16, end: u16) -> Result<(), String> {
    at_cell(d, [start, row])?;
    let start_point = point(d, [start, row])?;
    d.frame(vec![proof_input::pointer_button(
        start_point,
        PointerButton::Primary,
        true,
    )])?;
    for col in start + 1..=end {
        d.frame(vec![Event::PointerMoved(point(d, [col, row])?)])?;
    }
    let end_point = point(d, [end, row])?;
    d.frame(vec![proof_input::pointer_button(
        end_point,
        PointerButton::Primary,
        false,
    )])?;
    d.settle(2)
}

fn floor(d: &mut Driver<'_, '_>, row: u16) -> Result<(), String> {
    stroke(d, row, 0, 10)?;
    stroke(d, row, 13, 25)
}

fn select_tile(d: &mut Driver<'_, '_>, old: u16, new: u16) -> Result<(), String> {
    d.edit(&format!("Тайл {old}"), 0, &new.to_string())?;
    d.key(Key::Enter)
}

fn edit_zone_name(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let edge = d
        .app
        .map_editor
        .transform()
        .ok_or("Карта не отрисована.")?
        .rect
        .right();
    let output = d.display.last_output().ok_or("Интерфейс не отрисован.")?;
    let occurrence = proof_input::labels(output)
        .into_iter()
        .filter(|label| label.text == "Зона 1")
        .enumerate()
        .filter(|(_, label)| label.center().x > edge)
        .map(|(index, _)| index)
        .last()
        .ok_or("Не найдено поле имени зоны.")?;
    d.edit("Зона 1", occurrence, FINISH_NAME)
}

fn position(d: &Driver<'_, '_>) -> Result<[f32; 3], String> {
    d.app
        .map_player
        .as_ref()
        .map(|player| player.world_position())
        .ok_or_else(|| "Игрок не создан.".into())
}

fn respawn(d: &Driver<'_, '_>) -> Option<[u16; 2]> {
    d.app
        .map_player
        .as_ref()
        .and_then(|player| player.respawn_cell())
}

fn expect_position(d: &Driver<'_, '_>, expected: [f32; 2], tolerance: f32) -> Result<(), String> {
    let player = d.app.map_player.as_ref().ok_or("Игрок не создан.")?;
    let [x, y, z] = player.world_position();
    if (x - expected[0]).abs() > tolerance || (y - expected[1]).abs() > tolerance {
        return Err(format!(
            "Ожидалась позиция {expected:?}, игрок в [{x:.3}, {y:.3}]."
        ));
    }
    let projected = player.transform().ok_or("Камера игры не отрисована.")?;
    if !projected
        .rect
        .contains(projected.point_at(map(d)?.from_world([x, y, z])))
    {
        return Err(format!("Игрок [{x:.3}, {y:.3}] вне камеры."));
    }
    Ok(())
}

fn held(d: &mut Driver<'_, '_>, key: Key, pressed: bool) -> Result<(), String> {
    d.frame(vec![proof_input::key_event(key, Modifiers::NONE, pressed)])
}

fn jump_right(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let mut start = vec![proof_input::key_event(Key::D, Modifiers::NONE, true)];
    start.extend(proof_input::key(Key::Space, Modifiers::NONE));
    d.frame(start)
}

fn stop_at(d: &mut Driver<'_, '_>, target: f32) -> Result<(), String> {
    for _ in 0..200 {
        if position(d)?[0] >= target - 0.29 {
            held(d, Key::D, false)?;
            return d.settle(11);
        }
        d.frame(Vec::new())?;
    }
    held(d, Key::D, false)?;
    Err(format!(
        "Игрок не дошёл до {target:.2}: {:?}.",
        position(d)?
    ))
}
