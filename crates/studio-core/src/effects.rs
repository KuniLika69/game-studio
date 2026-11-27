//! Reusable particle presets and deterministic, time-based emitters.
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use glam::Vec3;
use serde::{Deserialize, Serialize};

pub type PresetId = u32;
pub type EmitterId = u64;
pub const MAX_PRESETS: usize = 64;
pub const MAX_ACTIVE: usize = 24;
pub const MAX_PARTICLES: usize = 768;
const MAX_PER_PARTICLE_EFFECT: usize = 192;
const MAX_ANCHOR_KEYS: usize = 320;
const MAX_FILE_BYTES: u64 = 16 * 1024;
const FORMAT: u8 = 1;
static FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Smoke,
    Dust,
    Rain,
    Splash,
    Trail,
}

impl Kind {
    pub const ALL: [Self; 5] = [
        Self::Smoke,
        Self::Dust,
        Self::Rain,
        Self::Splash,
        Self::Trail,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Smoke => "Дым",
            Self::Dust => "Пыль",
            Self::Rain => "Дождь",
            Self::Splash => "Брызги",
            Self::Trail => "След",
        }
    }

    fn profile(self) -> Profile {
        match self {
            Self::Smoke => Profile {
                rate: 29.0,
                lifetime: 1.55,
                default_duration: 2.1,
                color: [0.57, 0.62, 0.67],
            },
            Self::Dust => Profile {
                rate: 56.0,
                lifetime: 0.86,
                default_duration: 1.2,
                color: [0.80, 0.64, 0.43],
            },
            Self::Rain => Profile {
                rate: 93.0,
                lifetime: 0.53,
                default_duration: 3.0,
                color: [0.55, 0.76, 0.91],
            },
            Self::Splash => Profile {
                rate: 156.0,
                lifetime: 0.68,
                default_duration: 0.85,
                color: [0.40, 0.82, 0.95],
            },
            Self::Trail => Profile {
                rate: 47.0,
                lifetime: 0.92,
                default_duration: 2.3,
                color: [0.38, 0.85, 0.71],
            },
        }
    }
}

struct Profile {
    rate: f32,
    lifetime: f32,
    default_duration: f32,
    color: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub name: String,
    pub kind: Kind,
    /// In the editor this color is displayed and selected in sRGB.
    pub color: [f32; 3],
    pub intensity: f32,
    /// The full visible time, in seconds.
    pub duration: f32,
}

