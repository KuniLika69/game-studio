//! Dialogue-local curves of named GLB morph weights.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub const MIN_MS: u16 = 100;
pub const MAX_MS: u16 = 30_000;
const MAX_TARGETS: usize = 64;
const MAX_KEYS: usize = 768;
const MAX_CHANNEL_KEYS: usize = 192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Blink,
    Mouth,
    Brow,
    Gaze,
    Other,
}

impl Group {
    pub const ALL: [Self; 5] = [
        Self::Blink,
        Self::Mouth,
        Self::Brow,
        Self::Gaze,
        Self::Other,
    ];

    pub fn of(target: &str) -> Self {
        let target = target.to_ascii_lowercase();
        if target.contains("blink") || target.contains("eyesquint") || target.contains("eyewide") {
            Self::Blink
        } else if target.contains("mouth")
            || target.contains("jaw")
            || target.starts_with("viseme")
            || target.contains("tongue")
        {
            Self::Mouth
        } else if target.contains("brow") {
            Self::Brow
        } else if target.contains("eyelook") || target.contains("gaze") {
            Self::Gaze
        } else {
            Self::Other
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Blink => "Моргание",
            Self::Mouth => "Рот",
            Self::Brow => "Брови",
            Self::Gaze => "Взгляд",
            Self::Other => "Другие",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lane {
    pub duration_ms: u16,
    pub channels: Vec<Channel>,
}

impl Default for Lane {
    fn default() -> Self {
        Self {
            duration_ms: 2_500,
            channels: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub target: String,
    pub keys: Vec<Key>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub ms: u16,
    pub weight: f32,
}

impl Lane {
    pub fn validate(&self) -> Result<(), String> {
        let mut names = HashSet::new();
        if !(MIN_MS..=MAX_MS).contains(&self.duration_ms)
            || self.channels.len() > MAX_TARGETS
            || self.key_count() > MAX_KEYS
            || self.channels.iter().any(|channel| {
                !valid_name(&channel.target)
                    || !names.insert(&channel.target)
                    || !(1..=MAX_CHANNEL_KEYS).contains(&channel.keys.len())
                    || channel
                        .keys
                        .iter()
                        .any(|key| key.ms > self.duration_ms || !(0.0..=1.0).contains(&key.weight))
                    || channel.keys.windows(2).any(|pair| pair[0].ms >= pair[1].ms)
            })
        {
            return Err("Проверь длительность, каналы и ключи лицевой дорожки.".into());
        }
        Ok(())
    }

    pub fn key_count(&self) -> usize {
        self.channels.iter().map(|channel| channel.keys.len()).sum()
    }

    pub fn channel(&self, target: &str) -> Option<&Channel> {
        self.channels
            .iter()
            .find(|channel| channel.target == target)
    }

    pub fn weight(&self, target: &str, seconds: f32) -> Option<f32> {
        Some(self.channel(target)?.sample(self.sample_millis(seconds)))
    }

    pub fn sample(&self, seconds: f32) -> impl Iterator<Item = (&str, f32)> + '_ {
        let local_ms = self.sample_millis(seconds);
        self.channels
            .iter()
            .map(move |channel| (channel.target.as_str(), channel.sample(local_ms)))
    }

    fn sample_millis(&self, seconds: f32) -> f32 {
        if seconds.is_nan() {
            0.0
        } else {
            (seconds * 1000.0).clamp(0.0, f32::from(self.duration_ms))
        }
    }

    pub fn resize(&mut self, duration_ms: u16) -> bool {
        let earliest_end = self
            .channels
            .iter()
            .filter_map(|channel| channel.keys.last().map(|key| key.ms))
            .max()
            .unwrap_or(MIN_MS)
            .max(MIN_MS);
        let duration_ms = duration_ms.clamp(earliest_end, MAX_MS);
        let changed = self.duration_ms != duration_ms;
        self.duration_ms = duration_ms;
        changed
    }

    pub fn add_key(&mut self, target: &str, ms: u16) -> bool {
        if !valid_name(target) || ms > self.duration_ms || self.key_count() >= MAX_KEYS {
            return false;
        }
        if let Some(channel) = self
            .channels
            .iter_mut()
            .find(|channel| channel.target == target)
        {
            if channel.keys.len() >= MAX_CHANNEL_KEYS {
                return false;
            }
            let Err(index) = channel.keys.binary_search_by_key(&ms, |key| key.ms) else {
                return false;
            };
            let weight = channel.sample(f32::from(ms));
            channel.keys.insert(index, Key { ms, weight });
        } else {
            if self.channels.len() >= MAX_TARGETS {
                return false;
            }
            self.channels.push(Channel {
                target: target.into(),
                keys: vec![Key { ms, weight: 1.0 }],
            });
        }
        true
    }

    pub fn update_key(&mut self, target: &str, old_ms: u16, key: Key) -> bool {
        if key.ms > self.duration_ms || !(0.0..=1.0).contains(&key.weight) {
            return false;
        }
        let Some(channel) = self
            .channels
            .iter_mut()
            .find(|channel| channel.target == target)
        else {
            return false;
        };
        let Ok(index) = channel.keys.binary_search_by_key(&old_ms, |key| key.ms) else {
            return false;
        };
        if channel.keys[index] == key
            || (old_ms != key.ms
                && channel
                    .keys
                    .binary_search_by_key(&key.ms, |item| item.ms)
                    .is_ok())
        {
            return false;
        }
        channel.keys[index] = key;
        channel.keys.sort_unstable_by_key(|key| key.ms);
        true
    }

    pub fn remove_key(&mut self, target: &str, ms: u16) -> bool {
        let Some(index) = self
            .channels
            .iter()
            .position(|channel| channel.target == target)
        else {
            return false;
        };
        let keys = &mut self.channels[index].keys;
        let Ok(at) = keys.binary_search_by_key(&ms, |key| key.ms) else {
            return false;
        };
        keys.remove(at);
        if keys.is_empty() {
            self.channels.remove(index);
        }
        true
    }
}

impl Channel {
    fn sample(&self, millis: f32) -> f32 {
        let after = self.keys.partition_point(|key| f32::from(key.ms) <= millis);
        match (
            after.checked_sub(1).and_then(|index| self.keys.get(index)),
            self.keys.get(after),
        ) {
            (Some(start), Some(end)) => {
                let progress = (millis - f32::from(start.ms)) / f32::from(end.ms - start.ms);
                start.weight + (end.weight - start.weight) * progress
            }
            (Some(last), None) => last.weight,
            (None, Some(first)) if first.ms > 0 => first.weight * millis / f32::from(first.ms),
            _ => 0.0,
        }
    }
}

pub fn valid_name(target: &str) -> bool {
    !target.trim().is_empty() && target.len() <= 128 && !target.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_channels_interpolate_independently_and_keep_authored_zero() {
        let mut face = Lane::default();
        assert!(face.add_key("eyeBlinkLeft", 200));
        assert!(face.add_key("eyeBlinkLeft", 400));
        assert!(face.update_key(
            "eyeBlinkLeft",
            400,
            Key {
                ms: 400,
                weight: 0.0
            }
        ));
        assert!(face.add_key("jawOpen", 0));
        assert!(face.update_key(
            "jawOpen",
            0,
            Key {
                ms: 0,
                weight: 0.35
            }
        ));
        assert_eq!(face.weight("eyeBlinkLeft", 0.1), Some(0.5));
        assert_eq!(face.weight("eyeBlinkLeft", 0.3), Some(0.5));
        assert_eq!(face.weight("eyeBlinkLeft", 0.6), Some(0.0));
        assert_eq!(face.weight("eyeBlinkRight", 0.6), None);
        assert_eq!(face.weight("jawOpen", 9.9), Some(0.35));
        assert_eq!(Group::of("eyeLookUpLeft"), Group::Gaze);
        assert_eq!(Group::of("browInnerUp"), Group::Brow);
        assert_eq!(Group::of("eyeBlinkLeft"), Group::Blink);
        assert_eq!(Group::of("mouthFunnel"), Group::Mouth);
        assert!(face.validate().is_ok());
    }

    #[test]
    fn edited_keys_remain_unique_and_duration_never_truncates_them() {
        let mut face = Lane::default();
        face.add_key("jawOpen", 1000);
        face.add_key("jawOpen", 1500);
        assert!(!face.update_key(
            "jawOpen",
            1000,
            Key {
                ms: 1500,
                weight: 0.5
            }
        ));
        assert!(!face.add_key("jawOpen", 1500));
        assert!(face.resize(100));
        assert_eq!(face.duration_ms, 1500);
        assert!(face.update_key(
            "jawOpen",
            1500,
            Key {
                ms: 700,
                weight: 0.5
            }
        ));
        assert_eq!(face.channels[0].keys[0].ms, 700);
        assert!(face.remove_key("jawOpen", 700));
        assert!(face.remove_key("jawOpen", 1000));
        assert!(face.channels.is_empty());
        assert!(face.validate().is_ok());
    }
}
