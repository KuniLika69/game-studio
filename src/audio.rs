//! PCM WAV assets and the shared deterministic game mixer.
use std::{
    f32::consts::{FRAC_PI_4, TAU},
    sync::Arc,
};

use glam::Vec3;
use serde::{Deserialize, Serialize};

#[path = "audio_ambience.rs"]
mod ambience;
pub use ambience::AmbientLevels;
use ambience::{Ambience, Noise};
pub const MAX_WAV_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_VOICES: usize = 32;
pub const OUTPUT_RATE: u32 = 48_000;
const PCM_SUBTYPE: [u8; 16] = [
    1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
];

#[derive(Clone, Debug)]
pub struct Clip {
    pub sample_rate: u32,
    pub channels: u16,
    frames: Arc<[[f32; 2]]>,
}

impl Clip {
    pub fn decode_pcm_wav(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 12
            || bytes.len() > MAX_WAV_BYTES
            || bytes.get(..4) != Some(b"RIFF")
            || bytes.get(8..12) != Some(b"WAVE")
        {
            return Err("Нужен файл RIFF/WAVE PCM до 16 МБ.".into());
        }
        let end = 8_usize
            .checked_add(u32_at(bytes, 4)? as usize)
            .ok_or("Повреждён размер WAV.")?;
        if !(12..=bytes.len()).contains(&end) {
            return Err("Файл WAV обрывается раньше заявленного размера.".into());
        }
        let mut pos = 12;
        let mut format = None;
        let mut data = None;
        while pos < end {
            if end - pos < 8 {
                return Err("Повреждён заголовок блока WAV.".into());
            }
            let size = u32_at(bytes, pos + 4)? as usize;
            let from = pos + 8;
            let to = from.checked_add(size).ok_or("Слишком большой блок WAV.")?;
            let next = to
                .checked_add(size & 1)
                .ok_or("Слишком большой блок WAV.")?;
            if next > end {
                return Err("Данные блока WAV обрываются.".into());
            }
            match &bytes[pos..pos + 4] {
                b"fmt " if format.is_none() => format = Some(Format::read(&bytes[from..to])?),
                b"data" if data.is_none() => data = Some(&bytes[from..to]),
                b"fmt " | b"data" => return Err("Дублируется блок формата или данных WAV.".into()),
                _ => {}
            }
            pos = next;
        }

        let format = format.ok_or("В файле нет формата WAV.")?;
        let data = data.ok_or("В файле нет звуковых данных.")?;
        let stride = usize::from(format.alignment);
        if data.is_empty() || !data.len().is_multiple_of(stride) {
            return Err("Длина данных WAV не кратна размеру аудиокадра.".into());
        }
        let depth = usize::from(format.bits / 8);
        let frames = data
            .chunks_exact(stride)
            .map(|frame| {
                let left = pcm(&frame[..depth], format.bits);
                let right = if format.channels == 1 {
                    left
                } else {
                    pcm(&frame[depth..], format.bits)
                };
                [left, right]
            })
            .collect::<Vec<_>>();
        Ok(Self {
            sample_rate: format.rate,
            channels: format.channels,
            frames: frames.into(),
        })
    }

    pub fn frames(&self) -> &[[f32; 2]] {
        &self.frames
    }

    pub fn duration(&self) -> f64 {
        self.frames.len() as f64 / f64::from(self.sample_rate)
    }

    pub(crate) fn body_noise(breath: bool) -> Self {
        let mut noise = Noise::default();
        let seconds = if breath { 0.56 } else { 0.21 };
        let mut low = 0.0;
        let mut high = 0.0;
        let frames = (0..(OUTPUT_RATE as f32 * seconds) as usize).map(|index| {
            let time = index as f32 / OUTPUT_RATE as f32;
            let grain = noise.next();
            low += (grain - low) * if breath { 0.075 } else { 0.18 };
            high += (grain - high) * if breath { 0.009 } else { 0.06 };
            let profile = if breath {
                (std::f32::consts::PI * time / seconds).sin().max(0.0).powi(2)
            } else {
                (time * 210.0).min(1.0) * (-time * 21.0).exp() * (0.78 + 0.22 * (time * 105.0).sin())
            };
            let sound = (low - high) * profile * if breath { 0.24 } else { 0.33 };
            [sound, sound]
        }).collect::<Vec<_>>();
        Self { frames: frames.into(), channels: 1, sample_rate: OUTPUT_RATE }
    }

