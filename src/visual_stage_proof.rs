use std::{fs, path::Path};

use eframe::egui::Key;

use super::{
    Driver, FPS, SIZE, Sink, io_error, quarter_proof,
    staged_proof::{move_and_play, number, panel_click, play_linden, reload},
    top_proof,
};
use crate::{
    Studio, View, configure_context, mill, offscreen::Offscreen, proof_video::Video, viewport3d,
};

const BENCH: &str = "Скамья для проверки";
const EFFECT_RULE: &str = "Показать авторский эффект";

pub(super) fn record(output: &Path, releases: &Path, verification: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let staging = output.join("export-stage");
    fs::create_dir_all(&staging).map_err(io_error)?;
    let original_city = verification.join("v0.17/linden-marks.robot.json");
    let scene_city = output.join("linden-style.robot.json");
    let scene_map = output.join("river-style.robot.json");
    let preset = output.join("street-dust.effect.json");
    let staged_city = staging.join("LindenStyle.exe");
    let staged_map = staging.join("RiverStyle.exe");
    let city_game = releases.join("LindenStyle.exe");
    let map_game = releases.join("RiverStyle.exe");
    let video_file = output.join("stage-019.mp4");
    for path in [
        &scene_city,
        &scene_map,
        &preset,
        &staged_city,
        &staged_map,
        &city_game,
        &map_game,
        &video_file,
    ] {
        if path.exists() {
            return Err(format!("Путь занят. {}", path.display()));
        }
    }

    let mut video = Video::start(&video_file, SIZE[0], SIZE[1], FPS)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::default();
    app.desk.recovery_root = Some(output.join("recovery-test"));
    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((&original_city, &staged_city)),
        );
        d.settle(6)?;
        panel_click(&mut d, "Открыть…")?;
        author_material(&mut d, output)?;
        set_evening(&mut d)?;
        d.click("Персонаж")?;
        d.click("К детали")?;
        capture(&d, &output.join("linden-character.png"))?;
        author_effect(&mut d, &preset)?;
        rule_effect(&mut d, Some(BENCH))?;
        d.app.proof.as_mut().unwrap().scene = Some(scene_city);
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        quarter_proof::fresh(&mut d, View::Three)?;
        play_city(&mut d, true, Some(output))?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        app = d.app;
    }
    move_and_play(
        &mut video,
        &mut display,
        &staged_city,
        &city_game,
        "visual",
        &output.join("timing-city.json"),
    )?;
    eprintln!("0.19: граф материала, вечерний квартал, эффект правила и интерьер пройдены.");

    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(
            app,
            &mut display,
            &mut sink,
            Some((&scene_map, &staged_map)),
        );
        d.settle(6)?;
        d.click("Новый…")?;
        d.click(mill::MAP_NAME)?;
        d.click("Эффекты")?;
        d.app.proof.as_mut().unwrap().effect_file = Some(preset.clone());
        d.click("Импорт эффекта…")?;
        require_effect(&d)?;
        rule_effect(&mut d, None)?;
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "▶ Играть")?;
        play_map(&mut d)?;
        capture(&d, &output.join("river-2d.png"))?;
        panel_click(&mut d, "■ Стоп")?;
        panel_click(&mut d, "Собрать игру для Windows")?;
    }
    move_and_play(
        &mut video,
        &mut display,
        &staged_map,
        &map_game,
        "fx2d",
        &output.join("timing-river.json"),
    )?;
    video.finish()?;
    eprintln!("0.19: перенесённый эффект сработал в самостоятельной 2D-игре.");
    Ok(())
}

