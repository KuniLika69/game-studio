use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Add,
    Multiply,
    Screen,
}

impl BlendMode {
    pub const ALL: [Self; 4] = [Self::Normal, Self::Add, Self::Multiply, Self::Screen];

    pub fn is_default(&self) -> bool {
        *self == Self::Normal
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowQuality {
    #[default]
    Single,
    Cascaded,
}

impl ShadowQuality {
    pub fn is_default(&self) -> bool {
        *self == Self::Single
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PostProcess {
    pub exposure_ev: f32,
    pub bloom: bool,
    pub bloom_threshold: f32,
    #[serde(default, skip_serializing_if = "ShadowQuality::is_default")]
    pub shadow_quality: ShadowQuality,
}

impl Default for PostProcess {
    fn default() -> Self {
        Self {
            exposure_ev: 0.0,
            bloom: false,
            bloom_threshold: 1.0,
            shadow_quality: ShadowQuality::Single,
        }
    }
}

impl PostProcess {
    pub const EXPOSURE_EV_RANGE: RangeInclusive<f32> = -6.0..=6.0;
    pub const BLOOM_THRESHOLD_RANGE: RangeInclusive<f32> = 0.1..=8.0;

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn valid(self) -> bool {
        Self::EXPOSURE_EV_RANGE.contains(&self.exposure_ev)
            && Self::BLOOM_THRESHOLD_RANGE.contains(&self.bloom_threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_blend_modes_keep_their_serialized_names_and_indices() -> serde_json::Result<()> {
        for (index, (mode, name)) in BlendMode::ALL
            .into_iter()
            .zip(["Normal", "Add", "Multiply", "Screen"])
            .enumerate()
        {
            assert_eq!(mode.index(), index);
            let encoded = serde_json::to_value(mode)?;
            assert_eq!(encoded, name);
            assert_eq!(serde_json::from_value::<BlendMode>(encoded)?, mode);
        }
        Ok(())
    }

    #[test]
    fn a5_legacy_hdr_settings_keep_single_shadows_and_identical_bytes() -> serde_json::Result<()> {
        let old = r#"{"exposure_ev":1.25,"bloom":true,"bloom_threshold":2.5}"#;
        let restored: PostProcess = serde_json::from_str(old)?;
        assert_eq!(restored.shadow_quality, ShadowQuality::Single);
        assert_eq!(serde_json::to_string(&restored)?, old);
        Ok(())
    }
}
