use super::{
    Action, Condition, Event, InteractionState, MAX_COUNTER_VALUE, MAX_TRACE, RuleSet, Session,
    Story, Trace, TraceOutcome, World, engine,
};

pub(super) struct GameRules<'a> {
    pub session: &'a mut Session,
    pub rules: &'a RuleSet,
    pub world: &'a World,
    pub story: &'a Story,
    pub interactions: &'a mut InteractionState,
    pub event: Event,
    pub position: Option<[f32; 3]>,
}

impl engine::Host for GameRules<'_> {
    type Condition = Condition;
    type Command = Action;

    fn holds(&self, condition: Condition) -> bool {
        match condition {
            Condition::HasItem { item_id } => {
                self.world.key(item_id).is_some() && self.interactions.has_key(item_id)
            }
            Condition::Flag { flag_id, value } => {
                self.session.flag(self.rules, flag_id) == Some(value)
            }
            Condition::Counter {
                counter_id,
                comparison,
                value,
            } => self
                .session
                .counter(self.rules, counter_id)
                .is_some_and(|actual| comparison.accepts(actual, value)),
            Condition::InZone { zone_id } => self
                .rules
                .zone(zone_id)
                .zip(self.position)
                .is_some_and(|(zone, position)| zone.accepts(self.world, position)),
        }
    }

    fn supports(&self, action: Action) -> bool {
        match action {
            Action::Transition { scene_id } => self.session.transition_targets.contains(&scene_id),
            Action::Simulation(command) => self.session.simulation.supports(command),
            Action::PlayEffect(command) => {
                self.session.effect_presets.contains(&command.preset)
                    && command.resolve(self.world, self.position).is_some()
            }
            _ => action.target_exists(self.rules, self.world, self.story),
        }
    }

    fn already_fired(&self, id: u32) -> bool {
        self.session.fired.contains(&id)
    }

    fn remember_once(&mut self, id: u32) {
        if !self.session.reset_requested {
            self.session.fired.insert(id);
        }
    }

    fn apply(&mut self, action: Action) {
        let state = &mut self.session;
        match action {
            Action::SetFlag { flag_id, value } => {
                state.flags.insert(flag_id, value);
            }
            Action::UnlockDoor { door_id } => {
                self.world.unlock_door_by_rule(self.interactions, door_id);
            }
            Action::SetQuestStage { quest_id, stage } => {
                state.quest_stages.insert(quest_id, stage);
            }
            Action::SetCounter { counter_id, value } => {
                state.counters.insert(counter_id, value);
            }
            Action::AddCounter { counter_id, delta } => {
                let current = state.counter(self.rules, counter_id).unwrap_or(0);
                state.counters.insert(
                    counter_id,
                    current
                        .saturating_add(delta)
                        .clamp(-MAX_COUNTER_VALUE, MAX_COUNTER_VALUE),
                );
            }
            Action::Extension(call) => {
                let (_, factor) = self.rules.extensions.counter_action(call.id)
                    .expect("extension action checked before dispatch");
                let current = state.counter(self.rules, call.counter_id).unwrap_or(0);
                state.counters.insert(
                    call.counter_id,
                    current.saturating_mul(factor).clamp(-MAX_COUNTER_VALUE, MAX_COUNTER_VALUE),
                );
            }
            Action::StartTimer { timer_id } => {
                state
                    .timers
                    .insert(timer_id, self.rules.timer(timer_id).unwrap().seconds);
            }
            Action::StopTimer { timer_id } => {
                state.timers.remove(&timer_id);
            }
            Action::SetObjectEnabled { object_id, enabled } => {
                state.object_states.insert(object_id, enabled);
            }
            Action::ResetAttempt => {
                state.flags.clear();
                state.quest_stages.clear();
                state.fired.clear();
                state.counters.clear();
                state.timers.clear();
                state.object_states.clear();
                state.simulation.clear();
                state.pending_effects.clear();
                state.reset_requested = true;
            }
            Action::Transition { scene_id } => state.pending_transition = Some(scene_id),
            Action::Simulation(command) => state.simulation.queue(command),
            Action::PlayEffect(command) => {
                if let Some(emission) = command.resolve(self.world, self.position) {
                    state.pending_effects.push(emission);
                }
            }
        }
    }

    fn record(&mut self, rule_id: u32, outcome: TraceOutcome) {
        let event = if matches!(self.event, Event::EnterZone { .. } | Event::ActorEnteredZone { .. }) {
            self.rules
                .rules
                .iter()
                .find(|rule| rule.id == rule_id)
                .and_then(|rule| rule.event)
                .unwrap_or(self.event)
        } else {
            self.event
        };
        let trace = &mut self.session.trace;
        if trace.len() == MAX_TRACE {
            trace.pop_front();
        }
        trace.push_back(Trace {
            rule_id,
            event,
            outcome,
        });
    }

    fn halted(&self) -> bool {
        self.session.reset_requested || self.session.pending_transition.is_some()
    }
}
