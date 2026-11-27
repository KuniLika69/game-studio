use super::*;
use crate::tactics::{Battle, Cell, Field};

fn obstacles(map: &map2d::Map, cell: Cell) -> bool {
    map.collision_at(cell.map(i32::from)) == map2d::Collision::Solid
}

fn board(transform: map2d::ViewTransform) -> tactics_ui::Board {
    tactics_ui::Board {
        rect: transform.rect,
        origin: transform.origin,
        cell_points: transform.cell_points,
    }
}

impl Studio {
    pub(super) fn supports_tactics(&self) -> bool {
        self.scene
            .map_2d
            .as_ref()
            .is_some_and(|map| map.mode == map2d::Mode::Top)
    }

    pub(super) fn tactics_can_play(&self) -> Option<bool> {
        let scenario = self.scene.tactics.as_ref()?;
        let map = self.scene.map_2d.as_ref()?;
        let blocked = |cell| obstacles(map, cell);
        Some(
            map.mode == map2d::Mode::Top
                && scenario
                    .check_playable(Field {
                        size: map.size,
                        blocked: &blocked,
                    })
                    .is_ok(),
        )
    }

    pub(super) fn start_tactical_battle(&mut self, ctx: &egui::Context, resume: bool) -> bool {
        let (Some(scenario), Some(map)) = (&self.scene.tactics, &self.scene.map_2d) else {
            return false;
        };
        let blocked = |cell| obstacles(map, cell);
        let field = Field {
            size: map.size,
            blocked: &blocked,
        };
        let mut result = Battle::start(scenario, field);
        if resume
            && let Some(snapshot) = self
                .scene
                .checkpoint
                .as_ref()
                .and_then(|checkpoint| checkpoint.tactics.clone())
        {
            match Battle::restore(scenario, field, snapshot) {
                Ok(battle) => result = Ok(battle),
                Err(error) => self.status = format!("Сохранённый бой устарел. {error}"),
            }
        }
        match result {
            Ok(battle) => {
                self.tactics_battle = Some(battle);
                self.tactics_play = tactics_ui::Play::default();
                self.game_running = true;
                ctx.memory_mut(|memory| {
                    if let Some(focus) = memory.focused() {
                        memory.surrender_focus(focus);
                    }
                });
            }
            Err(error) => self.status = error,
        }
        true
    }

    pub(super) fn tactical_properties(&mut self, ctx: &egui::Context) -> bool {
        if self.editor_tab != EditorTab::Tactics || !self.supports_tactics() {
            return false;
        }
        egui::SidePanel::right("tactics_properties")
            .default_width(300.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let map = self.scene.map_2d.as_ref().expect("top map");
                    let blocked = |cell| obstacles(map, cell);
                    let field = Field {
                        size: map.size,
                        blocked: &blocked,
                    };
                    let changed = ui
                        .add_enabled_ui(!self.game_running, |ui| {
                            self.tactics_editor
                                .panel(ui, &mut self.scene.tactics, field)
                        })
                        .inner;
                    if changed {
                        self.changed();
                    }
                    ui.separator();
                    ui.weak(&self.status);
                });
            });
        true
    }

    pub(super) fn tactical_viewport(&mut self, ui: &mut egui::Ui) -> bool {
        if self.tactics_battle.is_some() {
            if self.standalone {
                ui.horizontal(|ui| {
                    ui.strong("Пошаговая тактика");
                    if ui.button("Начать заново").clicked() {
                        self.start_play(ui.ctx(), false);
                    }
                    if ui.button("Лицензии").clicked() {
                        self.licenses_open = true;
                    }
                });
            }
            let natural = self.environment_frame(self.game_position());
            let particles = self.effect_particles();
            let Some(battle) = &mut self.tactics_battle else {
                return true;
            };
            let map = self.scene.map_2d.as_ref().expect("tactical map");
            let blocked = |cell| obstacles(map, cell);
            let field = Field {
                size: map.size,
                blocked: &blocked,
            };
            ui.add_enabled_ui(!self.licenses_open, |ui| {
                self.tactics_play.header(ui, battle, field)
            });
            if let Some((transform, clicked)) =
                self.map_editor
                    .board(ui, &self.scene, false, natural.as_ref(), &particles)
            {
                let clicked = (!self.licenses_open).then_some(clicked).flatten();
                self.tactics_play
                    .board(ui, board(transform), battle, field, clicked);
            }
            return true;
        }
        if self.editor_tab != EditorTab::Tactics || !self.supports_tactics() {
            return false;
        }
        ui.label("Тактическое поле");
        let natural = self.environment_frame(self.game_position());
        let particles = self.effect_particles();
        if let Some((transform, clicked)) =
            self.map_editor
                .board(ui, &self.scene, true, natural.as_ref(), &particles)
            && let (Some(map), Some(scenario)) = (&self.scene.map_2d, &mut self.scene.tactics)
        {
            let blocked = |cell| obstacles(map, cell);
            if self.tactics_editor.board(
                ui,
                board(transform),
                scenario,
                Field {
                    size: map.size,
                    blocked: &blocked,
                },
                clicked,
            ) {
                self.changed();
            }
        }
        true
    }
}
