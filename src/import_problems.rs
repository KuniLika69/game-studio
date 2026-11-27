use super::{ACCENT, ImportError, Studio, scene::Scene, story};
use eframe::egui::{self, Color32, RichText};

const WARNING: Color32 = Color32::from_rgb(242, 196, 101);

#[derive(Default)]
pub(crate) struct Panel {
    pub(crate) open: bool,
    pub(crate) selected: Option<usize>,
    pub(crate) entries: Vec<(String, ImportError)>,
    pub(crate) references: Vec<(String, String)>,
}

impl Panel {
    pub(super) fn record(&mut self, label: impl Into<String>, error: ImportError) {
        let entry = (label.into(), error);
        if !self.entries.contains(&entry) {
            if self
                .selected
                .is_some_and(|index| index >= self.entries.len())
            {
                self.selected = None;
            }
            self.entries.push(entry);
        }
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.references.clear();
        self.selected = None;
    }

    pub(super) fn update_references(&mut self, scene: &Scene) {
        let references = reference_issues(scene);
        if self.references != references {
            self.references = references;
            if self
                .selected
                .is_some_and(|index| index >= self.entries.len())
            {
                self.selected = None;
            }
        }
    }
}

fn reference_issues(project: &Scene) -> Vec<(String, String)> {
    let links = &project.linked_scenes;
    std::iter::once((None, project))
        .chain(
            links
                .entries
                .iter()
                .map(|entry| (Some(entry.name.as_str()), &*entry.scene)),
        )
        .flat_map(|(location, scene)| location_issues(scene, links, location))
        .collect()
}

fn location_issues(
    scene: &Scene,
    links: &super::linked_scenes::LinkedScenes,
    location: Option<&str>,
) -> Vec<(String, String)> {
    let mut issues = Vec::new();
    let scoped = |owner: String| match location {
        Some(name) => format!("{name} · {owner}"),
        None => owner,
    };
    for rule in &scene.rules.rules {
        for (kind, id) in rule.missing_targets(scene, links) {
            if !rule.draft || id != 0 {
                let target = if id == 0 {
                    kind.to_owned()
                } else {
                    format!("{kind} #{id}")
                };
                issues.push((
                    scoped(format!("Правило «{}» #{}", rule.name, rule.id)),
                    target,
                ));
            }
        }
    }
    let missing_building = |id| {
        !scene
            .world
            .buildings
            .iter()
            .any(|building| building.id == id)
    };
    for zone in &scene.rules.zones {
        if let Some(id) = zone.building_id.filter(|&id| missing_building(id)) {
            issues.push((
                scoped(format!("Зона «{}» #{}", zone.name, zone.id)),
                format!("Дом #{id}"),
            ));
        }
    }
    for quest in &scene.story.quests {
        if let story::Objective::Enter { building_id } = quest.objective
            && missing_building(building_id)
        {
            issues.push((
                scoped(format!("Задание «{}» #{}", quest.title, quest.id)),
                format!("Дом #{building_id}"),
            ));
        }
    }
    issues
}

impl Studio {
    pub(super) fn record_import_problem(&mut self, label: impl Into<String>, error: ImportError) {
        self.status = format!("Ошибка импорта. {error}");
        self.import_problems.record(label, error);
    }

    pub(super) fn import_asset(
        &mut self,
        path: &std::path::Path,
    ) -> Result<super::assets::Source, ImportError> {
        self.assets.import(path).inspect_err(|error| {
            let label = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            self.record_import_problem(label, error.clone());
        })
    }

    pub(super) fn import_problems_button(&mut self, ui: &mut egui::Ui) {
        let count = self.import_problems.entries.len() + self.import_problems.references.len();
        let text = if count == 0 {
            RichText::new("Проблемы импорта")
        } else {
            RichText::new(format!("Проблемы импорта · {count}")).color(ACCENT)
        };
        if ui.button(text).clicked() {
            self.import_problems.open = !self.import_problems.open;
        }
    }

    pub(super) fn reference_banner(&mut self, ui: &mut egui::Ui) {
        let count = self.import_problems.references.len();
        if count == 0 {
            return;
        }
        if ui
            .button(
                RichText::new(format!("⚠ Висячие ссылки · {count}"))
                    .color(WARNING)
                    .strong(),
            )
            .on_hover_text("Найти, кто ссылается на удалённую цель")
            .clicked()
        {
            self.import_problems.open = true;
        }
        ui.separator();
    }

    pub(super) fn import_problems_window(&mut self, ctx: &egui::Context) {
        let panel = &mut self.import_problems;
        egui::Window::new("Проблемы импорта")
            .id(egui::Id::new("import-problems"))
            .open(&mut panel.open)
            .default_width(510.0)
            .default_pos([315.0, 140.0])
            .show(ctx, |ui| {
                if panel.entries.is_empty() && panel.references.is_empty() {
                    ui.label("Ошибок импорта нет.");
                }
                egui::ScrollArea::vertical()
                    .max_height(480.0)
                    .show(ui, |ui| {
                        let imports = panel
                            .entries
                            .iter()
                            .map(|(label, error)| (label, error.to_string(), false));
                        let references = panel.references.iter().map(|(owner, target)| {
                            (owner, format!("Проблемная ссылка · {target}"), true)
                        });
                        for (index, (label, detail, warning)) in
                            imports.chain(references).enumerate()
                        {
                            let selected = panel.selected == Some(index);
                            let frame = if selected {
                                egui::Frame::group(ui.style())
                                    .stroke(egui::Stroke::new(1.5_f32, ACCENT))
                            } else {
                                egui::Frame::group(ui.style())
                            };
                            frame.show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal_wrapped(|ui| {
                                    ui.strong(label);
                                    if ui.button("Показать").clicked() {
                                        panel.selected = Some(index);
                                    }
                                    if selected {
                                        ui.label(RichText::new("Выбрано").color(ACCENT));
                                    }
                                });
                                let text = RichText::new(detail);
                                ui.label(if warning { text.color(WARNING) } else { text });
                            });
                        }
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Action, Event, GameAction};
    use std::path::Path;

    #[test]
    fn a2_linked_chapters_share_the_project_scene_namespace() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../verification/v0.23/chapter-final/ten-chapters.robot.json");
        let project = Scene::load(&path).unwrap();
        assert_eq!(project.linked_scenes.entries.len(), 9);
        assert!(
            reference_issues(&project)
                .iter()
                .all(|(_, target)| !target.starts_with("Сцена #"))
        );
    }

    #[test]
    fn a2_draft_shows_links_to_deleted_targets_without_empty_ports() {
        let mut scene = Scene::empty();
        let rule_id = scene.rules.add_rule().unwrap();
        let rule = &mut scene.rules.rules[0];
        rule.draft = true;
        rule.enabled = false;
        rule.event = Some(Event::GameAction {
            action: GameAction::Mark,
        });
        rule.actions = vec![
            Action::SetFlag {
                flag_id: 0,
                value: true,
            },
            Action::SetFlag {
                flag_id: 42,
                value: true,
            },
        ];
        let issues = reference_issues(&scene);
        assert_eq!(
            issues,
            [(
                format!("Правило «Правило {rule_id}» #{rule_id}"),
                "Флаг #42".into()
            )]
        );
    }
}
