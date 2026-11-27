//! Portable authored sound assets and animation/footstep bindings.
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
    sync::Arc,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::audio::{Clip, Surface};

pub type SoundId = u32;
pub const MAX_SOUNDS: usize = 32;
pub const MAX_SOUND_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_EMBEDDED_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_FOOTSTEP_ZONES: usize = 48;
pub const MAX_STEP_VARIANTS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Wav(Arc<[u8]>);

impl Serialize for Wav {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = String::with_capacity(self.0.len() * 2);
        for &byte in self.0.iter() {
            text.push(DIGITS[usize::from(byte >> 4)] as char);
            text.push(DIGITS[usize::from(byte & 15)] as char);
        }
        serializer.serialize_str(&text)
    }
}

impl<'de> Deserialize<'de> for Wav {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let source = String::deserialize(deserializer)?;
        if source.len() > MAX_SOUND_BYTES * 2 || !source.len().is_multiple_of(2) {
            return Err(serde::de::Error::custom(
                "Некорректный размер вложенного WAV.",
            ));
        }
        let mut bytes = Vec::with_capacity(source.len() / 2);
        for pair in source.as_bytes().chunks_exact(2) {
            let high = (pair[0] as char).to_digit(16);
            let low = (pair[1] as char).to_digit(16);
            let byte = high
                .zip(low)
                .map(|(high, low)| (high * 16 + low) as u8)
                .ok_or_else(|| serde::de::Error::custom("Повреждены вложенные данные WAV."))?;
            bytes.push(byte);
        }
        Ok(Self(bytes.into()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sound {
    pub id: SoundId,
    pub name: String,
    wav: Wav,
}

impl Sound {
    pub fn wav_bytes(&self) -> &[u8] {
        &self.wav.0
    }

    pub fn clip(&self) -> Result<Arc<Clip>, String> {
        Clip::decode_pcm_wav(self.wav_bytes()).map(Arc::new)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarkerSound {
    pub sound: SoundId,
    pub gain: f32,
    pub spatial: bool,
    pub reach: f32,
    #[serde(default)]
    pub offset: [f32; 3],
}

impl MarkerSound {
    pub fn new(sound: SoundId) -> Self {
        Self {
            sound,
            gain: 0.85,
            spatial: true,
            reach: 15.0,
            offset: [0.0; 3],
        }
    }

    fn valid(self) -> bool {
        self.sound != 0
            && (0.0..=2.0).contains(&self.gain)
            && (1.0..=256.0).contains(&self.reach)
            && self
                .offset
                .iter()
                .all(|value| (-64.0..=64.0).contains(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FootstepZone {
    pub zone: u32,
    pub surface: Surface,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FootstepSettings {
    pub enabled: bool,
    pub default_surface: Surface,
    pub stride: f32,
    pub gain: f32,
    pub zones: Vec<FootstepZone>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gait_contacts: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub samples: BTreeMap<Surface, Vec<SoundId>>,
}

impl Default for FootstepSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            default_surface: Surface::Stone,
            stride: 0.58,
            gain: 0.72,
            zones: Vec::new(),
            gait_contacts: false,
            samples: BTreeMap::new(),
        }
    }
}

impl FootstepSettings {
    pub fn surface(&self, contains: impl Fn(u32) -> bool) -> Surface {
        self.zones
            .iter()
            .rev()
            .find(|entry| contains(entry.zone))
            .map_or(self.default_surface, |entry| entry.surface)
    }

    fn valid(&self, sounds: &HashSet<SoundId>) -> bool {
        let mut zones = HashSet::new();
        (0.1..=8.0).contains(&self.stride)
            && (0.0..=2.0).contains(&self.gain)
            && self.zones.len() <= MAX_FOOTSTEP_ZONES
            && self
                .zones
                .iter()
                .all(|entry| entry.zone != 0 && zones.insert(entry.zone))
            && self.samples.values().all(|variants| {
                let mut seen = HashSet::new();
                !variants.is_empty()
                    && variants.len() <= MAX_STEP_VARIANTS
                    && variants
                        .iter()
                        .all(|id| sounds.contains(id) && seen.insert(*id))
            })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub sounds: Vec<Sound>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<MarkerSound>,
    pub footsteps: FootstepSettings,
    #[serde(skip_serializing_if = "AtmosphereSettings::is_default")]
    pub ambience: AtmosphereSettings,
    next_id: SoundId,
}

impl Library {
    pub fn is_default(&self) -> bool {
        self.sounds.is_empty()
            && self.marker.is_none()
            && self.footsteps == FootstepSettings::default()
            && self.ambience.is_default()
            && self.next_id == 0
    }

    pub fn sound(&self, id: SoundId) -> Option<&Sound> {
        self.sounds.iter().find(|sound| sound.id == id)
    }

    pub fn step_sample(&self, surface: Surface, ordinal: u64) -> Option<&Sound> {
        let variants = self.footsteps.samples.get(&surface)?;
        let index = ordinal.checked_rem(variants.len() as u64)? as usize;
        self.sound(variants[index])
    }

    pub fn byte_count(&self) -> usize {
        self.sounds
            .iter()
            .map(|sound| sound.wav_bytes().len())
            .sum()
    }

    pub fn import(&mut self, path: &Path) -> Result<SoundId, String> {
        let file = File::open(path).map_err(|error| format!("Не удалось открыть WAV. {error}"))?;
        let mut bytes = Vec::new();
        file.take(MAX_SOUND_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Не удалось прочитать WAV. {error}"))?;
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Звук")
            .chars()
            .filter(|ch| !ch.is_control())
            .take(64)
            .collect::<String>();
        self.import_bytes(
            if name.trim().is_empty() {
                "Звук".into()
            } else {
                name
            },
            bytes,
        )
    }

    pub fn import_bytes(&mut self, name: String, bytes: Vec<u8>) -> Result<SoundId, String> {
        if self.sounds.len() >= MAX_SOUNDS
            || bytes.len() > MAX_SOUND_BYTES
            || self.byte_count().saturating_add(bytes.len()) > MAX_EMBEDDED_BYTES
        {
            return Err("Превышен лимит звуков проекта или размер WAV в 2 МБ.".into());
        }
        if !valid_name(&name) {
            return Err("Некорректное имя звука.".into());
        }
        Clip::decode_pcm_wav(&bytes)?;
        let next = self
            .sounds
            .iter()
            .map(|sound| sound.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("Закончились номера звуков.")?;
        let id = next.max(self.next_id);
        self.next_id = id.checked_add(1).ok_or("Закончились номера звуков.")?;
        self.sounds.push(Sound {
            id,
            name,
            wav: Wav(bytes.into()),
        });
        Ok(id)
    }

    pub fn import_step_recording(&mut self, path: &Path, surface: Surface) -> Result<usize, String> {
        let file = File::open(path).map_err(|error| format!("Не удалось открыть дорожку. {error}"))?;
        let mut bytes = Vec::new();
        file.take(crate::audio::MAX_WAV_BYTES as u64 + 1).read_to_end(&mut bytes)
            .map_err(|error| format!("Не удалось прочитать дорожку. {error}"))?;
        self.install_step_recording(&bytes, surface)
    }

    pub fn install_step_recording(&mut self, bytes: &[u8], surface: Surface) -> Result<usize, String> {
        let clip = Clip::decode_pcm_wav(bytes)?;
        let frames = clip.frames();
        let block = (clip.sample_rate / 100) as usize;
        let envelope: Vec<f32> = frames.chunks(block).map(|samples| {
            let energy = samples.iter().map(|[l, r]| ((l + r) * 0.5).powi(2)).sum::<f32>();
            (energy / samples.len() as f32).sqrt()
        }).collect();
        let peak = envelope.iter().copied().fold(0.0_f32, f32::max);
        let mut sorted = envelope.clone();
        sorted.sort_unstable_by(f32::total_cmp);
        let floor = sorted[sorted.len() / 2];
        let threshold = (peak * 0.21).max(floor * 4.0).max(0.002);
        let mut peaks: Vec<_> = envelope.iter().enumerate().filter(|&(i, value)| {
            *value >= threshold && (i == 0 || *value > envelope[i - 1])
                && envelope.get(i + 1).is_none_or(|next| *value >= *next)
        }).map(|(index, _)| index).collect();
        peaks.sort_unstable_by(|&a, &b| envelope[b].total_cmp(&envelope[a]));
        let mut selected = Vec::<usize>::new();
        for peak in peaks {
            if selected.iter().all(|point| point.abs_diff(peak) >= 24) {
                selected.push(peak);
                if selected.len() == MAX_STEP_VARIANTS { break; }
            }
        }
        if selected.len() < 2 {
            return Err("Нужно хотя бы два отдельных шага с короткими паузами между ними.".into());
        }
        selected.sort_unstable();
        let gain = (0.60 / frames.iter().map(|[l, r]| l.abs().max(r.abs())).fold(0.002, f32::max)).min(10.0);
        let mut draft = self.clone();
        let mut variants = Vec::new();
        for (ordinal, &index) in selected.iter().enumerate() {
            let margin = (envelope[index] * 0.065).max(floor * 1.8);
            let start = (index.saturating_sub(12)..index).rev()
                .find(|&i| envelope[i] <= margin).unwrap_or(index.saturating_sub(12));
            let stop = ((index + 8)..(index + 24).min(envelope.len()))
                .find(|&i| envelope[i] <= margin).unwrap_or((index + 22).min(envelope.len()));
            let start = start.saturating_sub(1) * block;
            let end = ((stop + 2) * block).min(frames.len());
            let mut sample = frames[start..end].to_vec();
            let length = sample.len();
            let fade = (block / 2).max(1) as f32;
            for (i, [l, r]) in sample.iter_mut().enumerate() {
                let mix = (*l + *r) * 0.5 * gain * (i.min(length - 1 - i) as f32 / fade).min(1.0);
                *l = mix;
                *r = mix;
            }
            let wav = crate::audio::pcm16_stereo_wav(&sample, clip.sample_rate)?;
            let name = format!("Шаг · {} · {}", surface.label(), ordinal + 1);
            let existing = draft.sounds.iter().find(|sound| sound.name == name && sound.wav_bytes() == wav);
            variants.push(match existing {
                Some(sound) => sound.id,
                None => draft.import_bytes(name, wav)?,
            });
        }
        draft.footsteps.samples.insert(surface, variants);
        draft.footsteps.enabled = true;
        draft.footsteps.gait_contacts = true;
        *self = draft;
        Ok(selected.len())
    }

    pub fn remove(&mut self, id: SoundId) -> bool {
        let Some(index) = self.sounds.iter().position(|sound| sound.id == id) else {
            return false;
        };
        self.sounds.remove(index);
        if self.marker.is_some_and(|marker| marker.sound == id) {
            self.marker = None;
        }
        self.footsteps.samples.retain(|_, variants| {
            variants.retain(|variant| *variant != id);
            !variants.is_empty()
        });
        true
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        if self.sounds.len() > MAX_SOUNDS
            || self.byte_count() > MAX_EMBEDDED_BYTES
            || self.sounds.iter().any(|sound| {
                sound.id == 0
                    || !ids.insert(sound.id)
                    || sound.wav_bytes().len() > MAX_SOUND_BYTES
                    || !valid_name(&sound.name)
            })
            || self
                .marker
                .is_some_and(|marker| !marker.valid() || !ids.contains(&marker.sound))
            || !self.footsteps.valid(&ids)
            || !self.ambience.valid()
        {
            return Err("Некорректные настройки аудио или лимит файлов проекта.".into());
        }
        for sound in &self.sounds {
            sound
                .clip()
                .map_err(|error| format!("«{}». {error}", sound.name))?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AtmosphereSettings {
    pub enabled: bool,
    pub rain: f32,
    pub room: f32,
    pub cloth: f32,
    pub breath: f32,
}

impl Default for AtmosphereSettings {
    fn default() -> Self {
        Self { enabled: false, rain: 0.23, room: 0.14, cloth: 0.41, breath: 0.43 }
    }
}

impl AtmosphereSettings {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    fn valid(self) -> bool {
        (0.0..=0.45).contains(&self.rain) && (0.0..=0.32).contains(&self.room)
            && (0.0..=1.0).contains(&self.cloth) && (0.0..=1.0).contains(&self.breath)
    }
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 64 && name.chars().all(|ch| !ch.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_steps_are_split_bound_and_deduplicated() {
        let source = include_bytes!("../demo_assets/character-showcase/mira-local-walk.wav");
        let mut project = Library::default();
        assert_eq!(project.install_step_recording(source, Surface::Stone).unwrap(), 6);
        assert!(project.footsteps.gait_contacts);
        for ordinal in 0..6 {
            let clip = project.step_sample(Surface::Stone, ordinal).unwrap().clip().unwrap();
            assert!((0.08..0.42).contains(&clip.duration()));
            let peak = clip.frames().iter().map(|pair| pair[0].abs()).fold(0.0, f32::max);
            assert!((0.035..0.66).contains(&peak));
        }
        let initial = project.clone();
        project.install_step_recording(source, Surface::Stone).unwrap();
        assert_eq!(initial, project);
        assert!(project.install_step_recording(b"invalid", Surface::Stone).is_err());
        assert_eq!(initial, project);
    }
    use crate::audio::{OUTPUT_RATE, pcm16_stereo_wav};

    #[test]
    fn embedding_carries_exact_wav_and_typed_marker_without_source_file() {
        let wav = pcm16_stereo_wav(&[[0.25, -0.125]; 256], OUTPUT_RATE).unwrap();
        let mut lib = Library::default();
        let id = lib
            .import_bytes("Звон по метке".into(), wav.clone())
            .unwrap();
        lib.marker = Some(MarkerSound::new(id));
        lib.footsteps.enabled = true;
        lib.footsteps.zones.push(FootstepZone {
            zone: 7,
            surface: Surface::Wood,
        });
        let bytes = serde_json::to_vec(&lib).unwrap();
        let restored: Library = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored, lib);
        assert_eq!(restored.sound(id).unwrap().wav_bytes(), wav);
        assert_eq!(
            restored.sound(id).unwrap().clip().unwrap().frames()[0],
            [0.25, -0.125]
        );
        assert_eq!(restored.footsteps.surface(|zone| zone == 7), Surface::Wood);
        assert_eq!(restored.footsteps.surface(|_| false), Surface::Stone);
    }

    #[test]
    fn footstep_variants_round_trip_and_removed_sounds_leave_no_dangling_ids() {
        let mut lib = Library::default();
        let wav = pcm16_stereo_wav(&[[0.25, -0.125]; 256], OUTPUT_RATE).unwrap();
        let left = lib.import_bytes("Левый".into(), wav.clone()).unwrap();
        let right = lib.import_bytes("Правый".into(), wav).unwrap();
        lib.footsteps
            .samples
            .insert(Surface::Stone, vec![left, right]);
        let json = serde_json::to_vec(&lib).unwrap();
        let mut restored: Library = serde_json::from_slice(&json).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.step_sample(Surface::Stone, 0).unwrap().id, left);
        assert_eq!(restored.step_sample(Surface::Stone, 1).unwrap().id, right);
        assert_eq!(restored.step_sample(Surface::Stone, 2).unwrap().id, left);
        assert!(restored.step_sample(Surface::Wood, 0).is_none());
        assert!(restored.remove(left));
        assert_eq!(restored.step_sample(Surface::Stone, 0).unwrap().id, right);
        assert!(restored.remove(right));
        assert!(restored.footsteps.samples.is_empty());
        restored.validate().unwrap();
    }
}