    pub(crate) fn door_latch() -> Self {
        let frames = (0..OUTPUT_RATE / 6).map(|index| {
            let t = index as f32 / OUTPUT_RATE as f32;
            let strike = |delay: f32, decay: f32, pitch: f32| {
                let age = (t - delay).max(0.0);
                let rise = (age * 1150.0).min(1.0);
                rise * (-age * decay).exp() * (TAU * pitch * age).sin()
            };
            let sound = 0.28 * strike(0.0, 67.0, 1590.0)
                + 0.20 * strike(0.012, 96.0, 2813.0)
                + 0.19 * strike(0.041, 40.0, 225.0);
            [sound, sound]
        }).collect::<Vec<_>>();
        Self { frames: frames.into(), channels: 1, sample_rate: OUTPUT_RATE }
    }
}

#[derive(Clone, Copy)]
struct Format {
    channels: u16,
    rate: u32,
    alignment: u16,
    bits: u16,
}

impl Format {
    fn read(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 16 {
            return Err("Неполный блок формата WAV.".into());
        }
        let tag = u16_at(bytes, 0)?;
        let channels = u16_at(bytes, 2)?;
        let rate = u32_at(bytes, 4)?;
        let byte_rate = u32_at(bytes, 8)?;
        let alignment = u16_at(bytes, 12)?;
        let bits = u16_at(bytes, 14)?;
        let integer_pcm = tag == 1
            || tag == 0xfffe
                && bytes.len() >= 40
                && u16_at(bytes, 16)? >= 22
                && (1..=bits).contains(&u16_at(bytes, 18)?)
                && bytes[24..40] == PCM_SUBTYPE;
        if !integer_pcm
            || !matches!(channels, 1 | 2)
            || !matches!(bits, 8 | 16 | 24 | 32)
            || !(8_000..=192_000).contains(&rate)
            || alignment != channels * (bits / 8)
            || byte_rate != rate * u32::from(alignment)
        {
            return Err(
                "Поддерживается целочисленный PCM WAV, моно или стерео, 8/16/24/32 бит.".into(),
            );
        }
        Ok(Self {
            channels,
            rate,
            alignment,
            bits,
        })
    }
}

fn u16_at(bytes: &[u8], index: usize) -> Result<u16, String> {
    bytes
        .get(index..index + 2)
        .map(|value| u16::from_le_bytes(value.try_into().unwrap()))
        .ok_or_else(|| "Неполные данные WAV.".into())
}

fn u32_at(bytes: &[u8], index: usize) -> Result<u32, String> {
    bytes
        .get(index..index + 4)
        .map(|value| u32::from_le_bytes(value.try_into().unwrap()))
        .ok_or_else(|| "Неполные данные WAV.".into())
}

