use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use eframe::egui;

use crate::{
    Studio,
    linked_scenes::LinkedScenes,
    scene::{Checkpoint, write_atomic},
};

const MAX_SAVE_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) fn path(checksum: [u8; 32]) -> Option<PathBuf> {
    let identifier = checksum
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(
        crate::editor_history::local_directory()?
            .join("saves")
            .join(format!("{identifier}.checkpoint.json")),
    )
}

#[derive(Default)]
pub(crate) struct Controls {
    path: Option<PathBuf>,
    feedback: Option<(String, Instant)>,
}

impl Controls {
    fn say(&mut self, message: String) {
        self.feedback = Some((message, Instant::now()));
    }
}

impl Studio {
    pub(super) fn open_game_save(&mut self, ctx: &egui::Context, path: Option<PathBuf>) {
        self.game_save.path = path;
        if self
            .game_save
            .path
            .as_ref()
            .is_some_and(|path| path.exists())
        {
            self.restore_game_save(ctx);
        }
    }

    pub(super) fn show_game_save_controls(&mut self, ctx: &egui::Context) {
        if self.game_save.path.is_none() || !self.game_running {
            return;
        }
        let controls = &self.scene.play_settings.controls;
        let save_hotkey = !crate::play_settings::uses_key(controls, egui::Key::F5);
        let load_hotkey = !crate::play_settings::uses_key(controls, egui::Key::F9);
        let (mut save_requested, mut load_requested) = ctx.input(|input| {
            let pressed = |key| input.events.iter().any(|event| matches!(
                event,
                egui::Event::Key { key: event_key, pressed: true, repeat: false, .. } if *event_key == key
            ));
            (save_hotkey && pressed(egui::Key::F5), load_hotkey && pressed(egui::Key::F9))
        });
        let mut archive_requested = false;
        let mut import_requested = false;
        egui::Area::new("chapter-save-controls".into())
            .anchor(egui::Align2::RIGHT_TOP, [-15.0, 44.0])
            .show(ctx, |ui| {
                egui::Frame::group(ui.style())
                    .fill(crate::PANEL)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let location = self
                                .link_playthrough
                                .as_ref()
                                .and_then(|play| play.root.linked_scenes.get(play.active))
                                .map_or("Исходная сцена", |entry| entry.name.as_str());
                            ui.label(location);
                            save_requested |= ui
                                .small_button(if save_hotkey {
                                    "Сохранить · F5"
                                } else {
                                    "Сохранить"
                                })
                                .clicked();
                            load_requested |= ui
                                .small_button(if load_hotkey {
                                    "Загрузить · F9"
                                } else {
                                    "Загрузить"
                                })
                                .clicked();
                            if self.proof.is_none() {
                                ui.menu_button("Файлы", |ui| {
                                    if ui.button("Сохранить в файл…").clicked() {
                                        archive_requested = true;
                                        ui.close();
                                    }
                                    if ui.button("Файл сохранения…").clicked() {
                                        import_requested = true;
                                        ui.close();
                                    }
                                });
                            }
                        });
                        if let Some((message, when)) = &self.game_save.feedback
                            && when.elapsed() < Duration::from_secs(5)
                        {
                            ui.label(message);
                        }
                    });
            });
        if save_requested {
            self.write_game_save();
        } else if load_requested {
            self.restore_game_save(ctx);
        } else if archive_requested {
            self.archive_game_save();
        } else if import_requested {
            self.import_game_save(ctx);
        }
    }

    fn archive_game_save(&mut self) {
        let Some(state) = self.full_checkpoint() else {
            return;
        };
        let Some(file) = rfd::FileDialog::new()
            .set_file_name("game.checkpoint.json")
            .add_filter("Сохранение игры", &["json"])
            .save_file()
        else {
            return;
        };
        let result = store(&file, &state, &self.chapter_root().linked_scenes);
        self.game_save
            .say(result.map_or_else(|error| error, |()| "Копия сохранения записана.".into()));
    }

    fn import_game_save(&mut self, ctx: &egui::Context) {
        let Some(file) = rfd::FileDialog::new()
            .add_filter("Сохранение игры", &["json"])
            .pick_file()
        else {
            return;
        };
        match restore(&file, &self.chapter_root().linked_scenes) {
            Ok(state) => {
                self.load_external_checkpoint(state, ctx);
                self.game_save
                    .say("Копия загружена. F5 запишет текущую игру.".into());
            }
            Err(error) => self.game_save.say(error),
        }
    }

    pub(super) fn write_game_save(&mut self) {
        let Some(path) = &self.game_save.path else {
            return;
        };
        let Some(state) = self.full_checkpoint() else {
            return;
        };
        let result = store(path, &state, &self.chapter_root().linked_scenes);
        self.game_save
            .say(result.map_or_else(|error| error, |()| "Игра сохранена.".into()));
    }

    fn restore_game_save(&mut self, ctx: &egui::Context) {
        let Some(path) = &self.game_save.path else {
            return;
        };
        match restore(path, &self.chapter_root().linked_scenes) {
            Ok(state) => {
                self.load_external_checkpoint(state, ctx);
                self.game_save.say("Игра загружена.".into());
            }
            Err(error) => self.game_save.say(error),
        }
    }

    pub(crate) fn load_external_checkpoint(&mut self, state: Checkpoint, ctx: &egui::Context) {
        self.stop();
        self.scene.checkpoint = Some(state);
        self.start_play(ctx, true);
    }
}

