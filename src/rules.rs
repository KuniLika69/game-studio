use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{
    ImportError,
    story::Story,
    world::{InteractionState, World},
};

use studio_rules::engine;

#[path = "rules_game_host.rs"]
mod game_host;

#[path = "rules_bindings.rs"]
mod bindings;

#[path = "rules_library.rs"]
mod library;
pub use bindings::{Binding, BindingValue, Port, PortType};
pub use library::{MAX_TEMPLATES, MarkBindings, MarkSource, MarkTarget, RuleTemplate, Template};
pub(crate) use library::{unbind_action, unbind_condition, unbind_event};

pub const MAX_RULES: usize = 64;
pub const MAX_FLAGS: usize = 64;
pub const MAX_ZONES: usize = 48;
pub const MAX_CONDITIONS: usize = 8;
pub const MAX_ACTIONS: usize = 8;
pub const MAX_STAGE: u16 = 999;
pub const MAX_COUNTERS: usize = 64;
pub const MAX_TIMERS: usize = 32;
pub const MAX_COUNTER_VALUE: i32 = 999_999;
pub const MAX_TIMER_SECONDS: f32 = 3600.0;
pub const PLAYER_ACTOR_ID: u64 = 1;
const MAX_TRACE: usize = 96;
const MAX_OBJECT_OVERRIDES: usize = 128;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
    pub flags: Vec<Flag>,
    pub zones: Vec<Zone>,
    pub counters: Vec<Counter>,
    pub timers: Vec<Timer>,
    pub templates: Vec<Template>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rule_templates: Vec<RuleTemplate>,
    pub initially_disabled_objects: Vec<u64>,
    #[serde(skip_serializing_if = "crate::extensions::Registry::is_empty")]
    pub extensions: crate::extensions::Registry,
    next_rule: u32,
    next_flag: u32,
    next_zone: u32,
    next_counter: u32,
    next_timer: u32,
    next_template: u32,
    next_rule_template: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub draft: bool,
    pub once: bool,
    pub event: Option<Event>,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flag {
    pub id: u32,
    pub name: String,
    pub initially_set: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Counter {
    pub id: u32,
    pub name: String,
    pub initial: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timer {
    pub id: u32,
    pub name: String,
    pub seconds: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub id: u32,
    pub name: String,
    /// X and Z are centered. Y is the base of the region.
    pub position: [f32; 3],
    pub size: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub building_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Pickup {
        item_id: u32,
    },
    Dialogue {
        npc_id: u64,
        node_id: u32,
        choice_id: u32,
    },
    Door {
        door_id: u64,
    },
    EnterZone {
        zone_id: u32,
    },
    ActorEnteredZone {
        actor_id: u64,
        zone_id: u32,
    },
    GameAction {
        action: GameAction,
    },
    TimerElapsed {
        timer_id: u32,
    },
    Simulation(crate::simulation_rules::Signal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameAction {
    Mark,
    Retry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Condition {
    HasItem {
        item_id: u32,
    },
    Flag {
        flag_id: u32,
        value: bool,
    },
    Counter {
        counter_id: u32,
        comparison: Comparison,
        value: i32,
    },
    InZone {
        zone_id: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Comparison {
    Equal,
    NotEqual,
    Less,
    AtMost,
    Greater,
    AtLeast,
}

impl Comparison {
    fn accepts(self, actual: i32, expected: i32) -> bool {
        match self {
            Self::Equal => actual == expected,
            Self::NotEqual => actual != expected,
            Self::Less => actual < expected,
            Self::AtMost => actual <= expected,
            Self::Greater => actual > expected,
            Self::AtLeast => actual >= expected,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    SetFlag { flag_id: u32, value: bool },
    UnlockDoor { door_id: u64 },
    SetQuestStage { quest_id: u32, stage: u16 },
    SetCounter { counter_id: u32, value: i32 },
    AddCounter { counter_id: u32, delta: i32 },
    StartTimer { timer_id: u32 },
    StopTimer { timer_id: u32 },
    SetObjectEnabled { object_id: u64, enabled: bool },
    ResetAttempt,
    Transition { scene_id: u32 },
    Simulation(crate::simulation_rules::Command),
    PlayEffect(crate::effect_rules::Command),
    Extension(crate::extensions::Call),
}

impl RuleSet {
    pub fn add_counter(&mut self) -> Option<u32> {
        let id = reserve(
            &mut self.next_counter,
            self.counters.iter().map(|counter| counter.id),
            MAX_COUNTERS,
        )?;
        self.counters.push(Counter {
            id,
            name: format!("Счётчик {id}"),
            initial: 0,
        });
        Some(id)
    }

    pub fn add_timer(&mut self) -> Option<u32> {
        let id = reserve(
            &mut self.next_timer,
            self.timers.iter().map(|timer| timer.id),
            MAX_TIMERS,
        )?;
        self.timers.push(Timer {
            id,
            name: format!("Таймер {id}"),
            seconds: 30.0,
        });
        Some(id)
    }

    pub fn counter(&self, id: u32) -> Option<&Counter> {
        self.counters.iter().find(|counter| counter.id == id)
    }

    pub fn timer(&self, id: u32) -> Option<&Timer> {
        self.timers.iter().find(|timer| timer.id == id)
    }

    pub fn remove_counter(&mut self, id: u32) -> bool {
        remove(&mut self.counters, |counter| counter.id == id)
    }

    pub fn remove_timer(&mut self, id: u32) -> bool {
        remove(&mut self.timers, |timer| timer.id == id)
    }

    pub fn add_rule(&mut self) -> Option<u32> {
        let id = reserve(
            &mut self.next_rule,
            self.rules.iter().map(|rule| rule.id),
            MAX_RULES,
        )?;
        self.rules.push(Rule {
            id,
            name: format!("Правило {id}"),
            enabled: true,
            draft: false,
            once: true,
            event: None,
            conditions: Vec::new(),
            actions: Vec::new(),
        });
        Some(id)
    }

    pub fn add_flag(&mut self) -> Option<u32> {
        let id = reserve(
            &mut self.next_flag,
            self.flags.iter().map(|flag| flag.id),
            MAX_FLAGS,
        )?;
        self.flags.push(Flag {
            id,
            name: format!("Флаг {id}"),
            initially_set: false,
        });
        Some(id)
    }

    pub fn add_zone(&mut self, position: [f32; 3]) -> Option<u32> {
        if !valid_position(position) {
            return None;
        }
        let id = reserve(
            &mut self.next_zone,
            self.zones.iter().map(|zone| zone.id),
            MAX_ZONES,
        )?;
        self.zones.push(Zone {
            id,
            name: format!("Зона {id}"),
            position,
            size: [2.0, 2.5, 2.0],
            building_id: None,
        });
        Some(id)
    }

    pub fn remove_rule(&mut self, id: u32) -> bool {
        remove(&mut self.rules, |rule| rule.id == id)
    }

    pub fn remove_flag(&mut self, id: u32) -> bool {
        remove(&mut self.flags, |flag| flag.id == id)
    }

    pub fn remove_zone(&mut self, id: u32) -> bool {
        remove(&mut self.zones, |zone| zone.id == id)
    }

    pub fn flag(&self, id: u32) -> Option<&Flag> {
        self.flags.iter().find(|flag| flag.id == id)
    }

    pub fn zone(&self, id: u32) -> Option<&Zone> {
        self.zones.iter().find(|zone| zone.id == id)
    }

    pub(crate) fn validate(&self) -> Result<(), ImportError> {
        self.extensions
            .validate()
            .map_err(|error| ImportError::new("Расширения правил", error))?;
        if self.rules.len() > MAX_RULES
            || self.flags.len() > MAX_FLAGS
            || self.zones.len() > MAX_ZONES
            || self.counters.len() > MAX_COUNTERS
            || self.timers.len() > MAX_TIMERS
            || !valid_object_ids(&self.initially_disabled_objects)
        {
            return Err(ImportError::new(
                "Правила сцены",
                "Превышен лимит элементов или неверно указан отключённый объект.",
            ));
        }
        let mut ids = HashSet::new();
        if let Some(rule) = self.rules.iter().find(|rule| {
            !valid_entry(rule.id, &rule.name, &mut ids)
                || rule.draft && rule.enabled
                || !valid_definition(rule.event, &rule.conditions, &rule.actions, rule.draft)
        }) {
            return Err(ImportError::new(
                format!("Правило №{}", rule.id),
                "Некорректное имя, номер, событие, условие или действие.",
            ));
        }
        ids.clear();
        if let Some(flag) = self
            .flags
            .iter()
            .find(|flag| !valid_entry(flag.id, &flag.name, &mut ids))
        {
            return Err(ImportError::new(format!("Флаг №{}", flag.id), "Некорректное имя или номер."));
        }
        ids.clear();
        if let Some(zone) = self.zones.iter().find(|zone| {
            !valid_entry(zone.id, &zone.name, &mut ids)
                || !valid_position(zone.position)
                || !zone
                    .size
                    .iter()
                    .all(|extent| (0.2..=128.0).contains(extent))
                || zone.building_id == Some(0)
        }) {
            return Err(ImportError::new(
                format!("Зона №{}", zone.id),
                "Некорректные размеры, положение, имя, номер или ссылка на здание.",
            ));
        }
        ids.clear();
        if let Some(counter) = self.counters.iter().find(|counter| {
            !valid_entry(counter.id, &counter.name, &mut ids) || !valid_counter(counter.initial)
        }) {
            return Err(ImportError::new(
                format!("Счётчик №{}", counter.id),
                "Некорректное имя, номер или значение.",
            ));
        }
        ids.clear();
        if let Some(timer) = self.timers.iter().find(|timer| {
            !valid_entry(timer.id, &timer.name, &mut ids)
                || !(0.05..=MAX_TIMER_SECONDS).contains(&timer.seconds)
        }) {
            return Err(ImportError::new(
                format!("Таймер №{}", timer.id),
                "Некорректное имя, номер или длительность.",
            ));
        }
        library::validate_templates(&self.templates)
            .map_err(|error| ImportError::new("Шаблоны механик", error))?;
        library::validate_rule_templates(&self.rule_templates)
            .map_err(|error| ImportError::new("Шаблоны правил", error))?;
        let calls = self.rules.iter().flat_map(|rule| &rule.actions)
            .chain(self.rule_templates.iter().flat_map(|rule| &rule.actions));
        validate_extensions(&self.extensions, calls.copied())
            .map_err(|error| ImportError::new("Расширения правил", error))
    }
}

impl Rule {
    pub(crate) fn authored_targets_exist(&self, scene: &crate::scene::Scene) -> bool {
        self.event.is_some() && self.missing_targets(scene, &scene.linked_scenes).is_empty()
    }

    pub(crate) fn missing_targets(
        &self,
        scene: &crate::scene::Scene,
        links: &crate::linked_scenes::LinkedScenes,
    ) -> Vec<(&'static str, u64)> {
        use crate::{
            effect_rules::Origin,
            simulation_rules::{Command, Signal},
        };

        let rules = &scene.rules;
        let world = &scene.world;
        let story = &scene.story;
        let zone_target = |id: u32| {
            rules.zone(id).map_or((false, "Зона", id.into()), |zone| {
                zone.building_id
                    .map_or((true, "Зона", id.into()), |building| {
                        (
                            world.buildings.iter().any(|entry| entry.id == building),
                            "Дом зоны",
                            building,
                        )
                    })
            })
        };
        let water = |id| {
            scene
                .environment
                .as_ref()?
                .waters
                .iter()
                .find(|entry| entry.id == id)
        };
        let mut targets = Vec::new();
        let mut check = |present: bool, kind: &'static str, id: u64| {
            if present || targets.contains(&(kind, id)) {
                return;
            }
            targets.push((kind, id));
        };
        if let Some(event) = self.event {
            match event {
                Event::Pickup { item_id } => {
                    check(world.key(item_id).is_some(), "Предмет", item_id.into())
                }
                Event::Dialogue {
                    npc_id,
                    node_id,
                    choice_id,
                } => {
                    let npc_exists = scene.npcs.iter().any(|npc| npc.id == npc_id);
                    check(npc_exists, "Собеседник", npc_id);
                    if npc_exists {
                        check(
                            story.answer(npc_id, node_id, choice_id).is_some(),
                            "Ответ диалога",
                            choice_id.into(),
                        );
                    }
                }
                Event::Door { door_id } => check(world.door(door_id).is_some(), "Дверь", door_id),
                Event::EnterZone { zone_id } => {
                    let (present, kind, id) = zone_target(zone_id);
                    check(present, kind, id);
                }
                Event::ActorEnteredZone { actor_id, zone_id } => {
                    check(
                        actor_id == PLAYER_ACTOR_ID
                            || scene.npcs.iter().any(|npc| npc.id == actor_id),
                        "Актёр",
                        actor_id,
                    );
                    let (present, kind, id) = zone_target(zone_id);
                    check(present, kind, id);
                }
                Event::GameAction { .. } => {}
                Event::TimerElapsed { timer_id } => {
                    check(rules.timer(timer_id).is_some(), "Таймер", timer_id.into())
                }
                Event::Simulation(Signal::Contact { sensor_id, .. }) => check(
                    scene
                        .physics
                        .sensors
                        .iter()
                        .any(|sensor| sensor.spec.id == sensor_id),
                    "Датчик",
                    sensor_id,
                ),
                Event::Simulation(Signal::Weather { .. }) => {
                    check(scene.environment.is_some(), "Окружение", 0)
                }
                Event::Simulation(Signal::WaterReached { zone }) => {
                    check(water(zone).is_some(), "Водная зона", zone.into())
                }
                Event::Simulation(Signal::RotorTurned) => {
                    check(scene.wind_rotor.is_some(), "Ротор", 0)
                }
            }
        }
        for condition in &self.conditions {
            match *condition {
                Condition::HasItem { item_id } => {
                    check(world.key(item_id).is_some(), "Предмет", item_id.into())
                }
                Condition::Flag { flag_id, .. } => {
                    check(rules.flag(flag_id).is_some(), "Флаг", flag_id.into())
                }
                Condition::Counter { counter_id, .. } => check(
                    rules.counter(counter_id).is_some(),
                    "Счётчик",
                    counter_id.into(),
                ),
                Condition::InZone { zone_id } => {
                    let (present, kind, id) = zone_target(zone_id);
                    check(present, kind, id);
                }
            }
        }
        for &action in &self.actions {
            let present = || action.target_exists(rules, world, story);
            match action {
                Action::SetFlag { flag_id, .. } => check(present(), "Флаг", flag_id.into()),
                Action::UnlockDoor { door_id } => check(present(), "Дверь", door_id),
                Action::SetQuestStage { quest_id, .. } => {
                    check(present(), "Задание", quest_id.into())
                }
                Action::SetCounter { counter_id, .. } | Action::AddCounter { counter_id, .. } => {
                    check(present(), "Счётчик", counter_id.into());
                }
                Action::StartTimer { timer_id } | Action::StopTimer { timer_id } => {
                    check(present(), "Таймер", timer_id.into());
                }
                Action::SetObjectEnabled { object_id, .. } => check(present(), "Объект", object_id),
                Action::ResetAttempt => {}
                Action::Transition { scene_id } => {
                    check(links.contains(scene_id), "Сцена", scene_id.into())
                }
                Action::Simulation(Command::Weather { .. }) => {
                    check(scene.environment.is_some(), "Окружение", 0)
                }
                Action::Simulation(Command::Water {
                    zone, height_cm, ..
                }) => match water(zone) {
                    Some(water) => check(
                        (water.floor_y..=water.max_level).contains(&(f32::from(height_cm) / 100.0)),
                        "Допустимый уровень воды в зоне",
                        zone.into(),
                    ),
                    None => check(false, "Водная зона", zone.into()),
                },
                Action::PlayEffect(command) => {
                    check(
                        scene.effects.preset(command.preset).is_some(),
                        "Эффект",
                        command.preset.into(),
                    );
                    if let Origin::Object(id) = command.origin {
                        check(
                            command.resolve(world, None).is_some(),
                            "Место эффекта, объект",
                            id,
                        );
                    }
                }
                Action::Extension(call) => {
                    check(
                        rules.counter(call.counter_id).is_some(),
                        "Счётчик",
                        call.counter_id.into(),
                    );
                    check(
                        rules.extensions.counter_action(call.id).is_some(),
                        "Действие расширения",
                        call.id,
                    );
                }
            }
        }
        targets
    }

    pub fn bindings_ready(&self) -> bool {
        self.event.is_some()
            && !self.actions.is_empty()
            && valid_definition(self.event, &self.conditions, &self.actions, false)
    }
}

fn validate_extensions(
    registry: &crate::extensions::Registry,
    actions: impl Iterator<Item = Action>,
) -> Result<(), String> {
    if actions.filter_map(Action::extension_id).any(|id| registry.counter_action(id).is_none()) {
        Err("Для действия правила не установлен пакет расширения.".into())
    } else {
        Ok(())
    }
}

fn valid_definition(
    event: Option<Event>,
    conditions: &[Condition],
    actions: &[Action],
    allow_unbound: bool,
) -> bool {
    let transitions = actions
        .iter()
        .filter(|action| matches!(action, Action::Transition { .. }))
        .count();
    conditions.len() <= MAX_CONDITIONS
        && actions.len() <= MAX_ACTIONS
        && (transitions == 0 || transitions == 1 && !actions.contains(&Action::ResetAttempt))
        && event.is_none_or(|event| event.valid_binding(allow_unbound))
        && conditions
            .iter()
            .all(|condition| condition.valid_binding(allow_unbound))
        && actions
            .iter()
            .all(|action| action.valid_binding(allow_unbound))
}

fn reserve(
    counter: &mut u32,
    ids: impl Iterator<Item = u32> + ExactSizeIterator,
    limit: usize,
) -> Option<u32> {
    if ids.len() >= limit {
        return None;
    }
    let id = ids.max().unwrap_or(0).checked_add(1)?.max(*counter);
    *counter = id.checked_add(1)?;
    Some(id)
}

fn remove<T>(entries: &mut Vec<T>, predicate: impl Fn(&T) -> bool) -> bool {
    let before = entries.len();
    entries.retain(|entry| !predicate(entry));
    entries.len() != before
}

fn valid_entry(id: u32, name: &str, ids: &mut HashSet<u32>) -> bool {
    valid_id(id)
        && ids.insert(id)
        && !name.trim().is_empty()
        && name.chars().count() <= 64
        && !name.chars().any(char::is_control)
}

fn valid_id(id: u32) -> bool {
    (1..u32::MAX).contains(&id)
}

fn binding_id(id: u32, allow_unbound: bool) -> bool {
    valid_id(id) || allow_unbound && id == 0
}

fn valid_counter(value: i32) -> bool {
    (-MAX_COUNTER_VALUE..=MAX_COUNTER_VALUE).contains(&value)
}

fn valid_object_ids(ids: &[u64]) -> bool {
    let mut seen = HashSet::new();
    ids.len() <= MAX_OBJECT_OVERRIDES && ids.iter().all(|&id| id != 0 && seen.insert(id))
}

fn valid_position([x, y, z]: [f32; 3]) -> bool {
    (-256.0..=256.0).contains(&x) && (-32.0..=128.0).contains(&y) && (-256.0..=256.0).contains(&z)
}

impl Zone {
    pub fn contains(&self, [x, y, z]: [f32; 3]) -> bool {
        (x - self.position[0]).abs() <= self.size[0] * 0.5
            && (self.position[1]..=self.position[1] + self.size[1]).contains(&y)
            && (z - self.position[2]).abs() <= self.size[2] * 0.5
    }

    pub(crate) fn accepts(&self, world: &World, position: [f32; 3]) -> bool {
        self.contains(position)
            && self.building_id.is_none_or(|id| {
                let [x, y, z] = position;
                world
                    .buildings
                    .iter()
                    .find(|building| building.id == id)
                    .is_some_and(|building| building.contains_interior(x, y, z))
            })
    }
}

impl Event {
    fn matches(self, emitted: Self) -> bool {
        match (self, emitted) {
            (
                Self::EnterZone { zone_id: legacy },
                Self::ActorEnteredZone {
                    actor_id: PLAYER_ACTOR_ID,
                    zone_id: authored,
                },
            ) | (
                Self::ActorEnteredZone {
                    actor_id: PLAYER_ACTOR_ID,
                    zone_id: authored,
                },
                Self::EnterZone { zone_id: legacy },
            ) => legacy == authored,
            _ => self == emitted,
        }
    }

    fn valid_binding(self, allow_unbound: bool) -> bool {
        match self {
            Self::Pickup { item_id } => binding_id(item_id, allow_unbound),
            Self::Dialogue {
                npc_id,
                node_id,
                choice_id,
            } => {
                npc_id != 0 && valid_id(node_id) && valid_id(choice_id)
                    || allow_unbound && npc_id == 0 && node_id == 0 && choice_id == 0
            }
            Self::Door { door_id } => door_id != 0 || allow_unbound,
            Self::EnterZone { zone_id } => binding_id(zone_id, allow_unbound),
            Self::ActorEnteredZone { actor_id, zone_id } => {
                actor_id != 0 && binding_id(zone_id, allow_unbound)
            }
            Self::GameAction { .. } => true,
            Self::TimerElapsed { timer_id } => binding_id(timer_id, allow_unbound),
            Self::Simulation(crate::simulation_rules::Signal::Contact { sensor_id, .. }) => {
                sensor_id != 0 || allow_unbound
            }
            Self::Simulation(crate::simulation_rules::Signal::WaterReached { zone }) => {
                binding_id(zone, allow_unbound)
            }
            Self::Simulation(_) => true,
        }
    }

    fn available(
        self,
        rules: &RuleSet,
        world: &World,
        story: &Story,
        state: &InteractionState,
    ) -> bool {
        match self {
            Self::Pickup { item_id } => world.key(item_id).is_some() && state.has_key(item_id),
            Self::Dialogue {
                npc_id,
                node_id,
                choice_id,
            } => story.answer(npc_id, node_id, choice_id).is_some(),
            Self::Door { door_id } => world.door(door_id).is_some(),
            Self::EnterZone { zone_id } => rules.zone(zone_id).is_some(),
            Self::ActorEnteredZone { actor_id, zone_id } => {
                actor_id != 0 && rules.zone(zone_id).is_some()
            }
            Self::GameAction { .. } => true,
            Self::TimerElapsed { timer_id } => rules.timer(timer_id).is_some(),
            Self::Simulation(_) => true,
        }
    }
}

impl Condition {
    fn valid_binding(self, allow_unbound: bool) -> bool {
        match self {
            Self::HasItem { item_id } => binding_id(item_id, allow_unbound),
            Self::Flag { flag_id, .. } => binding_id(flag_id, allow_unbound),
            Self::Counter {
                counter_id, value, ..
            } => binding_id(counter_id, allow_unbound) && valid_counter(value),
            Self::InZone { zone_id } => binding_id(zone_id, allow_unbound),
        }
    }
}

impl Action {
    fn extension_id(self) -> Option<u64> {
        match self {
            Self::Extension(call) => Some(call.id),
            _ => None,
        }
    }

    fn valid_id(self) -> bool {
        self.valid_binding(false)
    }

    fn valid_binding(self, allow_unbound: bool) -> bool {
        match self {
            Self::SetFlag { flag_id, .. } => binding_id(flag_id, allow_unbound),
            Self::UnlockDoor { door_id } => door_id != 0 || allow_unbound,
            Self::SetQuestStage { quest_id, stage } => {
                binding_id(quest_id, allow_unbound) && stage <= MAX_STAGE
            }
            Self::SetCounter { counter_id, value } => {
                binding_id(counter_id, allow_unbound) && valid_counter(value)
            }
            Self::AddCounter { counter_id, delta } => {
                binding_id(counter_id, allow_unbound) && valid_counter(delta)
            }
            Self::StartTimer { timer_id } | Self::StopTimer { timer_id } => {
                binding_id(timer_id, allow_unbound)
            }
            Self::SetObjectEnabled { object_id, .. } => object_id != 0 || allow_unbound,
            Self::ResetAttempt => true,
            Self::Transition { scene_id } => binding_id(scene_id, allow_unbound),
            Self::PlayEffect(command) => command.valid(allow_unbound),
            Self::Extension(call) => call.id != 0 && binding_id(call.counter_id, allow_unbound),
            Self::Simulation(crate::simulation_rules::Command::Weather {
                duration_tenths, ..
            }) => duration_tenths <= 6_000,
            Self::Simulation(crate::simulation_rules::Command::Water {
                zone,
                duration_tenths,
                ..
            }) => binding_id(zone, allow_unbound) && duration_tenths <= 6_000,
        }
    }

    fn target_exists(self, rules: &RuleSet, world: &World, story: &Story) -> bool {
        match self {
            Self::SetFlag { flag_id, .. } => rules.flag(flag_id).is_some(),
            Self::UnlockDoor { door_id } => world.door(door_id).is_some(),
            Self::SetQuestStage { quest_id, stage } => {
                stage <= MAX_STAGE && story.quest(quest_id).is_some()
            }
            Self::SetCounter { counter_id, .. } | Self::AddCounter { counter_id, .. } => {
                rules.counter(counter_id).is_some()
            }
            Self::StartTimer { timer_id } | Self::StopTimer { timer_id } => {
                rules.timer(timer_id).is_some()
            }
            Self::SetObjectEnabled { object_id, .. } => {
                world.objects.iter().any(|object| object.id == object_id)
                    || world.models.iter().any(|model| model.id == object_id)
            }
            Self::ResetAttempt => true,
            Self::Transition { scene_id } => valid_id(scene_id),
            Self::Simulation(_) | Self::PlayEffect(_) => false,
            Self::Extension(call) => {
                rules.counter(call.counter_id).is_some()
                    && rules.extensions.counter_action(call.id).is_some()
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    flags: Vec<(u32, bool)>,
    quest_stages: Vec<(u32, u16)>,
    fired: Vec<u32>,
    counters: Vec<(u32, i32)>,
    timers: Vec<(u32, f32)>,
    object_states: Vec<(u64, bool)>,
}

impl Snapshot {
    pub(crate) fn validate(&self) -> Result<(), ImportError> {
        let valid = |ids: Vec<u32>, limit: usize| {
            let mut seen = HashSet::new();
            ids.len() <= limit && ids.into_iter().all(|id| valid_id(id) && seen.insert(id))
        };
        if valid(self.flags.iter().map(|entry| entry.0).collect(), MAX_FLAGS)
            && valid(self.quest_stages.iter().map(|entry| entry.0).collect(), 64)
            && self.quest_stages.iter().all(|entry| entry.1 <= MAX_STAGE)
            && valid(self.fired.clone(), MAX_RULES)
            && valid(
                self.counters.iter().map(|entry| entry.0).collect(),
                MAX_COUNTERS,
            )
            && self.counters.iter().all(|entry| valid_counter(entry.1))
            && valid(
                self.timers.iter().map(|entry| entry.0).collect(),
                MAX_TIMERS,
            )
            && self
                .timers
                .iter()
                .all(|entry| entry.1 > 0.0 && entry.1 <= MAX_TIMER_SECONDS)
            && valid_object_ids(
                &self
                    .object_states
                    .iter()
                    .map(|entry| entry.0)
                    .collect::<Vec<_>>(),
            )
        {
            Ok(())
        } else {
            Err(ImportError::new(
                "Сохранение правил",
                "Некорректные флаги, этапы, счётчики, таймеры или состояния объектов.",
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trace {
    pub rule_id: u32,
    pub event: Event,
    pub outcome: TraceOutcome,
}

pub type TraceOutcome = engine::Outcome<Condition, Action>;

#[derive(Default)]
pub struct Session {
    flags: BTreeMap<u32, bool>,
    quest_stages: BTreeMap<u32, u16>,
    fired: BTreeSet<u32>,
    trace: VecDeque<Trace>,
    counters: BTreeMap<u32, i32>,
    timers: BTreeMap<u32, f32>,
    initially_disabled: BTreeSet<u64>,
    object_states: BTreeMap<u64, bool>,
    reset_requested: bool,
    transition_targets: BTreeSet<u32>,
    pending_transition: Option<u32>,
    simulation: crate::simulation_rules::Requests,
    effect_presets: BTreeSet<crate::effects::PresetId>,
    pending_effects: Vec<crate::effect_rules::Emission>,
}

impl Session {
    pub fn new(rules: &RuleSet) -> Self {
        Self {
            initially_disabled: rules.initially_disabled_objects.iter().copied().collect(),
            ..Self::default()
        }
    }

    pub fn counter(&self, rules: &RuleSet, id: u32) -> Option<i32> {
        let counter = rules.counter(id)?;
        Some(self.counters.get(&id).copied().unwrap_or(counter.initial))
    }

    pub fn timer_remaining(&self, id: u32) -> Option<f32> {
        self.timers.get(&id).copied()
    }

    pub fn object_enabled(&self, id: u64) -> bool {
        self.object_states
            .get(&id)
            .copied()
            .unwrap_or_else(|| !self.initially_disabled.contains(&id))
    }

    /// The host consumes a reset once, then restores physical actor and inventory state.
    pub fn take_reset_requested(&mut self) -> bool {
        std::mem::take(&mut self.reset_requested)
    }

    pub fn set_transition_targets(&mut self, ids: impl IntoIterator<Item = u32>) {
        self.transition_targets = ids.into_iter().filter(|&id| valid_id(id)).collect();
    }

    /// A transition is a frame command rather than checkpoint state.
    pub fn take_transition_request(&mut self) -> Option<u32> {
        self.pending_transition.take()
    }

    pub fn set_effects(&mut self, effects: &crate::effects::Library) {
        self.effect_presets = effects.presets.iter().map(|preset| preset.id).collect();
        self.pending_effects.clear();
    }

    pub fn take_effect_requests(&mut self) -> Vec<crate::effect_rules::Emission> {
        std::mem::take(&mut self.pending_effects)
    }

    pub fn set_environment(&mut self, environment: Option<&crate::environment::Environment>) {
        self.simulation.configure(environment);
    }

    pub fn take_simulation_commands(&mut self) -> Vec<crate::simulation_rules::Command> {
        self.simulation.take()
    }

    pub fn flag(&self, rules: &RuleSet, id: u32) -> Option<bool> {
        let flag = rules.flag(id)?;
        Some(self.flags.get(&id).copied().unwrap_or(flag.initially_set))
    }

    pub fn quest_stage(&self, id: u32) -> Option<u16> {
        self.quest_stages.get(&id).copied()
    }

    pub fn quest_stages(&self) -> impl Iterator<Item = (u32, u16)> + '_ {
        self.quest_stages.iter().map(|(&id, &stage)| (id, stage))
    }

    pub fn trace(&self) -> impl DoubleEndedIterator<Item = &Trace> + '_ {
        self.trace.iter()
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            flags: self.flags.iter().map(|(&id, &value)| (id, value)).collect(),
            quest_stages: self.quest_stages().collect(),
            fired: self.fired.iter().copied().collect(),
            counters: self
                .counters
                .iter()
                .map(|(&id, &value)| (id, value))
                .collect(),
            timers: self
                .timers
                .iter()
                .map(|(&id, &remaining)| (id, remaining))
                .collect(),
            object_states: self
                .object_states
                .iter()
                .map(|(&id, &value)| (id, value))
                .collect(),
        }
    }

    pub fn restore(&mut self, rules: &RuleSet, story: &Story, snapshot: &Snapshot) {
        self.flags = snapshot
            .flags
            .iter()
            .copied()
            .filter(|&(id, _)| rules.flag(id).is_some())
            .collect();
        self.quest_stages = snapshot
            .quest_stages
            .iter()
            .copied()
            .filter(|&(id, stage)| story.quest(id).is_some() && stage <= MAX_STAGE)
            .collect();
        self.fired = snapshot
            .fired
            .iter()
            .copied()
            .filter(|&id| rules.rules.iter().any(|rule| rule.id == id))
            .collect();
        self.counters = snapshot
            .counters
            .iter()
            .copied()
            .filter(|&(id, value)| rules.counter(id).is_some() && valid_counter(value))
            .collect();
        self.timers = snapshot
            .timers
            .iter()
            .copied()
            .filter(|&(id, remaining)| {
                rules.timer(id).is_some() && remaining > 0.0 && remaining <= MAX_TIMER_SECONDS
            })
            .collect();
        self.initially_disabled = rules.initially_disabled_objects.iter().copied().collect();
        self.object_states = snapshot.object_states.iter().copied().collect();
        self.reset_requested = false;
        self.pending_transition = None;
        self.pending_effects.clear();
        self.trace.clear();
    }

    /// Rules for one event are evaluated in editor order. Actions never emit a second event.
    #[cfg(test)]
    pub fn dispatch(
        &mut self,
        rules: &RuleSet,
        world: &World,
        story: &Story,
        interactions: &mut InteractionState,
        event: Event,
    ) -> usize {
        self.dispatch_event(rules, world, story, interactions, event, None)
    }

    pub fn dispatch_at(
        &mut self,
        rules: &RuleSet,
        world: &World,
        story: &Story,
        interactions: &mut InteractionState,
        event: Event,
        position: [f32; 3],
    ) -> usize {
        self.dispatch_event(rules, world, story, interactions, event, Some(position))
    }

    fn dispatch_event(
        &mut self,
        rules: &RuleSet,
        world: &World,
        story: &Story,
        interactions: &mut InteractionState,
        event: Event,
        position: Option<[f32; 3]>,
    ) -> usize {
        if self.reset_requested
            || self.pending_transition.is_some()
            || !event.available(rules, world, story, interactions)
        {
            return 0;
        }
        let matched = rules
            .rules
            .iter()
            .filter(|rule| {
                rule.enabled
                    && !rule.draft
                    && rule.event.is_some_and(|authored| authored.matches(event))
                    && rule.bindings_ready()
            })
            .map(|rule| engine::MatchedRule {
                id: rule.id,
                once: rule.once,
                conditions: &rule.conditions,
                actions: &rule.actions,
            });
        engine::dispatch(
            &mut game_host::GameRules {
                session: self,
                rules,
                world,
                story,
                interactions,
                event,
                position,
            },
            matched,
        )
    }
    pub fn move_player(
        &mut self,
        rules: &RuleSet,
        world: &World,
        story: &Story,
        interactions: &mut InteractionState,
        from: [f32; 3],
        to: [f32; 3],
    ) -> usize {
        self.move_actor(
            rules, world, story, interactions, PLAYER_ACTOR_ID, [from, to],
        )
    }

    fn move_actor(
        &mut self,
        rules: &RuleSet,
        world: &World,
        story: &Story,
        interactions: &mut InteractionState,
        actor_id: u64,
        [from, to]: [[f32; 3]; 2],
    ) -> usize {
        if actor_id == 0 || !from.into_iter().all(f32::is_finite) || !to.into_iter().all(f32::is_finite) {
            return 0;
        }
        rules
            .zones
            .iter()
            .filter(|zone| !zone.accepts(world, from) && zone.accepts(world, to))
            .map(|zone| {
                self.dispatch_at(
                    rules,
                    world,
                    story,
                    interactions,
                    Event::ActorEnteredZone {
                        actor_id,
                        zone_id: zone.id,
                    },
                    to,
                )
            })
            .sum()
    }

    pub fn advance_timers(
        &mut self,
        rules: &RuleSet,
        world: &World,
        story: &Story,
        interactions: &mut InteractionState,
        dt: f32,
        position: [f32; 3],
    ) -> usize {
        if !dt.is_finite() || dt <= 0.0 || self.timers.is_empty() {
            return 0;
        }
        let elapsed: Vec<_> = self
            .timers
            .iter_mut()
            .filter_map(|(&id, remaining)| {
                *remaining -= dt;
                (*remaining <= 0.000_01).then_some(id)
            })
            .collect();
        let mut fired = 0;
        for timer_id in elapsed {
            if self
                .timer_remaining(timer_id)
                .is_some_and(|remaining| remaining <= 0.000_01)
            {
                self.timers.remove(&timer_id);
                fired += self.dispatch_at(
                    rules,
                    world,
                    story,
                    interactions,
                    Event::TimerElapsed { timer_id },
                    position,
                );
            }
        }
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        story::{self, Objective},
        world::{DoorSettings, Interaction},
    };

    fn add_rule(
        set: &mut RuleSet,
        event: Event,
        conditions: Vec<Condition>,
        actions: Vec<Action>,
    ) -> u32 {
        let id = set.add_rule().unwrap();
        let rule = set.rules.last_mut().unwrap();
        rule.event = Some(event);
        rule.conditions = conditions;
        rule.actions = actions;
        id
    }

    #[test]
    fn a6_actor_event_runs_legacy_and_typed_player_rules_once_in_authored_order() {
        let mut rules = RuleSet::default();
        let zone_id = rules.add_zone([0.0; 3]).unwrap();
        let counter_id = rules.add_counter().unwrap();
        let legacy = Event::EnterZone { zone_id };
        let typed = Event::ActorEnteredZone {
            actor_id: PLAYER_ACTOR_ID,
            zone_id,
        };
        let order = [
            add_rule(&mut rules, legacy, vec![], vec![Action::SetCounter { counter_id, value: 5 }]),
            add_rule(&mut rules, typed, vec![], vec![Action::AddCounter { counter_id, delta: 2 }]),
            add_rule(&mut rules, legacy, vec![], vec![Action::AddCounter { counter_id, delta: 4 }]),
        ];
        let mut session = Session::new(&rules);
        let world = World::default();
        let story = Story::default();
        let mut interactions = InteractionState::default();
        let from = [3.0, 0.0, 0.0];
        let to = [0.0; 3];
        assert_eq!(
            session.move_player(&rules, &world, &story, &mut interactions, from, to),
            3
        );
        assert_eq!(session.counter(&rules, counter_id), Some(11));
        assert_eq!(
            session.trace().map(|trace| trace.rule_id).collect::<Vec<_>>(),
            order
        );
        assert_eq!(
            session.trace().map(|trace| trace.event).collect::<Vec<_>>(),
            [legacy, typed, legacy]
        );
        assert_eq!(
            session.move_player(&rules, &world, &story, &mut interactions, to, to),
            0
        );
    }

    #[test]
    fn a6_npc_zone_entry_never_dispatches_player_rules() {
        let mut rules = RuleSet::default();
        let zone_id = rules.add_zone([0.0; 3]).unwrap();
        let counter_id = rules.add_counter().unwrap();
        let npc = crate::npcs::FIRST_NPC_ID;
        let npc_event = Event::ActorEnteredZone { actor_id: npc, zone_id };
        add_rule(
            &mut rules,
            Event::EnterZone { zone_id },
            vec![],
            vec![Action::AddCounter { counter_id, delta: 100 }],
        );
        add_rule(
            &mut rules,
            npc_event,
            vec![],
            vec![Action::AddCounter { counter_id, delta: 2 }],
        );
        let mut session = Session::new(&rules);
        let world = World::default();
        let story = Story::default();
        let mut interactions = InteractionState::default();
        let crossing = [[3.0, 0.0, 0.0], [0.0; 3]];
        assert_eq!(
            session.move_actor(&rules, &world, &story, &mut interactions, npc + 1, crossing),
            0
        );
        assert_eq!(
            session.move_actor(&rules, &world, &story, &mut interactions, npc, crossing),
            1
        );
        assert_eq!(session.counter(&rules, counter_id), Some(2));
        assert_eq!(session.trace().next().unwrap().event, npc_event);
    }

    #[test]
    fn a6_zone_events_roundtrip_unchanged_in_library_json() {
        use serde_json::{from_value, json, to_value};

        for (wire, event) in [
            (
                json!({"EnterZone": {"zone_id": 41}}),
                Event::EnterZone { zone_id: 41 },
            ),
            (
                json!({"ActorEnteredZone": {"actor_id": 1, "zone_id": 41}}),
                Event::ActorEnteredZone { actor_id: PLAYER_ACTOR_ID, zone_id: 41 },
            ),
        ] {
            assert_eq!(from_value::<Event>(wire.clone()).unwrap(), event);
            assert_eq!(to_value(event).unwrap(), wire);
        }
    }

    #[test]
    fn event_chain_requires_exact_answer_item_and_door_before_finishing_room_stage() {
        let mut world = World::default();
        let house = world.add_building_at([0.0; 3]).unwrap();
        world.add_building_at([8.0, 0.0, 0.0]).unwrap();
        let item_pos = [3.6, 0.0, 2.9];
        let item_id = world.add_key_at(item_pos).unwrap();
        for building in &mut world.buildings {
            building.door = Some(DoorSettings {
                rule_lock: true,
                required_key: Some(item_id),
                ..DoorSettings::default()
            });
        }
        let door = world.buildings[0].doors().next().unwrap();
        let other = world.buildings[1].doors().next().unwrap();
        let mut story = Story::demo(21, [0.0; 2]);
        story.quests[0].objective = Objective::Rules {
            completion_stage: 4,
        };
        let quest_id = story.quests[0].id;
        let node = &story.conversations[0].nodes[0];
        let accepted = Event::Dialogue {
            npc_id: 21,
            node_id: node.id,
            choice_id: node.choices[0].id,
        };
        let wrong_answer = Event::Dialogue {
            npc_id: 21,
            node_id: node.id,
            choice_id: node.choices[1].id,
        };
        let mut rules = RuleSet::default();
        let accepted_flag = rules.add_flag().unwrap();
        let access_flag = rules.add_flag().unwrap();
        let zone_id = rules.add_zone([0.0, 0.0, 2.0]).unwrap();
        rules.zones[0].size = [3.0, 6.0, 3.0];
        rules.zones[0].building_id = Some(house);
        let stage = |stage| Action::SetQuestStage { quest_id, stage };
        let flag = |flag_id| Action::SetFlag {
            flag_id,
            value: true,
        };
        let is_set = |flag_id| Condition::Flag {
            flag_id,
            value: true,
        };
        add_rule(
            &mut rules,
            accepted,
            vec![],
            vec![flag(accepted_flag), stage(1)],
        );
        add_rule(
            &mut rules,
            Event::Pickup { item_id },
            vec![is_set(accepted_flag)],
            vec![stage(2)],
        );
        let door_event = Event::Door { door_id: door.id };
        add_rule(
            &mut rules,
            door_event,
            vec![is_set(accepted_flag), Condition::HasItem { item_id }],
            vec![
                Action::UnlockDoor { door_id: door.id },
                flag(access_flag),
                stage(3),
            ],
        );
        add_rule(
            &mut rules,
            Event::EnterZone { zone_id },
            vec![is_set(access_flag)],
            vec![stage(4)],
        );
        rules.validate().unwrap();
        story.validate().unwrap();
        world.validate().unwrap();
        let mut session = Session::default();
        let mut dialogue = story::Session::default();
        let mut interactions = InteractionState::default();
        let dispatch = |session: &mut Session, interactions: &mut InteractionState, event| {
            session.dispatch(&rules, &world, &story, interactions, event)
        };
        let outside = [0.0, 0.0, 2.65];
        let inside = [0.0, 0.0, 1.93];
        assert!(dialogue.open(&story, 21));
        assert_eq!(dialogue.choose_event(&story, 1), Some(wrong_answer));
        assert_eq!(dispatch(&mut session, &mut interactions, wrong_answer), 0);
        assert!(dialogue.open(&story, 21));
        assert_eq!(dialogue.choose_event(&story, 0), Some(accepted));
        assert_eq!(dialogue.objective(&story), None);
        assert_eq!(dispatch(&mut session, &mut interactions, accepted), 1);
        assert_eq!(session.quest_stage(quest_id), Some(1));
        assert_eq!(dispatch(&mut session, &mut interactions, door_event), 0);
        assert_eq!(
            session.trace().next_back().unwrap().outcome,
            TraceOutcome::Blocked(Condition::HasItem { item_id })
        );
        assert_eq!(
            world.interact(&mut interactions, outside),
            Some(Interaction::RuleLockedDoor { id: door.id })
        );
        assert_eq!(
            world.interact(&mut interactions, item_pos),
            Some(Interaction::Key { id: item_id })
        );
        assert_eq!(
            dispatch(&mut session, &mut interactions, Event::Pickup { item_id }),
            1
        );
        assert_eq!(session.quest_stage(quest_id), Some(2));
        assert_eq!(
            world.interact(&mut interactions, outside),
            Some(Interaction::RuleLockedDoor { id: door.id })
        );
        assert_eq!(
            dispatch(
                &mut session,
                &mut interactions,
                Event::Door { door_id: other.id }
            ),
            0
        );
        assert_eq!(dispatch(&mut session, &mut interactions, door_event), 1);
        assert_eq!(session.quest_stage(quest_id), Some(3));
        assert!(interactions.locked(&other));
        assert_eq!(interactions.fraction(&door), 0.0);
        assert!(
            matches!(world.interact(&mut interactions, outside),Some(Interaction::Door {id,opening:true}) if id == door.id)
        );
        for _ in 0..12 {
            world.advance_interactions(&mut interactions, 0.05, outside);
        }

        assert!(rules.zone(zone_id).unwrap().contains(outside));
        let move_player = |session: &mut Session, interactions: &mut InteractionState, from, to| {
            session.move_player(&rules, &world, &story, interactions, from, to)
        };
        assert_eq!(
            move_player(&mut session, &mut interactions, outside, outside),
            0
        );
        assert_eq!(
            move_player(
                &mut session,
                &mut interactions,
                [0.0, 4.0, 2.65],
                [0.0, 4.0, 1.93]
            ),
            0
        );
        assert_eq!(
            move_player(&mut session, &mut interactions, outside, inside),
            1
        );
        assert_eq!(session.quest_stage(quest_id), Some(4));

        let state: Snapshot =
            serde_json::from_slice(&serde_json::to_vec(&session.snapshot()).unwrap()).unwrap();
        state.validate().unwrap();
        let door_state =
            serde_json::from_slice(&serde_json::to_vec(&interactions.snapshot()).unwrap()).unwrap();
        let mut resumed = Session::default();
        let mut resumed_doors = InteractionState::default();
        resumed.restore(&rules, &story, &state);
        resumed_doors.restore(&world, &door_state);
        assert_eq!(resumed.quest_stage(quest_id), Some(4));
        assert_eq!(resumed.flag(&rules, access_flag), Some(true));
        assert!(!resumed_doors.locked(&door) && resumed_doors.locked(&other));
        assert_eq!(resumed_doors.fraction(&door), 1.0);
        assert_eq!(dispatch(&mut resumed, &mut resumed_doors, door_event), 0);
        assert_eq!(
            resumed.trace().next_back().unwrap().outcome,
            TraceOutcome::AlreadyFired
        );
        assert_eq!(resumed.quest_stage(quest_id), Some(4));
    }

    #[test]
    fn dangling_targets_do_not_change_flags_or_rebind_on_removal() {
        let world = World::default();
        let mut story = Story::demo(12, [0.0; 2]);
        let node = &story.conversations[0].nodes[0];
        let event = Event::Dialogue {
            npc_id: 12,
            node_id: node.id,
            choice_id: node.choices[0].id,
        };
        let mut rules = RuleSet::default();
        let flag_id = rules.add_flag().unwrap();
        let missing = Action::UnlockDoor { door_id: 123 };
        add_rule(
            &mut rules,
            event,
            vec![],
            vec![
                Action::SetFlag {
                    flag_id,
                    value: true,
                },
                missing,
            ],
        );
        let mut session = Session::default();
        assert_eq!(
            session.dispatch(
                &rules,
                &world,
                &story,
                &mut InteractionState::default(),
                event
            ),
            0
        );
        assert_eq!(session.flag(&rules, flag_id), Some(false));
        assert_eq!(
            session.trace().next_back().unwrap().outcome,
            TraceOutcome::MissingActionTarget(missing)
        );
        story.conversations[0].nodes[0].choices.remove(0);
        let mut dialogue = story::Session::default();
        assert!(dialogue.open(&story, 12));
        assert_ne!(dialogue.choose_event(&story, 0), Some(event));
        assert!(rules.remove_flag(flag_id));
        assert!(rules.add_flag().unwrap() > flag_id);
    }

    #[test]
    fn placed_model_rule_target_stays_enabled_after_restore() {
        let mut world = World::default();
        let id = crate::assets::Placement::next_id(&world.models);
        world.models.push(crate::assets::Placement {
            id,
            label: "Маяк".into(),
            source: crate::assets::Source::CityStreet,
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
            solid: false,
            material: Default::default(),
            material_graph: None,
            surface: None,
            paw_ground: false,
            light: None,
            collider: None,
            visual_offset: [0.0; 3],
            draw_distance: None,
            seat: None,
        });
        let story = Story::default();
        let mut rules = RuleSet::default();
        rules.initially_disabled_objects.push(id);
        let event = Event::GameAction {
            action: GameAction::Mark,
        };
        add_rule(
            &mut rules,
            event,
            vec![],
            vec![Action::SetObjectEnabled {
                object_id: id,
                enabled: true,
            }],
        );
        let mut session = Session::new(&rules);
        assert!(!session.object_enabled(id));
        assert_eq!(
            session.dispatch(
                &rules,
                &world,
                &story,
                &mut InteractionState::default(),
                event
            ),
            1,
        );
        assert!(session.object_enabled(id));
        let mut restored = Session::new(&rules);
        restored.restore(&rules, &story, &session.snapshot());
        assert!(restored.object_enabled(id));
    }

    #[test]
    fn elapsed_timer_survives_checkpoint_and_reset_is_a_single_host_request() {
        let world = World::demo();
        let story = Story::default();
        let object = world.objects[0].id;
        let mut rules = RuleSet::default();
        let counter = rules.add_counter().unwrap();
        let timer = rules.add_timer().unwrap();
        rules.timers[0].seconds = 0.10;
        add_rule(
            &mut rules,
            Event::GameAction {
                action: GameAction::Mark,
            },
            vec![],
            vec![
                Action::SetCounter {
                    counter_id: counter,
                    value: 1,
                },
                Action::StartTimer { timer_id: timer },
            ],
        );
        add_rule(
            &mut rules,
            Event::TimerElapsed { timer_id: timer },
            vec![],
            vec![
                Action::AddCounter {
                    counter_id: counter,
                    delta: 2,
                },
                Action::SetObjectEnabled {
                    object_id: object,
                    enabled: false,
                },
            ],
        );
        add_rule(
            &mut rules,
            Event::GameAction {
                action: GameAction::Retry,
            },
            vec![],
            vec![Action::ResetAttempt],
        );
        rules.rules.last_mut().unwrap().once = false;
        rules.validate().unwrap();
        let mut doors = InteractionState::default();
        let mut session = Session::new(&rules);
        assert_eq!(
            session.dispatch(
                &rules,
                &world,
                &story,
                &mut doors,
                Event::GameAction {
                    action: GameAction::Mark
                }
            ),
            1
        );
        assert_eq!(
            session.advance_timers(&rules, &world, &story, &mut doors, 0.04, [0.0; 3]),
            0
        );
        let snapshot = session.snapshot();
        snapshot.validate().unwrap();
        let mut resumed = Session::default();
        resumed.restore(&rules, &story, &snapshot);
        assert!((resumed.timer_remaining(timer).unwrap() - 0.06).abs() < 0.000_1);
        assert_eq!(
            resumed.advance_timers(&rules, &world, &story, &mut doors, 0.07, [0.0; 3]),
            1
        );
        assert_eq!(
            resumed.advance_timers(&rules, &world, &story, &mut doors, 0.07, [0.0; 3]),
            0
        );
        assert_eq!(resumed.counter(&rules, counter), Some(3));
        assert!(!resumed.object_enabled(object));
        assert!(!resumed.take_reset_requested());
        assert_eq!(
            resumed.dispatch(
                &rules,
                &world,
                &story,
                &mut doors,
                Event::GameAction {
                    action: GameAction::Retry
                }
            ),
            1
        );
        assert!(resumed.take_reset_requested());
        assert!(!resumed.take_reset_requested());
        assert_eq!(resumed.counter(&rules, counter), Some(0));
        assert!(resumed.object_enabled(object));
        assert_eq!(resumed.trace().count(), 2);
    }
}
