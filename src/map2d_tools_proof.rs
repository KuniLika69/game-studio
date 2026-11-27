//! Editor input proof for tile brushes, patch clipboard, and gesture history.
use eframe::egui::{Event, Key, PointerButton, Pos2};

use super::Driver;
use crate::{
    map2d::{Collision, Map, Mode, Tile},
    proof_input,
};

const AREA_FROM: [u16; 2] = [3, 2];
const AREA_TO: [u16; 2] = [7, 6];
const DIAGONAL: [u16; 2] = [8, 7];
const COPY_TO: [u16; 2] = [15, 8];
const CLIPPED: [u16; 2] = [24, 16];

pub(super) fn author(d: &mut Driver<'_, '_>) -> Result<(), String> {
    d.click("Новый…")?;
    if d.point("Продолжить без сохранения", 0).is_ok() {
        d.click("Продолжить без сохранения")?;
    }
    d.click("2D сверху")?;
    let blank = map(d)?.clone();
    if blank.mode != Mode::Top || blank.layers.iter().flatten().any(Option::is_some) {
        return Err("Для пробы инструментов нужна пустая карта сверху.".into());
    }

    d.click("Прямоугольник")?;
    start_drag(d, AREA_TO, AREA_FROM)?;
    if map(d)? != &blank {
        return Err("Предпросмотр прямоугольника изменил карту до отпускания кнопки.".into());
    }
    release(d, AREA_FROM)?;
    let rectangle = map(d)?.clone();
    if marked(&rectangle, 0) != 25 || tile(&rectangle, AREA_FROM, 0) != Some(atlas(0)) {
        return Err("Кисть не закрасила прямоугольник 5 × 5.".into());
    }
    d.click("Отменить")?;
    if map(d)? != &blank {
        return Err("Одна отмена должна возвратить весь прямоугольник к пустой карте.".into());
    }
    d.click("Повторить")?;
    if map(d)? != &rectangle {
        return Err("Повтор не вернул прямоугольник.".into());
    }

    d.click("Карандаш")?;
    at_cell(d, DIAGONAL)?;
    d.click("Заливка")?;
    d.edit("Тайл 0", 0, "8")?;
    d.key(Key::Enter)?;
    at_cell(d, [5, 4])?;
    if marked(map(d)?, 0) != 26
        || tile(map(d)?, AREA_TO, 0) != Some(atlas(8))
        || tile(map(d)?, DIAGONAL, 0) != Some(atlas(0))
    {
        return Err("Заливка пересекла диагональную границу или оставила дыры.".into());
    }

    d.click("Декор")?;
    d.click("Карандаш")?;
    at_cell(d, [5, 3])?;
    d.click("Высокие объекты")?;
    at_cell(d, [6, 5])?;
    let artwork = map(d)?.layers.clone();
    d.click("Столкновения")?;
    d.click("Стена")?;
    d.click("Прямоугольник")?;
    start_drag(d, [4, 3], [5, 4])?;
    release(d, [5, 4])?;
    if map(d)?.layers != artwork || map(d)?.collision_at([5, 4]) != Collision::Solid {
        return Err("Прямоугольная кисть коллизий повредила слой тайлов.".into());
    }
    let blocked = map(d)?.clone();
    d.click("Заливка")?;
    d.click("Без столкновения")?;
    at_cell(d, [4, 3])?;
    if map(d)?.layers != artwork || map(d)?.colliders.contains(&Collision::Solid) {
        return Err("Заливка коллизий не очистила выбранный блок.".into());
    }
    d.click("Отменить")?;
    if map(d)? != &blocked {
        return Err("Отмена не восстановила блок столкновений.".into());
    }

    d.click("Участок карты")?;
    start_drag(d, AREA_FROM, AREA_TO)?;
    release(d, AREA_TO)?;
    d.expect_hud("Скопировано 5 × 5 · тайлы и столкновения")?;
    let source = map(d)?.clone();
    d.click("Вставить участок")?;
    at_cell(d, COPY_TO)?;
    let pasted = map(d)?.clone();
    if marked(&pasted, 0) != 51
        || tile(&pasted, COPY_TO, 0) != Some(atlas(8))
        || tile(&pasted, [17, 9], 1) != Some(atlas(8))
        || tile(&pasted, [18, 11], 2) != Some(atlas(8))
        || pasted.collision_at([17, 10]) != Collision::Solid
        || tile(&pasted, [18, 9], 1).is_some()
    {
        return Err("Вставка не перенесла все слои и отдельные столкновения.".into());
    }
    d.click("Отменить")?;
    if map(d)? != &source {
        return Err("Одна отмена должна снять вставку целиком.".into());
    }
    d.click("Повторить")?;
    if map(d)? != &pasted {
        return Err("Повтор не вернул вставленный участок.".into());
    }

    at_cell(d, CLIPPED)?;
    if marked(map(d)?, 0) != 55 || map(d)?.collision_at([25, 17]) != Collision::Solid {
        return Err("Часть у правой нижней границы не обрезалась до четырёх клеток.".into());
    }

    d.click("Копировать участок")?;
    d.click("Слои тайлов")?;
    start_drag(d, [4, 3], [5, 4])?;
    release(d, [5, 4])?;
    d.expect_hud("Скопировано 2 × 2 · столкновения")?;
    d.click("Вставить участок")?;
    let artwork = map(d)?.layers.clone();
    at_cell(d, [12, 5])?;
    if map(d)?.layers != artwork || map(d)?.collision_at([13, 6]) != Collision::Solid {
        return Err("Вставка только коллизий изменила рисунок карты.".into());
    }

    d.click("Тайлы")?;
    d.click("Покрытие")?;
    d.click("Карандаш")?;
    let previous = map(d)?.clone();
    start_drag(d, [9, 12], [10, 12])?;
    for x in 11..=14 {
        d.frame(vec![Event::PointerMoved(point(d, [x, 12])?)])?;
    }
    release(d, [14, 12])?;
    let stroked = map(d)?;
    if (9..=14).any(|x| tile(stroked, [x, 12], 0) != Some(atlas(8))) {
        return Err("Непрерывный штрих оставил пустые клетки.".into());
    }
    d.click("Отменить")?;
    if map(d)? != &previous {
        return Err("Одна отмена должна снять весь непрерывный штрих.".into());
    }
    d.hold(24)
}

