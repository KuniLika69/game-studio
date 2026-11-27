use crate::environment::{Environment, WeatherPreset};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Contact {
    Entered,
    Exited,
    Pressed,
    Released,
}

impl Contact {
    pub const ALL: [Self; 4] = [Self::Entered, Self::Exited, Self::Pressed, Self::Released];

    pub fn label(self) -> &'static str {
        match self {
            Self::Entered => "Вошёл в датчик",
            Self::Exited => "Вышел из датчика",
            Self::Pressed => "Плита нажата",
            Self::Released => "Плита отпущена",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    Contact {
        sensor_id: u64,
        state: Contact,
    },
    Weather {
        preset: WeatherPreset,
        settled: bool,
    },
    WaterReached {
        zone: u32,
    },
    RotorTurned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    Weather {
        preset: WeatherPreset,
        duration_tenths: u16,
    },
    Water {
        zone: u32,
        height_cm: i16,
        duration_tenths: u16,
    },
}

#[derive(Default)]
pub(crate) struct Requests {
    enabled: bool,
    levels: Vec<(u32, f32, f32)>,
    queued: Vec<Command>,
}

impl Requests {
    pub fn configure(&mut self, environment: Option<&Environment>) {
        *self = Self::default();
        if let Some(environment) = environment {
            self.enabled = true;
            self.levels = environment
                .waters
                .iter()
                .map(|area| (area.id, area.floor_y, area.max_level))
                .collect();
        }
    }

    pub fn supports(&self, command: Command) -> bool {
        match command {
            Command::Weather {
                duration_tenths, ..
            } => self.enabled && duration_tenths <= 6_000,
            Command::Water {
                zone,
                height_cm,
                duration_tenths,
            } => {
                duration_tenths <= 6_000
                    && self.levels.iter().any(|&(id, floor, max)| {
                        zone == id && (floor..=max).contains(&(f32::from(height_cm) / 100.0))
                    })
            }
        }
    }

    pub fn queue(&mut self, command: Command) {
        self.queued.push(command);
    }
    pub fn take(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.queued)
    }
    pub fn clear(&mut self) {
        self.queued.clear();
    }
}

impl crate::Studio {
    pub(super) fn simulate_rules(&mut self, dt: f32) {
        use crate::{body_physics::Trigger, environment::EventKind, rules::Event};

        self.apply_simulation_commands();
        for event in self.advance_environment(dt) {
            let signal = match event.kind {
                EventKind::WeatherStarted { preset, .. } => Signal::Weather {
                    preset,
                    settled: false,
                },
                EventKind::WeatherSettled { preset, .. } => Signal::Weather {
                    preset,
                    settled: true,
                },
                EventKind::WaterLevelReached { zone } => Signal::WaterReached { zone },
            };
            self.dispatch_rule(Event::Simulation(signal));
        }
        for event in self.physics_runtime.take_events() {
            let state = match event.kind {
                Trigger::Entered(_) => Contact::Entered,
                Trigger::Exited(_) => Contact::Exited,
                Trigger::Pressed => Contact::Pressed,
                Trigger::Released => Contact::Released,
            };
            self.dispatch_rule(Event::Simulation(Signal::Contact {
                sensor_id: event.sensor_id,
                state,
            }));
        }
        if self.rotor_step(dt) {
            self.dispatch_rule(Event::Simulation(Signal::RotorTurned));
        }
        self.apply_simulation_commands();
    }

    fn apply_simulation_commands(&mut self) {
        let commands = self.rule_session.take_simulation_commands();
        let (Some(config), Some(runtime)) =
            (&self.scene.environment, &mut self.environment_session)
        else {
            return;
        };
        for command in commands {
            let result = match command {
                Command::Weather {
                    preset,
                    duration_tenths,
                } => runtime.set_weather(config, preset, f32::from(duration_tenths) / 10.0),
                Command::Water {
                    zone,
                    height_cm,
                    duration_tenths,
                } => runtime.set_water_level(
                    config,
                    zone,
                    f32::from(height_cm) / 100.0,
                    f32::from(duration_tenths) / 10.0,
                ),
            };
            if let Err(error) = result {
                self.status = error;
            }
        }
    }
}
