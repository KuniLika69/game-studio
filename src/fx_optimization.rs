//! Experiment with a measured and reversible reduction of one particle preset.
use serde::Serialize;

use crate::effects::{Library, PresetId};

const MIN_SAMPLES: usize = 60;
const MIN_PARTICLES: usize = 48;
const MIN_INTENSITY: f32 = 0.75;

#[derive(Clone, Debug, Serialize)]
pub struct Measurement {
    pub frames_ms: Vec<f32>,
    pub average_ms: f32,
    pub p95_ms: f32,
    pub peak_particles: usize,
}

impl Measurement {
    pub fn new(frames_ms: Vec<f32>, peak_particles: usize) -> Result<Self, String> {
        if frames_ms.len() < MIN_SAMPLES || frames_ms.iter().any(|ms| !ms.is_finite() || *ms <= 0.0)
        {
            return Err("Нужно не менее 60 измеренных кадров.".into());
        }
        let mut sorted = frames_ms.clone();
        sorted.sort_unstable_by(f32::total_cmp);
        let p95_ms = sorted[(95 * sorted.len()).div_ceil(100) - 1];
        let average_ms = frames_ms.iter().sum::<f32>() / frames_ms.len() as f32;
        Ok(Self {
            frames_ms,
            average_ms,
            p95_ms,
            peak_particles,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Proposal {
    pub preset: PresetId,
    pub previous_intensity: f32,
    pub candidate_intensity: f32,
}

pub fn propose(
    effects: &Library,
    preset: PresetId,
    target_ms: f32,
    measured: &Measurement,
) -> Option<Proposal> {
    let settings = &effects.preset(preset)?.settings;
    if !(1.0..=100.0).contains(&target_ms)
        || measured.p95_ms <= target_ms
        || measured.peak_particles < MIN_PARTICLES
        || settings.intensity < MIN_INTENSITY
    {
        return None;
    }
    Some(Proposal {
        preset,
        previous_intensity: settings.intensity,
        candidate_intensity: settings.intensity * 0.5,
    })
}

impl Proposal {
    pub fn apply(self, effects: &mut Library) -> Result<(), String> {
        self.set(effects, self.previous_intensity, self.candidate_intensity)
    }

    pub fn undo(self, effects: &mut Library) -> Result<(), String> {
        self.set(effects, self.candidate_intensity, self.previous_intensity)
    }

    fn set(self, effects: &mut Library, expected: f32, replacement: f32) -> Result<(), String> {
        let preset = effects
            .presets
            .iter_mut()
            .find(|preset| preset.id == self.preset)
            .ok_or("Эффект удалён после замера.")?;
        if preset.settings.intensity != expected {
            return Err("Интенсивность изменена после замера. Повтори измерение.".into());
        }
        preset.settings.intensity = replacement;
        Ok(())
    }
}

pub fn measured_gain(before: &Measurement, after: &Measurement) -> bool {
    let tolerance = (before.p95_ms * 0.03).max(0.05);
    after.peak_particles < before.peak_particles && after.p95_ms + tolerance < before.p95_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Kind;

    #[test]
    fn proposal_uses_both_measurements_and_applies_reversibly_to_one_preset() {
        let mut library = Library::default();
        let id = library.create(Kind::Rain).unwrap();
        library.create(Kind::Smoke).unwrap();
        let before = library.clone();
        let pixels = Measurement::new(vec![19.0; 60], 95).unwrap();
        let small = Measurement::new(vec![19.0; 60], 15).unwrap();
        let fast = Measurement::new(vec![10.0; 60], 95).unwrap();
        assert!(propose(&library, id, 16.67, &small).is_none());
        assert!(propose(&library, id, 16.67, &fast).is_none());
        let proposal = propose(&library, id, 16.67, &pixels).unwrap();
        proposal.apply(&mut library).unwrap();
        assert_eq!(library.presets[1], before.presets[1]);
        assert_eq!(library.preset(id).unwrap().settings.intensity, 0.5);
        assert!(proposal.apply(&mut library).is_err());
        proposal.undo(&mut library).unwrap();
        assert_eq!(library, before);
        let noise = Measurement::new(vec![18.8; 60], 38).unwrap();
        let improvement = Measurement::new(vec![16.0; 60], 38).unwrap();
        assert!(!measured_gain(&pixels, &noise));
        assert!(measured_gain(&pixels, &improvement));
    }
}
