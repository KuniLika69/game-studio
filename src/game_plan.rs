//! Bounded authoring plan compiled into ordinary maps, quests and rules.
use serde::{Deserialize, Serialize};

use crate::{
    Scene, SpritePixel, assets,
    map2d::{Collision, Layer, Map, Mode, Tile},
    rules::{Action, Comparison, Condition, Event, GameAction, RuleSet},
    story::Objective,
};

pub(crate) const SIGNALS: &str = "Сигналы";
pub(crate) const TIMEOUT: &str = "Время вышло · R для повтора";
pub(crate) const FINISH_RULE: &str = "Победа у выходного маяка";
const LIMIT: usize = 70;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Format {
    Workshop3d,
    #[default]
    Top,
    Side,
}

impl Format {
    pub const ALL: [Self; 3] = [Self::Workshop3d, Self::Top, Self::Side];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Workshop3d => "3D · Мастерская",
            Self::Top => "2D сверху · Поиск сигналов",
            Self::Side => "Платформер · Поиск сигналов",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Atmosphere {
    #[default]
    BlueHour,
    AmberRain,
    MorningMist,
}

impl Atmosphere {
    pub const ALL: [Self; 3] = [Self::BlueHour, Self::AmberRain, Self::MorningMist];

    pub const fn title(self) -> &'static str {
        match self {
            Self::BlueHour => "Синий час · Маршрут вдоль лужи",
            Self::AmberRain => "Янтарный навес · Подход сбоку",
            Self::MorningMist => "Утренний туман · Открытый проход",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Controls {
    #[default]
    Wasd,
    Arrows,
}

impl Controls {
    pub const ALL: [Self; 2] = [Self::Wasd, Self::Arrows];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Wasd => "WASD и стрелки",
            Self::Arrows => "Стрелки",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Plan {
    pub title: String,
    pub format: Format,
    pub atmosphere: Atmosphere,
    pub controls: Controls,
    pub hero: String,
    pub companion: String,
    pub signals: u8,
    pub seconds: f32,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            title: "Сигнальная тропа".into(),
            format: Format::Top,
            atmosphere: Atmosphere::BlueHour,
            controls: Controls::Wasd,
            hero: "Смотрительница маяков".into(),
            companion: "Мастер".into(),
            signals: 2,
            seconds: 22.0,
        }
    }
}

impl Plan {
    pub fn validate(&self) -> Result<(), String> {
        let valid_text = |value: &str| {
            !value.trim().is_empty()
                && value.chars().count() <= LIMIT
                && !value.chars().any(char::is_control)
        };
        if ![&self.title, &self.hero, &self.companion]
            .into_iter()
            .all(|value| valid_text(value))
        {
            return Err(format!(
                "Название и имена должны содержать от 1 до {LIMIT} символов."
            ));
        }
        if !(1..=3).contains(&self.signals) || !(8.0..=180.0).contains(&self.seconds) {
            return Err("Доступны 1–3 сигнала и таймер от 8 до 180 секунд.".into());
        }
        Ok(())
    }

    pub fn build(&self, library: &mut assets::Library) -> Result<Scene, String> {
        self.validate()?;
        let mut scene = match self.format {
            Format::Workshop3d => {
                use crate::rain_workshop::Style;
                let style = match self.atmosphere {
                    Atmosphere::BlueHour => Style::BlueHour,
                    Atmosphere::AmberRain => Style::AmberRain,
                    Atmosphere::MorningMist => Style::MorningMist,
                };
                let mut scene = crate::rain_workshop::create(library, style)?;
                if let Some(hero) = &mut scene.avatar {
                    hero.name = self.hero.trim().into();
                }
                if let Some(npc) = scene.npcs.first_mut() {
                    npc.avatar.name = self.companion.trim().into();
                    if let Some(conversation) = scene.story.conversations.first_mut() {
                        conversation.name.clone_from(&npc.avatar.name);
                    }
                }
                scene
            }
            Format::Top => self.signal_map(Mode::Top)?,
            Format::Side => self.signal_map(Mode::Side)?,
        };
        if self.controls == Controls::Arrows {
            let controls = &mut scene.play_settings.controls;
            for binding in [
                &mut controls.forward,
                &mut controls.back,
                &mut controls.left,
                &mut controls.right,
            ] {
                if let Some(arrow) = binding.alternate.take() {
                    binding.main = arrow;
                }
            }
        }
        scene.validate()?;
        Ok(scene)
    }

