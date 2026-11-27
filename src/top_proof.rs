//! Background input for an independent tiled top-down game and its exported player.
use std::path::Path;

use eframe::egui::{Event, Key, Modifiers, PointerButton};

use super::Driver;
use crate::{
    map2d::{Collision, Mode, Tile},
    proof_input,
    rules::{self, Action, Condition, GameAction, TraceOutcome},
    world::WorldObject,
};

const FIRST: [u16; 2] = [7, 10];
const SECOND: [u16; 2] = [16, 10];
const FINISH: [u16; 2] = [20, 10];
const WALL: [u16; 2] = [9, 10];
const FIRST_NAME: &str = "Печать у ворот";
const SECOND_NAME: &str = "Печать на площади";
const FINISH_NAME: &str = "Маяк финиша";
const QUEST_NAME: &str = "Маршрут обхода";
const TIMER_NAME: &str = "Время обхода";
const TIMER_SECONDS: &str = "5.5";
const LAST: usize = usize::MAX;

pub(super) fn author(d: &mut Driver<'_, '_>, atlas: &Path, library: &Path) -> Result<(), String> {
    author_top(d, atlas, library, false)
}

pub(super) fn author_top(
    d: &mut Driver<'_, '_>,
    atlas: &Path,
    library: &Path,
    history: bool,
) -> Result<(), String> {
    d.click("Новый…")?;
    d.click("2D сверху")?;
    if !d.app.scene.map_2d.as_ref().is_some_and(|map| {
        map.mode == Mode::Top
            && map
                .layers
                .iter()
                .all(|layer| layer.iter().all(Option::is_none))
    }) {
        return Err("Проверка должна начинаться с пустой карты 2D сверху.".into());
    }

    let (width, height) = image::image_dimensions(atlas).map_err(|error| error.to_string())?;
    if width % 32 != 0 || height % 32 != 0 {
        d.click("Весь PNG как один тайл")?;
    }
    d.app
        .proof
        .as_mut()
        .ok_or("Нет окружения фоновой проверки.")?
        .map_png = Some(atlas.into());
    d.click("Импорт PNG…")?;
    let imported = d
        .app
        .scene
        .map_2d
        .as_ref()
        .and_then(|map| map.atlases.last())
        .filter(|atlas| atlas.id > 2)
        .ok_or_else(|| d.app.status.clone())?
        .id;
    if history {
        super::map2d_tools_proof::rectangle(d, [3, 9], [22, 11])?;
    } else {
        for row in [9, 10, 11] {
            road(d, row)?;
        }
    }
    let map = d.app.scene.map_2d.as_ref().unwrap();
    let road_tile = map.layers[0][map_index(map.size, WALL)].ok_or("Дорога не закрашена.")?;
    if road_tile.atlas != imported || map.layers[0].iter().flatten().count() < 54 {
        return Err("Кисть не перенесла импортированный PNG на полотно дороги.".into());
    }

    let title = map.atlas(imported).unwrap().name.clone();
    d.click(&title)?;
    d.click_n("Плитка двора", LAST)?;
    d.click("Декор")?;
    d.edit("Тайл 0", 0, "8")?;
    d.key(Key::Enter)?;
    at_cell(d, FIRST)?;
    at_cell(d, SECOND)?;
    if env!("CARGO_PKG_VERSION") != "0.14.0" {
        d.click("Плитка двора")?;
        d.click_n("Деревья и фонари", LAST)?;
        d.click("Высокие объекты")?;
        edit_prefix(d, "Тайл ", "0")?;
        for cell in [[5, 8], [5, 11], [11, 8], [13, 11], [18, 8], [22, 11]] {
            at_cell(d, cell)?;
        }
        if d.app
            .scene
            .map_2d
            .as_ref()
            .is_none_or(|map| map.layers[2].iter().flatten().count() != 6)
        {
            return Err("Высокие объекты не появились на отдельном слое.".into());
        }
    }
    d.click("Столкновения")?;
    d.click("Стена")?;
    at_cell(d, WALL)?;
    check_wall(d, Collision::Solid, road_tile)?;
    d.click("Без столкновения")?;
    at_cell(d, WALL)?;
    check_wall(d, Collision::Empty, road_tile)?;
    d.click("Стена")?;
    at_cell(d, WALL)?;
    check_wall(d, Collision::Solid, road_tile)?;

    d.click("Зоны")?;
    for (index, cell, name) in [(1, FIRST, FIRST_NAME), (2, SECOND, SECOND_NAME)] {
        d.click("+ Зона")?;
        edit_map_name(d, &format!("Зона {index}"), name)?;
        if !d
            .app
            .scene
            .rules
            .zones
            .last()
            .is_some_and(|zone| zone.name == name)
        {
            return Err(format!("Имя зоны «{name}» не изменилось."));
        }
        at_cell(d, cell)?;
    }
    d.click("Метки")?;
    d.click("+ Метка")?;
    edit_map_name(d, "Метка 1", FINISH_NAME)?;
    at_cell(d, FINISH)?;
    finish(d)?;

    d.click("NPC")?;
    d.click("Задания")?;
    d.click("+ Задание")?;
    d.edit("Новое задание", LAST, QUEST_NAME)?;
    d.edit(
        "Дойди до указанного места.",
        LAST,
        "Отметь две печати клавишей F, пока идёт отсчёт.",
    )?;
    d.edit(
        "Цель достигнута.",
        LAST,
        "Обе печати проверены. Маяк включён.",
    )?;
    d.click("По правилам")?;

    d.click("Правила")?;
    d.click("Библиотека")?;
    d.edit("Путь к файлу .rules.json", LAST, &library.to_string_lossy())?;
    d.click("Открыть библиотеку")?;
    for zone in [FIRST_NAME, SECOND_NAME] {
        d.click("Выбери источник")?;
        d.click_n(&format!("F в зоне · {zone}"), LAST)?;
    }
    d.click("Выбери цель")?;
    d.click_n(&format!("Включить · {FINISH_NAME}"), LAST)?;
    let before = history.then(|| d.app.scene.rules.clone());
    d.click("Применить шаблон")?;
    if d.app.scene.rules.counters.len() != 1 || d.app.scene.rules.rules.len() != 4 {
        return Err("Не удалось импортировать и переназначить шаблон отметок.".into());
    }
    if let Some(before) = before {
        let applied = d.app.scene.rules.clone();
        d.click("Отменить")?;
        if d.app.scene.rules != before {
            return Err("Отмена не сняла весь пакет правил одним действием.".into());
        }
        d.click("Повторить")?;
        if d.app.scene.rules != applied {
            return Err("Повтор не вернул весь пакет правил.".into());
        }
    }

    d.click("Таймеры")?;
    d.click("+ Таймер")?;
    d.edit("Таймер 1", LAST, TIMER_NAME)?;
    edit_prefix(d, "Секунд ", TIMER_SECONDS)?;
    d.click("Сценарии")?;
    new_rule(d, "Начать отсчёт у ворот", "Игровое действие")?;
    d.click("+ Условие")?;
    d.click("Значение флага")?;
    d.click_n("Игрок в зоне", LAST)?;
    click_actions(d, "+ Действие", 0)?;
    click_actions(d, "Установить флаг", LAST)?;
    click_action_kind(d, "Запустить таймер")?;
    add_quest_stage(d, "1")?;

    new_rule(d, "Сбросить попытку по времени", "Таймер истёк")?;
    add_reset(d)?;
    new_rule(d, "Повторить попытку вручную", "Игровое действие")?;
    d.click("Отметить · F")?;
    d.click_n("Повторить попытку · R", LAST)?;
    add_reset(d)?;

    new_rule(d, "Завершить обход и остановить таймер", "Игровое действие")?;
    d.click("+ Условие")?;
    d.click("Значение флага")?;
    d.click_n("Сравнение счётчика", LAST)?;
    edit_prefix(d, "Значение ", "2")?;
    add_quest_stage(d, "4")?;
    click_actions(d, "+ Действие", 0)?;
    click_actions(d, "Установить флаг", LAST)?;
    click_action_kind(d, "Остановить таймер")?;
    check_scene(d)?;
    d.hold(24)
}

