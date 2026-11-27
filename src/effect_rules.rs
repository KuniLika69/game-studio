use serde::{Deserialize, Serialize};

use crate::{effects::PresetId, world::World};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub preset: PresetId,
    pub origin: Origin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    Player,
    Object(u64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emission {
    pub preset: PresetId,
    pub at: [f32; 3],
    pub follow_player: bool,
}

impl Command {
    pub(crate) fn valid(self, unbound: bool) -> bool {
        (self.preset != 0 || unbound)
            && self.preset != PresetId::MAX
            && (!matches!(self.origin, Origin::Object(0)) || unbound)
    }

    pub(crate) fn resolve(self, world: &World, player: Option<[f32; 3]>) -> Option<Emission> {
        let at = match self.origin {
            Origin::Player => player?,
            Origin::Object(id) => {
                anchors(world).find_map(|(anchor, _, at)| (anchor == id).then_some(at))?
            }
        };
        at.iter()
            .all(|value| value.is_finite())
            .then_some(Emission {
                preset: self.preset,
                at,
                follow_player: self.origin == Origin::Player,
            })
    }
}

pub(crate) fn anchors(world: &World) -> impl Iterator<Item = (u64, &str, [f32; 3])> + '_ {
    world
        .objects
        .iter()
        .map(|object| (object.id, object.name.as_str(), object.position))
        .chain(
            world
                .models
                .iter()
                .map(|model| (model.id, model.label.as_str(), model.position)),
        )
}

impl crate::Studio {
    pub(super) fn flush_rule_effects(&mut self) {
        for emission in self.rule_session.take_effect_requests() {
            let emitter = self.play_effect(emission.preset, emission.at);
            if emission.follow_player
                && let Some(id) = emitter
            {
                self.follow_effect(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        effects::{Kind, Library},
        rules::{Action, Event, GameAction, RuleSet, Session, TraceOutcome},
        story::Story,
        world::{InteractionState, WorldKind},
    };

    #[test]
    fn typed_effect_preflight_is_atomic_and_once_emission_has_exact_object_anchor() {
        let mut world = World::default();
        let anchor = world.add(WorldKind::Platform).unwrap();
        world.objects[0].position = [3.0, 2.0, -1.0];
        let mut rules = RuleSet::default();
        let flag = rules.add_flag().unwrap();
        rules.add_rule().unwrap();
        rules.rules[0].event = Some(Event::GameAction {
            action: GameAction::Mark,
        });
        rules.rules[0].once = true;
        let mut presets = Library::default();
        let preset = presets.create(Kind::Smoke).unwrap();
        let command = Command {
            preset,
            origin: Origin::Object(anchor),
        };
        let mut session = Session::new(&rules);
        let mut interactions = InteractionState::default();
        let story = Story::default();
        let dispatch =
            |session: &mut Session, rules: &RuleSet, interactions: &mut InteractionState| {
                session.dispatch_at(
                    rules,
                    &world,
                    &story,
                    interactions,
                    Event::GameAction {
                        action: GameAction::Mark,
                    },
                    [0.0; 3],
                )
            };
        rules.rules[0].actions = vec![
            Action::SetFlag {
                flag_id: flag,
                value: true,
            },
            Action::PlayEffect(command),
        ];
        assert_eq!(dispatch(&mut session, &rules, &mut interactions), 0);
        assert_eq!(session.flag(&rules, flag), Some(false));
        assert_eq!(
            session.trace().next_back().unwrap().outcome,
            TraceOutcome::MissingActionTarget(Action::PlayEffect(command))
        );
        session.set_effects(&presets);
        rules.rules[0].actions[1] = Action::PlayEffect(Command {
            origin: Origin::Object(anchor + 31),
            ..command
        });
        assert_eq!(dispatch(&mut session, &rules, &mut interactions), 0);
        assert_eq!(session.flag(&rules, flag), Some(false));
        assert!(session.take_effect_requests().is_empty());
        rules.rules[0].actions[1] = Action::PlayEffect(command);
        assert_eq!(dispatch(&mut session, &rules, &mut interactions), 1);
        assert_eq!(session.flag(&rules, flag), Some(true));
        assert_eq!(dispatch(&mut session, &rules, &mut interactions), 0);
        assert_eq!(
            session.take_effect_requests(),
            [Emission {
                preset,
                at: [3.0, 2.0, -1.0],
                follow_player: false,
            }]
        );
        assert!(session.take_effect_requests().is_empty());
    }

    #[test]
    fn player_effect_captures_event_position_and_allows_trail_attachment() {
        let mut world = World::default();
        let mut rules = RuleSet::default();
        let mut presets = Library::default();
        let preset = presets.create(Kind::Smoke).unwrap();
        rules.add_rule().unwrap();
        let event = Event::GameAction {
            action: GameAction::Mark,
        };
        rules.rules[0].event = Some(event);
        rules.rules[0].actions = vec![Action::PlayEffect(Command {
            preset,
            origin: Origin::Player,
        })];
        let mut session = Session::new(&rules);
        session.set_effects(&presets);
        let mut interactions = InteractionState::default();
        let story = Story::default();
        assert_eq!(
            session.dispatch(&rules, &world, &story, &mut interactions, event),
            0
        );
        assert_eq!(
            session.dispatch_at(
                &rules,
                &world,
                &story,
                &mut interactions,
                event,
                [1.0, 0.2, 4.0]
            ),
            1
        );
        world.add(WorldKind::Crate);
        assert_eq!(
            session.take_effect_requests(),
            [Emission {
                preset,
                at: [1.0, 0.2, 4.0],
                follow_player: true,
            }]
        );
        assert!(session.take_effect_requests().is_empty());
    }

    #[test]
    fn live_game_frame_materializes_rule_particles_at_the_authored_object() {
        use crate::{Studio, proof_input, scene::Scene};
        use eframe::egui::{self, Key, Modifiers};

        let mut app = Studio {
            standalone: true,
            scene: Scene::empty(),
            ..Studio::default()
        };
        let preset = app.scene.effects.create(Kind::Smoke).unwrap();
        let anchor = app.scene.world.add(WorldKind::Platform).unwrap();
        app.scene.world.objects[0].position = [3.5, 0.0, -1.0];
        app.scene.rules.add_rule().unwrap();
        app.scene.rules.rules[0].event = Some(Event::GameAction {
            action: GameAction::Mark,
        });
        app.scene.rules.rules[0].actions = vec![Action::PlayEffect(Command {
            preset,
            origin: Origin::Object(anchor),
        })];
        let ctx = egui::Context::default();
        let mut driver = proof_input::Driver::new([900, 600]);
        app.start_play(&ctx, false);
        assert!(!app.effects_active());
        let _ = ctx.run(
            driver.raw(proof_input::key(Key::F, Modifiers::NONE)),
            |ctx| {
                app.run_frame(ctx);
            },
        );
        assert!(app.effects_active());
        assert!(app.rule_session.take_effect_requests().is_empty());
        for _ in 0..12 {
            let _ = ctx.run(driver.raw(Vec::new()), |ctx| app.run_frame(ctx));
        }
        let particles = app.effect_particles();
        assert!(!particles.is_empty());
        assert!(particles.iter().all(|particle| {
            particle.kind == Kind::Smoke
                && (particle.position.x - 3.5).abs() < 0.8
                && (particle.position.z + 1.0).abs() < 0.8
        }));
    }
}
