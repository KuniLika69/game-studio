//! The authored scene and a separate playing process use the same scene and resources.
use std::{collections::HashSet, fs, path::Path};

use eframe::egui::{Event, Key};
use serde_json::json;

use super::{
    Driver, FPS, SIZE, Sink, io_error, proof_input, quarter_proof,
    staged_proof::{number, panel_click, reload},
};
use crate::{
    Studio, View, asset_sources, configure_context,
    editor_workspace::Workspace,
    game_export::GamePackage,
    offscreen::Offscreen,
    play_settings::CameraMode,
    proof_video::Video,
    studio_welcome::Page,
    viewport3d::{self, Camera},
};

pub(super) fn author(output: &Path, exe: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let project = output.join("coast-village.robot.json");
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut video = Video::start(&output.join("from-blank-to-exe.mp4"), SIZE[0], SIZE[1], FPS)?;
    let mut sink = Sink::Video(&mut video);
    let mut app = Studio {
        welcome: Some(Page::Home),
        workspace: Workspace::Create,
        ..Studio::default()
    };
    app.desk.recovery_root = Some(output.join("recovery"));
    let mut d = Driver::new(app, &mut display, &mut sink, Some((&project, exe)));
    d.settle(8)?;
    d.hold(18)?;
    d.click("Создать проект")?;
    d.click("3D-сцена")?;
    d.click("Окружение")?;
    d.click("Прибрежная деревня")?;
    d.click("Собрать деревню")?;
    let original = d
        .app
        .scene
        .village
        .as_ref()
        .ok_or_else(|| d.app.status.clone())?
        .recipe
        .clone();
    let house_id = d
        .app
        .scene
        .village
        .as_ref()
        .unwrap()
        .houses()
        .next()
        .ok_or("Нет сгенерированных домов.")?
        .0;
    let house = d
        .app
        .scene
        .world
        .models
        .iter()
        .find(|item| item.id == house_id)
        .ok_or("Нет модели дома.")?
        .clone();
    let road_before = road(&d.app)?.source.clone();
    screenshot(&mut d, &output.join("village-start.png"))?;
    d.hold(35)?;

    d.click(&house.label)?;
    if d.app.world_selection != Some(house_id) {
        return Err(format!("Инспектор не выбрал «{}».", house.label));
    }
    number(&mut d, "X ", &(house.position[0] - 1.1).to_string())?;
    let moved = d
        .app
        .scene
        .world
        .models
        .iter()
        .find(|item| item.id == house_id)
        .unwrap()
        .clone();
    if (moved.position[0] - house.position[0]).abs() < 0.5 {
        return Err("Ручная правка положения дома не применилась.".into());
    }
    d.click("Защита домов")?;
    d.point(&format!("{} · изменён вручную", house.label), 0)?;
    d.hold(27)?;
    d.click("Пересоздать дома")?;
    d.click("Защита домов")?;
    d.click("Точки маршрута")?;
    number(&mut d, "X ", "1.25")?;
    edit_units(&mut d, "Ширина дороги ", "3.10")?;
    d.click("Пересоздать дорогу")?;
    let modified = d
        .app
        .scene
        .village
        .as_ref()
        .ok_or("Нет параметров дороги.")?
        .recipe
        .clone();
    if modified.route == original.route
        || modified.road_width == original.road_width
        || road(&d.app)?.source == road_before
    {
        return Err(format!(
            "Геометрия дороги не перестроилась. {}",
            d.app.status
        ));
    }
    d.hold(32)?;
    d.click("Точки маршрута")?;
    number(&mut d, "Зерно ", "613")?;
    d.click("Пересоздать растения")?;
    let house_after = d
        .app
        .scene
        .world
        .models
        .iter()
        .find(|item| item.id == house_id);
    if house_after != Some(&moved) || d.app.scene.village.as_ref().unwrap().recipe.seed != 613 {
        return Err(format!(
            "Ручная правка потерялась или растения не пересоздались. {}",
            d.app.status
        ));
    }
    screenshot(&mut d, &output.join("village-edited.png"))?;
    d.hold(33)?;

    d.click("Персонаж")?;
    d.click("Собрать чиби…")?;
    d.click("Собрать персонажа")?;
    check_chibi(&d.app)?;
    d.click("Лицо крупно")?;
    screenshot(&mut d, &output.join("lumi-face.png"))?;
    d.hold(37)?;
    d.click("Морская пелерина")?;
    edit_units(&mut d, "Ширина головы ", "1.06")?;
    d.click("Применить внешность")?;
    if !d.app.scene.avatar.as_ref().is_some_and(|actor| {
        actor.look.scales.contains_key("Head")
            && actor
                .bone_attachments
                .iter()
                .any(|part| part.name == "Пелерина Луми")
    }) {
        return Err(format!(
            "Костюм или пропорции не применились. {}",
            d.app.status
        ));
    }
    screenshot(&mut d, &output.join("lumi-cape.png"))?;
    d.hold(36)?;
    d.click("Закрыть мастерскую")?;
    panel_click(&mut d, "▶ Играть")?;
    if !d.app.game_running {
        return Err("Предпросмотр игры не запустился.".into());
    }
    d.hold(29)?;
    panel_click(&mut d, "■ Стоп")?;
    panel_click(&mut d, "Сохранить как…")?;
    if !project.is_file() {
        return Err(d.app.status.clone());
    }
    reload(&mut d)?;
    check_chibi(&d.app)?;
    if d.app
        .scene
        .world
        .models
        .iter()
        .find(|item| item.id == house_id)
        != Some(&moved)
    {
        return Err("Правка дома пропала при повторном открытии.".into());
    }
    panel_click(&mut d, "Собрать игру для Windows")?;
    if !exe.is_file() {
        return Err(d.app.status.clone());
    }
    d.hold(54)?;
    let data = json!({
        "input_actions": d.input_actions, "reopened": true, "preview_started": true,
        "original_seed": original.seed, "edited_seed": 613, "road_before": original.route,
        "road_after": modified.route, "road_width_before_m": original.road_width,
        "road_width_after_m": modified.road_width, "preserved_house_id": house_id,
        "house_before_m": house.position, "house_after_m": moved.position,
        "coast_uses_chibi": true, "cape_saved": true, "executable": exe.file_name().unwrap_or_default().to_string_lossy(),
    });
    fs::write(
        output.join("editor.json"),
        serde_json::to_vec_pretty(&data).map_err(|err| err.to_string())?,
    )
    .map_err(io_error)?;
    video.finish()
}