fn map_index(size: [u16; 2], [col, row]: [u16; 2]) -> usize {
    usize::from(row) * usize::from(size[0]) + usize::from(col)
}

fn check_wall(d: &Driver<'_, '_>, expected: Collision, tile: Tile) -> Result<(), String> {
    let map = d.app.scene.map_2d.as_ref().ok_or("Карта отсутствует.")?;
    if map.collision_at(WALL.map(i32::from)) != expected
        || map.layers[0][map_index(map.size, WALL)] != Some(tile)
    {
        return Err("При правке коллизии изменилась плитка или не обновилась стена.".into());
    }
    Ok(())
}

fn point(d: &Driver<'_, '_>, cell: [u16; 2]) -> Result<eframe::egui::Pos2, String> {
    d.app
        .map_editor
        .transform()
        .map(|view| view.at_cell(cell))
        .ok_or("Не найдена проекция карты.")
        .map_err(str::to_owned)
}

fn at_cell(d: &mut Driver<'_, '_>, cell: [u16; 2]) -> Result<(), String> {
    let point = point(d, cell)?;
    d.frame(proof_input::hover(point))?;
    d.frame(proof_input::click(point))?;
    d.settle(3)
}

fn road(d: &mut Driver<'_, '_>, row: u16) -> Result<(), String> {
    at_cell(d, [3, row])?;
    let start = point(d, [3, row])?;
    d.frame(vec![proof_input::pointer_button(
        start,
        PointerButton::Primary,
        true,
    )])?;
    for col in 4..=22 {
        d.frame(vec![Event::PointerMoved(point(d, [col, row])?)])?;
    }
    let end = point(d, [22, row])?;
    d.frame(vec![proof_input::pointer_button(
        end,
        PointerButton::Primary,
        false,
    )])?;
    d.settle(3)
}