    fn signal_map(&self, mode: Mode) -> Result<Scene, String> {
        let mut scene = Scene::empty();
        let mut map = make_map(mode, self.atmosphere);
        let (marks, exit) = match mode {
            Mode::Top => ([[7, 10], [14, 10], [18, 10]], [22, 10]),
            Mode::Side => ([[8, 13], [16, 13], [23, 13]], [29, 13]),
        };
        let position = |map: &Map, [x, y]: [u16; 2]| {
            map.to_world([
                f32::from(x) + 0.5,
                if mode == Mode::Top {
                    f32::from(y) + 0.5
                } else {
                    f32::from(y) + 1.0
                },
            ])
        };
        let quest = scene
            .story
            .add_quest()
            .ok_or("Не удалось создать задачу.")?;
        let task = scene.story.quests.last_mut().expect("new quest");
        task.title = self.title.trim().to_owned();
        task.description = format!(
            "{} зажигает сигналы клавишей F и подтверждает выход у голубого маяка. R начинает новую попытку.",
            self.hero.trim()
        );
        task.completion = "Маршрут завершён".into();
        task.objective = Objective::Rules {
            completion_stage: 2,
        };

        let rules = &mut scene.rules;
        let marks_count = counter(rules, SIGNALS)?;
        let expired = counter(rules, TIMEOUT)?;
        let finished = rules.add_flag().ok_or("Не удалось создать флаг финиша.")?;
        rules.flags.last_mut().expect("new flag").name = "Маршрут завершён".into();
        let timer = rules.add_timer().ok_or("Не удалось создать таймер.")?;
        let watch = rules.timers.last_mut().expect("new timer");
        watch.name = "До отключения маяка".into();
        watch.seconds = self.seconds;
        let [spawn_x, spawn_row] = map.spawn;
        let start = zone(
            rules,
            "Линия старта",
            [
                spawn_x + 1.25,
                0.0,
                if mode == Mode::Top { spawn_row } else { 0.0 },
            ],
        )?;
        let start_region = rules.zones.last_mut().expect("new start zone");
        if mode == Mode::Top {
            start_region.size[2] = f32::from(map.size[1]) * 2.0;
        } else {
            start_region.size[1] = f32::from(map.size[1]);
        }
        let gate_row = if mode == Mode::Top {
            spawn_row.floor()
        } else {
            spawn_row - 1.0
        };
        map.paint(
            [4, gate_row as u16],
            Layer::Detail,
            Some(Tile { atlas: 1, index: 7 }),
        );
        add_rule(
            rules,
            "Начать отсчёт на старте",
            Event::EnterZone { zone_id: start },
            vec![],
            vec![
                Action::StartTimer { timer_id: timer },
                Action::SetQuestStage {
                    quest_id: quest,
                    stage: 1,
                },
            ],
            true,
        )?;
        let not_expired = Condition::Counter {
            counter_id: expired,
            comparison: Comparison::Equal,
            value: 0,
        };
        let not_finished = Condition::Flag {
            flag_id: finished,
            value: false,
        };
        for (index, cell) in marks.into_iter().enumerate() {
            map.paint(cell, Layer::Detail, Some(Tile { atlas: 1, index: 6 }));
            let mark = zone(
                rules,
                &format!("Сигнал {}", index + 1),
                position(&map, cell),
            )?;
            add_rule(
                rules,
                &format!("Зажечь сигнал {}", index + 1),
                Event::GameAction {
                    action: GameAction::Mark,
                },
                vec![
                    Condition::InZone { zone_id: mark },
                    not_expired,
                    not_finished,
                ],
                vec![Action::AddCounter {
                    counter_id: marks_count,
                    delta: 1,
                }],
                true,
            )?;
        }
        map.paint(exit, Layer::Detail, Some(Tile { atlas: 1, index: 8 }));
        let exit = zone(rules, "Выходной маяк", position(&map, exit))?;
        add_rule(
            rules,
            FINISH_RULE,
            Event::GameAction {
                action: GameAction::Mark,
            },
            vec![
                Condition::InZone { zone_id: exit },
                Condition::Counter {
                    counter_id: marks_count,
                    comparison: Comparison::AtLeast,
                    value: self.signals.into(),
                },
                not_expired,
                not_finished,
            ],
            vec![
                Action::SetQuestStage {
                    quest_id: quest,
                    stage: 2,
                },
                Action::SetFlag {
                    flag_id: finished,
                    value: true,
                },
                Action::StopTimer { timer_id: timer },
            ],
            true,
        )?;
        add_rule(
            rules,
            "Попытка истекла",
            Event::TimerElapsed { timer_id: timer },
            vec![not_finished],
            vec![Action::SetCounter {
                counter_id: expired,
                value: 1,
            }],
            true,
        )?;
        add_rule(
            rules,
            "Новая попытка",
            Event::GameAction {
                action: GameAction::Retry,
            },
            vec![],
            vec![Action::ResetAttempt],
            false,
        )?;
        scene.map_2d = Some(map);
        scene.sprite_art.drawn_only = true;
        scene.sprite_pixels = scout_sprite(self.atmosphere);
        Ok(scene)
    }
}

fn counter(rules: &mut RuleSet, name: &str) -> Result<u32, String> {
    let id = rules.add_counter().ok_or("Не удалось создать счётчик.")?;
    rules.counters.last_mut().expect("new counter").name = name.into();
    Ok(id)
}

fn zone(rules: &mut RuleSet, name: &str, position: [f32; 3]) -> Result<u32, String> {
    let id = rules.add_zone(position).ok_or("Не удалось создать зону.")?;
    let zone = rules.zones.last_mut().expect("new zone");
    zone.name = name.into();
    zone.size = [1.3, 1.5, 1.3];
    Ok(id)
}

fn add_rule(
    rules: &mut RuleSet,
    name: &str,
    event: Event,
    conditions: Vec<Condition>,
    actions: Vec<Action>,
    once: bool,
) -> Result<(), String> {
    rules.add_rule().ok_or("Не удалось создать правило.")?;
    let rule = rules.rules.last_mut().expect("new rule");
    rule.name = name.into();
    rule.event = Some(event);
    rule.conditions = conditions;
    rule.actions = actions;
    rule.once = once;
    Ok(())
}

fn make_map(mode: Mode, atmosphere: Atmosphere) -> Map {
    let mut map = Map::new(mode);
    if mode == Mode::Side {
        map.resize([32, 18]);
        map.spawn = [3.5, 14.0];
    }
    let landscape = match atmosphere {
        Atmosphere::BlueHour => 3,
        Atmosphere::AmberRain => 1,
        Atmosphere::MorningMist => 11,
    };
    let tile = |index| Some(Tile { atlas: 1, index });
    for y in 0..map.size[1] {
        for x in 0..map.size[0] {
            let cell = [x, y];
            if mode == Mode::Side {
                let gap = (11..=12).contains(&x) || (19..=20).contains(&x);
                if y >= 14 {
                    map.paint(
                        cell,
                        Layer::Ground,
                        tile(if gap {
                            4
                        } else if y == 14 {
                            13
                        } else {
                            12
                        }),
                    );
                    if !gap {
                        map.paint_collision(cell, Collision::Solid);
                    }
                }
                continue;
            }
            let border = x == 0 || y == 0 || x + 1 == map.size[0] || y + 1 == map.size[1];
            let walk = (9..=11).contains(&y) || (5..=10).contains(&y) && (7..=8).contains(&x);
            let water = (13..=19).contains(&x) && (3..=7).contains(&y);
            map.paint(
                cell,
                Layer::Ground,
                tile(if border {
                    9
                } else if water {
                    4
                } else if walk {
                    5
                } else {
                    landscape
                }),
            );
            if border || water {
                map.paint_collision(cell, Collision::Solid);
            } else if !walk && (x * 13 + y * 7) % 17 == 0 {
                map.paint(
                    cell,
                    Layer::Tall,
                    Some(Tile {
                        atlas: 2,
                        index: (x + y) % 3,
                    }),
                );
                map.paint_collision(cell, Collision::Solid);
            }
        }
    }
    map
}

fn scout_sprite(atmosphere: Atmosphere) -> Vec<SpritePixel> {
    let cape = match atmosphere {
        Atmosphere::BlueHour => [52, 156, 149, 255],
        Atmosphere::AmberRain => [200, 124, 60, 255],
        Atmosphere::MorningMist => [125, 139, 193, 255],
    };
    let mut pixels = Vec::new();
    for y in 24..82 {
        for x in 22..74 {
            let dx = i32::from(x) - 48;
            let dy = i32::from(y) - 43;
            let color = if dx * dx + dy * dy < 294 && y < 55 {
                if (38..=47).contains(&y) && dx.abs() < 12 {
                    [253, 219, 137, 255]
                } else {
                    [24, 43, 51, 255]
                }
            } else if (48..74).contains(&y) && dx.abs() < 11 + (i32::from(y) - 50) / 3 {
                cape
            } else if y >= 72 && ((36..45).contains(&x) || (51..60).contains(&x)) {
                [29, 37, 50, 255]
            } else {
                continue;
            };
            pixels.push(SpritePixel {
                direction: 0,
                frame: None,
                baked: false,
                x,
                y,
                color,
            });
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_signal_plan_builds_editable_maps_and_an_exact_victory_condition() {
        for format in [Format::Top, Format::Side] {
            let plan = Plan {
                format,
                signals: 3,
                ..Plan::default()
            };
            let scene = plan.build(&mut assets::Library::default()).unwrap();
            assert_eq!(scene.rules.zones.len(), 5);
            assert!(scene.rules.rules.iter().any(|rule| rule.name == FINISH_RULE
                && rule.conditions.iter().any(|condition| matches!(
                    condition,
                    Condition::Counter {
                        comparison: Comparison::AtLeast,
                        value: 3,
                        ..
                    }
                ))));
            let copy = serde_json::from_slice(&serde_json::to_vec(&scene).unwrap()).unwrap();
            assert_eq!(scene, copy);
        }
        assert!(
            Plan {
                seconds: 2.0,
                ..Plan::default()
            }
            .build(&mut assets::Library::default())
            .is_err()
        );
    }
}
