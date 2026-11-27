//! Authoring and gameplay controls for the shared tactical battle.
use eframe::egui::{self, Color32, FontId, Key, Modifiers, Pos2, Rect, Stroke};

use crate::tactics::{Battle, Cell, Event, Field, Goal, Outcome, Scenario, Team, Unit, UnitId};

const PLAYER: Color32 = Color32::from_rgb(94, 205, 239);
const RIVAL: Color32 = Color32::from_rgb(246, 136, 116);
const TARGET: Color32 = Color32::from_rgb(246, 221, 130);

#[derive(Clone, Copy)]
pub struct Board {
    pub rect: Rect,
    pub origin: Pos2,
    pub cell_points: f32,
}

impl Board {
    fn center(self, [x, row]: Cell) -> Pos2 {
        self.origin + egui::vec2(f32::from(x) + 0.5, f32::from(row) + 0.5) * self.cell_points
    }

    fn painter(self, ui: &egui::Ui) -> egui::Painter {
        ui.painter().with_clip_rect(self.rect)
    }
}

#[derive(Clone, Copy)]
enum Placement {
    Unit(UnitId),
    Goal,
}

#[derive(Default)]
pub struct Editor {
    selected: Option<UnitId>,
    placement: Option<Placement>,
    message: String,
}

impl Editor {
    pub fn panel(
        &mut self,
        ui: &mut egui::Ui,
        scenario: &mut Option<Scenario>,
        field: Field<'_>,
    ) -> bool {
        ui.heading("Пошаговая тактика");
        let Some(scenario) = scenario else {
            if ui.button("Настроить пошаговую тактику").clicked() {
                *scenario = Some(Scenario::default());
                return true;
            }
            return false;
        };
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (team, label) in [(Team::Player, "+ Боец игрока"), (Team::Rival, "+ Соперник")]
            {
                if ui.button(label).clicked() {
                    if let Some(id) = scenario.add_unit(field, team, None) {
                        self.selected = Some(id);
                        self.placement = Some(Placement::Unit(id));
                        self.message.clear();
                        changed = true;
                    } else {
                        self.message = "Места для нового бойца нет.".into();
                    }
                }
            }
        });
        if !scenario
            .units
            .iter()
            .any(|unit| Some(unit.id) == self.selected)
        {
            self.selected = scenario.units.first().map(|unit| unit.id);
        }
        let selected_name = scenario
            .units
            .iter()
            .find(|unit| Some(unit.id) == self.selected)
            .map_or("Выбрать бойца", |unit| unit.name.as_str());
        let before = self.selected;
        egui::ComboBox::from_id_salt("tactics_unit")
            .selected_text(selected_name)
            .show_ui(ui, |ui| {
                for unit in &scenario.units {
                    ui.selectable_value(&mut self.selected, Some(unit.id), &unit.name);
                }
            });
        if self.selected != before {
            self.placement = None;
        }

        if let Some(index) = scenario
            .units
            .iter()
            .position(|unit| Some(unit.id) == self.selected)
        {
            changed |= self.unit_panel(ui, scenario, field, index);
        }
        ui.separator();
        changed |= self.rules_panel(ui, scenario, field);
        if self.placement.is_some() {
            ui.label("Щёлкни по свободной клетке карты.");
            if ui.button("Отмена размещения").clicked() {
                self.placement = None;
            }
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if let Err(reason) = scenario.check_playable(field) {
            ui.weak(reason);
        }
        changed
    }

    fn unit_panel(
        &mut self,
        ui: &mut egui::Ui,
        scenario: &mut Scenario,
        field: Field<'_>,
        index: usize,
    ) -> bool {
        let unit = &mut scenario.units[index];
        let before = unit.clone();
        let name = ui.text_edit_singleline(&mut unit.name);
        if name.changed() {
            unit.name = unit
                .name
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect();
        }
        if unit.name.trim().is_empty() {
            unit.name = before.name.clone();
        }
        ui.horizontal_wrapped(|ui| {
            for team in [Team::Player, Team::Rival] {
                ui.selectable_value(&mut unit.team, team, team.label());
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::DragValue::new(&mut unit.health)
                    .range(1..=999)
                    .prefix("Здоровье "),
            );
            ui.add(
                egui::DragValue::new(&mut unit.points)
                    .range(1..=24)
                    .prefix("Очки действий "),
            );
            ui.add(
                egui::DragValue::new(&mut unit.damage)
                    .range(1..=999)
                    .prefix("Урон "),
            );
            ui.add(
                egui::DragValue::new(&mut unit.range)
                    .range(1..=24)
                    .prefix("Дальность "),
            );
        });
        let mut changed = *unit != before;
        let id = unit.id;
        let mut cell = unit.cell;
        if ui.button("Поставить бойца на карту").clicked() {
            self.placement = Some(Placement::Unit(id));
            self.message.clear();
        }
        if ui.small_button("Убрать бойца").clicked() {
            scenario.units.remove(index);
            self.selected = None;
            self.placement = None;
            return true;
        }
        if position(ui, field, &mut cell) {
            changed |= scenario.place_unit(field, id, cell);
        }
        changed
    }

    fn rules_panel(
        &mut self,
        ui: &mut egui::Ui,
        scenario: &mut Scenario,
        field: Field<'_>,
    ) -> bool {
        let before = scenario.rules;
        let goal = &mut scenario.rules.goal;
        ui.label("Условие победы");
        let caption = match goal {
            Goal::EliminateRivals => "Победить всех соперников",
            Goal::ReachCell { .. } => "Дойти до клетки",
        };
        egui::ComboBox::from_id_salt("tactics_goal")
            .selected_text(caption)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(*goal == Goal::EliminateRivals, "Победить всех соперников")
                    .clicked()
                {
                    *goal = Goal::EliminateRivals;
                    self.placement = None;
                }
                if ui
                    .selectable_label(matches!(goal, Goal::ReachCell { .. }), "Дойти до клетки")
                    .clicked()
                    && !matches!(goal, Goal::ReachCell { .. })
                {
                    let [width, height] = field.size;
                    *goal = Goal::ReachCell {
                        cell: [width - 1, height - 1],
                    };
                }
            });
        if let Goal::ReachCell { cell } = goal {
            if ui.button("Поставить цель на карту").clicked() {
                self.placement = Some(Placement::Goal);
                self.message.clear();
            }
            position(ui, field, cell);
        }
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::DragValue::new(&mut scenario.rules.move_cost)
                    .range(1..=24)
                    .prefix("Цена шага "),
            );
            ui.add(
                egui::DragValue::new(&mut scenario.rules.attack_cost)
                    .range(1..=24)
                    .prefix("Цена атаки "),
            );
        });
        ui.checkbox(
            &mut scenario.rules.line_of_sight,
            "Препятствия блокируют атаку",
        );
        let mut timer = scenario.rules.round_limit.is_some();
        if ui
            .checkbox(&mut timer, "Ограничить число раундов")
            .changed()
        {
            scenario.rules.round_limit = timer.then_some(8);
        }
        if let Some(limit) = &mut scenario.rules.round_limit {
            ui.add(
                egui::DragValue::new(limit)
                    .range(1..=999)
                    .prefix("Лимит раундов "),
            );
        }
        scenario.rules != before
    }

    pub fn board(
        &mut self,
        ui: &egui::Ui,
        board: Board,
        scenario: &mut Scenario,
        field: Field<'_>,
        clicked: Option<Cell>,
    ) -> bool {
        let painter = board.painter(ui);
        if let Goal::ReachCell { cell } = scenario.rules.goal {
            draw_goal(&painter, board, cell);
        }
        for unit in &scenario.units {
            draw_unit(
                &painter,
                board,
                unit,
                unit.cell,
                unit.health,
                self.selected == Some(unit.id),
            );
        }
        let Some(cell) = clicked else {
            return false;
        };
        let Some(placement) = self.placement else {
            self.selected = scenario
                .units
                .iter()
                .find(|unit| unit.cell == cell)
                .map(|unit| unit.id);
            return false;
        };
        let changed = match placement {
            Placement::Unit(id) => {
                if !scenario.available_cell(field, cell, Some(id)) {
                    self.message = "Клетка занята или перекрыта.".into();
                    return false;
                }
                scenario.place_unit(field, id, cell)
            }
            Placement::Goal => {
                if !field.clear(cell) {
                    self.message = "Цель должна стоять на проходимой клетке.".into();
                    return false;
                }
                let target = Goal::ReachCell { cell };
                let changed = scenario.rules.goal != target;
                scenario.rules.goal = target;
                changed
            }
        };
        self.placement = None;
        self.message.clear();
        changed
    }
}

