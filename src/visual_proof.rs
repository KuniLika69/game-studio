use std::path::Path;

use eframe::egui::Key;
use glam::Vec3;

use super::{Driver, quarter_proof, staged_proof};
use crate::{assets::Placement, proof_input};

const LABEL: &str = "Скамья для проверки";
const FOOT: [f32; 3] = [4.30, 0.076, 2.48];

pub(super) fn author(d: &mut Driver<'_, '_>, chair: &Path) -> Result<(), String> {
    d.click("3D")?;
    let separate_lighting = d.point("Свет", 0).is_ok();
    d.click(if separate_lighting {
        "Свет"
    } else {
        "Окружение"
    })?;
    d.click("Освещение сцены")?;
    staged_proof::number(d, "Дневной свет", "0.82")?;
    staged_proof::number(d, "Свет внутри", "1.45")?;
    if !matches!(
        d.app.scene.lighting,
        crate::assets::materials::Lighting {
            daylight: 0.82,
            interior_gain: 1.45,
            ..
        }
    ) {
        return Err("Редактор не сохранил настройки освещения.".into());
    }
    d.click("Освещение сцены")?;
    if separate_lighting {
        d.click("Окружение")?;
    }
    let before = d.app.scene.world.models.len();
    choose_glb(d, chair, "Импорт GLB…")?;
    if d.app.scene.world.models.len() != before + 1 {
        return Err(format!("Не импортирована скамья. {}", d.app.status));
    }

    let [px, py, pz] = {
        let item = d
            .app
            .scene
            .world
            .models
            .last()
            .ok_or("Модель отсутствует.")?;
        let asset = d
            .app
            .assets
            .get(&item.source)
            .ok_or("Модель не загрузилась.")?;
        let low = Vec3::from_array(asset.model.bounds_min);
        let high = Vec3::from_array(asset.model.bounds_max);
        let rotation = glam::Quat::from_rotation_y(-90_f32.to_radians());
        let pivot = Vec3::new((low.x + high.x) * 0.5, low.y, (low.z + high.z) * 0.5);
        (Vec3::from_array(FOOT) - rotation * (pivot * Vec3::from_array(item.scale))).to_array()
    };
    let imported_label = d.app.scene.world.models.last().unwrap().label.clone();
    d.edit(&imported_label, usize::MAX, LABEL)?;
    d.key(Key::Enter)?;
    vector_field(d, "Позиция", "X", px)?;
    vector_field(d, "Позиция", "Y", py)?;
    vector_field(d, "Позиция", "Z", pz)?;
    vector_field(d, "Поворот, °", "Y", -90.0)?;

    d.click("Материалы GLB")?;
    staged_proof::number(d, "U", "2.0")?;
    d.click("Задать шероховатость")?;
    staged_proof::number(d, "Шероховатость", "0.35")?;
    staged_proof::number(d, "Сила карты нормалей", "0.65")?;
    staged_proof::number(d, "Свечение", "0.72")?;
    d.click("Материалы GLB")?;

    d.click("Отдельный коллайдер")?;
    vector_field(d, "Центр коллайдера, м", "Y", 0.225)?;
    vector_field(d, "Размер коллайдера, м", "Y", 0.46)?;
    d.click("Точка посадки")?;
    vector_field(d, "Положение сиденья, м", "Y", 0.49)?;
    vector_field(d, "Куда встаёт герой, м", "Y", -py)?;
    vector_field(d, "Куда встаёт герой, м", "Z", 1.0)?;

    let before = placement(d)?.clone();
    if before.material.uv_scale != [2.0, 1.0]
        || before.material.roughness != Some(0.35)
        || before.material.normal_strength != 0.65
        || before.material.emissive_strength != 0.72
        || before
            .collider
            .is_none_or(|collider| collider.size[1] != 0.46)
        || before.seat.is_none_or(|anchor| anchor.sit[1] != 0.49)
        || (before.rotation[1] + 90.0).abs() > 0.01
    {
        return Err(format!(
            "Редактор не применил модельные параметры {:?}",
            before
        ));
    }
    d.click("К объекту")?;
    d.hold(36)?;
    choose_glb(
        d,
        &chair.with_file_name("bench_alt.glb"),
        "Заменить модель GLB…",
    )?;
    let after = placement(d)?;
    if after.id != before.id
        || after.source == before.source
        || after.position != before.position
        || after.custom_collider() != before.custom_collider()
        || after.seat_pose() != before.seat_pose()
        || after.material != before.material
    {
        return Err("При замене модели изменились её идентификатор, коллайдер, точка посадки или настройки материала.".into());
    }
    d.click("К объекту")?;
    d.hold(42)
}

pub(super) fn sit(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let seat = placement(d)?
        .seat_pose()
        .ok_or("Скамья потеряла точку посадки.")?;
    let start = d.app.game_position().to_array();
    quarter_proof::walk(d, [seat.stand[0], seat.stand[2]])?;
    d.settle(8)?;
    d.key(Key::E)?;
    d.settle(96)?;
    if d.app
        .seating
        .as_ref()
        .is_none_or(|active| active.seat.id != seat.id || active.amount < 0.998)
    {
        return Err(format!(
            "Персонаж не сел на новую модель у {:?}. Игрок {:?}, действие {:?}. {}",
            seat.stand,
            d.app.game_position(),
            d.app
                .scene
                .world
                .interaction_near(&d.app.interactions, d.app.game_position().to_array()),
            d.app.status
        ));
    }
    d.hold(42)?;
    d.key(Key::E)?;
    d.settle(96)?;
    if d.app.seating.is_some() {
        return Err("Персонаж не встал на указанную точку.".into());
    }
    quarter_proof::walk(d, [start[0], start[2]])
}

fn placement<'d>(d: &'d Driver<'_, '_>) -> Result<&'d Placement, String> {
    d.app
        .scene
        .world
        .models
        .iter()
        .find(|item| item.label == LABEL)
        .ok_or_else(|| "Модель с коллайдером не найдена.".into())
}

fn choose_glb(d: &mut Driver<'_, '_>, file: &Path, action: &str) -> Result<(), String> {
    if !file.is_file() {
        return Err(format!("Отсутствует модель {}", file.display()));
    }
    d.app
        .proof
        .as_mut()
        .ok_or("Не настроен фоновый редактор.")?
        .import_glb = Some(file.to_owned());
    d.click(action)
}

fn vector_field(d: &mut Driver<'_, '_>, title: &str, axis: &str, value: f32) -> Result<(), String> {
    let header = d.point(title, usize::MAX)?;
    let rendered = d.display.last_output().ok_or("Пустой интерфейс.")?;
    let point = proof_input::labels(rendered)
        .into_iter()
        .filter(|label| {
            let at = label.center();
            at.x >= header.x - 100.0
                && (header.y..header.y + 54.0).contains(&at.y)
                && label
                    .text
                    .strip_prefix(axis)
                    .is_some_and(|suffix| suffix.trim().parse::<f32>().is_ok())
        })
        .min_by(|a, b| a.center().y.total_cmp(&b.center().y))
        .ok_or_else(|| format!("Нет поля {axis} в «{title}»."))?
        .center();
    d.input.skip_frames(42);
    d.frame(proof_input::click(point))?;
    d.frame(proof_input::replace_text(&format!("{value:.4}")))?;
    d.settle(3)?;
    d.key(Key::Enter)
}
