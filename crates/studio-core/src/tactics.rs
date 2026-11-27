//! Shared tactical board rules. The editor and game supply the same terrain and commands.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub type Cell = [u16; 2];
pub type UnitId = u32;
pub const MAX_UNITS: usize = 48;
const MAX_EDGE: u16 = 96;

#[derive(Clone, Copy)]
pub struct Field<'a> {
    pub size: [u16; 2],
    pub blocked: &'a dyn Fn(Cell) -> bool,
}

impl Field<'_> {
    pub fn contains(self, [x, row]: Cell) -> bool {
        x < self.size[0] && row < self.size[1]
    }

    pub fn clear(self, cell: Cell) -> bool {
        self.contains(cell) && !(self.blocked)(cell)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Team {
    Player,
    Rival,
}

impl Team {
    pub fn label(self) -> &'static str {
        match self {
            Self::Player => "Игрок",
            Self::Rival => "Соперник",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub name: String,
    pub team: Team,
    pub cell: Cell,
    pub health: u16,
    pub points: u8,
    pub damage: u16,
    pub range: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Goal {
    #[default]
    EliminateRivals,
    ReachCell {
        cell: Cell,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub goal: Goal,
    pub move_cost: u8,
    pub attack_cost: u8,
    pub line_of_sight: bool,
    pub round_limit: Option<u16>,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            goal: Goal::EliminateRivals,
            move_cost: 1,
            attack_cost: 2,
            line_of_sight: true,
            round_limit: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scenario {
    pub units: Vec<Unit>,
    pub rules: Rules,
    next_id: UnitId,
}

impl Scenario {
    pub fn add_unit(
        &mut self,
        field: Field<'_>,
        team: Team,
        target: Option<Cell>,
    ) -> Option<UnitId> {
        if self.units.len() >= MAX_UNITS || !valid_size(field.size) {
            return None;
        }
        let clear = |cell| self.available_cell(field, cell, None);
        let cell = if let Some(target) = target {
            clear(target).then_some(target)?
        } else {
            let [w, h] = field.size;
            (0..h)
                .flat_map(|row| (0..w).map(move |x| [x, row]))
                .find(|&cell| clear(cell))?
        };
        let id = self
            .units
            .iter()
            .map(|unit| unit.id)
            .max()
            .unwrap_or_default()
            .checked_add(1)?
            .max(self.next_id);
        self.next_id = id.checked_add(1)?;
        self.units.push(Unit {
            id,
            name: format!("{} {id}", team.label()),
            team,
            cell,
            health: 8,
            points: 4,
            damage: 3,
            range: 1,
        });
        Some(id)
    }

    pub fn available_cell(&self, field: Field<'_>, cell: Cell, skip: Option<UnitId>) -> bool {
        field.clear(cell)
            && self
                .units
                .iter()
                .all(|unit| unit.cell != cell || skip == Some(unit.id))
    }

    pub fn place_unit(&mut self, field: Field<'_>, id: UnitId, cell: Cell) -> bool {
        if !self.available_cell(field, cell, Some(id)) {
            return false;
        }
        let Some(unit) = self.units.iter_mut().find(|unit| unit.id == id) else {
            return false;
        };
        if unit.cell == cell {
            return false;
        }
        unit.cell = cell;
        true
    }

    /// Saving a draft permits unfinished teams and cells still being set up.
    pub fn validate(&self) -> Result<(), String> {
        let rules = self.rules;
        if self.units.len() > MAX_UNITS
            || !(1..=24).contains(&rules.move_cost)
            || !(1..=24).contains(&rules.attack_cost)
            || rules
                .round_limit
                .is_some_and(|limit| !(1..=999).contains(&limit))
        {
            return Err("Недопустимый размер отрядов, цена действия или число раундов.".into());
        }
        let mut ids = HashSet::with_capacity(self.units.len());
        let mut cells = HashSet::with_capacity(self.units.len());
        if self.units.iter().any(|unit| {
            unit.id == 0
                || !ids.insert(unit.id)
                || !cells.insert(unit.cell)
                || unit.cell.iter().any(|&axis| axis >= MAX_EDGE)
                || unit.name.trim().is_empty()
                || unit.name.chars().count() > 64
                || unit.name.chars().any(char::is_control)
                || !(1..=999).contains(&unit.health)
                || !(1..=24).contains(&unit.points)
                || !(1..=999).contains(&unit.damage)
                || !(1..=24).contains(&unit.range)
        }) {
            return Err("Недопустимые параметры бойца или два бойца на одной клетке.".into());
        }
        if let Goal::ReachCell { cell } = rules.goal
            && cell.iter().any(|&axis| axis >= MAX_EDGE)
        {
            return Err("Цель тактики за пределами поддерживаемой сетки.".into());
        }
        Ok(())
    }

    pub fn check_playable(&self, field: Field<'_>) -> Result<(), String> {
        self.validate()?;
        if !valid_size(field.size) {
            return Err("Недопустимый размер тактического поля.".into());
        }
        if !self.units.iter().any(|unit| unit.team == Team::Player) {
            return Err("Добавь хотя бы одного бойца игрока.".into());
        }
        match self.rules.goal {
            Goal::EliminateRivals if !self.units.iter().any(|unit| unit.team == Team::Rival) => {
                return Err("Для этой цели добавь хотя бы одного соперника.".into());
            }
            Goal::ReachCell { cell } if !field.clear(cell) => {
                return Err("Укажи свободную клетку для цели.".into());
            }
            _ => {}
        }
        if let Some(unit) = self.units.iter().find(|unit| !field.clear(unit.cell)) {
            return Err(format!(
                "«{}» стоит на стене или за краем карты.",
                unit.name
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    #[default]
    Ongoing,
    Won,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitState {
    pub id: UnitId,
    pub cell: Cell,
    pub health: u16,
    pub points: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub round: u32,
    pub units: Vec<UnitState>,
}

impl Snapshot {
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::with_capacity(self.units.len());
        if self.round == 0
            || self.units.len() > MAX_UNITS
            || self.units.iter().any(|unit| {
                unit.id == 0
                    || !ids.insert(unit.id)
                    || unit.cell.iter().any(|&axis| axis >= MAX_EDGE)
                    || unit.points > 24
                    || unit.health > 999
            })
        {
            return Err("Некорректные данные сохранённого тактического боя.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Fighter {
    pub spec: Unit,
    pub state: UnitState,
}

impl Fighter {
    pub fn alive(&self) -> bool {
        self.state.health > 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Move {
        unit: UnitId,
        path: Vec<Cell>,
    },
    Attack {
        unit: UnitId,
        target: UnitId,
        damage: u16,
        health: u16,
    },
    Turn {
        team: Team,
        round: u32,
    },
    Finish(Outcome),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejected {
    Finished,
    UnknownUnit,
    EnemyUnit,
    AllyTarget,
    BusyCell,
    Unreachable,
    TooFar,
    NoSight,
    NoPoints,
    ChangedField,
}

impl Rejected {
    pub fn label(self) -> &'static str {
        match self {
            Self::Finished => "Бой завершён.",
            Self::UnknownUnit => "Боец выбыл или отсутствует.",
            Self::EnemyUnit => "Сейчас можно командовать только своим отрядом.",
            Self::AllyTarget => "Выбери соперника.",
            Self::BusyCell => "Клетка занята или недоступна.",
            Self::Unreachable => "Путь к клетке перекрыт.",
            Self::TooFar => "Соперник вне дальности атаки.",
            Self::NoSight => "Между бойцами есть препятствие.",
            Self::NoPoints => "Не хватает очков действий.",
            Self::ChangedField => "Размер тактического поля изменился.",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Battle {
    rules: Rules,
    size: [u16; 2],
    pub round: u32,
    pub outcome: Outcome,
    fighters: Vec<Fighter>,
}

impl Battle {
    pub fn start(scenario: &Scenario, field: Field<'_>) -> Result<Self, String> {
        scenario.check_playable(field)?;
        let fighters = scenario
            .units
            .iter()
            .cloned()
            .map(|spec| Fighter {
                state: UnitState {
                    id: spec.id,
                    cell: spec.cell,
                    health: spec.health,
                    points: spec.points,
                },
                spec,
            })
            .collect();
        let mut battle = Self {
            rules: scenario.rules,
            size: field.size,
            round: 1,
            outcome: Outcome::Ongoing,
            fighters,
        };
        battle.outcome = battle.compute_outcome();
        Ok(battle)
    }

    pub fn restore(
        scenario: &Scenario,
        field: Field<'_>,
        snapshot: Snapshot,
    ) -> Result<Self, String> {
        let mut battle = Self::start(scenario, field)?;
        snapshot.validate()?;
        if snapshot.units.len() != battle.fighters.len() {
            return Err("В сохранённом бое некорректное число раундов или бойцов.".into());
        }
        let mut positions = HashSet::new();
        let mut ids = HashSet::new();
        if snapshot.units.iter().any(|state| {
            let Some(fighter) = battle.fighter(state.id) else {
                return true;
            };
            !ids.insert(state.id)
                || !field.contains(state.cell)
                || state.health > fighter.spec.health
                || state.points > fighter.spec.points
                || state.health > 0 && (!field.clear(state.cell) || !positions.insert(state.cell))
        }) {
            return Err("Бойцы сохранённого боя не соответствуют полю или сценарию.".into());
        }
        battle.round = snapshot.round;
        for state in snapshot.units {
            let index = battle.index(state.id).expect("validated unit");
            battle.fighters[index].state = state;
        }
        battle.outcome = battle.compute_outcome();
        Ok(battle)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            round: self.round,
            units: self.fighters.iter().map(|fighter| fighter.state).collect(),
        }
    }

    pub fn rules(&self) -> Rules {
        self.rules
    }

    pub fn fighters(&self) -> impl Iterator<Item = &Fighter> {
        self.fighters.iter()
    }

    pub fn fighter(&self, id: UnitId) -> Option<&Fighter> {
        self.index(id).map(|index| &self.fighters[index])
    }

    pub fn at(&self, cell: Cell) -> Option<&Fighter> {
        self.fighters
            .iter()
            .find(|fighter| fighter.alive() && fighter.state.cell == cell)
    }

    pub fn reachable(&self, field: Field<'_>, id: UnitId) -> Vec<Cell> {
        if field.size != self.size {
            return Vec::new();
        }
        let Ok(index) = self.actor(Team::Player, id) else {
            return Vec::new();
        };
        let actor = &self.fighters[index];
        let steps = usize::from(actor.state.points / self.rules.move_cost);
        self.search(field, index, steps)
            .visited
            .into_iter()
            .skip(1)
            .collect()
    }

    pub fn move_unit(
        &mut self,
        field: Field<'_>,
        id: UnitId,
        destination: Cell,
    ) -> Result<Vec<Event>, Rejected> {
        let index = self.actor(Team::Player, id)?;
        if field.size != self.size {
            return Err(Rejected::ChangedField);
        }
        if !field.clear(destination) || self.at(destination).is_some() {
            return Err(Rejected::BusyCell);
        }
        let search = self.search(field, index, usize::MAX);
        let path = search.path(destination).ok_or(Rejected::Unreachable)?;
        let actor = &self.fighters[index];
        if path.len() > usize::from(actor.state.points / self.rules.move_cost) {
            return Err(Rejected::NoPoints);
        }
        let mut events = Vec::with_capacity(2);
        self.move_along(index, path, &mut events);
        self.announce_finish(&mut events);
        Ok(events)
    }

    pub fn attack(
        &mut self,
        field: Field<'_>,
        id: UnitId,
        target: UnitId,
    ) -> Result<Vec<Event>, Rejected> {
        let index = self.actor(Team::Player, id)?;
        if field.size != self.size {
            return Err(Rejected::ChangedField);
        }
        let target = self.check_attack(field, index, target)?;
        let mut events = Vec::with_capacity(2);
        self.hit(index, target, &mut events);
        self.announce_finish(&mut events);
        Ok(events)
    }

    /// Resolve the opponent's whole phase deterministically before returning player control.
    pub fn end_turn(&mut self, field: Field<'_>) -> Result<Vec<Event>, Rejected> {
        if self.outcome != Outcome::Ongoing {
            return Err(Rejected::Finished);
        }
        if field.size != self.size {
            return Err(Rejected::ChangedField);
        }
        let mut events = vec![Event::Turn {
            team: Team::Rival,
            round: self.round,
        }];
        for index in 0..self.fighters.len() {
            let actor = &mut self.fighters[index];
            if !actor.alive() || actor.spec.team != Team::Rival {
                continue;
            }
            actor.state.points = actor.spec.points;
            self.rival_turn(field, index, &mut events);
            self.announce_finish(&mut events);
            if self.outcome != Outcome::Ongoing {
                return Ok(events);
            }
        }
        self.round = self.round.saturating_add(1);
        self.announce_finish(&mut events);
        if self.outcome == Outcome::Ongoing {
            for fighter in &mut self.fighters {
                if fighter.alive() && fighter.spec.team == Team::Player {
                    fighter.state.points = fighter.spec.points;
                }
            }
            events.push(Event::Turn {
                team: Team::Player,
                round: self.round,
            });
        }
        Ok(events)
    }

    fn index(&self, id: UnitId) -> Option<usize> {
        self.fighters
            .iter()
            .position(|fighter| fighter.spec.id == id)
    }

    fn actor(&self, team: Team, id: UnitId) -> Result<usize, Rejected> {
        if self.outcome != Outcome::Ongoing {
            return Err(Rejected::Finished);
        }
        let index = self
            .index(id)
            .filter(|&i| self.fighters[i].alive())
            .ok_or(Rejected::UnknownUnit)?;
        if self.fighters[index].spec.team != team {
            return Err(Rejected::EnemyUnit);
        }
        Ok(index)
    }

    fn check_attack(
        &self,
        field: Field<'_>,
        attacker: usize,
        target: UnitId,
    ) -> Result<usize, Rejected> {
        let actor = &self.fighters[attacker];
        let target = self
            .index(target)
            .filter(|&i| self.fighters[i].alive())
            .ok_or(Rejected::UnknownUnit)?;
        let rival = &self.fighters[target];
        if actor.spec.team == rival.spec.team {
            return Err(Rejected::AllyTarget);
        }
        if distance(actor.state.cell, rival.state.cell) > u16::from(actor.spec.range) {
            return Err(Rejected::TooFar);
        }
        if self.rules.line_of_sight && !visible(field, actor.state.cell, rival.state.cell) {
            return Err(Rejected::NoSight);
        }
        if actor.state.points < self.rules.attack_cost {
            return Err(Rejected::NoPoints);
        }
        Ok(target)
    }

    fn hit(&mut self, actor: usize, target: usize, events: &mut Vec<Event>) {
        let attacker = self.fighters[actor].spec.id;
        let damage = self.fighters[actor]
            .spec
            .damage
            .min(self.fighters[target].state.health);
        self.fighters[actor].state.points -= self.rules.attack_cost;
        let defender = &mut self.fighters[target];
        defender.state.health -= damage;
        events.push(Event::Attack {
            unit: attacker,
            target: defender.spec.id,
            damage,
            health: defender.state.health,
        });
    }

    fn search(&self, field: Field<'_>, actor: usize, limit: usize) -> Search {
        let mut occupied = vec![false; area(field.size)];
        for (index, fighter) in self.fighters.iter().enumerate() {
            if index != actor && fighter.alive() && field.contains(fighter.state.cell) {
                occupied[index_of(field.size, fighter.state.cell)] = true;
            }
        }
        Search::new(field, self.fighters[actor].state.cell, &occupied, limit)
    }

    fn move_along(&mut self, actor: usize, path: Vec<Cell>, events: &mut Vec<Event>) {
        let fighter = &mut self.fighters[actor];
        fighter.state.cell = *path.last().expect("nonempty path");
        fighter.state.points -= path.len() as u8 * self.rules.move_cost;
        events.push(Event::Move {
            unit: fighter.spec.id,
            path,
        });
    }

    fn rival_turn(&mut self, field: Field<'_>, actor: usize, events: &mut Vec<Event>) {
        loop {
            let opponent = |fighter: &Fighter| fighter.alive() && fighter.spec.team == Team::Player;
            let target = self
                .fighters
                .iter()
                .filter(|fighter| opponent(fighter))
                .filter_map(|fighter| self.check_attack(field, actor, fighter.spec.id).ok())
                .min_by_key(|&index| {
                    (
                        self.fighters[index].state.health,
                        self.fighters[index].spec.id,
                    )
                });
            if let Some(target) = target {
                self.hit(actor, target, events);
                continue;
            }
            let fighter = &self.fighters[actor];
            let budget = usize::from(fighter.state.points / self.rules.move_cost);
            if budget == 0 {
                break;
            }
            let search = self.search(field, actor, usize::MAX);
            let destination = search.visited.iter().copied().find(|&cell| {
                self.fighters
                    .iter()
                    .filter(|fighter| opponent(fighter))
                    .any(|rival| {
                        distance(cell, rival.state.cell) <= u16::from(fighter.spec.range)
                            && (!self.rules.line_of_sight || visible(field, cell, rival.state.cell))
                    })
            });
            let Some(mut path) = destination.and_then(|cell| search.path(cell)) else {
                break;
            };
            path.truncate(budget);
            if path.is_empty() {
                break;
            }
            self.move_along(actor, path, events);
        }
    }

    fn compute_outcome(&self) -> Outcome {
        if !self
            .fighters
            .iter()
            .any(|fighter| fighter.alive() && fighter.spec.team == Team::Player)
        {
            return Outcome::Lost;
        }
        let won = match self.rules.goal {
            Goal::EliminateRivals => !self
                .fighters
                .iter()
                .any(|fighter| fighter.alive() && fighter.spec.team == Team::Rival),
            Goal::ReachCell { cell } => self
                .at(cell)
                .is_some_and(|fighter| fighter.spec.team == Team::Player),
        };
        if won {
            Outcome::Won
        } else if self
            .rules
            .round_limit
            .is_some_and(|limit| self.round > u32::from(limit))
        {
            Outcome::Lost
        } else {
            Outcome::Ongoing
        }
    }

    fn announce_finish(&mut self, events: &mut Vec<Event>) {
        let outcome = self.compute_outcome();
        if self.outcome != outcome {
            self.outcome = outcome;
            events.push(Event::Finish(outcome));
        }
    }
}

fn valid_size([width, height]: [u16; 2]) -> bool {
    (1..=MAX_EDGE).contains(&width) && (1..=MAX_EDGE).contains(&height)
}

fn area([width, height]: [u16; 2]) -> usize {
    usize::from(width) * usize::from(height)
}

fn index_of([width, _]: [u16; 2], [x, row]: Cell) -> usize {
    usize::from(row) * usize::from(width) + usize::from(x)
}

fn distance([x, row]: Cell, [other_x, other_row]: Cell) -> u16 {
    x.abs_diff(other_x) + row.abs_diff(other_row)
}

struct Search {
    size: [u16; 2],
    start: Cell,
    visited: Vec<Cell>,
    parent: Vec<Option<Cell>>,
}

impl Search {
    fn new(field: Field<'_>, start: Cell, occupied: &[bool], limit: usize) -> Self {
        let mut result = Self {
            size: field.size,
            start,
            visited: Vec::new(),
            parent: vec![None; area(field.size)],
        };
        if !field.clear(start) {
            return result;
        }
        result.parent[index_of(field.size, start)] = Some(start);
        result.visited.push(start);
        let mut depths = vec![0];
        let mut cursor = 0;
        while let Some(&cell) = result.visited.get(cursor) {
            let depth = depths[cursor];
            cursor += 1;
            if depth == limit {
                break;
            }
            let [x, row] = cell;
            for adjacent in [
                row.checked_sub(1).map(|row| [x, row]),
                x.checked_sub(1).map(|x| [x, row]),
                Some([x + 1, row]),
                Some([x, row + 1]),
            ]
            .into_iter()
            .flatten()
            .filter(|&cell| field.clear(cell))
            {
                let index = index_of(field.size, adjacent);
                if result.parent[index].is_none() && !occupied[index] {
                    result.parent[index] = Some(cell);
                    result.visited.push(adjacent);
                    depths.push(depth + 1);
                }
            }
        }
        result
    }

    fn path(&self, mut end: Cell) -> Option<Vec<Cell>> {
        if end[0] >= self.size[0] || end[1] >= self.size[1] {
            return None;
        }
        let mut path = Vec::new();
        while end != self.start {
            let parent = self.parent[index_of(self.size, end)]?;
            path.push(end);
            end = parent;
        }
        path.reverse();
        Some(path)
    }
}

fn visible(field: Field<'_>, from: Cell, to: Cell) -> bool {
    let (from, to) = if from < to { (from, to) } else { (to, from) };
    let [mut x, mut row] = from.map(i32::from);
    let [target_x, target_row] = to.map(i32::from);
    let dx = (target_x - x).abs();
    let dy = -(target_row - row).abs();
    let sx = (target_x - x).signum();
    let sy = (target_row - row).signum();
    let mut error = dx + dy;
    while [x, row] != [target_x, target_row] {
        let twice = error * 2;
        if twice >= dy
            && twice <= dx
            && !field.clear([(x + sx) as u16, row as u16])
            && !field.clear([x as u16, (row + sy) as u16])
        {
            return false;
        }
        if twice >= dy {
            error += dy;
            x += sx;
        }
        if twice <= dx {
            error += dx;
            row += sy;
        }
        if [x, row] != [target_x, target_row] && !field.clear([x as u16, row as u16]) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_respects_occupied_cells_terrain_cost_and_undo_free_snapshot() {
        let field = Field {
            size: [7, 6],
            blocked: &|cell| cell == [2, 1],
        };
        let mut scenario = Scenario::default();
        scenario.rules.move_cost = 2;
        scenario.rules.goal = Goal::ReachCell { cell: [1, 3] };
        let hero = scenario
            .add_unit(field, Team::Player, Some([1, 1]))
            .unwrap();
        scenario
            .add_unit(field, Team::Player, Some([0, 1]))
            .unwrap();
        let mut battle = Battle::start(&scenario, field).unwrap();
        let reachable = battle.reachable(field, hero);
        assert!(reachable.contains(&[1, 3]));
        assert!(!reachable.contains(&[0, 1]));
        assert!(!reachable.contains(&[2, 1]));
        assert_eq!(
            battle.move_unit(field, hero, [3, 1]),
            Err(Rejected::NoPoints)
        );
        assert_eq!(
            battle.move_unit(field, hero, [0, 1]),
            Err(Rejected::BusyCell)
        );
        let events = battle.move_unit(field, hero, [1, 3]).unwrap();
        assert_eq!(
            events,
            [
                Event::Move {
                    unit: hero,
                    path: vec![[1, 2], [1, 3]]
                },
                Event::Finish(Outcome::Won)
            ]
        );
        assert_eq!(battle.fighter(hero).unwrap().state.points, 0);
        let bytes = serde_json::to_vec(&battle.snapshot()).unwrap();
        let snapshot = serde_json::from_slice(&bytes).unwrap();
        let loaded = Battle::restore(&scenario, field, snapshot).unwrap();
        assert_eq!(loaded.outcome, Outcome::Won);
        assert_eq!(loaded.snapshot(), battle.snapshot());
    }

    #[test]
    fn attacks_and_deterministic_enemy_phase_share_points_and_terrain_rules() {
        let field = Field {
            size: [8, 5],
            blocked: &|cell| matches!(cell, [2, 1] | [3, 1]),
        };
        let mut scenario = Scenario::default();
        let hero = scenario
            .add_unit(field, Team::Player, Some([1, 1]))
            .unwrap();
        let rival = scenario.add_unit(field, Team::Rival, Some([4, 1])).unwrap();
        scenario.units[0].range = 3;
        let mut battle = Battle::start(&scenario, field).unwrap();
        assert_eq!(battle.attack(field, hero, rival), Err(Rejected::NoSight));
        let mut second = battle.clone();
        let events = battle.end_turn(field).unwrap();
        assert_eq!(events, second.end_turn(field).unwrap());
        assert_eq!(
            events.first(),
            Some(&Event::Turn {
                team: Team::Rival,
                round: 1
            })
        );
        assert_eq!(
            events.last(),
            Some(&Event::Turn {
                team: Team::Player,
                round: 2
            })
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Move { unit, .. } if *unit == rival))
        );
        assert_eq!(battle.state_for_test(hero).points, 4);
        let before = battle.state_for_test(rival).health;
        battle.attack(field, hero, rival).unwrap();
        assert!(battle.state_for_test(rival).health < before);
    }

    #[test]
    fn edited_round_limit_and_combat_victory_are_reusable_conditions() {
        let field = Field {
            size: [5, 4],
            blocked: &|_| false,
        };
        let mut scenario = Scenario::default();
        let hero = scenario
            .add_unit(field, Team::Player, Some([1, 1]))
            .unwrap();
        let rival = scenario.add_unit(field, Team::Rival, Some([2, 1])).unwrap();
        scenario.rules.round_limit = Some(1);
        scenario.units[1].health = 2;
        let mut limited = Battle::start(&scenario, field).unwrap();
        assert!(
            limited
                .end_turn(field)
                .unwrap()
                .contains(&Event::Finish(Outcome::Lost))
        );
        let mut winning = Battle::start(&scenario, field).unwrap();
        assert!(
            winning
                .attack(field, hero, rival)
                .unwrap()
                .contains(&Event::Finish(Outcome::Won))
        );
        assert_eq!(winning.end_turn(field), Err(Rejected::Finished));
    }

    impl Battle {
        fn state_for_test(&self, id: UnitId) -> UnitState {
            self.fighter(id).unwrap().state
        }
    }
}