fn pcm(bytes: &[u8], bits: u16) -> f32 {
    match bits {
        8 => (f32::from(bytes[0]) - 128.0) / 128.0,
        16 => f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32_768.0,
        24 => {
            let high = if bytes[2] & 0x80 == 0 { 0 } else { 0xff };
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], high]) as f32 / 8_388_608.0
        }
        32 => i32::from_le_bytes(bytes.try_into().unwrap()) as f32 / 2_147_483_648.0,
        _ => unreachable!("validated PCM width"),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Listener {
    pub position: Vec3,
    pub right: Vec3,
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            right: Vec3::X,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Source {
    Direct,
    World {
        position: Vec3,
        reference_distance: f32,
        max_distance: f32,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct VoiceSettings {
    pub source: Source,
    pub gain: f32,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            source: Source::Direct,
            gain: 1.0,
        }
    }
}

impl VoiceSettings {
    fn valid(self) -> bool {
        (0.0..=2.0).contains(&self.gain)
            && match self.source {
                Source::Direct => true,
                Source::World {
                    position,
                    reference_distance,
                    max_distance,
                } => {
                    position.is_finite()
                        && (0.05..=256.0).contains(&reference_distance)
                        && (reference_distance..=1024.0).contains(&max_distance)
                }
            }
    }
}

pub struct Mixer {
    sample_rate: u32,
    voices: Vec<Voice>,
    ambience: Ambience,
}

struct Voice {
    clip: Arc<Clip>,
    settings: VoiceSettings,
    index: usize,
    phase: u32,
    dialogue: bool,
}

impl Mixer {
    pub fn new(sample_rate: u32) -> Result<Self, String> {
        if !(8_000..=192_000).contains(&sample_rate) {
            return Err("Частота аудиомикшера вне диапазона.".into());
        }
        Ok(Self {
            sample_rate,
            voices: Vec::new(),
            ambience: Ambience::default(),
        })
    }

    pub fn play(&mut self, clip: Arc<Clip>, settings: VoiceSettings) -> bool {
        self.play_on(clip, settings, false)
    }

    pub fn play_dialogue(&mut self, clip: Arc<Clip>, settings: VoiceSettings) -> bool {
        self.stop_dialogue();
        self.play_on(clip, settings, true)
    }

    pub fn stop_dialogue(&mut self) {
        self.voices.retain(|voice| !voice.dialogue);
    }

    fn play_on(&mut self, clip: Arc<Clip>, settings: VoiceSettings, dialogue: bool) -> bool {
        if clip.frames.is_empty() || !settings.valid() {
            return false;
        }
        if self.voices.len() >= MAX_VOICES {
            self.voices.remove(0);
        }
        self.voices.push(Voice {
            clip,
            settings,
            index: 0,
            phase: 0,
            dialogue,
        });
        true
    }

    pub fn clear(&mut self) {
        self.voices.clear();
        self.ambience = Ambience::default();
    }

    pub fn is_active(&self) -> bool {
        !self.voices.is_empty() || self.ambience.active()
    }

    pub fn ambience(&mut self, level: AmbientLevels) {
        self.ambience.levels(level);
    }

    /// Produces stereo linear samples. Split buffers give the same waveform at a fixed listener pose.
    pub fn render(&mut self, output: &mut [[f32; 2]], listener: Listener) {
        output.fill([0.0; 2]);
        self.ambience.render(output, self.sample_rate);
        if self.voices.is_empty() {
            return;
        }
        for voice in &mut self.voices {
            let spatial = !matches!(voice.settings.source, Source::Direct);
            let [gain_left, gain_right] = gains(voice.settings, listener);
            for frame in output.iter_mut() {
                let Some([left, right]) = voice.next(self.sample_rate) else {
                    break;
                };
                let [left, right] = if spatial {
                    [(left + right) * 0.5; 2]
                } else {
                    [left, right]
                };
                frame[0] += left * gain_left;
                frame[1] += right * gain_right;
            }
        }
        self.voices
            .retain(|voice| voice.index < voice.clip.frames.len());
        for frame in output {
            frame[0] = frame[0].clamp(-1.0, 1.0);
            frame[1] = frame[1].clamp(-1.0, 1.0);
        }
    }
}

impl Voice {
    fn next(&mut self, output_rate: u32) -> Option<[f32; 2]> {
        let frames = &self.clip.frames;
        let current = *frames.get(self.index)?;
        let next = frames.get(self.index + 1).copied().unwrap_or(current);
        let fraction = self.phase as f32 / output_rate as f32;
        let result = [
            current[0] + (next[0] - current[0]) * fraction,
            current[1] + (next[1] - current[1]) * fraction,
        ];
        self.phase += self.clip.sample_rate;
        self.index += (self.phase / output_rate) as usize;
        self.phase %= output_rate;
        Some(result)
    }
}

fn gains(settings: VoiceSettings, listener: Listener) -> [f32; 2] {
    let Source::World {
        position,
        reference_distance,
        max_distance,
    } = settings.source
    else {
        return [settings.gain; 2];
    };
    let direction = position - listener.position;
    let distance = direction.length();
    if !distance.is_finite() || distance >= max_distance {
        return [0.0; 2];
    }
    let right = listener.right.try_normalize().unwrap_or(Vec3::X);
    let pan = direction.normalize_or_zero().dot(right).clamp(-1.0, 1.0);
    let (gain_right, gain_left) = ((pan + 1.0) * FRAC_PI_4).sin_cos();
    let rolloff = reference_distance / distance.max(reference_distance);
    let edge = ((max_distance - distance) / (max_distance * 0.2).max(0.01)).clamp(0.0, 1.0);
    let volume = settings.gain * rolloff * edge;
    [gain_left * volume, gain_right * volume]
}

pub fn pcm16_stereo_wav(frames: &[[f32; 2]], sample_rate: u32) -> Result<Vec<u8>, String> {
    if !(8_000..=192_000).contains(&sample_rate) {
        return Err("Частота WAV вне диапазона.".into());
    }
    let length = frames
        .len()
        .checked_mul(4)
        .and_then(|bytes| u32::try_from(bytes).ok())
        .filter(|&bytes| bytes <= u32::MAX - 36)
        .ok_or("Запись WAV слишком большая.")?;
    let mut bytes = Vec::with_capacity(44 + length as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(length + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&length.to_le_bytes());
    for channel in frames.iter().flatten() {
        let value = if channel.is_finite() { *channel } else { 0.0 };
        bytes.extend_from_slice(
            &((value * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16).to_le_bytes(),
        );
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Surface {
    #[default]
    Stone,
    Wood,
    Grass,
    Gravel,
    Metal,
    Water,
}

impl Surface {
    pub const ALL: [Self; 6] = [
        Self::Stone,
        Self::Wood,
        Self::Grass,
        Self::Gravel,
        Self::Metal,
        Self::Water,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Stone => "Камень",
            Self::Wood => "Дерево",
            Self::Grass => "Трава",
            Self::Gravel => "Гравий",
            Self::Metal => "Металл",
            Self::Water => "Вода",
        }
    }
}

pub struct Footsteps {
    presets: [[Arc<Clip>; 3]; 6],
}

impl Footsteps {
    pub fn new(sample_rate: u32) -> Result<Self, String> {
        if !(8_000..=192_000).contains(&sample_rate) {
            return Err("Частота шагов вне диапазона.".into());
        }
        Ok(Self {
            presets: Surface::ALL.map(|surface| {
                std::array::from_fn(|variant| {
                    Arc::new(footstep(surface, sample_rate, variant as u32))
                })
            }),
        })
    }

    pub fn sample(&self, surface: Surface, ordinal: u64) -> Arc<Clip> {
        self.presets[surface as usize][(ordinal % 3) as usize].clone()
    }
}

fn footstep(surface: Surface, sample_rate: u32, variant: u32) -> Clip {
    let (seconds, noise_mix, decay, first_tone, second_tone, tone_decay, filter_rate) =
        match surface {
            Surface::Stone => (0.17, 0.40, 33.0, 125.0, 240.0, 36.0, 0.36),
            Surface::Wood => (0.23, 0.20, 26.0, 93.0, 181.0, 20.0, 0.23),
            Surface::Grass => (0.18, 0.61, 21.0, 78.0, 119.0, 32.0, 0.16),
            Surface::Gravel => (0.29, 0.57, 16.0, 111.0, 152.0, 32.0, 0.61),
            Surface::Metal => (0.33, 0.21, 75.0, 286.0, 617.0, 12.0, 0.50),
            Surface::Water => (0.32, 0.57, 13.0, 196.0, 312.0, 24.0, 0.26),
        };
    let mut state = 1_279_u32 + surface as u32 * 311 + variant * 7_369;
    let mut low = 0.0;
    let mut previous = 0.0;
    let count = (seconds * sample_rate as f32).round() as usize;
    let pitch = 0.94 + variant as f32 * 0.053;
    let frames = (0..count)
        .map(|index| {
            let t = index as f32 / sample_rate as f32;
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 8) as f32 / 8_388_608.0 - 1.0;
            low += (noise - low) * filter_rate;
            let bright = (low - previous) * 0.45 + low;
            previous = low;
            let burst = match surface {
                Surface::Gravel => 0.64 + 0.36 * (t * 80.0 * TAU).sin().powi(4),
                Surface::Water => 0.45 + 0.55 * (t * 39.0 * TAU).sin().powi(2),
                Surface::Grass => 0.58 + 0.42 * (t * 46.0 * TAU).sin().abs(),
                _ => 1.0,
            };
            let percussion = bright * noise_mix * (-t * decay).exp() * burst;
            let resonance = ((t * first_tone * pitch * TAU).sin() * 0.47
                + (t * second_tone * pitch * TAU).sin() * 0.23)
                * (-t * tone_decay).exp();
            let follow = if surface == Surface::Wood && t > 0.06 {
                (t * 64.0 * TAU).sin() * (-(t - 0.06) * 34.0).exp() * 0.082
            } else {
                0.0
            };
            let attack = (t / 0.004).min(1.0);
            let tail = ((seconds - t) / 0.025).clamp(0.0, 1.0);
            let sample =
                ((percussion + resonance + follow) * attack * tail * 0.72).clamp(-1.0, 1.0);
            [sample; 2]
        })
        .collect::<Vec<_>>();
    Clip {
        sample_rate,
        channels: 1,
        frames: frames.into(),
    }
}

#[derive(Default)]
pub struct StepMeter {
    previous: Option<Vec3>,
    distance: f32,
}

impl StepMeter {
    pub fn advance(&mut self, feet: Vec3, grounded: bool, stride: f32) -> usize {
        if !feet.is_finite() || !(0.1..=8.0).contains(&stride) {
            self.previous = None;
            self.distance = 0.0;
            return 0;
        }
        let Some(before) = self.previous.replace(feet) else {
            return 0;
        };
        let displacement = (feet - before).with_y(0.0).length();
        if !grounded || displacement > 2.0 {
            self.distance = 0.0;
            return 0;
        }
        self.distance += displacement;
        let count = (self.distance / stride).floor() as usize;
        self.distance %= stride;
        count.min(3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(bytes: &mut Vec<u8>, name: &[u8; 4], content: &[u8]) {
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&(content.len() as u32).to_le_bytes());
        bytes.extend_from_slice(content);
        if content.len() % 2 != 0 {
            bytes.push(0);
        }
    }

    fn raw_wav(channels: u16, bits: u16, pcm: &[u8], extensible: bool) -> Vec<u8> {
        let rate = 44_100_u32;
        let alignment = channels * bits / 8;
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&(if extensible { 0xfffe_u16 } else { 1 }).to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&rate.to_le_bytes());
        fmt.extend_from_slice(&(rate * u32::from(alignment)).to_le_bytes());
        fmt.extend_from_slice(&alignment.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        if extensible {
            fmt.extend_from_slice(&22_u16.to_le_bytes());
            fmt.extend_from_slice(&bits.to_le_bytes());
            fmt.extend_from_slice(&3_u32.to_le_bytes());
            fmt.extend_from_slice(&PCM_SUBTYPE);
        }
        let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
        chunk(&mut bytes, b"JUNK", &[1, 2, 3]);
        chunk(&mut bytes, b"data", pcm);
        chunk(&mut bytes, b"fmt ", &fmt);
        let total = bytes.len() as u32 - 8;
        bytes[4..8].copy_from_slice(&total.to_le_bytes());
        bytes
    }

    #[test]
    fn pcm_wav_reads_unsigned_and_signed_depths_with_padded_chunks() {
        let eight = Clip::decode_pcm_wav(&raw_wav(1, 8, &[0, 128, 255], false)).unwrap();
        assert_eq!(eight.channels, 1);
        assert_eq!(eight.frames(), &[[-1.0; 2], [0.0; 2], [127.0 / 128.0; 2]]);
        let sixteen = Clip::decode_pcm_wav(&raw_wav(2, 16, &[0, 128, 0, 64], false)).unwrap();
        assert_eq!(sixteen.frames(), &[[-1.0, 0.5]]);
        let twenty_four = Clip::decode_pcm_wav(&raw_wav(1, 24, &[0, 0, 192], false)).unwrap();
        assert_eq!(twenty_four.frames(), &[[-0.5; 2]]);
        let thirty_two =
            Clip::decode_pcm_wav(&raw_wav(2, 32, &[0, 0, 0, 128, 0, 0, 0, 64], true)).unwrap();
        assert_eq!(thirty_two.frames(), &[[-1.0, 0.5]]);
    }

    #[test]
    fn malformed_headers_and_partial_frames_are_rejected() {
        let mut valid = raw_wav(1, 16, &[0, 64], false);
        valid.pop();
        assert!(Clip::decode_pcm_wav(&valid).is_err());
        assert!(Clip::decode_pcm_wav(&raw_wav(2, 24, &[0, 0, 0], false)).is_err());
        assert!(Clip::decode_pcm_wav(&raw_wav(3, 16, &[0; 6], false)).is_err());
    }

    #[test]
    fn same_clip_pans_across_listener_and_attenuates_with_distance() {
        let pcm = pcm16_stereo_wav(&vec![[0.5; 2]; 48], OUTPUT_RATE).unwrap();
        let clip = Arc::new(Clip::decode_pcm_wav(&pcm).unwrap());
        let sample = |x| {
            let mut mix = Mixer::new(OUTPUT_RATE).unwrap();
            assert!(mix.play(
                clip.clone(),
                VoiceSettings {
                    source: Source::World {
                        position: Vec3::X * x,
                        reference_distance: 1.0,
                        max_distance: 12.0,
                    },
                    ..VoiceSettings::default()
                },
            ));
            let mut output = [[0.0; 2]; 16];
            mix.render(&mut output, Listener::default());
            output[0]
        };
        let left = sample(-1.0);
        let right = sample(1.0);
        let farther = sample(4.0);
        let outside = sample(14.0);
        assert!(left[0] > 0.48 && left[1].abs() < 1e-5);
        assert!(right[1] > 0.48 && right[0].abs() < 1e-5);
        assert!(farther[1] > 0.1 && farther[1] < right[1] * 0.3);
        assert_eq!(outside, [0.0; 2]);
    }

    #[test]
    fn integer_phase_preserves_offline_wave_under_different_block_splits() {
        let frames = (0..80)
            .map(|i| [i as f32 / 150.0, -(i as f32) / 190.0])
            .collect::<Vec<_>>();
        let wav = pcm16_stereo_wav(&frames, 44_100).unwrap();
        let input = Arc::new(Clip::decode_pcm_wav(&wav).unwrap());
        let mut first = Mixer::new(OUTPUT_RATE).unwrap();
        let mut second = Mixer::new(OUTPUT_RATE).unwrap();
        assert!(first.play(input.clone(), VoiceSettings::default()));
        assert!(second.play(input, VoiceSettings::default()));
        let mut one = vec![[0.0; 2]; 105];
        let mut split = one.clone();
        first.render(&mut one, Listener::default());
        for part in split.chunks_mut(7) {
            second.render(part, Listener::default());
        }
        assert_eq!(one, split);
        assert!(!first.is_active());
        assert_ne!(one[16][0], one[16][1]);
        let one_wav = pcm16_stereo_wav(&one, OUTPUT_RATE).unwrap();
        let split_wav = pcm16_stereo_wav(&split, OUTPUT_RATE).unwrap();
        assert_eq!(one_wav, split_wav);
        assert_eq!(Clip::decode_pcm_wav(&one_wav).unwrap().frames().len(), 105);
    }

    #[test]
    fn footstep_materials_and_variations_are_deterministic_and_stride_based() {
        let bank = Footsteps::new(OUTPUT_RATE).unwrap();
        let stone = bank.sample(Surface::Stone, 0);
        let wood = bank.sample(Surface::Wood, 0);
        let metal = bank.sample(Surface::Metal, 0);
        assert!(stone.duration() < wood.duration());
        assert!(wood.duration() < metal.duration());
        assert_ne!(
            bank.sample(Surface::Water, 0).frames(),
            bank.sample(Surface::Water, 1).frames()
        );
        assert_eq!(stone.frames(), bank.sample(Surface::Stone, 3).frames());
        let mut step = StepMeter::default();
        assert_eq!(step.advance(Vec3::ZERO, true, 0.56), 0);
        assert_eq!(step.advance(Vec3::X * 0.28, true, 0.56), 0);
        assert_eq!(step.advance(Vec3::X * 0.59, true, 0.56), 1);
        assert_eq!(step.advance(Vec3::X * 1.9, false, 0.56), 0);
        assert_eq!(step.advance(Vec3::X * 8.0, true, 0.56), 0);
    }
}
