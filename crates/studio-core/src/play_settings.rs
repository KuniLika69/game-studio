use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaySettings {
    pub controls: Controls,
    pub movement_3d: Movement3d,
    pub movement_2d: Movement2d,
    pub camera: CameraSettings,
}

impl PlaySettings {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn validate_with(&self, supports_key: impl Fn(&str) -> bool) -> Result<(), String> {
        if !Action::ALL.into_iter().all(|action| {
            self.controls.for_action(action).keys().all(|key| {
                supports_key(key)
                    || (action == Action::Sprint && matches!(key, "Shift" | "Ctrl" | "Alt"))
            })
        }) || !self.movement_3d.valid()
            || !self.movement_2d.valid()
            || !self.camera.valid()
        {
            return Err("Некорректные клавиши, параметры движения или камеры.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ControlFrame {
    pub movement: [f32; 2],
    pub sprint: bool,
    pub jump: bool,
    pub interact: bool,
    pub mark: bool,
    pub retry: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Sprint,
    Jump,
    Interact,
    Mark,
    Retry,
}

impl Action {
    pub const ALL: [Self; 9] = [
        Self::Forward,
        Self::Back,
        Self::Left,
        Self::Right,
        Self::Sprint,
        Self::Jump,
        Self::Interact,
        Self::Mark,
        Self::Retry,
    ];
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Controls {
    pub forward: Binding,
    pub back: Binding,
    pub left: Binding,
    pub right: Binding,
    pub sprint: Binding,
    pub jump: Binding,
    pub interact: Binding,
    pub mark: Binding,
    pub retry: Binding,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            forward: Binding::new("W", Some("Up")),
            back: Binding::new("S", Some("Down")),
            left: Binding::new("A", Some("Left")),
            right: Binding::new("D", Some("Right")),
            sprint: Binding {
                main: "Shift".into(),
                alternate: None,
            },
            jump: Binding::new("Space", None),
            interact: Binding::new("E", None),
            mark: Binding::new("F", None),
            retry: Binding::new("R", None),
        }
    }
}

impl Controls {
    pub fn for_action(&self, action: Action) -> &Binding {
        match action {
            Action::Forward => &self.forward,
            Action::Back => &self.back,
            Action::Left => &self.left,
            Action::Right => &self.right,
            Action::Sprint => &self.sprint,
            Action::Jump => &self.jump,
            Action::Interact => &self.interact,
            Action::Mark => &self.mark,
            Action::Retry => &self.retry,
        }
    }

    pub fn for_action_mut(&mut self, action: Action) -> &mut Binding {
        match action {
            Action::Forward => &mut self.forward,
            Action::Back => &mut self.back,
            Action::Left => &mut self.left,
            Action::Right => &mut self.right,
            Action::Sprint => &mut self.sprint,
            Action::Jump => &mut self.jump,
            Action::Interact => &mut self.interact,
            Action::Mark => &mut self.mark,
            Action::Retry => &mut self.retry,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub main: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate: Option<String>,
}

impl Binding {
    fn new(main: &str, alternate: Option<&str>) -> Self {
        Self {
            main: main.into(),
            alternate: alternate.map(str::to_owned),
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.main.as_str()).chain(self.alternate.as_deref())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Movement3d {
    pub walk_speed: f32,
    pub sprint_speed: f32,
    pub acceleration: f32,
    pub sprint_acceleration: f32,
    pub braking: f32,
    pub air_acceleration: f32,
    pub jump_speed: f32,
}

impl Default for Movement3d {
    fn default() -> Self {
        Self {
            walk_speed: 3.15,
            sprint_speed: 5.7,
            acceleration: 18.0,
            sprint_acceleration: 25.0,
            braking: 25.0,
            air_acceleration: 8.5,
            jump_speed: 6.65,
        }
    }
}

impl Movement3d {
    fn valid(self) -> bool {
        (0.25..=24.0).contains(&self.walk_speed)
            && (self.walk_speed..=30.0).contains(&self.sprint_speed)
            && [
                self.acceleration,
                self.sprint_acceleration,
                self.braking,
                self.air_acceleration,
            ]
            .into_iter()
            .all(|value| (0.5..=200.0).contains(&value))
            && (0.2..=20.0).contains(&self.jump_speed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Movement2d {
    pub walk_speed: f32,
    pub sprint_speed: f32,
    pub acceleration: f32,
    pub braking: f32,
    pub jump_speed: f32,
}

impl Default for Movement2d {
    fn default() -> Self {
        Self {
            walk_speed: 3.8,
            sprint_speed: 6.4,
            acceleration: 28.0,
            braking: 28.0,
            jump_speed: 10.4,
        }
    }
}

impl Movement2d {
    fn valid(self) -> bool {
        (0.25..=24.0).contains(&self.walk_speed)
            && (self.walk_speed..=30.0).contains(&self.sprint_speed)
            && (0.5..=200.0).contains(&self.acceleration)
            && (0.5..=200.0).contains(&self.braking)
            && (0.2..=20.0).contains(&self.jump_speed)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraMode {
    #[default]
    Follow,
    Fixed,
    Free,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraSettings {
    pub mode: CameraMode,
    pub distance: Option<f32>,
    pub sensitivity: f32,
    pub follow_rate: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            mode: CameraMode::Follow,
            distance: None,
            sensitivity: 1.0,
            follow_rate: 12.0,
        }
    }
}

impl CameraSettings {
    fn valid(self) -> bool {
        self.distance
            .is_none_or(|value| (0.66..=38.0).contains(&value))
            && (0.1..=4.0).contains(&self.sensitivity)
            && (1.0..=40.0).contains(&self.follow_rate)
    }
}
