//! Drives the reusable-object controls through the virtual editor viewport.
use std::path::Path;

use crate::{EditorTab, assets::Source, object_library};

use super::Driver;

pub(super) fn save_selected(d: &mut Driver<'_, '_>, path: &Path) -> Result<(), String> {
    library_path(d, path)?;
    let selected = d
        .app
        .scene
        .world
        .models
        .iter()
        .find(|item| Some(item.id) == d.app.world_selection)
        .ok_or("Нет выбранной модели для библиотеки.")?
        .clone();
    if !matches!(selected.source, Source::Imported(_))
        || selected.collider.is_none()
        || selected.seat.is_none()
        || selected.material.is_default()
    {
        return Err("Для проверки нужен импортированный объект с настроенными материалом, коллайдером и посадкой.".into());
    }
    d.click("Сохранить объект в библиотеку")?;
    let packed = object_library::read(path).map_err(|error| format!("{} {error}", d.app.status))?;
    let mut expected = selected;
    expected.position = packed.placement.position;
    expected.id = packed.placement.id;
    if packed.placement != expected {
        return Err("В библиотеке сохранены другие настройки объекта.".into());
    }
    d.hold(24)
}

pub(super) fn reinsert(d: &mut Driver<'_, '_>, path: &Path) -> Result<(), String> {
    let packed = object_library::read(path)?;
    d.click("Новый…")?;
    d.click("Пустая сцена")?;
    if !d.app.scene.world.models.is_empty() || d.app.assets.get(&packed.placement.source).is_some()
    {
        return Err("Для проверки переноса нужен новый проект без исходной модели.".into());
    }
    library_path(d, path)?;
    d.click("Вставить объект из библиотеки")?;
    let item = d
        .app
        .scene
        .world
        .models
        .last()
        .ok_or_else(|| d.app.status.clone())?;
    let asset = d
        .app
        .assets
        .get(&item.source)
        .ok_or("Не загрузились ресурсы GLB объекта.")?;
    if asset.model.images.is_empty() || d.app.assets.export_glb(&item.source).is_none() {
        return Err("В новом проекте не загрузились текстуры объекта.".into());
    }
    let mut actual = item.clone();
    actual.id = packed.placement.id;
    actual.position = packed.placement.position;
    if actual != packed.placement || item.custom_collider().is_none() || item.seat_pose().is_none()
    {
        return Err(
            "При вставке изменились материал, коллайдер, положение посадки или модель.".into(),
        );
    }
    d.hold(36)
}

fn library_path(d: &mut Driver<'_, '_>, path: &Path) -> Result<(), String> {
    if d.app.editor_tab != EditorTab::World {
        d.click("Окружение")?;
    }
    if d.point("Вставить объект из библиотеки", 0).is_err() {
        d.click("Библиотека объектов")?;
    }
    let file = path.to_string_lossy();
    if d.point(&file, 0).is_err() {
        d.edit("Путь к файлу .studio-object", 0, &file)?;
    }
    Ok(())
}
