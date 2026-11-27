use serde::{Deserialize, Serialize};

pub const MAX_PARTS: usize = 128;
pub const MAX_KEYS: usize = 4_096;
const KEY_TIME_TOLERANCE: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    Box,
    Sphere,
    Cylinder,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartMotion {
    #[default]
    Static,
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
    LeftFoot,
    RightFoot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub id: u64,
    pub name: String,
    pub shape: Shape,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub size: [f32; 3],
    pub color: [f32; 3],
    #[serde(default)]
    pub motion: PartMotion,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub part_id: u64,
    pub time: f32,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
}

impl Keyframe {
    fn pose(&self) -> Pose {
        Pose {
            position: self.position,
            rotation: self.rotation,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub duration: f32,
    pub event_time: f32,
    pub keys: Vec<Keyframe>,
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            duration: 1.2,
            event_time: 0.72,
            keys: Vec::new(),
        }
    }
}

impl Animation {
    pub fn set_duration(&mut self, duration: f32) {
        let duration = duration.clamp(0.05, 60.0);
        let old_duration = self.duration;
        for key in &mut self.keys {
            key.time = (key.time / old_duration) * duration;
        }
        self.event_time = (self.event_time / old_duration) * duration;
        self.duration = duration;
    }

    pub fn sample_part(&self, part_id: u64, time: f32) -> Pose {
        let time = time.clamp(0.0, self.duration);
        let mut previous = (0.0, Pose::default());
        let mut next = (self.duration, Pose::default());

        for key in self.keys.iter().filter(|key| key.part_id == part_id) {
            if key.time <= time && key.time >= previous.0 {
                previous = (key.time, key.pose());
            }
            if key.time >= time && key.time <= next.0 {
                next = (key.time, key.pose());
            }
        }

        if previous.0 == next.0 {
            return previous.1;
        }

        let progress = (time - previous.0) / (next.0 - previous.0);
        let mix =
            |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * progress);
        Pose {
            position: mix(previous.1.position, next.1.position),
            rotation: mix(previous.1.rotation, next.1.rotation),
        }
    }

    pub fn keyframe_mut(&mut self, part_id: u64, time: f32) -> &mut Keyframe {
        let time = time.clamp(0.0, self.duration);
        let index = self
            .keys
            .iter()
            .position(|key| key.part_id == part_id && (key.time - time).abs() < KEY_TIME_TOLERANCE);
        if let Some(index) = index {
            return &mut self.keys[index];
        }

        let pose = self.sample_part(part_id, time);
        self.keys.push(Keyframe {
            part_id,
            time,
            position: pose.position,
            rotation: pose.rotation,
        });
        let last_index = self.keys.len() - 1;
        &mut self.keys[last_index]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub color: [f32; 3],
    pub count: u32,
    pub intensity: f32,
    pub duration: f32,
}

impl Default for Effect {
    fn default() -> Self {
        Self {
            color: [1.0, 0.52, 0.17],
            count: 24,
            intensity: 1.0,
            duration: 0.55,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpritePixel {
    pub direction: u8,
    pub x: u8,
    pub y: u8,
    pub color: [u8; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u8>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub baked: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub const MAX_SPRITE_FRAMES: u8 = 8;
pub const MAX_SPRITE_PIXELS: usize = 90_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpriteArt {
    pub drawn_only: bool,
    pub use_on_plan: bool,
    pub frames: u8,
    pub fps: u8,
}

impl Default for SpriteArt {
    fn default() -> Self {
        Self {
            drawn_only: false,
            use_on_plan: false,
            frames: 1,
            fps: 8,
        }
    }
}
