//! A battle authored from an empty map by background GUI input.
use eframe::egui::{Event, Key, PointerButton, Pos2};

use super::{Driver, staged_proof};
use crate::{
    map2d::{Collision, Mode},
    proof_input,
    tactics::{Battle, Goal, Outcome, Scenario, Team, UnitId},
};
use std::path::Path;

const HERO: [u16; 2] = [3, 5];
const ALLY: [u16; 2] = [3, 6];
const RIVAL: [u16; 2] = [7, 7];
const GOAL: [u16; 2] = [8, 7];
const LAST: usize = usize::MAX;

pub(super) fn author(d: &mut Driver<'_, '_>, atlas: &Path) -> Result<(), String> {
    d.click("Новый…")?;
    if d.point("Продолжить без сохранения", 0).is_ok() {
        d.click("Продолжить без сохранения")?;
    }
    d.click_n("2D сверху", LAST)?;
    if d.app.scene.tactics.is_some()
        || !d.app.scene.map_2d.as_ref().is_some_and(|map| {
            map.mode == Mode::Top && map.layers.iter().flatten().all(Option::is_none)
        })
    {
        return Err("Тактический пример должен начинаться с пустой карты.".into());
    }
    d.app
        .proof
        .as_mut()
        .ok_or("Нет фонового окружения.")?
        .map_png = Some(atlas.into());
    d.click("Импорт PNG…")?;
    if d.app.scene.map_2d.as_ref().unwrap().atlases.len() != 3 {
        return Err("Не загружен PNG для тактики.".into());
    }
    d.click("Прямоугольник")?;
    d.edit("Тайл 0", 0, "2")?;
    d.key(Key::Enter)?;
    rectangle(d, [1, 3], [12, 9])?;
    d.edit("Тайл 2", 0, "9")?;
    d.key(Key::Enter)?;
    rectangle(d, [5, 4], [5, 6])?;
    d.click("Столкновения")?;
    d.click("Стена")?;
    rectangle(d, [5, 4], [5, 6])?;
    let map = d.app.scene.map_2d.as_ref().unwrap();
    if map.collision_at([5, 5]) != Collision::Solid || map.collision_at([5, 7]) != Collision::Empty
    {
        return Err("Коллизия тактической стены не отделена от свободного прохода.".into());
    }

    d.click("Тактика")?;
    d.click("Настроить пошаговую тактику")?;
    d.click("+ Боец игрока")?;
    at_cell(d, HERO)?;
    d.edit("Игрок 1", LAST, "Разведчик")?;
    d.edit("Очки действий 4", 0, "6")?;
    d.key(Key::Enter)?;
    d.click("+ Боец игрока")?;
    at_cell(d, ALLY)?;
    d.edit("Игрок 2", LAST, "Связной")?;
    d.click("+ Соперник")?;
    at_cell(d, RIVAL)?;
    d.edit("Соперник 3", LAST, "Дозорный")?;
    d.edit("Очки действий 4", 0, "2")?;
    d.key(Key::Enter)?;
    d.edit("Здоровье 8", 0, "2")?;
    d.key(Key::Enter)?;
    d.click("Победить всех соперников")?;
    d.click_n("Дойти до клетки", LAST)?;
    d.click("Поставить цель на карту")?;
    at_cell(d, GOAL)?;
    d.edit("Цена шага 1", 0, "2")?;
    d.key(Key::Enter)?;
    d.click("Ограничить число раундов")?;
    d.edit("Лимит раундов 8", 0, "4")?;
    d.key(Key::Enter)?;

    let config = scenario(d)?;
    let [hero, ally, rival] = config.units.as_slice() else {
        return Err("Позиции трёх бойцов не сохранены.".into());
    };
    if (hero.team, hero.cell, hero.points, hero.name.as_str())
        != (Team::Player, HERO, 6, "Разведчик")
        || (ally.team, ally.cell, ally.name.as_str()) != (Team::Player, ALLY, "Связной")
        || (
            rival.team,
            rival.cell,
            rival.points,
            rival.health,
            rival.name.as_str(),
        ) != (Team::Rival, RIVAL, 2, 2, "Дозорный")
        || config.rules.goal != (Goal::ReachCell { cell: GOAL })
        || config.rules.move_cost != 2
        || config.rules.round_limit != Some(4)
    {
        return Err(format!(
            "Изменения тактического сценария не сохранились. {config:?}"
        ));
    }
    d.hold(28)
}