fn position(ui: &mut egui::Ui, field: Field<'_>, cell: &mut Cell) -> bool {
    let previous = *cell;
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::DragValue::new(&mut cell[0])
                .range(0..=field.size[0] - 1)
                .prefix("Столбец "),
        );
        ui.add(
            egui::DragValue::new(&mut cell[1])
                .range(0..=field.size[1] - 1)
                .prefix("Строка "),
        );
    });
    *cell != previous
}

#[derive(Default)]
pub struct Play {
    pub selected: Option<UnitId>,
    pub message: String,
}

impl Play {
    pub fn header(&mut self, ui: &mut egui::Ui, battle: &mut Battle, field: Field<'_>) {
        if !self.selected.is_some_and(|id| {
            battle
                .fighter(id)
                .is_some_and(|fighter| fighter.alive() && fighter.spec.team == Team::Player)
        }) {
            self.selected = battle
                .fighters()
                .find(|fighter| fighter.alive() && fighter.spec.team == Team::Player)
                .map(|fighter| fighter.spec.id);
        }
        if ui.is_enabled() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) {
            self.next(battle);
        }
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("Раунд {}", battle.round));
            let caption = match battle.outcome {
                Outcome::Ongoing => "Ход игрока",
                Outcome::Won => "Победа",
                Outcome::Lost => "Поражение",
            };
            ui.strong(caption);
            let active = ui.is_enabled() && battle.outcome == Outcome::Ongoing;
            if ui
                .add_enabled(active, egui::Button::new("Завершить ход"))
                .clicked()
                || active && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
            {
                let events = battle.end_turn(field);
                self.describe(battle, events);
            }
        });
        if let Some(fighter) = self.selected.and_then(|id| battle.fighter(id)) {
            ui.label(format!(
                "{} · Здоровье {} / {} · Очки действий {} / {}",
                fighter.spec.name,
                fighter.state.health,
                fighter.spec.health,
                fighter.state.points,
                fighter.spec.points
            ));
        }
        ui.horizontal_wrapped(|ui| {
            ui.weak(
                "ЛКМ — выбрать, идти или атаковать · Tab — сменить бойца · Enter — завершить ход",
            );
            if !self.message.is_empty() {
                ui.label(&self.message);
            }
        });
    }

    pub fn board(
        &mut self,
        ui: &egui::Ui,
        board: Board,
        battle: &mut Battle,
        field: Field<'_>,
        clicked: Option<Cell>,
    ) {
        if let Some(cell) = clicked
            && battle.outcome == Outcome::Ongoing
        {
            self.click(battle, field, cell);
        }
        let painter = board.painter(ui);
        if let Goal::ReachCell { cell } = battle.rules().goal {
            draw_goal(&painter, board, cell);
        }
        if let Some(id) = self.selected {
            for cell in battle.reachable(field, id) {
                let center = board.center(cell);
                let tile =
                    Rect::from_center_size(center, egui::Vec2::splat(board.cell_points * 0.87));
                painter.rect_filled(tile, 3.0, PLAYER.gamma_multiply(0.25));
                painter.circle_filled(center, 2.0, PLAYER);
            }
        }
        for fighter in battle.fighters().filter(|fighter| fighter.alive()) {
            draw_unit(
                &painter,
                board,
                &fighter.spec,
                fighter.state.cell,
                fighter.state.health,
                self.selected == Some(fighter.spec.id),
            );
        }
    }

    fn next(&mut self, battle: &Battle) {
        let units = battle
            .fighters()
            .filter(|fighter| fighter.alive() && fighter.spec.team == Team::Player)
            .map(|fighter| fighter.spec.id)
            .collect::<Vec<_>>();
        if let Some(index) = units.iter().position(|&id| Some(id) == self.selected) {
            self.selected = Some(units[(index + 1) % units.len()]);
        } else {
            self.selected = units.first().copied();
        }
    }

    fn click(&mut self, battle: &mut Battle, field: Field<'_>, cell: Cell) {
        let target = battle
            .at(cell)
            .map(|fighter| (fighter.spec.team, fighter.spec.id));
        if let Some((Team::Player, id)) = target {
            self.selected = Some(id);
            self.message.clear();
            return;
        }
        let Some(id) = self.selected else {
            return;
        };
        let result = if let Some((Team::Rival, enemy)) = target {
            battle.attack(field, id, enemy)
        } else {
            battle.move_unit(field, id, cell)
        };
        self.describe(battle, result);
    }

    fn describe(&mut self, battle: &Battle, result: Result<Vec<Event>, crate::tactics::Rejected>) {
        let label = |unit| {
            battle
                .fighter(unit)
                .map_or("Боец", |fighter| fighter.spec.name.as_str())
        };
        self.message = match result {
            Err(error) => error.label().into(),
            Ok(events) => events
                .iter()
                .rev()
                .find_map(|event| match event {
                    Event::Attack {
                        unit,
                        target,
                        damage,
                        ..
                    } => Some(format!(
                        "{} атакует {} · −{}",
                        label(*unit),
                        label(*target),
                        damage
                    )),
                    Event::Move { unit, path } => {
                        Some(format!("{} · шагов {}", label(*unit), path.len()))
                    }
                    _ => None,
                })
                .unwrap_or_default(),
        };
    }
}

fn draw_unit(
    painter: &egui::Painter,
    board: Board,
    unit: &Unit,
    cell: Cell,
    health: u16,
    selected: bool,
) {
    let center = board.center(cell);
    if !board.rect.expand(board.cell_points).contains(center) {
        return;
    }
    let radius = board.cell_points * 0.36;
    let color = if unit.team == Team::Player {
        PLAYER
    } else {
        RIVAL
    };
    painter.circle_filled(center, radius, Color32::from_rgb(23, 38, 53));
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(if selected { 3.0_f32 } else { 1.8_f32 }, color),
    );
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        health.to_string(),
        FontId::monospace((radius * 1.13).clamp(8.0, 22.0)),
        color,
    );
}

fn draw_goal(painter: &egui::Painter, board: Board, cell: Cell) {
    let center = board.center(cell);
    let span = board.cell_points * 0.80;
    let area = Rect::from_center_size(center, egui::Vec2::splat(span));
    painter.rect_filled(area, 2.0, TARGET.gamma_multiply(0.32));
    painter.rect_stroke(
        area,
        2.0,
        Stroke::new(2.0_f32, TARGET),
        egui::StrokeKind::Inside,
    );
}