fn author_material(d: &mut Driver<'_, '_>, output: &Path) -> Result<(), String> {
    d.click("Поиск…")?;
    d.edit("Имя объекта, правила или ресурса", 0, BENCH)?;
    d.click_n(BENCH, usize::MAX)?;
    d.click("К объекту")?;
    quarter_proof::turn(d, -std::f32::consts::FRAC_PI_2, 0.32)?;
    d.settle(12)?;
    capture(d, &output.join("material-before.png"))?;
    let baseline = d.last.clone();
    if d.point("Граф материала…", 0).is_err() {
        top_proof::scroll_panel(d, -250.0)?;
    }
    d.click("Граф материала…")?;
    d.click("Шаблоны")?;
    d.click_n("Металл", usize::MAX)?;
    d.click("Применить шаблон")?;
    d.click("Закрыть граф")?;
    let after = selected_bench(d)?;
    if after
        .material_graph
        .as_ref()
        .and_then(|graph| graph.compile())
        .is_none()
    {
        return Err("Редактор не применил рабочий граф для скамьи.".into());
    }
    capture(d, &output.join("material-after.png"))?;
    if center_changes(&baseline, &d.last) < 100 {
        return Err("В ракурсе скамьи не видно изменения от графа материала.".into());
    }
    d.click("Граф материала…")?;
    d.click("На улице")?;
    d.click("Закрыть граф")?;
    capture(d, &output.join("material-outdoor.png"))?;
    let outdoor = d.last.clone();
    d.click("Граф материала…")?;
    d.click("В помещении")?;
    d.click("Закрыть граф")?;
    capture(d, &output.join("material-indoor.png"))?;
    if center_changes(&outdoor, &d.last) < 100 {
        return Err("В ракурсе скамьи не видно смены уличного и комнатного света.".into());
    }
    if d.point("Свет сцены", 0).is_ok() {
        d.click("Свет сцены")?;
    }
    Ok(())
}

fn selected_bench<'a>(d: &'a Driver<'_, '_>) -> Result<&'a crate::assets::Placement, String> {
    d.app
        .scene
        .world
        .models
        .iter()
        .find(|item| item.label == BENCH)
        .ok_or_else(|| "В квартале отсутствует скамья.".into())
}

fn set_evening(d: &mut Driver<'_, '_>) -> Result<(), String> {
    d.click("Среда")?;
    d.click("Настроить время суток и погоду")?;
    if d.point("Начальный час 13", 0).is_err()
        && !d.display.last_output().is_some_and(|output| {
            crate::proof_input::find_label_containing(output, "Начальный час", 0).is_some()
        })
    {
        d.click("Время суток")?;
    }
    number(d, "Начальный час", "17.2")?;
    number(d, "Часов за секунду", "0")?;
    Ok(())
}

fn author_effect(d: &mut Driver<'_, '_>, path: &Path) -> Result<(), String> {
    d.click("Эффекты")?;
    d.click("Добавить эффект")?;
    d.click_n("Пыль", usize::MAX)?;
    number(d, "Интенсивность", "1.7")?;
    number(d, "Длительность", "3.4")?;
    require_effect(d)?;
    d.click("Воспроизвести эффект")?;
    d.settle(22)?;
    if d.app.effect_particles().is_empty() {
        return Err("Предпросмотр эффекта не запустился.".into());
    }
    d.app.proof.as_mut().unwrap().effect_file = Some(path.to_owned());
    d.click("Сохранить эффект…")?;
    if !path.is_file() {
        return Err("Выбранный эффект не записан в переносимый файл.".into());
    }
    Ok(())
}

fn require_effect(d: &Driver<'_, '_>) -> Result<u32, String> {
    let fx = d
        .app
        .scene
        .effects
        .presets
        .iter()
        .find(|entry| entry.settings.kind == crate::effects::Kind::Dust)
        .ok_or("Пресет пыли отсутствует.")?;
    if (fx.settings.intensity - 1.7).abs() > 0.001 || (fx.settings.duration - 3.4).abs() > 0.001 {
        return Err(format!("Настройки эффекта не совпали. {:?}", fx.settings));
    }
    Ok(fx.id)
}

