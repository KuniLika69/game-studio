use std::{fs, path::Path, time::Instant};

use serde_json::json;

use super::{
    Driver, FPS, SIZE, Sink, io_error, proof_input,
    staged_proof::{number, panel_click, reload},
};
use crate::{
    Studio, configure_context, offscreen::Offscreen, proof_video::Video, studio_welcome::Page,
    viewport3d,
};

const LAMP: &str = "Фонарь у входа";

pub(super) fn record(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let scene = output.join("edited-example.robot.json");
    let game = output.join("EditedExample.exe");
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut movie = Video::start(&output.join("onboarding-022.mp4"), SIZE[0], SIZE[1], FPS)?;
    let start = Instant::now();
    {
        let mut sink = Sink::Video(&mut movie);
        let app = Studio {
            welcome: Some(Page::Home),
            ..Studio::default()
        };
        let mut d = Driver::new(app, &mut display, &mut sink, Some((&scene, &game)));
        d.input = proof_input::Driver::with_scale(SIZE, 1.25);
        d.app.desk.recovery_root = Some(output.join("onboarding-recovery"));
        d.settle(8)?;
        if (d.display.context().pixels_per_point() - 1.25).abs() > 0.005 {
            return Err("Фоновый интерфейс не получил масштаб 125%.".into());
        }
        for label in ["Создать проект", "Открыть проект", "Попробовать пример"]
        {
            d.point(label, 0)?;
        }
        d.hold(30)?;
        d.click("Попробовать пример")?;
        d.click("Мастерская после дождя")?;
        d.point("Сохранить", 0)?;
        d.point("▶ Играть", 0)?;
        d.point("Собрать игру для Windows", 0)?;
        let original = d
            .app
            .scene
            .avatar
            .as_ref()
            .ok_or("В примере нет героя.")?
            .human_style;
        d.click("Лицо и одежда…")?;
        d.click("Улыбка")?;
        d.click("Применить внешность")?;
        if d.app.scene.avatar.as_ref().unwrap().human_style == original {
            return Err("Редактор не изменил внешность.".into());
        }
        d.click("Закрыть настройки внешности")?;
        search(&mut d, LAMP)?;
        let before = lamp(&d.app)?;
        d.click("Точечный свет")?;
        number(&mut d, "Яркость ", "3.1")?;
        if lamp(&d.app)? != 3.1 {
            return Err("Выбранный фонарь не изменил яркость.".into());
        }
        panel_click(&mut d, "▶ Играть")?;
        if !d.app.game_running {
            return Err("Кнопка не открыла игру.".into());
        }
        d.settle(30)?;
        d.hold(32)?;
        panel_click(&mut d, "■ Стоп")?;
        d.click("Отменить")?;
        if lamp(&d.app)? != before || d.app.scene.avatar.as_ref().unwrap().human_style == original {
            return Err("Отмена должна вернуть только последнюю правку света.".into());
        }
        panel_click(&mut d, "Сохранить как…")?;
        reload(&mut d)?;
        panel_click(&mut d, "Собрать игру для Windows")?;
        if !game.is_file() {
            return Err(format!("Изменённый пример не собрался. {}", d.app.status));
        }
        fs::write(
            output.join("onboarding.json"),
            serde_json::to_vec_pretty(&json!({
                "native_egui_dpi_simulation":125,
                "capture_pixels":SIZE,
                "input_actions":d.input_actions,
                "computer_script_wall_seconds":start.elapsed().as_secs_f32(),
                "human_tester":false,
                "start_play_undo_save_reload_export":true
            }))
            .map_err(|error| error.to_string())?,
        )
        .map_err(io_error)?;
        d.hold(30)?;
    }
    movie.finish()
}

fn search(d: &mut Driver<'_, '_>, name: &str) -> Result<(), String> {
    d.click("Поиск…")?;
    d.edit("Имя объекта, правила или ресурса", 0, name)?;
    d.click_n(name, usize::MAX)
}

fn lamp(app: &Studio) -> Result<f32, String> {
    app.scene
        .world
        .models
        .iter()
        .find(|item| item.label == LAMP)
        .and_then(|item| item.light.map(|light| light.intensity))
        .ok_or_else(|| "В примере нет выбранного фонаря.".into())
}
