//! Scripted input proof for generation, collision validation and normal game controls.
use std::{fs, hint::black_box, path::Path, time::Instant};

use super::{
    Driver, SIZE, Sink, io_error, proof_input,
    staged_proof::{number, panel_click, reload},
    top_proof,
};
use crate::{
    Studio, configure_context,
    dungeon::{self, Cell},
    dungeon_map,
    game_export::GamePackage,
    offscreen::Offscreen,
};

pub(super) fn author(d: &mut Driver<'_, '_>) -> Result<(), String> {
    d.click("Новый…")?;
    if d.point("Продолжить без сохранения", 0).is_ok() {
        d.click("Продолжить без сохранения")?;
    }
    d.click("Сгенерировать закрытую карту…")?;
    number(d, "Ширина ", "18")?;
    number(d, "Высота ", "18")?;
    d.click("Построить карту")?;
    d.click("Создать сцену")?;
    let (exit, expected) = current_route(d)?;
    if d.app
        .scene
        .map_2d
        .as_ref()
        .is_none_or(|map| map.size != [18, 18])
        || expected.len() < 7
    {
        return Err("Генератор не создал карту 18 × 18 с проходом.".into());
    }
    d.click("Проверка маршрута")?;
    let success = format!("Путь найден · {} шагов по клеткам", expected.len() - 1);
    d.click("Проверить путь")?;
    d.expect_hud(&success)?;
    d.click("Столкновения")?;
    d.click("Стена")?;
    let cell = d
        .app
        .map_editor
        .transform()
        .ok_or("Нет проекции карты.")?
        .at_cell(exit);
    d.frame(proof_input::hover(cell))?;
    d.frame(proof_input::click(cell))?;
    d.settle(4)?;
    d.click("Проверить путь")?;
    d.expect_hud("Выход недостижим по клеткам")?;
    d.click("Отменить")?;
    d.click("Проверить путь")?;
    d.expect_hud(&success)?;
    if current_route(d)?.1 != expected {
        return Err("Отмена коллизии не вернула тот же путь.".into());
    }
    d.hold(22)
}

pub(super) fn play(d: &mut Driver<'_, '_>) -> Result<usize, String> {
    let (exit, path) = current_route(d)?;
    if !d.app.game_running {
        panel_click(d, "▶ Играть")?;
    }
    let mut start = path[0];
    for (index, pair) in path.windows(2).enumerate() {
        let direction = |pair: &[Cell]| [pair[0][0] != pair[1][0], pair[0][1] != pair[1][1]];
        let next_pair = path.get(index + 1..index + 3);
        if next_pair.is_none_or(|next| direction(pair) != direction(next)) {
            let end = pair[1];
            let [x, z] = dungeon_map::center(end);
            if start[0] != end[0] {
                top_proof::move_axis(d, 0, x)?;
            }
            if start[1] != end[1] {
                top_proof::move_axis(d, 2, z)?;
            }
            start = end;
        }
    }
    let [x, z] = dungeon_map::center(exit);
    let foot = d.app.game_position();
    if (foot.x - x).hypot(foot.z - z) >= 0.42 {
        return Err(format!(
            "Герой не дошёл до выхода {exit:?}. Положение {foot:?}."
        ));
    }
    let name = dungeon_map::COUNTER_NAME;
    let counter = d
        .app
        .scene
        .rules
        .counters
        .iter()
        .find(|row| row.name == name)
        .ok_or("В проекте нет счётчика выхода.")?
        .id;
    if d.app.rule_session.counter(&d.app.scene.rules, counter) != Some(1) {
        return Err("Движок не сработал при входе в зону выхода.".into());
    }
    d.expect_hud(&format!("{name} · 1"))?;
    d.hold(24)?;
    Ok(path.len() - 1)
}

fn current_route(d: &Driver<'_, '_>) -> Result<(Cell, Vec<Cell>), String> {
    let scene = &d.app.scene;
    let map = scene.map_2d.as_ref().ok_or("В сцене нет карты.")?;
    let zone = scene
        .rules
        .zones
        .iter()
        .find(|zone| zone.name == dungeon_map::EXIT_NAME)
        .ok_or("В сцене нет зоны выхода.")?;
    let exit = dungeon_map::exit_cell(scene, zone.id).ok_or("Выход вне карты.")?;
    let path = dungeon_map::route(map, exit).ok_or("Клеточный путь до выхода не найден.")?;
    Ok((exit, path))
}

pub(super) fn smoke(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let project = output.join("closed-labyrinth.robot.json");
    let game = output.join("ClosedLabyrinth.exe");
    let mut renderer = Offscreen::new(SIZE, crate::viewport3d::VIEWPORT_MSAA)?;
    configure_context(renderer.context());
    let mut app = Studio::default();
    app.desk.recovery_root = Some(output.join("recovery-test"));
    let mut sink = Sink::None;
    let mut d = Driver::new(app, &mut renderer, &mut sink, Some((&project, &game)));
    d.settle(6)?;
    author(&mut d)?;
    panel_click(&mut d, "Сохранить как…")?;
    reload(&mut d)?;
    let steps = play(&mut d)?;
    fs::write(output.join("dungeon-editor.json"), report(&d.app, steps)?).map_err(io_error)?;
    panel_click(&mut d, "■ Стоп")?;
    panel_click(&mut d, "Собрать игру для Windows")?;
    if !game.is_file() {
        return Err(format!("EXE карты не записан. {}", d.app.status));
    }
    Ok(())
}

pub(super) fn player(package: GamePackage, report_path: &Path) -> Result<(), String> {
    let mut renderer = Offscreen::new(SIZE, crate::viewport3d::VIEWPORT_MSAA)?;
    configure_context(renderer.context());
    let app = Studio::from_package(package, renderer.context());
    let mut sink = Sink::None;
    let mut d = Driver::new(app, &mut renderer, &mut sink, None);
    d.settle(6)?;
    let steps = play(&mut d)?;
    fs::write(report_path, report(&d.app, steps)?).map_err(io_error)
}

pub(super) fn metrics(path: &Path) -> Result<(), String> {
    let mut results = Vec::new();
    for (size, rooms) in [([18, 18], 4), ([34, 24], 8), ([96, 96], 24)] {
        let settings = dungeon::Settings {
            size,
            rooms,
            ..Default::default()
        };
        let mut times = Vec::new();
        let mut lengths = Vec::new();
        for seed in 0..256 {
            let start = Instant::now();
            let layout = black_box(dungeon::generate(black_box(dungeon::Settings {
                seed,
                ..settings
            }))?);
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            lengths.push(layout.route.len() - 1);
        }
        times.sort_unstable_by(f64::total_cmp);
        lengths.sort_unstable();
        results.push(serde_json::json!({
            "width": size[0], "height": size[1], "rooms": rooms,
            "first_seed": 0, "last_seed": 255,
            "average_ms": times.iter().sum::<f64>() / times.len() as f64,
            "p95_ms": times[(95 * times.len()).div_ceil(100) - 1],
            "minimum_route_steps": lengths[0], "maximum_route_steps": lengths[lengths.len() - 1],
        }));
    }
    let data = serde_json::to_vec_pretty(&results).map_err(|error| error.to_string())?;
    fs::write(path, data).map_err(io_error)
}

fn report(app: &Studio, steps: usize) -> Result<String, String> {
    let [width, height] = app.scene.map_2d.as_ref().ok_or("В сцене нет карты.")?.size;
    Ok(format!(
        "{{\"route_steps\":{steps},\"exit_counter\":1,\"width\":{width},\"height\":{height}}}\n"
    ))
}
