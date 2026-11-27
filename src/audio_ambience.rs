use std::f32::consts::TAU;

#[derive(Clone, Copy, Default, PartialEq)]
pub struct AmbientLevels {
    pub rain: f32,
    pub room: f32,
}

pub(super) struct Ambience {
    target: AmbientLevels,
    current: AmbientLevels,
    random: Noise,
    wind: [f32; 2],
    room: f32,
    cycle: f32,
}

impl Default for Ambience {
    fn default() -> Self {
        Self {
            target: AmbientLevels::default(),
            current: AmbientLevels::default(),
            random: Noise::default(),
            wind: [0.0; 2],
            room: 0.0,
            cycle: 0.0,
        }
    }
}

impl Ambience {
    pub(super) fn levels(&mut self, target: AmbientLevels) {
        self.target = AmbientLevels {
            rain: target.rain.clamp(0.0, 0.45),
            room: target.room.clamp(0.0, 0.32),
        };
    }

    pub(super) fn active(&self) -> bool {
        self.target != AmbientLevels::default() || self.current.rain > 0.0001
            || self.current.room > 0.0001
    }

    pub(super) fn render(&mut self, output: &mut [[f32; 2]], sample_rate: u32) {
        if !self.active() {
            return;
        }
        let hz = sample_rate as f32;
        let crossfade = 1.0 - (-7.0 / hz).exp();
        let rain_filter = 1.0 - (-3200.0 / hz).exp();
        let room_filter = 1.0 - (-430.0 / hz).exp();
        for frame in output {
            self.current.rain += (self.target.rain - self.current.rain) * crossfade;
            self.current.room += (self.target.room - self.current.room) * crossfade;
            self.cycle = (self.cycle + 50.0 / hz).fract();
            let phase = TAU * self.cycle;
            let more_room = self.random.next();
            self.room += (more_room - self.room) * room_filter;
            let quiet = (0.42 * self.room + 0.035 * phase.sin() + 0.020 * (phase * 2.0).sin())
                * self.current.room;
            for (channel, wind) in frame.iter_mut().zip(&mut self.wind) {
                let grain = self.random.next();
                *wind += (grain - *wind) * rain_filter;
                let drop = (grain.abs() - 0.92).max(0.0) * grain.signum();
                *channel += quiet + (0.31 * *wind + 0.13 * grain + 0.52 * drop) * self.current.rain;
            }
        }
    }
}

pub(super) struct Noise(u32);

impl Default for Noise {
    fn default() -> Self {
        Self(0x8C5A_408F)
    }
}

impl Noise {
    pub(super) fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        f32::from((self.0 >> 16) as u16) * (2.0 / 65_535.0) - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indoor_mix_crossfades_without_a_spike_or_stereo_jump() {
        let mut source = Ambience::default();
        source.levels(AmbientLevels { rain: 0.23, room: 0.0 });
        let mut outside = [[0.0; 2]; 512];
        for _ in 0..30 {
            outside.fill([0.0; 2]);
            source.render(&mut outside, 48_000);
        }
        let previous = source.current;
        source.levels(AmbientLevels { rain: 0.025, room: 0.12 });
        let mut transition = [[0.0; 2]; 128];
        source.render(&mut transition, 48_000);
        assert!((source.current.rain - previous.rain).abs() < 0.006);
        assert!(source.current.room > 0.0 && source.current.room < 0.006);
        let mut inside = [[0.0; 2]; 48000];
        source.render(&mut inside, 48_000);
        assert!((source.current.rain - 0.025).abs() < 0.002);
        let difference = |slice: &[[f32; 2]]| {
            slice.iter().map(|f| (f[0] - f[1]).abs()).sum::<f32>() / slice.len() as f32
        };
        assert!(difference(&inside[40_000..]) < 0.25 * difference(&outside));
        assert!(outside.iter().chain(inside.iter()).flatten().all(|p| p.abs() < 0.24));
    }
}
