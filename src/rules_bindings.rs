use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{
    effect_rules::{self, Origin},
    effects,
    linked_scenes::LinkedScenes,
};

use super::{Action, Condition, Event, RuleSet, RuleTemplate, Story, World};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Port {
    Input,
    Parameter(u8),
    Output(u8),
    Anchor(u8),
}

impl Port {
    pub fn caption(self) -> String {
        match self {
            Self::Input => "Вход".into(),
            Self::Parameter(index) => format!("Параметр {}", usize::from(index) + 1),
            Self::Output(index) => format!("Выход {}", usize::from(index) + 1),
            Self::Anchor(index) => format!("Источник эффекта {}", usize::from(index) + 1),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortType {
    Item,
    Answer,
    Door,
    Zone,
    Timer,
    Counter,
    Flag,
    Quest,
    Object,
    Scene,
    Effect,
    EffectAnchor,
}

impl PortType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Item => "Предмет",
            Self::Answer => "Ответ персонажа",
            Self::Door => "Дверь",
            Self::Zone => "Зона",
            Self::Timer => "Таймер",
            Self::Counter => "Счётчик",
            Self::Flag => "Флаг",
            Self::Quest => "Задание",
            Self::Object => "Объект сцены",
            Self::Scene => "Сцена",
            Self::Effect => "Эффект",
            Self::EffectAnchor => "Объект для эффекта",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortDescription {
    pub port: Port,
    pub kind: PortType,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub port: Port,
    pub value: BindingValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingValue {
    Item(u32),
    Answer {
        npc_id: u64,
        node_id: u32,
        choice_id: u32,
    },
    Door(u64),
    Zone(u32),
    Timer(u32),
    Counter(u32),
    Flag(u32),
    Quest(u32),
    Object(u64),
    Scene(u32),
    Effect(effects::PresetId),
    EffectAnchor(u64),
}

impl BindingValue {
    pub fn kind(self) -> PortType {
        match self {
            Self::Item(_) => PortType::Item,
            Self::Answer { .. } => PortType::Answer,
            Self::Door(_) => PortType::Door,
            Self::Zone(_) => PortType::Zone,
            Self::Timer(_) => PortType::Timer,
            Self::Counter(_) => PortType::Counter,
            Self::Flag(_) => PortType::Flag,
            Self::Quest(_) => PortType::Quest,
            Self::Object(_) => PortType::Object,
            Self::Scene(_) => PortType::Scene,
            Self::Effect(_) => PortType::Effect,
            Self::EffectAnchor(_) => PortType::EffectAnchor,
        }
    }

    pub(super) fn exists(self, rules: &RuleSet, world: &World, story: &Story) -> bool {
        match self {
            Self::Item(id) => world.key(id).is_some(),
            Self::Answer {
                npc_id,
                node_id,
                choice_id,
            } => story.answer(npc_id, node_id, choice_id).is_some(),
            Self::Door(id) => world.door(id).is_some(),
            Self::Zone(id) => rules.zone(id).is_some_and(|zone| {
                zone.building_id
                    .is_none_or(|building| world.buildings.iter().any(|house| house.id == building))
            }),
            Self::Timer(id) => rules.timer(id).is_some(),
            Self::Counter(id) => rules.counter(id).is_some(),
            Self::Flag(id) => rules.flag(id).is_some(),
            Self::Quest(id) => story.quest(id).is_some(),
            Self::Object(id) => world.objects.iter().any(|object| object.id == id),
            Self::Scene(id) => super::valid_id(id),
            Self::Effect(_) | Self::EffectAnchor(_) => false,
        }
    }
}

impl RuleTemplate {
    pub fn ports(&self) -> Vec<PortDescription> {
        self.port_types()
            .into_iter()
            .map(|(port, kind)| {
                let name = self
                    .port_names
                    .iter()
                    .find(|(key, _)| *key == port)
                    .map_or_else(|| kind.name().into(), |(_, name)| name.clone());
                PortDescription { port, kind, name }
            })
            .collect()
    }

    pub fn name_port(&mut self, port: Port, name: String) -> bool {
        if !valid_name(&name) || !self.port_types().iter().any(|(key, _)| *key == port) {
            return false;
        }
        if let Some((_, existing)) = self.port_names.iter_mut().find(|(key, _)| *key == port) {
            *existing = name;
        } else {
            self.port_names.push((port, name));
        }
        true
    }

    pub(super) fn valid_port_names(&self) -> bool {
        let mut seen = HashSet::new();
        let valid = self.port_types();
        self.port_names.len() <= valid.len()
            && self.port_names.iter().all(|(port, name)| {
                valid_name(name) && seen.insert(port) && valid.iter().any(|(key, _)| key == port)
            })
    }

    fn port_types(&self) -> Vec<(Port, PortType)> {
        self.event
            .and_then(event_type)
            .map(|kind| (Port::Input, kind))
            .into_iter()
            .chain(
                self.conditions
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(index, condition)| {
                        (Port::Parameter(index as u8), condition_type(condition))
                    }),
            )
            .chain(
                self.actions
                    .iter()
                    .copied()
                    .enumerate()
                    .flat_map(|(index, action)| {
                        let index = index as u8;
                        let output = action_type(action).map(|kind| (Port::Output(index), kind));
                        let anchor = matches!(
                            action,
                            Action::PlayEffect(effect_rules::Command {
                                origin: Origin::Object(_),
                                ..
                            })
                        )
                        .then_some((Port::Anchor(index), PortType::EffectAnchor));
                        [output, anchor].into_iter().flatten()
                    }),
            )
            .collect()
    }
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control)
}

fn event_type(event: Event) -> Option<PortType> {
    match event {
        Event::Pickup { .. } => Some(PortType::Item),
        Event::Dialogue { .. } => Some(PortType::Answer),
        Event::Door { .. } => Some(PortType::Door),
        Event::EnterZone { .. } | Event::ActorEnteredZone { .. } => Some(PortType::Zone),
        Event::TimerElapsed { .. } => Some(PortType::Timer),
        Event::GameAction { .. } => None,
        Event::Simulation(_) => None,
    }
}

fn condition_type(condition: Condition) -> PortType {
    match condition {
        Condition::HasItem { .. } => PortType::Item,
        Condition::Flag { .. } => PortType::Flag,
        Condition::Counter { .. } => PortType::Counter,
        Condition::InZone { .. } => PortType::Zone,
    }
}

fn action_type(action: Action) -> Option<PortType> {
    match action {
        Action::SetFlag { .. } => Some(PortType::Flag),
        Action::UnlockDoor { .. } => Some(PortType::Door),
        Action::SetQuestStage { .. } => Some(PortType::Quest),
        Action::SetCounter { .. } | Action::AddCounter { .. } | Action::Extension(_) => Some(PortType::Counter),
        Action::StartTimer { .. } | Action::StopTimer { .. } => Some(PortType::Timer),
        Action::SetObjectEnabled { .. } => Some(PortType::Object),
        Action::ResetAttempt => None,
        Action::Transition { .. } => Some(PortType::Scene),
        Action::Simulation(_) => None,
        Action::PlayEffect(_) => Some(PortType::Effect),
    }
}

impl RuleSet {
    pub fn apply_bound_rule(
        &mut self,
        template_id: u32,
        bindings: &[Binding],
        world: &World,
        story: &Story,
        scenes: &LinkedScenes,
        effects: &effects::Library,
    ) -> Result<u32, String> {
        let template = self
            .rule_templates
            .iter()
            .find(|entry| entry.id == template_id)
            .ok_or("Выбери сохранённое правило.")?;
        let ports = template.ports();
        let mut used = HashSet::new();
        for binding in bindings {
            let descriptor = ports
                .iter()
                .find(|port| port.port == binding.port)
                .ok_or_else(|| format!("Лишняя привязка «{}».", binding.port.caption()))?;
            if !used.insert(binding.port) {
                return Err(format!("Дублируется привязка «{}».", descriptor.name));
            }
            if binding.value.kind() != descriptor.kind {
                return Err(format!(
                    "Для «{}» нужен тип «{}».",
                    descriptor.name,
                    descriptor.kind.name()
                ));
            }
            let destination_exists = match binding.value {
                BindingValue::Scene(id) => scenes.contains(id),
                BindingValue::Effect(id) => effects.preset(id).is_some(),
                BindingValue::EffectAnchor(id) => {
                    effect_rules::anchors(world).any(|(anchor, _, _)| id == anchor)
                }
                value => value.exists(self, world, story),
            };
            if !destination_exists {
                return Err(format!(
                    "Объект для «{}» отсутствует в сцене.",
                    descriptor.name
                ));
            }
        }
        if let Some(missing) = ports.iter().find(|port| !used.contains(&port.port)) {
            return Err(format!("Назначь привязку «{}».", missing.name));
        }

        // Resolve a temporary definition before adding an editor rule.
        let mut event = template.event;
        let mut conditions = template.conditions.clone();
        let mut actions = template.actions.clone();
        for binding in bindings {
            match binding.port {
                Port::Input => event = event.and_then(|event| bind_event(event, binding.value)),
                Port::Parameter(index) => {
                    let condition = &mut conditions[usize::from(index)];
                    *condition = bind_condition(*condition, binding.value)
                        .ok_or("Несовместимое условие пакета.")?;
                }
                Port::Output(index) => {
                    let action = &mut actions[usize::from(index)];
                    *action = bind_action(*action, binding.value)
                        .ok_or("Несовместимое действие пакета.")?;
                }
                Port::Anchor(index) => {
                    let action = &mut actions[usize::from(index)];
                    let (Action::PlayEffect(command), BindingValue::EffectAnchor(id)) =
                        (action, binding.value)
                    else {
                        return Err("Несовместимый источник эффекта.".into());
                    };
                    command.origin = Origin::Object(id);
                }
            }
        }
        if !super::valid_definition(event, &conditions, &actions, false)
            || template.event.is_some() != event.is_some()
        {
            return Err("Связанные данные правила некорректны.".into());
        }
        super::validate_extensions(&self.extensions, actions.iter().copied())?;
        let id = self.apply_rule_template(template_id)?;
        let rule = self.rules.last_mut().unwrap();
        rule.event = event;
        rule.conditions = conditions;
        rule.actions = actions;
        Ok(id)
    }
}

fn bind_event(event: Event, value: BindingValue) -> Option<Event> {
    Some(match (event, value) {
        (Event::Pickup { .. }, BindingValue::Item(item_id)) => Event::Pickup { item_id },
        (
            Event::Dialogue { .. },
            BindingValue::Answer {
                npc_id,
                node_id,
                choice_id,
            },
        ) => Event::Dialogue {
            npc_id,
            node_id,
            choice_id,
        },
        (Event::Door { .. }, BindingValue::Door(door_id)) => Event::Door { door_id },
        (Event::EnterZone { .. }, BindingValue::Zone(zone_id)) => Event::EnterZone { zone_id },
        (Event::ActorEnteredZone { actor_id, .. }, BindingValue::Zone(zone_id)) => {
            Event::ActorEnteredZone { actor_id, zone_id }
        }
        (Event::TimerElapsed { .. }, BindingValue::Timer(timer_id)) => {
            Event::TimerElapsed { timer_id }
        }
        _ => return None,
    })
}

fn bind_condition(condition: Condition, value: BindingValue) -> Option<Condition> {
    Some(match (condition, value) {
        (Condition::HasItem { .. }, BindingValue::Item(item_id)) => Condition::HasItem { item_id },
        (Condition::Flag { value, .. }, BindingValue::Flag(flag_id)) => {
            Condition::Flag { flag_id, value }
        }
        (
            Condition::Counter {
                comparison, value, ..
            },
            BindingValue::Counter(counter_id),
        ) => Condition::Counter {
            counter_id,
            comparison,
            value,
        },
        (Condition::InZone { .. }, BindingValue::Zone(zone_id)) => Condition::InZone { zone_id },
        _ => return None,
    })
}

fn bind_action(action: Action, value: BindingValue) -> Option<Action> {
    Some(match (action, value) {
        (Action::SetFlag { value, .. }, BindingValue::Flag(flag_id)) => {
            Action::SetFlag { flag_id, value }
        }
        (Action::UnlockDoor { .. }, BindingValue::Door(door_id)) => Action::UnlockDoor { door_id },
        (Action::SetQuestStage { stage, .. }, BindingValue::Quest(quest_id)) => {
            Action::SetQuestStage { quest_id, stage }
        }
        (Action::SetCounter { value, .. }, BindingValue::Counter(counter_id)) => {
            Action::SetCounter { counter_id, value }
        }
        (Action::AddCounter { delta, .. }, BindingValue::Counter(counter_id)) => {
            Action::AddCounter { counter_id, delta }
        }
        (Action::Extension(call), BindingValue::Counter(counter_id)) => {
            Action::Extension(crate::extensions::Call { counter_id, ..call })
        }
        (Action::StartTimer { .. }, BindingValue::Timer(timer_id)) => {
            Action::StartTimer { timer_id }
        }
        (Action::StopTimer { .. }, BindingValue::Timer(timer_id)) => Action::StopTimer { timer_id },
        (Action::SetObjectEnabled { enabled, .. }, BindingValue::Object(object_id)) => {
            Action::SetObjectEnabled { object_id, enabled }
        }
        (Action::PlayEffect(command), BindingValue::Effect(preset)) => {
            Action::PlayEffect(effect_rules::Command { preset, ..command })
        }
        (Action::Transition { .. }, BindingValue::Scene(scene_id)) => {
            Action::Transition { scene_id }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Comparison;

    #[test]
    fn named_package_checks_type_presence_and_uniqueness_before_inserting_rule() {
        let mut rules = RuleSet::default();
        let zone = rules.add_zone([0.0; 3]).unwrap();
        let counter = rules.add_counter().unwrap();
        let rule = rules.add_rule().unwrap();
        rules.rules[0].event = Some(Event::EnterZone { zone_id: zone });
        rules.rules[0].conditions.push(Condition::Counter {
            counter_id: counter,
            comparison: Comparison::AtLeast,
            value: 2,
        });
        rules.rules[0].actions.push(Action::AddCounter {
            counter_id: counter,
            delta: 5,
        });
        let template = rules.save_rule_template(rule).unwrap();
        assert!(rules.rule_templates[0].name_port(Port::Input, "Вход во двор".into()));
        assert!(!rules.rule_templates[0].name_port(Port::Output(7), "Неизвестный выход".into()));
        let bytes = serde_json::to_vec(&rules).unwrap();
        let mut rules: RuleSet = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(rules.rule_templates[0].ports()[0].name, "Вход во двор");

        let world = World::default();
        let story = Story::default();
        let scenes = LinkedScenes::default();
        let effects = effects::Library::default();
        let input = Binding {
            port: Port::Input,
            value: BindingValue::Zone(zone),
        };
        let parameter = Binding {
            port: Port::Parameter(0),
            value: BindingValue::Counter(counter),
        };
        let output = Binding {
            port: Port::Output(0),
            value: BindingValue::Counter(counter),
        };
        let wrong_type = Binding {
            value: BindingValue::Counter(counter),
            ..input
        };
        let removed = Binding {
            value: BindingValue::Zone(zone + 1),
            ..input
        };
        for bindings in [
            vec![input],
            vec![input, input, parameter, output],
            vec![wrong_type, parameter, output],
            vec![removed, parameter, output],
        ] {
            assert!(
                rules
                    .apply_bound_rule(template, &bindings, &world, &story, &scenes, &effects)
                    .is_err()
            );
            assert_eq!(rules.rules.len(), 1);
        }
        rules
            .apply_bound_rule(
                template,
                &[input, parameter, output],
                &world,
                &story,
                &scenes,
                &effects,
            )
            .unwrap();
        let bound = rules.rules.last().unwrap();
        assert!(bound.draft && !bound.enabled && bound.bindings_ready());
        assert_eq!(bound.event, Some(Event::EnterZone { zone_id: zone }));
        assert_eq!(
            bound.actions,
            [Action::AddCounter {
                counter_id: counter,
                delta: 5
            }]
        );
        rules.validate().unwrap();
    }

    #[test]
    fn named_effect_and_anchor_outputs_never_inherit_source_ids() {
        use std::fs;

        use crate::{
            effect_rules::{Command, Emission},
            effects::Kind,
            rules::{GameAction, Session},
            world::{InteractionState, WorldKind},
        };

        let mut source = RuleSet::default();
        let mut source_effects = effects::Library::default();
        let mut source_world = World::default();
        let source_id = source_effects.create(Kind::Smoke).unwrap();
        let source_anchor = source_world.add(WorldKind::Platform).unwrap();
        let rule = source.add_rule().unwrap();
        let event = Event::GameAction {
            action: GameAction::Mark,
        };
        source.rules[0].event = Some(event);
        source.rules[0].actions = vec![Action::PlayEffect(Command {
            preset: source_id,
            origin: Origin::Object(source_anchor),
        })];
        source.save_rule_template(rule).unwrap();
        assert!(source.rule_templates[0].name_port(Port::Output(0), "Дым маяка".into()));
        assert!(source.rule_templates[0].name_port(Port::Anchor(0), "Метка маяка".into()));
        let file = std::env::temp_dir().join(format!(
            "studio-effect-rule-{}.rules.json",
            std::process::id()
        ));
        source.save_library(&file).unwrap();

        let mut effects = effects::Library::default();
        let mut world = World::default();
        let mut target = RuleSet::default();
        assert_eq!(effects.create(Kind::Smoke), Some(source_id));
        assert_eq!(world.add(WorldKind::Platform), Some(source_anchor));
        let preset = effects.create(Kind::Smoke).unwrap();
        let anchor = world.add(WorldKind::Platform).unwrap();
        world.objects.last_mut().unwrap().position = [6.0, 0.4, -1.0];
        assert_eq!(target.load_library(&file).unwrap(), 1);
        fs::remove_file(file).unwrap();
        let template = &target.rule_templates[0];
        let template_id = template.id;
        assert_eq!(
            template.actions,
            [Action::PlayEffect(Command {
                preset: 0,
                origin: Origin::Object(0),
            })]
        );
        let ports = template.ports();
        assert_eq!(
            ports
                .iter()
                .map(|port| port.name.as_str())
                .collect::<Vec<_>>(),
            ["Дым маяка", "Метка маяка"]
        );
        let story = Story::default();
        let linked = LinkedScenes::default();
        let preset_port = Binding {
            port: Port::Output(0),
            value: BindingValue::Effect(preset),
        };
        let anchor_port = Binding {
            port: Port::Anchor(0),
            value: BindingValue::EffectAnchor(anchor),
        };
        assert!(
            target
                .apply_bound_rule(
                    template_id,
                    &[preset_port],
                    &world,
                    &story,
                    &linked,
                    &effects
                )
                .is_err()
        );
        let absent_preset = Binding {
            value: BindingValue::Effect(preset + 100),
            ..preset_port
        };
        assert!(
            target
                .apply_bound_rule(
                    template_id,
                    &[absent_preset, anchor_port],
                    &world,
                    &story,
                    &linked,
                    &effects
                )
                .is_err()
        );
        assert!(target.rules.is_empty());

        target
            .apply_bound_rule(
                template_id,
                &[anchor_port, preset_port],
                &world,
                &story,
                &linked,
                &effects,
            )
            .unwrap();
        let assigned = &mut target.rules[0];
        assert!(!assigned.enabled);
        assigned.enabled = true;
        assigned.draft = false;
        target.validate().unwrap();
        let mut session = Session::new(&target);
        session.set_effects(&effects);
        let mut interactions = InteractionState::default();
        assert_eq!(
            session.dispatch_at(&target, &world, &story, &mut interactions, event, [0.0; 3]),
            1
        );
        assert_eq!(
            session.take_effect_requests(),
            [Emission {
                preset,
                at: [6.0, 0.4, -1.0],
                follow_player: false,
            }]
        );
    }
}