pub(super) fn play(d: &mut Driver<'_, '_>, preview: bool) -> Result<(), String> {
    let ids = scenario(d)?
        .units
        .iter()
        .map(|unit| unit.id)
        .collect::<Vec<_>>();
    let [hero, ally, enemy] = ids.as_slice() else {
        return Err("Нет трёх бойцов.".into());
    };
    let [hero, ally, enemy] = [*hero, *ally, *enemy];
    d.settle(5)?;
    d.expect_hud("Раунд 1")?;
    at_cell(d, ALLY)?;
    if d.app.tactics_play.selected != Some(ally) || cell(d, hero)? != HERO {
        return Err("Выбор союзника изменил занятость клеток.".into());
    }
    d.key(Key::Tab)?;
    if d.app.tactics_play.selected != Some(hero) {
        return Err("Tab не вернул выбор разведчику.".into());
    }
    at_cell(d, [5, 5])?;
    d.expect_hud("Клетка занята или недоступна")?;
    at_cell(d, [6, 5])?;
    d.expect_hud("Не хватает очков действий")?;
    at_cell(d, [4, 7])?;
    if cell(d, hero)? != [4, 7] || battle(d)?.fighter(hero).unwrap().state.points != 0 {
        return Err("Ход разведчика не использовал шесть очков за три клетки.".into());
    }

    if preview {
        let snapshot = battle(d)?.snapshot();
        staged_proof::panel_click(d, "■ Стоп")?;
        staged_proof::panel_click(d, "Сохранить")?;
        staged_proof::reload(d)?;
        staged_proof::panel_click(d, "Продолжить")?;
        if battle(d)?.snapshot() != snapshot {
            return Err("Продолжение потеряло бойцов, позиции, раунд или очки действий.".into());
        }
    }

    d.key(Key::Enter)?;
    d.expect_hud("Раунд 2")?;
    if cell(d, enemy)? != [6, 7] || battle(d)?.fighter(hero).unwrap().state.points != 6 {
        return Err("Соперник не выполнил свой ход или у игрока не восстановились очки.".into());
    }
    at_cell(d, [5, 7])?;
    at_cell(d, [6, 7])?;
    if battle(d)?.fighter(enemy).unwrap().alive() || battle(d)?.outcome != Outcome::Ongoing {
        return Err("Атака не победила дозорного или вернулась старая цель победы.".into());
    }
    d.expect_hud("Разведчик атакует")?;
    d.click("Завершить ход")?;
    d.expect_hud("Раунд 3")?;
    at_cell(d, GOAL)?;
    d.settle(4)?;
    if cell(d, hero)? != GOAL || battle(d)?.outcome != Outcome::Won {
        return Err("Тактическая цель не сработала при входе на заданную клетку.".into());
    }
    d.expect_hud("Победа")?;
    d.hold(52)
}

fn scenario<'a>(d: &'a Driver<'_, '_>) -> Result<&'a Scenario, String> {
    d.app
        .scene
        .tactics
        .as_ref()
        .ok_or_else(|| "Тактический сценарий не создан.".into())
}

fn battle<'a>(d: &'a Driver<'_, '_>) -> Result<&'a Battle, String> {
    d.app
        .tactics_battle
        .as_ref()
        .ok_or_else(|| "Бой не запущен.".into())
}

fn cell(d: &Driver<'_, '_>, id: UnitId) -> Result<[u16; 2], String> {
    battle(d)?
        .fighter(id)
        .map(|fighter| fighter.state.cell)
        .ok_or_else(|| "Боец отсутствует.".into())
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
    d.settle(4)
}

fn rectangle(d: &mut Driver<'_, '_>, from: [u16; 2], to: [u16; 2]) -> Result<(), String> {
    let from = point(d, from)?;
    let to = point(d, to)?;
    d.frame(proof_input::hover(from))?;
    d.frame(vec![proof_input::pointer_button(
        from,
        PointerButton::Primary,
        true,
    )])?;
    d.frame(vec![Event::PointerMoved(to)])?;
    d.frame(vec![proof_input::pointer_button(
        to,
        PointerButton::Primary,
        false,
    )])?;
    d.settle(4)
}
