use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::{Motion, Studio, model::Model, viewport3d::BlendPose};

pub const DEFAULT_BLEND_SECONDS: f32 = 0.24;
pub const BLEND_RANGE: std::ops::RangeInclusive<f32> = 0.0..=2.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Graph {
    pub fade_seconds: f32,
    pub start: Option<usize>,
    pub stop: Option<usize>,
    pub turn_left: Option<usize>,
    pub turn_right: Option<usize>,
}

impl Default for Graph {
    fn default() -> Self {
        Self {
            fade_seconds: DEFAULT_BLEND_SECONDS,
            start: None,
            stop: None,
            turn_left: None,
            turn_right: None,
        }
    }
}

impl Graph {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn valid(self) -> bool {
        BLEND_RANGE.contains(&self.fade_seconds)
            && self.clips().into_iter().flatten().all(|clip| clip < 4096)
    }

    pub fn slots(&mut self) -> [(&'static str, &mut Option<usize>); 4] {
        [
            ("Начало движения", &mut self.start),
            ("Остановка", &mut self.stop),
            ("Поворот влево", &mut self.turn_left),
            ("Поворот вправо", &mut self.turn_right),
        ]
    }

    fn clips(self) -> [Option<usize>; 4] {
        [self.start, self.stop, self.turn_left, self.turn_right]
    }
}

#[derive(Clone, Copy)]
struct Sample {
    clip: Option<usize>,
    elapsed: f32,
}

#[derive(Clone, Copy)]
struct OneShot {
    entry: Sample,
    clip: usize,
    clip_length: f32,
    playback_length: f32,
    fade: f32,
    age: f32,
    moving_goal: bool,
    exit: Option<Exit>,
}

#[derive(Clone, Copy)]
struct Exit {
    started: f32,
    held_elapsed: f32,
    weight: f32,
}

impl OneShot {
    fn clip_elapsed(self) -> f32 {
        (self.age / self.playback_length * self.clip_length).min(self.clip_length.next_down())
    }

