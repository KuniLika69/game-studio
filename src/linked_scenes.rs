use std::{collections::HashSet, path::Path};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::{
    asset_sources,
    assets::Library,
    rules::{Action, RuleSet},
    scene::Scene,
};

pub const ROOT_SCENE_ID: u32 = 1;
const MAX_LINKED_SCENES: usize = 12;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkedScenes {
    pub entries: Vec<LinkedScene>,
    next_id: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinkedScene {
    pub id: u32,
    pub name: String,
    pub scene: Box<Scene>,
}

impl LinkedScenes {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_default(&self) -> bool {
        self.is_empty() && self.next_id == 0
    }

    pub fn contains(&self, id: u32) -> bool {
        id == ROOT_SCENE_ID || self.get(id).is_some()
    }

    pub fn targets(&self) -> impl Iterator<Item = (u32, &str)> + '_ {
        std::iter::once((ROOT_SCENE_ID, "Исходная сцена")).chain(
            self.entries
                .iter()
                .map(|entry| (entry.id, entry.name.as_str())),
        )
    }

    pub fn get(&self, id: u32) -> Option<&LinkedScene> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn attach(&mut self, name: String, mut scene: Scene) -> Result<u32, String> {
        if !scene.linked_scenes.is_empty() {
            return Err(
                "Присоединяемая сцена должна быть отдельной, без вложенного проекта.".into(),
            );
        }
        if self.entries.len() >= MAX_LINKED_SCENES || !valid_name(&name) {
            return Err("Некорректное имя или превышен предел связанных сцен.".into());
        }
        scene.validate_content()?;
        scene.checkpoint = None;
        let id = self
            .entries
            .iter()
            .map(|scene| scene.id)
            .max()
            .unwrap_or(ROOT_SCENE_ID)
            .checked_add(1)
            .filter(|id| *id < u32::MAX)
            .map(|id| id.max(self.next_id))
            .filter(|id| *id < u32::MAX)
            .ok_or("Не удалось назначить идентификатор сцены.")?;

        // An imported scene uses a different namespace. Its transitions need explicit remapping.
        for rule in &mut scene.rules.rules {
            let mut needs_binding = false;
            for action in &mut rule.actions {
                if let Action::Transition { scene_id } = action {
                    *scene_id = 0;
                    needs_binding = true;
                }
            }
            if needs_binding {
                rule.enabled = false;
                rule.draft = true;
            }
        }
        self.next_id = id + 1;
        self.entries.push(LinkedScene {
            id,
            name,
            scene: Box::new(scene),
        });
        Ok(id)
    }

    pub fn attach_file(&mut self, path: &Path, assets: &mut Library) -> Result<u32, String> {
        let scene = Scene::load(path)?;
        if !scene.linked_scenes.is_empty() {
            return Err(
                "Присоединяемая сцена должна быть отдельной, без вложенного проекта.".into(),
            );
        }
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy())
            .unwrap_or_default()
            .trim_end_matches(".robot")
            .trim()
            .chars()
            .take(64)
            .collect::<String>();
        let mut staged = assets.clone();
        for (source, _) in asset_sources(&scene) {
            staged.ensure(source, Some(path))?;
        }
        let id = self.attach(name, scene)?;
        *assets = staged;
        Ok(id)
    }

    pub fn remove(&mut self, id: u32, root_rules: &RuleSet) -> Result<bool, String> {
        let uses = |rules: &RuleSet| active_transitions(rules).any(|target| target == id);
        if uses(root_rules)
            || self
                .entries
                .iter()
                .any(|entry| entry.id != id && uses(&entry.scene.rules))
        {
            return Err("Сцену ещё использует включённое правило. Сначала измени его цель.".into());
        }
        let old = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        Ok(self.entries.len() != old)
    }

    pub(crate) fn validate_for(&self, root: &Scene) -> Result<(), String> {
        let mut seen = HashSet::new();
        if self.entries.len() > MAX_LINKED_SCENES
            || self.entries.iter().any(|entry| {
                !(ROOT_SCENE_ID + 1..u32::MAX).contains(&entry.id)
                    || !seen.insert(entry.id)
                    || !valid_name(&entry.name)
                    || !entry.scene.linked_scenes.is_empty()
            })
        {
            return Err("Некорректные номера, имена или вложенность связанных сцен.".into());
        }
        for entry in &self.entries {
            entry
                .scene
                .validate_content()
                .map_err(|error| format!("Сцена «{}». {error}", entry.name))?;
        }
        if all_scenes(root)
            .flat_map(|scene| active_transitions(&scene.rules))
            .any(|id| !self.contains(id))
        {
            return Err("Включённое правило ссылается на отсутствующую сцену.".into());
        }
        Ok(())
    }
}