pub(super) fn player(package: GamePackage, output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::from_package(package, display.context());
    check_chibi(&app)?;
    if !app.game_running {
        return Err("Собранная игра не запустилась.".into());
    }
    let route = app
        .scene
        .village
        .as_ref()
        .ok_or("В EXE нет рецепта деревни.")?
        .recipe
        .route
        .clone();
    let asset_cost = asset_sources(&app.scene)
        .map(|(source, _)| source)
        .collect::<HashSet<_>>()
        .into_iter()
        .filter_map(|source| app.assets.get(source))
        .map(|item| item.cost)
        .sum::<usize>();
    let adapter = display.adapter_info().name.clone();
    app.view = View::Three;
    app.camera.yaw = 0.04;
    app.camera.pitch = 0.28;
    let mut video = Video::start(&output.join("walk-from-exe.mp4"), SIZE[0], SIZE[1], FPS)?;
    let mut sink = Sink::Video(&mut video);
    let mut d = Driver::new(app, &mut display, &mut sink, None);
    d.settle(36)?;
    d.display.start_timing();
    for (index, &point) in route.iter().enumerate().skip(1) {
        quarter_proof::walk(&mut d, point)?;
        if index == 2 {
            screenshot(&mut d, &output.join("village-exe.png"))?;
        }
    }
    let foot = d.app.game_position();
    let mut frames = d.display.stop_timing();
    frames.sort_unstable_by(f32::total_cmp);
    let percentile = |p: f32| {
        frames
            .get(((frames.len().saturating_sub(1)) as f32 * p).round() as usize)
            .copied()
    };
    let settings = d.app.scene.play_settings.camera;
    d.app.scene.play_settings.camera.mode = CameraMode::Fixed;
    let height = d.app.scene.avatar.as_ref().unwrap().height;
    d.app.camera = Camera {
        target: [foot.x, foot.y + height * 0.71, foot.z],
        yaw: d.app.avatar_heading.to_radians() + 0.17,
        pitch: 0.055,
        distance: 1.68,
    };
    screenshot(&mut d, &output.join("lumi-exe.png"))?;
    d.settle(115)?;
    d.app.scene.play_settings.camera = settings;
    let data = json!({
        "exported_game": true, "process_id": std::process::id(), "route": route,
        "end_position_m": foot.to_array(), "gpu": adapter,
        "viewport_px": SIZE, "walk_frames_measured": frames.len(), "frame_p50_ms": percentile(0.50),
        "frame_p95_ms": percentile(0.95), "timing": "Offscreen render and GPU readback per frame",
        "decoded_and_packaged_model_bytes": asset_cost,
    });
    fs::write(
        output.join("game.json"),
        serde_json::to_vec_pretty(&data).map_err(|err| err.to_string())?,
    )
    .map_err(io_error)?;
    video.finish()
}

fn road(app: &Studio) -> Result<&crate::assets::Placement, String> {
    app.scene
        .world
        .models
        .iter()
        .find(|item| item.label == "Дорога по маршруту")
        .ok_or_else(|| "Не найдена геометрия дороги.".into())
}

fn check_chibi(app: &Studio) -> Result<(), String> {
    let avatar = app
        .scene
        .avatar
        .as_ref()
        .ok_or_else(|| app.status.clone())?;
    let (model, _) = app.avatar_asset().ok_or("Модель чиби не загрузилась.")?;
    if !model.has_named_material("Lumi | Hood") || avatar.walk.is_none() || avatar.run.is_none() {
        return Err(format!(
            "Не загружена модель чиби с ходьбой и бегом. {}",
            app.status
        ));
    }
    Ok(())
}

fn edit_units(d: &mut Driver<'_, '_>, prefix: &str, target: &str) -> Result<(), String> {
    let labels = proof_input::labels(d.display.last_output().ok_or("Пустой интерфейс.")?);
    let label = labels
        .iter()
        .find(|label| label.text.starts_with(prefix))
        .ok_or_else(|| format!("Нет поля «{prefix}»."))?;
    d.edit(&label.text, 0, target)?;
    d.key(Key::Enter)
}

fn screenshot(d: &mut Driver<'_, '_>, path: &Path) -> Result<(), String> {
    d.position = None;
    d.frame(vec![Event::PointerGone])?;
    d.settle(5)?;
    image::save_buffer(path, &d.last, SIZE[0], SIZE[1], image::ColorType::Rgb8)
        .map_err(|err| err.to_string())
}