    fn begin_exit(&mut self) {
        let weight = if self.fade > 0.0 {
            smooth(self.age.min(self.playback_length - self.age) / self.fade)
        } else {
            1.0
        };
        self.exit = Some(Exit {
            started: self.age,
            held_elapsed: self.clip_elapsed(),
            weight,
        });
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Playback {
    shot: Option<OneShot>,
    latched_turn: bool,
}

#[derive(Clone, Copy)]
struct Frame {
    from: Motion,
    to: Motion,
    entry: Sample,
    stable: bool,
    aim_error: Option<f32>,
    travel_speed: f32,
    animation_speed: f32,
    dt: f32,
}

impl Playback {
    fn advance(&mut self, graph: Graph, model: &Model, frame: Frame) {
        if frame.to == Motion::Jump || !graph.clips().into_iter().any(|clip| clip.is_some()) {
            *self = Self::default();
            return;
        }
        let error = frame.aim_error.unwrap_or(0.0);
        if error.abs() < 20.0 {
            self.latched_turn = false;
        }
        let moving = |motion| matches!(motion, Motion::Walk | Motion::Run);
        if let Some(shot) = &mut self.shot {
            if shot.moving_goal != moving(frame.to) && shot.exit.is_none() {
                shot.begin_exit();
            }
            shot.age += frame.dt;
            let end = shot
                .exit
                .map_or(shot.playback_length, |exit| exit.started + shot.fade);
            if shot.age < end {
                return;
            }
            self.shot = None;
        }
        if !frame.stable {
            return;
        }
        let edge = match (frame.from, frame.to) {
            (Motion::Idle, to) if moving(to) => graph.start,
            (from, Motion::Idle) if moving(from) => graph.stop,
            _ => None,
        };
        let turn = frame.travel_speed > 0.16 && error.abs() >= 52.0 && !self.latched_turn;
        let turn_clip = if turn {
            if error < 0.0 {
                graph.turn_left
            } else {
                graph.turn_right
            }
        } else {
            None
        };
        let Some(clip) = edge.or(turn_clip) else {
            return;
        };
        let Some(animation) = model.animations.get(clip) else {
            return;
        };
        if !animation.duration.is_finite() || animation.duration <= 0.0 {
            return;
        }
        let playback_length = (animation.duration / frame.animation_speed).max(graph.fade_seconds);
        if !playback_length.is_finite() {
            return;
        }
        self.latched_turn |= turn;
        self.shot = Some(OneShot {
            entry: frame.entry,
            clip,
            clip_length: animation.duration,
            playback_length,
            fade: graph.fade_seconds.min(playback_length * 0.5),
            age: 0.0,
            moving_goal: moving(frame.to),
            exit: None,
        });
    }

    pub(super) fn pose(
        &self,
        base_clip: Option<usize>,
        base_elapsed: f32,
    ) -> Option<(Option<usize>, f32, Option<BlendPose>)> {
        let shot = self.shot?;
        if let Some(exit) = shot.exit {
            let amount = smooth((shot.age - exit.started) / shot.fade);
            return Some((
                base_clip,
                base_elapsed,
                Some(BlendPose {
                    clip: Some(shot.clip),
                    elapsed: exit.held_elapsed,
                    weight: 1.0 - exit.weight * (1.0 - amount),
                }),
            ));
        }
        let clip_elapsed = shot.clip_elapsed();
        let left = shot.playback_length - shot.age;
        if left < shot.fade {
            return Some((
                base_clip,
                base_elapsed,
                Some(BlendPose {
                    clip: Some(shot.clip),
                    elapsed: clip_elapsed,
                    weight: smooth(1.0 - left / shot.fade),
                }),
            ));
        }
        let blend = (shot.age < shot.fade).then(|| BlendPose {
            clip: shot.entry.clip,
            elapsed: shot.entry.elapsed,
            weight: smooth(shot.age / shot.fade),
        });
        Some((Some(shot.clip), clip_elapsed, blend))
    }
}

fn smooth(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Studio {
    pub(super) fn player_blend_seconds(&self) -> f32 {
        self.scene
            .avatar
            .as_ref()
            .filter(|_| self.scene.map_2d.is_none())
            .map_or(DEFAULT_BLEND_SECONDS, |avatar| {
                avatar.motion_graph.fade_seconds
            })
    }

    pub(super) fn advance_actor_transition(
        &mut self,
        next: Motion,
        input: [f32; 2],
        speed: f32,
        dt: f32,
    ) {
        let Some(avatar) = self.scene.avatar.as_ref() else {
            return;
        };
        let graph = avatar.motion_graph;
        if !graph.clips().into_iter().any(|clip| clip.is_some()) {
            return;
        }
        let Some((model, _)) = self.avatar_asset() else {
            return;
        };
        let direction = Vec2::from_array(input);
        let aim_error = (direction.length_squared() > 0.0025).then(|| {
            let aim = direction.x.atan2(direction.y).to_degrees();
            (aim - self.avatar_heading + 180.0).rem_euclid(360.0) - 180.0
        });
        let frame = Frame {
            from: self.avatar_motion,
            to: next,
            entry: Sample {
                clip: avatar.clip(self.avatar_motion),
                elapsed: self.avatar_clip_time * avatar.animation_speed,
            },
            stable: self.avatar_previous.is_none(),
            aim_error,
            travel_speed: speed,
            animation_speed: avatar.animation_speed,
            dt,
        };
        let mut playback = self.actor_transition;
        playback.advance(graph, model, frame);
        self.actor_transition = playback;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{View, assets::Source, playback};

    #[test]
    fn reversing_a_start_fades_from_its_current_contribution() {
        let model = crate::model::decode_glb(include_bytes!("../demo_assets/emma.glb")).unwrap();
        let config = Graph {
            start: Some(2),
            fade_seconds: 0.12,
            ..Graph::default()
        };
        let mut player = Playback::default();
        let frame = Frame {
            from: Motion::Idle,
            to: Motion::Walk,
            entry: Sample {
                clip: Some(0),
                elapsed: 0.3,
            },
            stable: true,
            aim_error: None,
            travel_speed: 0.40,
            animation_speed: 1.0,
            dt: 0.06,
        };
        player.advance(config, &model, frame);
        player.advance(
            config,
            &model,
            Frame {
                from: Motion::Walk,
                ..frame
            },
        );
        let (clip, active_elapsed, blend) = player.pose(Some(1), 0.14).unwrap();
        assert_eq!(clip, Some(2));
        assert!((blend.unwrap().weight - 0.5).abs() < 0.00001);

        let reverse = Frame {
            from: Motion::Walk,
            to: Motion::Idle,
            stable: false,
            dt: 0.03,
            ..frame
        };
        player.advance(config, &model, reverse);
        let (clip, _, blend) = player.pose(Some(0), 0.4).unwrap();
        let blend = blend.unwrap();
        assert_eq!(clip, Some(0));
        assert_eq!(blend.clip, Some(2));
        assert_eq!(blend.elapsed, active_elapsed);
        assert!((0.5..0.65).contains(&blend.weight));
        player.advance(
            config,
            &model,
            Frame {
                dt: 0.13,
                ..reverse
            },
        );
        assert!(player.pose(Some(0), 0.55).is_none());
    }

    #[test]
    fn player_graph_blends_selected_start_turn_and_stop_clips() {
        let mut studio = Studio::default();
        studio.city_demo();
        studio.assets.ensure(&Source::Emma, None).unwrap();
        studio.assign_avatar(Source::Emma, "Проверка переходов".into());
        let avatar = studio.scene.avatar.as_mut().unwrap();
        let start = avatar.run.expect("Emma run clip");
        avatar.motion_graph = Graph {
            fade_seconds: 0.12,
            start: Some(start),
            stop: Some(start),
            turn_right: Some(start),
            ..Graph::default()
        };
        let config = avatar.motion_graph;
        assert_eq!(
            serde_json::from_value::<Graph>(serde_json::to_value(config).unwrap()).unwrap(),
            config
        );
        assert!(
            !Graph {
                fade_seconds: f32::NAN,
                ..config
            }
            .valid()
        );
        assert!(
            !Graph {
                stop: Some(4096),
                ..config
            }
            .valid()
        );
        assert_eq!(
            serde_json::from_str::<Graph>("{}").unwrap(),
            Graph::default()
        );
        studio.game_running = true;
        studio.view = View::Three;
        studio.game.z = -10.0;
        studio.scene.npcs.clear();
        let ctx = eframe::egui::Context::default();
        let simulate = |studio: &mut Studio, movement: [f32; 2], frames| {
            let mut shot_entry = None;
            for _ in 0..frames {
                studio.simulate_intent(
                    &ctx,
                    playback::Intent {
                        dt: 1.0 / 60.0,
                        movement,
                        ..Default::default()
                    },
                );
                let draw = studio.avatar_draw_without_foot().unwrap();
                if shot_entry.is_none()
                    && let Some(blend) = draw.blend
                    && studio
                        .actor_transition
                        .shot
                        .is_some_and(|shot| shot.age == 0.0)
                {
                    shot_entry = draw.clip.map(|clip| (clip, blend.weight));
                }
            }
            shot_entry
        };
        assert_eq!(simulate(&mut studio, [0.0, -1.0], 110), Some((start, 0.0)));
        assert_eq!(studio.avatar_motion, Motion::Walk);
        assert!(studio.actor_transition.shot.is_none());
        assert_eq!(simulate(&mut studio, [-1.0, 0.0], 110), Some((start, 0.0)));
        assert!(studio.actor_transition.shot.is_none());
        assert_eq!(simulate(&mut studio, [0.0, 0.0], 110), Some((start, 0.0)));
        assert_eq!(studio.avatar_motion, Motion::Idle);
        assert!(studio.actor_transition.shot.is_none());
        assert_eq!(
            studio.avatar_draw_without_foot().unwrap().clip,
            studio.scene.avatar.as_ref().unwrap().idle
        );
    }
}