pub fn all_scenes(root: &Scene) -> impl Iterator<Item = &Scene> {
    std::iter::once(root).chain(
        root.linked_scenes
            .entries
            .iter()
            .map(|entry| entry.scene.as_ref()),
    )
}

fn active_transitions(rules: &RuleSet) -> impl Iterator<Item = u32> + '_ {
    rules
        .rules
        .iter()
        .filter(|rule| rule.enabled && !rule.draft)
        .flat_map(|rule| {
            rule.actions.iter().filter_map(|action| match action {
                Action::Transition { scene_id } => Some(*scene_id),
                _ => None,
            })
        })
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control)
}

pub fn inspector(
    ui: &mut egui::Ui,
    root: &mut Scene,
    assets: &mut Library,
    status: &mut String,
) -> bool {
    ui.collapsing("Связанные сцены", |ui| {
        let key = ui.id().with("linked_scene_path");
        let mut path = ui
            .data(|data| data.get_temp::<String>(key))
            .unwrap_or_default();
        if ui
            .add(egui::TextEdit::singleline(&mut path).hint_text("Путь к .robot.json"))
            .changed()
        {
            ui.data_mut(|data| data.insert_temp(key, path.clone()));
        }
        let mut changed = false;
        if ui
            .add_enabled(
                !path.trim().is_empty(),
                egui::Button::new("Присоединить сцену"),
            )
            .clicked()
        {
            *status = match root
                .linked_scenes
                .attach_file(Path::new(path.trim()), assets)
            {
                Ok(_) => {
                    changed = true;
                    "Сцена добавлена в проект вместе с ресурсами.".into()
                }
                Err(error) => error,
            };
        }
        let mut remove = None;
        for entry in &mut root.linked_scenes.entries {
            ui.push_id(entry.id, |ui| {
                ui.horizontal(|ui| {
                    let before = entry.name.clone();
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut entry.name).char_limit(64))
                        .changed();
                    if !valid_name(&entry.name) {
                        entry.name = before;
                    }
                    if ui.small_button("Убрать связь").clicked() {
                        remove = Some(entry.id);
                    }
                });
            });
        }
        if let Some(id) = remove {
            match root.linked_scenes.remove(id, &root.rules) {
                Ok(removed) => {
                    changed |= removed;
                    if removed && let Some(checkpoint) = &mut root.checkpoint {
                        if checkpoint.scene_id == Some(id) {
                            root.checkpoint = None;
                        } else {
                            checkpoint.visited.remove(&id);
                        }
                    }
                }
                Err(error) => *status = error,
            }
        }
        changed
    })
    .body_returned
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        assets::{Placement, Source},
        proof_input::{Driver, click, find_label, replace_text},
        rules::{self, Event, GameAction, TraceOutcome},
        rules_ui::Editor,
        world::InteractionState,
    };
    use std::fs;

    #[test]
    fn linked_targets_are_exact_and_transition_is_consumed_once() {
        let mut root = Scene::empty();
        let mut child = Scene::empty();
        child.rules.add_rule().unwrap();
        child.rules.rules[0].actions.push(Action::Transition {
            scene_id: ROOT_SCENE_ID,
        });
        let link = root.linked_scenes.attach("Двор".into(), child).unwrap();
        let child_rule = &root.linked_scenes.get(link).unwrap().scene.rules.rules[0];
        assert!(child_rule.draft && !child_rule.enabled);
        assert_eq!(child_rule.actions, [Action::Transition { scene_id: 0 }]);

        let event = Event::GameAction {
            action: GameAction::Mark,
        };
        root.rules.add_rule().unwrap();
        root.rules.rules[0].event = Some(event);
        root.rules.rules[0].actions = vec![Action::Transition { scene_id: link }];
        assert!(root.linked_scenes.remove(link, &root.rules).is_err());
        root.validate().unwrap();
        let mut session = rules::Session::new(&root.rules);
        let mut interactions = InteractionState::default();
        assert_eq!(
            session.dispatch_at(
                &root.rules,
                &root.world,
                &root.story,
                &mut interactions,
                event,
                [0.0; 3]
            ),
            0
        );
        assert_eq!(
            session.trace().next_back().unwrap().outcome,
            TraceOutcome::MissingActionTarget(Action::Transition { scene_id: link })
        );
        session.set_transition_targets(root.linked_scenes.targets().map(|(id, _)| id));
        assert_eq!(
            session.dispatch_at(
                &root.rules,
                &root.world,
                &root.story,
                &mut interactions,
                event,
                [0.0; 3]
            ),
            1
        );
        assert_eq!(session.take_transition_request(), Some(link));
        assert_eq!(session.take_transition_request(), None);
        assert_eq!(
            session.dispatch_at(
                &root.rules,
                &root.world,
                &root.story,
                &mut interactions,
                event,
                [0.0; 3]
            ),
            0
        );
    }

    #[test]
    fn attaches_a_scene_and_authors_its_transition_through_the_editor() {
        let name = format!("Workshop-{}", std::process::id());
        let child_file = std::env::temp_dir().join(format!("{name}.robot.json"));
        Scene::empty().save(&child_file).unwrap();
        let mut root = Scene::empty();
        let mut assets = Library::default();
        let mut status = String::new();
        let mut editor = Editor::default();
        let ctx = egui::Context::default();
        let mut driver = Driver::new([800, 1000]);
        let mut show = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                inspector(ui, &mut root, &mut assets, &mut status);
                editor.show(ui, &mut root);
            });
        };
        let path = child_file.to_string_lossy();
        let mut output = ctx.run(driver.raw(Vec::new()), &mut show);
        for (label, text) in [
            ("Связанные сцены", None),
            ("Путь к .robot.json", Some(path.as_ref())),
            ("Присоединить сцену", None),
            ("Связанные сцены", None),
            ("+ Правило", None),
            ("Выбрать событие", None),
            ("Игровое действие", None),
            ("+ Действие", None),
            ("Перезапустить попытку", None),
            ("Перейти в сцену", None),
            ("Исходная сцена", None),
            (&name, None),
        ] {
            let occurrence = 0;
            if label == "Перейти в сцену" && find_label(&output, label, occurrence).is_none()
            {
                let at = find_label(&output, "Запустить таймер", 0)
                    .or_else(|| find_label(&output, "Задать счётчик", 0))
                    .expect("popup action list");
                output = ctx.run(
                    driver.raw(vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -230.0),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]),
                    &mut show,
                );
                for _ in 0..14 {
                    output = ctx.run(driver.raw(Vec::new()), &mut show);
                }
            }
            let point =
                find_label(&output, label, occurrence).unwrap_or_else(|| panic!("missing {label}"));
            driver.skip_frames(45);
            let _ = ctx.run(driver.raw(click(point)), &mut show);
            if let Some(text) = text {
                let _ = ctx.run(driver.raw(replace_text(text)), &mut show);
            }
            output = ctx.run(driver.raw(Vec::new()), &mut show);
        }
        fs::remove_file(child_file).unwrap();
        let link = root.linked_scenes.entries.first().unwrap();
        assert_eq!(link.name, name);
        assert_eq!(
            root.rules.rules[0].event,
            Some(Event::GameAction {
                action: GameAction::Mark
            })
        );
        assert_eq!(
            root.rules.rules[0].actions,
            [Action::Transition { scene_id: link.id }]
        );
        assert!(root.rules.rules[0].enabled);
        root.validate().unwrap();
    }

    #[test]
    fn linked_scene_collects_its_archived_glb_in_the_owning_project() {
        let address = |name| {
            std::env::temp_dir().join(format!(
                "studio-scene-link-{name}-{}.robot.json",
                std::process::id()
            ))
        };
        let child_file = address("source");
        let root_file = address("root");
        let mut source_assets = Library::default();
        let source = source_assets
            .import_bytes(include_bytes!("../demo_assets/quarter/bench_alt.glb").to_vec())
            .unwrap();
        let mut child = Scene::empty();
        child.world.models.push(Placement::at_ground(
            &[],
            &source_assets.get(&source).unwrap().model,
            source.clone(),
            "Модель двора".into(),
            [0.0; 3],
            1.8,
            0.0,
        ));
        source_assets
            .save_files(asset_sources(&child), &child_file)
            .unwrap();
        child.save(&child_file).unwrap();

        let mut root = Scene::empty();
        let mut shared = Library::default();
        let id = root
            .linked_scenes
            .attach_file(&child_file, &mut shared)
            .unwrap();
        assert_eq!(
            root.linked_scenes.get(id).unwrap().scene.world.models[0].source,
            source
        );
        shared
            .save_files(all_scenes(&root).flat_map(asset_sources), &root_file)
            .unwrap();
        root.save(&root_file).unwrap();
        drop(source_assets);
        let reopened = Scene::load(&root_file).unwrap();
        let mut registered = Library::default();
        for (source, _) in all_scenes(&reopened).flat_map(asset_sources) {
            registered.ensure(source, Some(&root_file)).unwrap();
        }
        assert_eq!(reopened, root);
        assert!(!registered.get(&source).unwrap().model.images.is_empty());
        let Source::Imported(hash) = source else {
            unreachable!()
        };
        for path in [child_file, root_file] {
            let folder = path.with_extension("assets");
            fs::remove_file(folder.join(format!("{hash}.glb"))).unwrap();
            fs::remove_dir(folder).unwrap();
            fs::remove_file(path).unwrap();
        }
    }
}