fn rule_effect(d: &mut Driver<'_, '_>, anchor: Option<&str>) -> Result<(), String> {
    d.click("Правила")?;
    if d.point("+ Правило", 0).is_err() {
        d.click("Сценарии")?;
    }
    top_proof::new_rule(d, EFFECT_RULE, "Игровое действие")?;
    if d.point("+ Действие", 0).is_err() {
        top_proof::scroll_panel(d, -230.0)?;
    }
    d.click("+ Действие")?;
    let first = if d.point("Установить флаг", usize::MAX).is_ok() {
        "Установить флаг"
    } else if d.point("Задать этап задания", usize::MAX).is_ok() {
        "Задать этап задания"
    } else {
        "Перезапустить попытку"
    };
    d.click_n(first, usize::MAX)?;
    top_proof::click_action_kind(d, "Показать эффект")?;
    if let Some(anchor) = anchor {
        if d.point("Игрок", usize::MAX).is_err() {
            top_proof::scroll_panel(d, -250.0)?;
        }
        d.click_n("Игрок", usize::MAX)?;
        if d.point(anchor, usize::MAX).is_err() {
            let point = d
                .point("Кованый фонарь", usize::MAX)
                .or_else(|_| d.point("Липовый проезд · север", usize::MAX))?;
            for _ in 0..6 {
                top_proof::wheel(d, point, -210.0)?;
                if d.point(anchor, usize::MAX).is_ok() {
                    break;
                }
            }
        }
        d.click_n(anchor, usize::MAX)?;
    }
    let preset = require_effect(d)?;
    if !d
        .app
        .scene
        .rules
        .rules
        .iter()
        .rev()
        .find(|rule| rule.name == EFFECT_RULE)
        .is_some_and(|rule| {
            rule.actions.iter().any(|action| {
                matches!(action,
            crate::rules::Action::PlayEffect(command) if command.preset == preset)
            })
        })
    {
        return Err("Кнопка не создала действие с нужным переносимым пресетом.".into());
    }
    Ok(())
}

pub(super) fn play_city(
    d: &mut Driver<'_, '_>,
    preview: bool,
    output: Option<&Path>,
) -> Result<(), String> {
    if selected_bench(d)?
        .material_graph
        .as_ref()
        .and_then(|graph| graph.compile())
        .is_none()
    {
        return Err("Граф материала потерян при загрузке 3D-игры.".into());
    }
    let seat = selected_bench(d)?
        .seat_pose()
        .ok_or("Скамья потеряла место посадки.")?;
    let start = d.app.game_position().to_array();
    quarter_proof::walk(d, [seat.stand[0], seat.stand[2]])?;
    d.key(Key::F)?;
    d.settle(22)?;
    if d.app.effect_particles().is_empty() {
        return Err("Игровое правило не породило эффект в 3D.".into());
    }
    d.hold(40)?;
    quarter_proof::walk(d, [start[0], start[2]])?;
    play_linden(d, preview)?;
    if let Some(output) = output {
        capture(d, &output.join("linden-interior.png"))?;
    }
    Ok(())
}

pub(super) fn play_map(d: &mut Driver<'_, '_>) -> Result<(), String> {
    require_effect(d)?;
    d.key(Key::F)?;
    d.settle(90)?;
    if d.app.effect_particles().is_empty() || d.app.rule_session.quest_stage(1) != Some(2) {
        return Err("2D-игра не показала перенесённый эффект или не завершила цель.".into());
    }
    d.expect_hud("Выполнено · Река сверху")?;
    d.hold(52)
}

fn capture(d: &Driver<'_, '_>, path: &Path) -> Result<(), String> {
    image::save_buffer(path, &d.last, SIZE[0], SIZE[1], image::ColorType::Rgb8)
        .map_err(|error| format!("Не удалось сохранить кадр. {error}"))
}

fn center_changes(before: &[u8], after: &[u8]) -> usize {
    let width = SIZE[0] as usize;
    (280..575)
        .map(|y| {
            let start = (y * width + 385) * 3;
            let end = (y * width + 1000) * 3;
            before[start..end]
                .chunks_exact(3)
                .zip(after[start..end].chunks_exact(3))
                .filter(|(a, b)| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 2))
                .count()
        })
        .sum()
}