fn edit_prefix(d: &mut Driver<'_, '_>, prefix: &str, value: &str) -> Result<(), String> {
    let label = |d: &Driver<'_, '_>| {
        d.display.last_output().and_then(|output| {
            proof_input::labels(output)
                .into_iter()
                .find(|label| label.text.starts_with(prefix))
                .map(|label| label.text)
        })
    };
    if label(d).is_none() {
        scroll_panel(d, -238.0)?;
    }
    let label = label(d).ok_or_else(|| format!("Поле «{prefix}» не найдено."))?;
    d.edit(&label, LAST, value)?;
    d.key(Key::Enter)
}

fn edit_map_name(d: &mut Driver<'_, '_>, name: &str, value: &str) -> Result<(), String> {
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
        .filter(|label| label.text == name)
        .enumerate()
        .filter(|(_, label)| label.center().x > edge)
        .map(|(index, _)| index)
        .last()
        .ok_or_else(|| format!("Поле имени «{name}» не найдено в панели карты."))?;
    d.edit(name, occurrence, value)
}

pub(super) fn new_rule(d: &mut Driver<'_, '_>, name: &str, event: &str) -> Result<(), String> {
    if d.point("+ Правило", 0).is_err() {
        scroll_panel(d, 1250.0)?;
    }
    d.click("+ Правило")?;
    let title = d
        .app
        .scene
        .rules
        .rules
        .last()
        .ok_or("Правило не создано.")?
        .name
        .clone();
    d.edit(&title, LAST, name)?;
    d.click("Выбрать событие")?;
    d.click_n(event, LAST)
}

fn add_reset(d: &mut Driver<'_, '_>) -> Result<(), String> {
    click_actions(d, "+ Действие", 0)?;
    click_actions(d, "Установить флаг", LAST)?;
    click_action_kind(d, "Перезапустить попытку")
}

fn add_quest_stage(d: &mut Driver<'_, '_>, stage: &str) -> Result<(), String> {
    click_actions(d, "+ Действие", 0)?;
    click_actions(d, "Установить флаг", LAST)?;
    d.click_n("Задать этап задания", LAST)?;
    if stage != "1" {
        edit_prefix(d, "Этап ", stage)?;
    }
    Ok(())
}