impl Settings {
    pub fn for_kind(kind: Kind) -> Self {
        let profile = kind.profile();
        Self {
            name: kind.label().into(),
            kind,
            color: profile.color,
            intensity: 1.0,
            duration: profile.default_duration,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > 64
            || self.name.chars().any(char::is_control)
            || !self.color.iter().all(|value| (0.0..=1.0).contains(value))
            || !(0.0..=3.0).contains(&self.intensity)
            || !(0.05..=30.0).contains(&self.duration)
        {
            return Err("Некорректное имя, цвет, интенсивность или длительность эффекта.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: PresetId,
    #[serde(flatten)]
    pub settings: Settings,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub presets: Vec<Preset>,
    /// The timeline is owned by the scene's animation. Here we keep only its chosen effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker_effect: Option<PresetId>,
    next_id: PresetId,
}

impl Library {
    pub fn is_default(&self) -> bool {
        self.presets.is_empty() && self.marker_effect.is_none() && self.next_id == 0
    }

    pub fn preset(&self, id: PresetId) -> Option<&Preset> {
        self.presets.iter().find(|preset| preset.id == id)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::with_capacity(self.presets.len());
        if self.presets.len() > MAX_PRESETS
            || self
                .presets
                .iter()
                .any(|preset| preset.id == 0 || !ids.insert(preset.id))
            || self.marker_effect.is_some_and(|id| !ids.contains(&id))
        {
            return Err("Лимит, номера эффектов или привязка к метке некорректны.".into());
        }
        for preset in &self.presets {
            preset.settings.validate()?;
        }
        Ok(())
    }

    pub fn create(&mut self, kind: Kind) -> Option<PresetId> {
        self.insert(Settings::for_kind(kind)).ok()
    }

    fn insert(&mut self, settings: Settings) -> Result<PresetId, String> {
        settings.validate()?;
        if self.presets.len() >= MAX_PRESETS {
            return Err("Библиотека эффектов заполнена.".into());
        }
        let next = self
            .presets
            .iter()
            .map(|preset| preset.id)
            .max()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or("Закончились номера эффектов.")?;
        let id = self.next_id.max(next);
        self.next_id = id.checked_add(1).ok_or("Закончились номера эффектов.")?;
        self.presets.push(Preset { id, settings });
        Ok(id)
    }

    pub fn remove(&mut self, id: PresetId) -> bool {
        let Some(index) = self.presets.iter().position(|preset| preset.id == id) else {
            return false;
        };
        self.presets.remove(index);
        if self.marker_effect == Some(id) {
            self.marker_effect = None;
        }
        true
    }

    pub fn export(&self, id: PresetId, path: &Path) -> Result<(), String> {
        let preset = self.preset(id).ok_or("Эффект для экспорта отсутствует.")?;
        preset.settings.validate()?;
        let file = Portable {
            version: FORMAT,
            effect: preset.settings.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?;
        atomic_write(path, &bytes).map_err(|error| format!("Не удалось сохранить эффект. {error}"))
    }

    pub fn import(&mut self, path: &Path) -> Result<PresetId, String> {
        let file =
            File::open(path).map_err(|error| format!("Не удалось открыть эффект. {error}"))?;
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err("Файл эффекта больше 16 КБ.".into());
        }
        let portable: Portable = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Файл эффекта не читается. {error}"))?;
        if portable.version != FORMAT {
            return Err(format!(
                "Версия файла эффекта {} пока не поддерживается.",
                portable.version
            ));
        }
        self.insert(portable.effect)
    }

    pub fn animation_effect(&self, old: f32, next: f32, marker: f32) -> Option<PresetId> {
        self.marker_effect
            .filter(|_| crossed_marker(old, next, marker))
    }
}

pub fn crossed_marker(old: f32, next: f32, marker: f32) -> bool {
    if old == next || !old.is_finite() || !next.is_finite() || !marker.is_finite() {
        return false;
    }
    if next > old {
        (old..next).contains(&marker)
    } else {
        marker >= old || marker < next
    }
}

#[derive(Serialize, Deserialize)]
struct Portable {
    version: u8,
    effect: Settings,
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let filename = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Нет имени файла"))?;
    let suffix = format!(
        ".{}.{}.tmp",
        std::process::id(),
        FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let mut temporary_name = filename.to_os_string();
    temporary_name.push(suffix);
    let temporary = path.with_file_name(temporary_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    let result = result.and_then(|_| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub kind: Kind,
    pub position: Vec3,
    pub velocity: Vec3,
    /// Radius, in world units.
    pub radius: f32,
    pub color: [f32; 3],
    pub opacity: f32,
}

#[derive(Default)]
pub struct Session {
    active: Vec<Emitter>,
    next: EmitterId,
}

struct Emitter {
    id: EmitterId,
    settings: Settings,
    seed: u64,
    start: f64,
    origin: Vec3,
    anchors: Vec<Key>,
}

#[derive(Clone, Copy)]
struct Key {
    time: f64,
    position: Vec3,
}

impl Session {
    pub fn is_active(&self) -> bool {
        !self.active.is_empty()
    }

    pub fn play(
        &mut self,
        library: &Library,
        id: PresetId,
        origin: Vec3,
        seconds: f64,
    ) -> Option<EmitterId> {
        let settings = &library.preset(id)?.settings;
        self.spawn(settings, origin, seconds, u64::from(id))
    }

    pub fn spawn(
        &mut self,
        settings: &Settings,
        origin: Vec3,
        seconds: f64,
        seed: u64,
    ) -> Option<EmitterId> {
        if !origin.is_finite()
            || !seconds.is_finite()
            || seconds < 0.0
            || settings.validate().is_err()
            || settings.intensity == 0.0
        {
            return None;
        }
        let id = self.next.checked_add(1)?;
        self.expire(seconds);
        if self.active.len() >= MAX_ACTIVE {
            self.active.remove(0);
        }
        self.next = id;
        self.active.push(Emitter {
            id,
            settings: settings.clone(),
            seed: hash(seed ^ id.rotate_left(27)),
            start: seconds,
            origin,
            anchors: Vec::new(),
        });
        Some(id)
    }

    pub fn move_emitter(&mut self, id: EmitterId, origin: Vec3, seconds: f64) -> bool {
        if !origin.is_finite() || !seconds.is_finite() {
            return false;
        }
        let Some(emitter) = self.active.iter_mut().find(|emitter| emitter.id == id) else {
            return false;
        };
        if emitter.settings.kind != Kind::Trail
            || seconds < emitter.start
            || seconds >= emitter.start + f64::from(emitter.settings.duration)
        {
            return false;
        }
        if emitter.anchors.is_empty() {
            emitter.anchors.push(Key {
                time: emitter.start,
                position: emitter.origin,
            });
        }
        let last = emitter.anchors.last_mut().expect("initial anchor");
        if seconds < last.time {
            return false;
        }
        if seconds == last.time {
            last.position = origin;
        } else {
            emitter.anchors.push(Key {
                time: seconds,
                position: origin,
            });
        }
        // Keep one key before the oldest possible surviving particle for interpolation.
        let cutoff = seconds - 1.6;
        let old = emitter
            .anchors
            .partition_point(|key| key.time < cutoff)
            .saturating_sub(1);
        let excess = emitter.anchors.len().saturating_sub(MAX_ANCHOR_KEYS);
        emitter.anchors.drain(..old.max(excess));
        true
    }

    pub fn expire(&mut self, seconds: f64) {
        if seconds.is_finite() {
            self.active
                .retain(|item| seconds < item.start + f64::from(item.settings.duration));
        }
    }

    pub fn samples(
        &self,
        seconds: f64,
        budget: usize,
        wind_at: impl Fn(Vec3) -> Vec3,
    ) -> Vec<Particle> {
        let mut particles = Vec::new();
        let cap = budget.min(MAX_PARTICLES);
        if cap == 0 || !seconds.is_finite() {
            return particles;
        }
        for emitter in &self.active {
            if particles.len() == cap {
                break;
            }
            emitter.append(seconds, &wind_at, cap, &mut particles);
        }
        particles
    }
}

impl Emitter {
    fn origin_at(&self, time: f64) -> Vec3 {
        let after = self.anchors.partition_point(|key| key.time <= time);
        if after == 0 {
            return self.anchors.first().map_or(self.origin, |key| key.position);
        }
        let earlier = self.anchors[after - 1];
        let Some(later) = self.anchors.get(after) else {
            return earlier.position;
        };
        let blend = ((time - earlier.time) / (later.time - earlier.time)) as f32;
        earlier.position.lerp(later.position, blend)
    }

    fn append(
        &self,
        seconds: f64,
        wind_at: &impl Fn(Vec3) -> Vec3,
        cap: usize,
        particles: &mut Vec<Particle>,
    ) {
        let elapsed = seconds - self.start;
        if !(0.0..f64::from(self.settings.duration)).contains(&elapsed) {
            return;
        }
        let kind = self.settings.kind;
        let profile = kind.profile();
        let rate = f64::from((profile.rate * self.settings.intensity).max(1.0));
        let ttl = f64::from(profile.lifetime.min(self.settings.duration));
        let ending = if kind == Kind::Splash {
            elapsed.min(ttl * 0.26)
        } else {
            elapsed
        };
        let to = (ending * rate).floor() as u32;
        let from = (((elapsed - ttl).max(0.0) * rate).floor() as u32)
            .max(to.saturating_sub(MAX_PER_PARTICLE_EFFECT as u32 - 1));
        let tail = 1.0 - smooth01(((elapsed as f32 / self.settings.duration) - 0.78) / 0.22);
        let stationary_wind = self.anchors.is_empty().then(|| wind_at(self.origin));
        for birth in from..=to {
            if particles.len() >= cap {
                break;
            }
            let released = f64::from(birth) / rate;
            let age = (elapsed - released) as f32;
            if age < 0.0 || age >= ttl as f32 {
                continue;
            }
            let progress = age / ttl as f32;
            let direction = random_direction(self.seed, birth);
            let a = random(self.seed, birth, 1);
            let b = random(self.seed, birth, 2);
            let c = random(self.seed, birth, 3);
            let origin = self.origin_at(self.start + released);
            let wind = stationary_wind.unwrap_or_else(|| wind_at(origin));
            let wind = if wind.is_finite() {
                wind.clamp_length_max(22.0)
            } else {
                Vec3::ZERO
            };
            let sample = self.particle(origin, direction, wind, age, progress, [a, b, c]);
            if let Some(mut sample) = sample {
                sample.opacity *= tail * (0.70 + 0.30 * self.settings.intensity.min(1.0));
                if sample.opacity > 0.001 {
                    particles.push(sample);
                }
            }
        }
    }

    fn particle(
        &self,
        origin: Vec3,
        radial: Vec3,
        wind: Vec3,
        t: f32,
        progress: f32,
        [a, b, c]: [f32; 3],
    ) -> Option<Particle> {
        let (offset, velocity, acceleration, radius, alpha) = match self.settings.kind {
            Kind::Smoke => (
                radial * a * 0.20,
                radial * b * 0.25 + Vec3::Y * (0.38 + c * 0.47) + wind * 0.20,
                Vec3::ZERO,
                0.095 + b * 0.095 + progress * 0.22,
                (0.20 + c * 0.20) * smooth01(t * 13.0) * (1.0 - progress).powi(2),
            ),
            Kind::Dust => (
                radial * (a * 0.24) + Vec3::Y * 0.04,
                radial * (0.35 + b * 0.9) + Vec3::Y * (0.20 + c * 0.31) + wind * 0.14,
                Vec3::NEG_Y * 0.50,
                0.044 + a * 0.059 + progress * 0.13,
                (0.27 + b * 0.25) * (1.0 - progress).powi(2),
            ),
            Kind::Rain => (
                radial * (a.sqrt() * 2.2) + Vec3::Y * (2.0 + b * 1.2),
                wind * 0.55 + Vec3::NEG_Y * (4.6 + c * 2.0),
                Vec3::ZERO,
                0.012 + c * 0.009,
                (0.22 + a * 0.39) * (1.0 - progress * 0.76),
            ),
            Kind::Splash => (
                radial * (a * 0.12),
                radial * (0.6 + b * 1.4) + Vec3::Y * (1.43 + c * 1.9) + wind * 0.09,
                Vec3::NEG_Y * 9.81,
                0.017 + a * 0.029,
                (0.46 + b * 0.46) * (1.0 - progress),
            ),
            Kind::Trail => (
                radial * (a * 0.095) + Vec3::Y * 0.055,
                radial * (b * 0.24) + Vec3::Y * (0.08 + c * 0.15) + wind * 0.10,
                Vec3::ZERO,
                0.044 + a * 0.024 + progress * 0.07,
                (0.33 + b * 0.36) * (1.0 - progress).powi(2),
            ),
        };
        let position = origin + offset + velocity * t + acceleration * (t * t * 0.5);
        if matches!(self.settings.kind, Kind::Splash | Kind::Rain) && position.y < origin.y {
            return None;
        }
        Some(Particle {
            kind: self.settings.kind,
            position,
            velocity: velocity + acceleration * t,
            radius,
            color: self.settings.color,
            opacity: alpha.clamp(0.0, 1.0),
        })
    }
}

fn random_direction(seed: u64, index: u32) -> Vec3 {
    let angle = random(seed, index, 0) * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    Vec3::new(cos, 0.0, sin)
}

fn random(seed: u64, index: u32, lane: u64) -> f32 {
    let word = hash(seed ^ (u64::from(index) << 32) ^ lane.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    (word >> 40) as f32 / 16_777_216.0
}

fn hash(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn smooth01(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_preset_emits_its_own_shape_and_motion() {
        for kind in Kind::ALL {
            let settings = Settings::for_kind(kind);
            let mut session = Session::default();
            session.spawn(&settings, Vec3::ZERO, 0.0, 17).unwrap();
            let shape = session.samples(0.24, 200, |_| Vec3::ZERO);
            assert!(!shape.is_empty(), "{kind:?}");
            assert!(
                shape
                    .iter()
                    .all(|sample| sample.kind == kind && sample.radius > 0.0)
            );
            match kind {
                Kind::Smoke => assert!(
                    shape
                        .iter()
                        .all(|particle| particle.velocity.y > 0.35 && particle.radius > 0.09)
                ),
                Kind::Dust => assert!(shape.iter().all(|particle| particle.position.y < 0.28)),
                Kind::Rain => assert!(
                    shape
                        .iter()
                        .all(|particle| particle.velocity.y < -4.0 && particle.radius < 0.024)
                ),
                Kind::Splash => {
                    assert!(shape.iter().any(|particle| particle.position.y > 0.22));
                    assert!(shape.iter().any(|particle| particle.velocity.y < 0.0));
                }
                Kind::Trail => {
                    assert!(shape.iter().all(|particle| particle.position.x.abs() < 0.2))
                }
            }
        }
    }

    #[test]
    fn particles_are_seeded_and_independent_of_intermediate_frames() {
        let settings = Settings::for_kind(Kind::Smoke);
        let mut first = Session::default();
        first.spawn(&settings, Vec3::Y, 4.0, 16).unwrap();
        let mut other = Session::default();
        other.spawn(&settings, Vec3::Y, 4.0, 16).unwrap();
        let mut changed = Session::default();
        changed.spawn(&settings, Vec3::Y, 4.0, 17).unwrap();
        let winds = |_| Vec3::new(4.5, 0.0, 0.0);
        for frame in 1..=40 {
            first.samples(4.0 + f64::from(frame) / 60.0, 80, winds);
        }
        let followup = first.samples(4.72, 80, winds);
        assert_eq!(followup, other.samples(4.72, 80, winds));
        assert_ne!(followup, changed.samples(4.72, 80, winds));
        let calm = first.samples(4.72, 80, |_| Vec3::ZERO);
        assert!(
            calm.iter()
                .zip(followup)
                .all(|(old, new)| old.position.x < new.position.x)
        );
    }

    #[test]
    fn moving_trail_spawns_in_historical_positions() {
        let settings = Settings::for_kind(Kind::Trail);
        let mut session = Session::default();
        let emitter = session.spawn(&settings, Vec3::ZERO, 0.0, 4).unwrap();
        session.move_emitter(emitter, Vec3::X * 2.0, 0.5);
        session.move_emitter(emitter, Vec3::X * 4.0, 1.0);
        let trail = session.samples(1.0, 200, |_| Vec3::ZERO);
        assert!(trail.iter().any(|sample| sample.position.x < 1.3));
        assert!(trail.iter().any(|sample| sample.position.x > 3.6));
        assert!(trail.iter().all(|sample| sample.position.x <= 4.13));
    }

    #[test]
    fn culling_keeps_preview_cost_bounded_and_effects_finish_at_duration() {
        let mut library = Library::default();
        let id = library.create(Kind::Dust).unwrap();
        let mut session = Session::default();
        let mut first = 0;
        for index in 0..MAX_ACTIVE + 7 {
            let handle = session
                .play(&library, id, Vec3::X * index as f32, 0.0)
                .unwrap();
            if index == 0 {
                first = handle;
            }
        }
        assert_eq!(session.active.len(), MAX_ACTIVE);
        assert!(session.active.iter().all(|item| item.id != first));
        assert_eq!(session.samples(0.2, 3, |_| Vec3::ZERO).len(), 3);
        assert!(session.samples(1.21, usize::MAX, |_| Vec3::ZERO).is_empty());
        session.expire(1.21);
        assert!(session.active.is_empty());
    }

    #[test]
    fn marker_is_crossed_once_and_works_after_wrapping() {
        let mut library = Library::default();
        let effect = library.create(Kind::Splash).unwrap();
        library.marker_effect = Some(effect);
        assert_eq!(library.animation_effect(0.64, 0.72, 0.72), None);
        assert_eq!(library.animation_effect(0.72, 0.75, 0.72), Some(effect));
        assert_eq!(library.animation_effect(0.75, 0.87, 0.72), None);
        assert_eq!(library.animation_effect(0.92, 0.04, 0.01), Some(effect));
        assert_eq!(library.animation_effect(0.50, 0.50, 0.50), None);
        assert!(library.remove(effect));
        assert_eq!(library.marker_effect, None);
        let later = library.create(Kind::Dust).unwrap();
        assert!(later > effect);
    }

    #[test]
    fn scene_and_individual_portable_preset_roundtrip_without_reusing_ids() {
        let mut source = Library::default();
        let id = source.create(Kind::Rain).unwrap();
        let definition = &mut source.presets[0].settings;
        definition.name = "Синий ливень".into();
        definition.color = [0.16, 0.58, 0.68];
        definition.intensity = 2.4;
        definition.duration = 7.5;
        source.marker_effect = Some(id);
        let original = serde_json::to_vec_pretty(&source).unwrap();
        let decoded: Library = serde_json::from_slice(&original).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, source);

        let leaf = format!(
            "studio-{}-{}.effect.json",
            std::process::id(),
            FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(leaf);
        source.export(id, &path).unwrap();
        let mut target = Library::default();
        let old = target.create(Kind::Smoke).unwrap();
        let imported = target.import(&path).unwrap();
        assert_ne!(imported, old);
        assert_eq!(
            target.preset(imported).unwrap().settings,
            source.preset(id).unwrap().settings
        );
        assert_eq!(target.marker_effect, None);

        let previous = target.clone();
        fs::write(
            &path,
            serde_json::to_vec(&Portable {
                version: FORMAT + 1,
                effect: source.preset(id).unwrap().settings.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(target.import(&path).is_err());
        assert_eq!(target, previous);
        fs::remove_file(path).unwrap();
    }
}