fn store(path: &Path, state: &Checkpoint, scenes: &LinkedScenes) -> Result<(), String> {
    state.validate_for(scenes)?;
    let bytes = serde_json::to_vec(state).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_SAVE_BYTES {
        return Err("Файл сохранения больше 4 МБ.".into());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Недоступна папка сохранений. {error}"))?;
    }
    write_atomic(path, &bytes).map_err(|error| format!("Ход игры не сохранён. {error}"))
}

pub(crate) fn restore(path: &Path, scenes: &LinkedScenes) -> Result<Checkpoint, String> {
    let file = File::open(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => "Сохранения ещё нет.".into(),
        _ => format!("Не удалось открыть сохранение. {error}"),
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_SAVE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Не удалось прочитать сохранение. {error}"))?;
    if bytes.len() as u64 > MAX_SAVE_BYTES {
        return Err("Файл сохранения больше 4 МБ.".into());
    }
    let state = serde_json::from_slice::<Checkpoint>(&bytes)
        .map_err(|error| format!("Сохранение повреждено. {error}"))?;
    state.validate_for(scenes)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        playback::Intent,
        rules::{Action, Event, GameAction},
    };

    #[test]
    fn single_scene_can_save_and_restore_an_older_file_without_overwriting_main_slot() {
        let checkpoint_file = |slot: &str| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            std::env::temp_dir().join(format!(
                "studio-single-save-{}-{now}-{slot}.json",
                std::process::id()
            ))
        };
        let main = checkpoint_file("main");
        let archive = checkpoint_file("archive");
        let mut scene = crate::Scene::default();
        let counter = scene.rules.add_counter().unwrap();
        scene.rules.add_rule().unwrap();
        let rule = scene.rules.rules.last_mut().unwrap();
        rule.once = false;
        rule.event = Some(Event::GameAction {
            action: GameAction::Mark,
        });
        rule.actions.push(Action::AddCounter {
            counter_id: counter,
            delta: 1,
        });
        let mut studio = Studio::default();
        let ctx = egui::Context::default();
        studio.set_scene(scene, None, "");
        studio.start_play(&ctx, false);
        studio.open_game_save(&ctx, Some(main.clone()));
        let mark = Intent {
            mark: true,
            dt: 1.0 / 60.0,
            ..Intent::default()
        };
        studio.advance_intent(&ctx, mark);
        let older = studio.full_checkpoint().unwrap();
        store(&archive, &older, &studio.scene.linked_scenes).unwrap();
        studio.advance_intent(&ctx, mark);
        studio.write_game_save();
        let current = fs::read(&main).unwrap();
        let saved = restore(&archive, &studio.scene.linked_scenes).unwrap();
        studio.load_external_checkpoint(saved, &ctx);
        assert_eq!(
            studio.rule_session.counter(&studio.scene.rules, counter),
            Some(1)
        );
        assert_eq!(fs::read(&main).unwrap(), current);
        studio.restore_game_save(&ctx);
        assert_eq!(
            studio.rule_session.counter(&studio.scene.rules, counter),
            Some(2)
        );
        fs::remove_file(main).unwrap();
        fs::remove_file(archive).unwrap();
    }
}