pub(super) fn scroll_panel(d: &mut Driver<'_, '_>, amount: f32) -> Result<(), String> {
    let rect = d.display.context().content_rect();
    wheel(
        d,
        eframe::egui::pos2(rect.right() - 112.0, rect.top() + rect.height() * 0.70),
        amount,
    )
}

pub(super) fn wheel(
    d: &mut Driver<'_, '_>,
    point: eframe::egui::Pos2,
    amount: f32,
) -> Result<(), String> {
    d.frame(proof_input::hover(point))?;
    d.frame(vec![Event::MouseWheel {
        unit: eframe::egui::MouseWheelUnit::Point,
        delta: eframe::egui::vec2(0.0, amount),
        modifiers: Modifiers::NONE,
    }])?;
    d.settle(13)
}

pub(super) fn click_action_kind(d: &mut Driver<'_, '_>, label: &str) -> Result<(), String> {
    if d.point(label, LAST).is_err() {
        let menu = d
            .point("Запустить таймер", LAST)
            .or_else(|_| d.point("Задать счётчик", LAST))?;
        wheel(d, menu, -215.0)?;
    }
    d.click_n(label, LAST)
}

fn click_actions(d: &mut Driver<'_, '_>, label: &str, occurrence: usize) -> Result<(), String> {
    if d.point(label, occurrence).is_err() {
        scroll_panel(d, -245.0)?;
    }
    d.click_n(label, occurrence)
}

fn check_scene(d: &Driver<'_, '_>) -> Result<(), String> {
    let scene = &d.app.scene;
    scene.validate()?;
    let map = scene.map_2d.as_ref().ok_or("Карта отсутствует.")?;
    for (name, [x, z]) in [(FIRST_NAME, FIRST), (SECOND_NAME, SECOND)] {
        if !scene
            .rules
            .zones
            .iter()
            .any(|zone| zone.name == name && zone.position == [x as f32 + 0.5, 0.0, z as f32 + 0.5])
            || map.layers[1][map_index(map.size, [x, z])].is_none()
        {
            return Err(format!("Зона и декор отметки «{name}» не совпадают."));
        }
    }
    if !scene
        .rules
        .timers
        .iter()
        .any(|timer| timer.name == TIMER_NAME && (timer.seconds - 5.5).abs() < 0.01)
        || !scene.rules.rules.iter().any(|rule| {
            rule.conditions
                .iter()
                .any(|condition| matches!(condition, Condition::Counter { value: 2, .. }))
                && rule
                    .actions
                    .iter()
                    .any(|action| matches!(action, Action::SetQuestStage { stage: 4, .. }))
        })
        || !scene
            .rules
            .initially_disabled_objects
            .contains(&finish(d)?.id)
    {
        return Err("Правила финиша, таймер или порог счётчика не настроены.".into());
    }
    Ok(())
}

fn finish<'d>(d: &'d Driver<'_, '_>) -> Result<&'d WorldObject, String> {
    d.app
        .scene
        .world
        .objects
        .iter()
        .find(|object| object.name == FINISH_NAME)
        .ok_or("Маяк финиша отсутствует.")
        .map_err(str::to_owned)
}

fn position(d: &Driver<'_, '_>) -> Result<[f32; 3], String> {
    d.app
        .map_player
        .as_ref()
        .map(|player| player.world_position())
        .ok_or("Игрок на 2D-карте не создан.")
        .map_err(str::to_owned)
}

fn held(d: &mut Driver<'_, '_>, key: Key, pressed: bool) -> Result<(), String> {
    d.frame(vec![proof_input::key_event(key, Modifiers::NONE, pressed)])
}

