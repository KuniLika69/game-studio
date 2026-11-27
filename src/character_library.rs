use std::path::Path;

use crate::{
    assets::{Library, Source},
    avatar::Avatar,
    scene::Scene,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Player,
    Npc,
    Decoration,
}

impl Role {
    pub const ALL: [Self; 3] = [Self::Player, Self::Npc, Self::Decoration];

    pub fn label(self) -> &'static str {
        match self {
            Self::Player => "Игрок",
            Self::Npc => "NPC",
            Self::Decoration => "Декоративный персонаж",
        }
    }
}

fn sources(avatar: &Avatar) -> impl Iterator<Item = (&Source, &str)> {
    avatar.resources()
}

pub fn save(scene: &Scene, assets: &Library, path: &Path) -> Result<(), String> {
    let avatar = scene
        .avatar
        .as_ref()
        .ok_or("Добавь персонажа для сохранения.")?;
    let mut preset = Scene::empty();
    preset.avatar = Some(avatar.clone());
    preset.sprite_art = scene.sprite_art;
    preset.sprite_pixels.clone_from(&scene.sprite_pixels);
    preset.validate()?;
    assets.save_files(sources(avatar), path)?;
    preset.save(path)
}

pub fn load(assets: &mut Library, path: &Path) -> Result<Scene, String> {
    let preset = Scene::load(path)?;
    let avatar = preset
        .avatar
        .as_ref()
        .ok_or("В файле отсутствует персонаж.")?;
    for (source, label) in sources(avatar) {
        assets
            .ensure(source, Some(path))
            .map_err(|error| format!("{label}. {error}"))?;
    }
    Ok(preset)
}