fn map<'a>(d: &'a Driver<'_, '_>) -> Result<&'a Map, String> {
    d.app
        .scene
        .map_2d
        .as_ref()
        .ok_or_else(|| "Карта отсутствует.".into())
}

fn atlas(index: u16) -> Tile {
    Tile { atlas: 1, index }
}

fn tile(map: &Map, [col, row]: [u16; 2], layer: usize) -> Option<Tile> {
    map.layers[layer][usize::from(row) * usize::from(map.size[0]) + usize::from(col)]
}

fn marked(map: &Map, layer: usize) -> usize {
    map.layers[layer].iter().flatten().count()
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

fn start_drag(d: &mut Driver<'_, '_>, from: [u16; 2], to: [u16; 2]) -> Result<(), String> {
    let from = point(d, from)?;
    let to = point(d, to)?;
    d.frame(proof_input::hover(from))?;
    d.frame(vec![proof_input::pointer_button(
        from,
        PointerButton::Primary,
        true,
    )])?;
    d.frame(vec![Event::PointerMoved(to)])?;
    d.settle(4)
}

fn release(d: &mut Driver<'_, '_>, cell: [u16; 2]) -> Result<(), String> {
    let point = point(d, cell)?;
    d.frame(vec![proof_input::pointer_button(
        point,
        PointerButton::Primary,
        false,
    )])?;
    d.settle(4)
}

pub(super) fn rectangle(
    d: &mut Driver<'_, '_>,
    from: [u16; 2],
    to: [u16; 2],
) -> Result<(), String> {
    d.click("Прямоугольник")?;
    start_drag(d, from, to)?;
    release(d, to)?;
    d.click("Карандаш")
}

#[cfg(test)]
mod terrain_capture {
    use std::{fs, path::Path};

    use serde_json::{Value, json};

    use super::*;
    use crate::{
        Studio, configure_context,
        editor_workspace::Workspace,
        map2d::{self, Layer},
        offscreen::Offscreen,
        proof::{SIZE, Sink, io_error, staged_proof::reload},
        viewport3d::VIEWPORT_MSAA,
    };

    #[test]
    #[ignore = "Uses the offscreen GPU and writes the 2D editor screenshots"]
    fn capture_terrain_workspaces() {
        capture(Path::new("verification/living-world/2d")).unwrap();
    }

    fn capture(output: &Path) -> Result<(), String> {
        fs::create_dir_all(output).map_err(io_error)?;
        let mut display = Offscreen::new(SIZE, VIEWPORT_MSAA)?;
        configure_context(display.context());
        let top = capture_mode(
            &mut display,
            output,
            Mode::Top,
            "forest-top",
            "Лес в сумерках",
            "Лесной брод",
        )?;
        let side = capture_mode(
            &mut display,
            output,
            Mode::Side,
            "station-side",
            "Неоновая станция",
            "Станция «Маяк»",
        )?;
        let report = json!({
            "capture_pixels": SIZE,
            "capture_method": "automated native editor render and input, projects saved and reopened",
            "maps": [top, side],
        });
        fs::write(
            output.join("terrain-workspaces.json"),
            serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
        )
        .map_err(io_error)
    }

    fn capture_mode(
        display: &mut Offscreen,
        output: &Path,
        mode: Mode,
        filename: &str,
        style: &str,
        title: &str,
    ) -> Result<Value, String> {
        let mut app = Studio {
            workspace: Workspace::Create,
            ..Studio::default()
        };
        app.desk.recovery_root = Some(output.join("recovery"));
        app.set_scene(map2d::scene(mode, false), None, title);
        let file = output.join(format!("{filename}.robot.json"));
        let unused_export = output.join(format!("{filename}.exe"));
        let mut sink = Sink::None;
        let mut d = Driver::new(app, display, &mut sink, Some((&file, &unused_export)));
        d.settle(6)?;
        d.click("Наборы тайлов")?;
        if mode == Mode::Side {
            d.click("Лес в сумерках")?;
            d.click("Неоновая станция")?;
        }
        d.click("Добавить набор")?;
        let Some(atlas) = map(&d)?.atlases.last().filter(|a| a.name == style) else {
            return Err(format!("Набор «{style}» не создан через редактор."));
        };
        let id = atlas.id;
        let sample = [2, 10];
        at_cell(&mut d, sample)?;
        if tile(map(&d)?, sample, 0)
            != Some(Tile {
                atlas: id,
                index: 0,
            })
        {
            return Err(format!(
                "Кисть не применила набор «{style}» в сцене «{title}»."
            ));
        }
        let map = d.app.scene.map_2d.as_mut().expect("2D map");
        match mode {
            Mode::Top => paint_brook(map, id),
            Mode::Side => paint_station(map, id),
        }
        d.app.changed();
        d.click("Наборы тайлов")?;
        if d.point("Показать столкновения", 0).is_err() {
            let anchor = d.point("Импорт PNG…", 0)?;
            d.frame(vec![
                Event::PointerMoved(anchor),
                Event::MouseWheel {
                    unit: eframe::egui::MouseWheelUnit::Point,
                    delta: eframe::egui::vec2(0.0, -250.0),
                    modifiers: eframe::egui::Modifiers::NONE,
                },
            ])?;
            d.settle(8)?;
        }
        d.click("Показать столкновения")?;
        d.position = None;
        d.frame(vec![Event::PointerGone])?;
        d.settle(4)?;
        let screenshot = format!("{filename}.png");
        image::save_buffer(
            output.join(&screenshot),
            &d.last,
            SIZE[0],
            SIZE[1],
            image::ColorType::Rgb8,
        )
        .map_err(|error| error.to_string())?;

        d.click("Сохранить как…")?;
        if !file.is_file() {
            return Err(format!("Карта не сохранилась. {}", d.app.status));
        }
        reload(&mut d)?;
        d.app.scene.validate()?;
        d.click("▶ Играть")?;
        if !d.app.game_running {
            return Err(format!("Предпросмотр «{title}» не запустился."));
        }
        d.click("■ Стоп")?;
        if d.app.game_running {
            return Err(format!("Предпросмотр «{title}» не остановился."));
        }

        let saved_map = d.app.scene.map_2d.as_ref().expect("2D map");
        let painted_cells = [0, 1, 2].map(|layer| marked(saved_map, layer));
        Ok(json!({
            "title": title,
            "mode": mode.title(),
            "terrain_pack": style,
            "screenshot": screenshot,
            "project": format!("{filename}.robot.json"),
            "painted_cells_by_layer": painted_cells,
            "placed_colliders": saved_map.colliders.iter().filter(|value| **value != Collision::Empty).count(),
            "input_actions": d.input_actions,
            "preview_started_and_stopped": true,
            "brush_used_new_atlas": true,
            "saved_and_reopened": true,
        }))
    }

    fn paint_brook(map: &mut Map, atlas: u32) {
        map.resize([28, 16]);
        map.set_spawn([3.5, 7.5]);
        for y in 0..map.size[1] {
            for x in 0..map.size[0] {
                let cell = [x, y];
                let edge = x == 0 || y == 0 || x == map.size[0] - 1 || y == map.size[1] - 1;
                let river_left = if y < 5 {
                    11
                } else if y > 10 {
                    13
                } else {
                    12
                };
                let river = (river_left..river_left + 4).contains(&x);
                let crossing = (7..=8).contains(&y);
                let path = crossing
                    || (x == 5 && (3..=12).contains(&y))
                    || (x > 16 && (19..=24).contains(&x) && (4..=5).contains(&y));
                let index = if edge {
                    1
                } else if path {
                    2
                } else if river {
                    3
                } else {
                    0
                };
                map.paint(cell, Layer::Ground, Some(Tile { atlas, index }));
                if edge || river && !crossing {
                    map.paint_collision(cell, Collision::Solid);
                }
                if !edge && !river && !path && (x * 47 + y * 19 + x * y) % 11 < 3 {
                    map.paint(cell, Layer::Detail, Some(Tile { atlas, index: 6 }));
                }
            }
        }
        for cell in [[4, 6], [4, 9], [11, 6], [11, 9], [16, 6], [16, 9], [23, 5]] {
            map.paint(cell, Layer::Tall, Some(Tile { atlas, index: 7 }));
        }
    }

    fn paint_station(map: &mut Map, atlas: u32) {
        map.resize([34, 14]);
        map.set_spawn([3.5, 10.0]);
        for x in 0..map.size[0] {
            let floor = match x {
                8..=9 | 21..=22 => None,
                10..=15 | 26..=30 => Some(9),
                _ => Some(10),
            };
            for y in 0..map.size[1] {
                let cell = [x, y];
                let index = if let Some(top) = floor.filter(|&top| y >= top) {
                    map.paint_collision(cell, Collision::Solid);
                    Some(if y == top { 4 } else { 5 })
                } else if y >= 12 {
                    Some(3)
                } else if (2..=6).contains(&x) && (5..=7).contains(&y)
                    || (25..=31).contains(&x) && (3..=5).contains(&y)
                {
                    Some(if y % 2 == 0 { 1 } else { 0 })
                } else {
                    None
                };
                if let Some(index) = index {
                    map.paint(cell, Layer::Ground, Some(Tile { atlas, index }));
                }
            }
        }
        for x in [8, 9, 17, 18, 21, 22, 23] {
            let cell = [x, if x < 16 { 8 } else { 7 }];
            map.paint(cell, Layer::Ground, Some(Tile { atlas, index: 4 }));
            map.paint_collision(cell, Collision::Platform);
        }
        for cell in [[2, 7], [4, 7], [6, 7], [26, 5], [28, 5], [30, 5]] {
            map.paint(cell, Layer::Detail, Some(Tile { atlas, index: 6 }));
        }
        for cell in [[6, 9], [14, 8], [20, 9], [27, 8], [32, 9]] {
            map.paint(cell, Layer::Tall, Some(Tile { atlas, index: 7 }));
        }
        map.set_respawn(Some([4, 9]));
    }
}