pub(super) fn move_axis(d: &mut Driver<'_, '_>, axis: usize, target: f32) -> Result<(), String> {
    let sign = (target - position(d)?[axis]).signum();
    let key = match (axis, sign > 0.0) {
        (0, true) => Key::D,
        (0, false) => Key::A,
        (2, true) => Key::S,
        _ => Key::W,
    };
    if (target - position(d)?[axis]).abs() > 0.20 {
        held(d, key, true)?;
        for _ in 0..290 {
            if (target - position(d)?[axis]) * sign <= 0.24 {
                break;
            }
            d.frame(Vec::new())?;
        }
        held(d, key, false)?;
        d.settle(9)?;
    }
    if (target - position(d)?[axis]).abs() > 0.32 {
        return Err(format!(
            "Игрок {:?} не дошёл по оси {axis} до {target}.",
            position(d)?
        ));
    }
    Ok(())
}

fn require_count(d: &Driver<'_, '_>, id: u32, count: i32) -> Result<(), String> {
    let rule_set = &d.app.scene.rules;
    let actual = d.app.rule_session.counter(rule_set, id);
    if actual != Some(count) {
        return Err(format!(
            "Счётчик отметок ожидал {count}, получено {actual:?}."
        ));
    }
    let counter = rule_set.counter(id).ok_or("Счётчик не найден.")?;
    d.expect_hud(&format!("{} · {count}", counter.name))
}

pub(super) fn play(d: &mut Driver<'_, '_>, preview: bool) -> Result<(), String> {
    check_scene(d)?;
    let rules = &d.app.scene.rules;
    let counter = rules.counters.first().ok_or("Нет счётчика отметок.")?.id;
    let timer = rules.timers.first().ok_or("Нет таймера.")?.id;
    let finish = finish(d)?.id;
    if d.app.rule_session.object_enabled(finish) {
        return Err("Маяк включён до обеих отметок.".into());
    }
    d.settle(8)?;
    require_count(d, counter, 0)?;
    move_axis(d, 0, 7.5)?;
    d.key(Key::F)?;
    require_count(d, counter, 1)?;
    if d.app.playback.recording() {
        d.key(Key::F6)?;
    }
    d.expect_hud(TIMER_NAME)?;
    d.key(Key::F)?;
    require_count(d, counter, 1)?;
    if !d.app.rule_session.trace().any(|trace| {
        matches!(
            trace.event,
            rules::Event::GameAction {
                action: GameAction::Mark
            }
        ) && trace.outcome == TraceOutcome::AlreadyFired
    }) {
        return Err("Повтор той же отметки не отразился в журнале.".into());
    }
    d.hold(26)?;
    for _ in 0..111 {
        if d.app.rule_session.timer_remaining(timer).is_none() {
            break;
        }
        d.settle(4)?;
    }
    require_count(d, counter, 0)?;
    d.expect_hud("Попытка сброшена")?;
    if (position(d)?[0] - 3.5).abs() > 0.03 {
        return Err("Таймер не вернул игрока к стартовой клетке.".into());
    }
    d.key(Key::R)?;
    require_count(d, counter, 0)?;
    d.hold(20)?;

    move_axis(d, 0, 7.5)?;
    d.key(Key::F)?;
    require_count(d, counter, 1)?;
    held(d, Key::D, true)?;
    d.settle(35)?;
    held(d, Key::D, false)?;
    d.settle(4)?;
    if !(8.3..9.0).contains(&position(d)?[0]) {
        return Err(format!(
            "Стена не остановила игрока перед клеткой {WALL:?}: {:?}.",
            position(d)?
        ));
    }
    move_axis(d, 2, 9.5)?;
    move_axis(d, 0, 10.5)?;
    move_axis(d, 2, 10.5)?;
    move_axis(d, 0, 16.5)?;
    d.key(Key::F)?;
    require_count(d, counter, 2)?;
    if !d.app.rule_session.object_enabled(finish)
        || d.app.rule_session.timer_remaining(timer).is_some()
    {
        return Err("Две отметки не включили маяк или таймер не остановился.".into());
    }
    d.expect_hud(&format!("Выполнено · {QUEST_NAME}"))?;
    if d.app.playback.recording() {
        d.key(Key::F6)?;
    }
    if preview {
        d.expect_hud("Завершить обход")?;
    }
    move_axis(d, 0, 20.5)?;
    d.hold(42)
}
