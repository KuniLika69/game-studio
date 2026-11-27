//! Authored sky, weather and local air/water volumes. Units are metres and seconds.
use std::collections::{BTreeMap, HashSet};
use std::f32::consts::TAU;

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::ImportError;
use crate::render_settings::BlendMode;

pub type ZoneId = u32;
pub const MAX_ZONES: usize = 48;
pub const MAX_CUES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DayCycle {
    pub start_hour: f32,
    pub hours_per_second: f32,
    /// Compass bearing of the sun at noon.
    pub noon_bearing: f32,
}

impl Default for DayCycle {
    fn default() -> Self {
        Self {
            start_hour: 14.0,
            hours_per_second: 0.0,
            noon_bearing: 140.0,
        }
    }
}

impl DayCycle {
    pub fn hour_at(self, seconds: f64) -> f32 {
        let speed = f64::from(self.hours_per_second);
        if speed <= 0.0 || !seconds.is_finite() || seconds <= 0.0 {
            return self.start_hour;
        }
        let hour = f64::from(self.start_hour) + seconds.rem_euclid(24.0 / speed) * speed;
        hour.rem_euclid(24.0) as f32
    }

    fn valid(self) -> bool {
        (0.0..24.0).contains(&self.start_hour)
            && (0.0..=3.0).contains(&self.hours_per_second)
            && (-360.0..=360.0).contains(&self.noon_bearing)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum WeatherPreset {
    #[default]
    Clear,
    Rain,
    Fog,
    Snow,
}

impl WeatherPreset {
    pub const ALL: [Self; 4] = [Self::Clear, Self::Rain, Self::Fog, Self::Snow];

    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Ясно",
            Self::Rain => "Дождь",
            Self::Fog => "Туман",
            Self::Snow => "Снег",
        }
    }

    fn values(self) -> Weather {
        match self {
            Self::Clear => Weather {
                cloud: 0.03,
                sunlight: 1.0,
                fog: 0.0012,
                rain: 0.0,
                snow: 0.0,
                wind: 0.35,
            },
            Self::Rain => Weather {
                cloud: 0.87,
                sunlight: 0.37,
                fog: 0.018,
                rain: 1.0,
                snow: 0.0,
                wind: 2.8,
            },
            Self::Fog => Weather {
                cloud: 0.46,
                sunlight: 0.55,
                fog: 0.075,
                rain: 0.0,
                snow: 0.0,
                wind: 0.20,
            },
            Self::Snow => Weather {
                cloud: 0.76,
                sunlight: 0.46,
                fog: 0.028,
                rain: 0.0,
                snow: 1.0,
                wind: 1.65,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Weather {
    cloud: f32,
    sunlight: f32,
    fog: f32,
    rain: f32,
    #[serde(default)]
    snow: f32,
    wind: f32,
}

impl Weather {
    fn blend(self, to: Self, factor: f32) -> Self {
        Self {
            cloud: lerp(self.cloud, to.cloud, factor),
            sunlight: lerp(self.sunlight, to.sunlight, factor),
            fog: lerp(self.fog, to.fog, factor),
            rain: lerp(self.rain, to.rain, factor),
            snow: lerp(self.snow, to.snow, factor),
            wind: lerp(self.wind, to.wind, factor),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct SurfaceWeather {
    wet: f32,
    snow: f32,
}

impl SurfaceWeather {
    fn initial(weather: Weather) -> Self {
        Self { wet: weather.rain, snow: weather.snow }
    }

    fn valid(self) -> bool {
        (0.0..=1.0).contains(&self.wet) && (0.0..=1.0).contains(&self.snow)
    }

    fn advance(&mut self, weather: Weather, seconds: f64) {
        if seconds <= 0.0 { return; }
        let soak = (weather.rain + 0.18 * self.snow * (1.0 - weather.snow)).clamp(0.0, 1.0);
        let dry = (21.0 - weather.sunlight * 10.0 - weather.wind * 0.9).max(3.5);
        let wet_time = if soak > self.wet { 0.85 } else { dry };
        let snow_time = if weather.snow >= self.snow { 1.25 } else { 31.0 / (0.7 + weather.sunlight + 2.5 * weather.rain) };
        let factor = |time: f32| -(-(seconds as f32) / time).exp_m1();
        self.wet = lerp(self.wet, soak, factor(wet_time)).clamp(0.0, 1.0);
        self.snow = lerp(self.snow, weather.snow, factor(snow_time)).clamp(0.0, 1.0);
    }

    fn transition(&mut self, previous: Weather, target: Weather, duration: f64, elapsed: f64) {
        if duration > 0.0 {
            let time = elapsed.clamp(0.0, duration);
            let step = time / 8.0;
            for index in 0..8 {
                let factor = (step * (f64::from(index) + 0.5) / duration) as f32;
                self.advance(previous.blend(target, factor), step);
            }
        }
        self.advance(target, (elapsed - duration).max(0.0));
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeatherCue {
    pub id: u32,
    pub start_seconds: f32,
    pub preset: WeatherPreset,
    pub blend_seconds: f32,
}

impl WeatherCue {
    pub fn end(&self) -> f64 {
        f64::from(self.start_seconds) + f64::from(self.blend_seconds)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherTrack {
    pub initial: WeatherPreset,
    pub cues: Vec<WeatherCue>,
    next_id: u32,
}

impl WeatherTrack {
    pub fn add_cue(&mut self, preset: WeatherPreset) -> Option<u32> {
        if self.cues.len() >= MAX_CUES {
            return None;
        }
        let start_seconds = self.cues.last().map_or(5.0, |cue| (cue.end() + 1.0) as f32);
        if !(0.0..=86_400.0).contains(&start_seconds) {
            return None;
        }
        let id = self
            .cues
            .iter()
            .map(|cue| cue.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)?
            .max(self.next_id);
        self.next_id = id.checked_add(1)?;
        self.cues.push(WeatherCue {
            id,
            start_seconds,
            preset,
            blend_seconds: 2.0,
        });
        Some(id)
    }

    fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        let mut previous: Option<&WeatherCue> = None;
        if self.cues.len() > MAX_CUES
            || self.cues.iter().any(|cue| {
                let invalid = cue.id == 0
                    || !ids.insert(cue.id)
                    || !(0.0..=86_400.0).contains(&cue.start_seconds)
                    || !valid_blend(cue.blend_seconds)
                    || previous.is_some_and(|previous| {
                        cue.start_seconds <= previous.start_seconds
                            || f64::from(cue.start_seconds) < previous.end()
                    });
                previous = Some(cue);
                invalid
            })
        {
            return Err(
                "Переходы погоды должны иметь разные номера и идти по времени без перекрытий."
                    .into(),
            );
        }
        Ok(())
    }

    fn at(&self, seconds: f64) -> Weather {
        let mut previous = self.initial.values();
        for cue in &self.cues {
            let start = f64::from(cue.start_seconds);
            if seconds < start {
                break;
            }
            let target = cue.preset.values();
            if seconds < cue.end() {
                return previous.blend(target, ramp(start, cue.blend_seconds, seconds));
            }
            previous = target;
        }
        previous
    }

    fn surface_at(&self, seconds: f64) -> SurfaceWeather {
        let mut previous = self.initial.values();
        let mut surface = SurfaceWeather::initial(previous);
        let mut last_end = 0.0;
        for cue in &self.cues {
            let start = f64::from(cue.start_seconds);
            if seconds < start { break; }
            surface.advance(previous, start - last_end);
            let target = cue.preset.values();
            surface.transition(previous, target, f64::from(cue.blend_seconds), seconds.min(cue.end()) - start);
            if seconds < cue.end() { return surface; }
            last_end = cue.end();
            previous = target;
        }
        surface.advance(previous, seconds - last_end);
        surface
    }

    fn events_between(&self, before: f64, after: f64) -> Vec<Event> {
        let mut events = Vec::new();
        let crossed = |time: f64| time > before && time <= after;
        for cue in &self.cues {
            let start = f64::from(cue.start_seconds);
            if start > after {
                break;
            }
            if crossed(start) {
                events.push(Event {
                    seconds: start,
                    kind: EventKind::WeatherStarted {
                        preset: cue.preset,
                        cue: Some(cue.id),
                    },
                });
            }
            if crossed(cue.end()) {
                events.push(Event {
                    seconds: cue.end(),
                    kind: EventKind::WeatherSettled {
                        preset: cue.preset,
                        cue: Some(cue.id),
                    },
                });
            }
        }
        events
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindZone {
    pub id: ZoneId,
    pub name: String,
    pub center: [f32; 3],
    /// Full width, height and depth.
    pub size: [f32; 3],
    pub direction: [f32; 3],
    pub speed: f32,
    pub gust: f32,
    pub gust_hz: f32,
    pub feather: f32,
}

impl WindZone {
    fn sample(&self, point: Vec3, seconds: f64) -> Vec3 {
        let relative = point - Vec3::from_array(self.center);
        let border = Vec3::from_array(self.size) * 0.5 - relative.abs();
        let distance = border.min_element();
        if distance < 0.0 {
            return Vec3::ZERO;
        }
        let influence = if self.feather > 0.0 {
            smooth01(distance / self.feather)
        } else {
            1.0
        };
        let phase = phase_at(seconds, self.gust_hz)
            + (f64::from(self.id) * 1.792).rem_euclid(std::f64::consts::TAU) as f32
            + relative.dot(Vec3::new(0.17, 0.09, 0.24));
        let speed = (self.speed + self.gust * phase.sin()).max(0.0);
        Vec3::from_array(self.direction).normalize_or_zero() * (influence * speed)
    }

    fn valid(&self) -> bool {
        valid_name(&self.name)
            && valid_position(&self.center)
            && self.size.iter().all(|size| (0.05..=1024.0).contains(size))
            && valid_direction(self.direction)
            && (0.0..=50.0).contains(&self.speed)
            && (0.0..=35.0).contains(&self.gust)
            && (0.01..=10.0).contains(&self.gust_hz)
            && (0.0..=512.0).contains(&self.feather)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaterZone {
    pub id: ZoneId,
    pub name: String,
    /// Horizontal centre and extents in world X/Z.
    pub center: [f32; 2],
    pub size: [f32; 2],
    /// A soft-edged surface over authored terrain, without generated basin walls.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rounded_edge: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shore: Option<WaterShore>,
    pub floor_y: f32,
    pub max_level: f32,
    pub level: f32,
    /// Current along X/Z in metres per second.
    pub current: [f32; 2],
    pub buoyancy: f32,
    pub linear_drag: f32,
    pub tint: [f32; 3],
    #[serde(default, skip_serializing_if = "BlendMode::is_default")]
    pub blend_mode: BlendMode,
    pub wave_height: f32,
    pub wave_hz: f32,
}

impl WaterZone {
    fn valid(&self) -> bool {
        valid_name(&self.name)
            && valid_position(&self.center)
            && valid_position(&[self.floor_y, self.max_level])
            && (0.05..=256.0).contains(&(self.max_level - self.floor_y))
            && (self.floor_y..=self.max_level).contains(&self.level)
            && self.size.iter().all(|size| (0.05..=1024.0).contains(size))
            && self.shore.is_none_or(WaterShore::valid)
            && self
                .current
                .iter()
                .all(|speed| (-25.0..=25.0).contains(speed))
            && (0.0..=6.0).contains(&self.buoyancy)
            && (0.0..=25.0).contains(&self.linear_drag)
            && self.tint.iter().all(|color| (0.0..=1.0).contains(color))
            && (0.0..=0.8).contains(&self.wave_height)
            && (0.02..=10.0).contains(&self.wave_hz)
    }

    fn sample_surface(&self, level: f32) -> Option<WaterSurface> {
        (level - self.floor_y > 0.001).then(|| self.basin(level))
    }

    fn basin(&self, level: f32) -> WaterSurface {
        let depth = (level - self.floor_y).max(0.0);
        WaterSurface {
            id: self.id,
            center: self.center,
            size: self.size,
            rounded_edge: self.rounded_edge,
            shore: self.shore,
            height: level,
            floor_y: self.floor_y,
            current: self.current,
            tint: self.tint,
            blend_mode: self.blend_mode,
            wave_height: self.wave_height.min(depth * 0.45),
            wave_hz: self.wave_hz,
        }
    }

    fn sample_body(&self, level: f32, center: Vec3, height: f32) -> Option<WaterSample> {
        let surface = self.sample_surface(level)?;
        if !surface.contains_xz(center) {
            return None;
        }
        let bottom = center.y - height * 0.5;
        let top = center.y + height * 0.5;
        let floor = surface.floor_at(Vec2::new(center.x, center.z));
        let immersion = ((top.min(level) - bottom.max(floor)) / height).clamp(0.0, 1.0);
        (immersion > 0.0).then_some(WaterSample {
            zone: self.id,
            surface_y: level,
            immersion,
            current: Vec3::new(self.current[0], 0.0, self.current[1]),
            linear_drag: self.linear_drag,
            buoyancy: self.buoyancy,
        })
    }
}

/// A sloping floor, measured from world y=0 at the authored shoreline.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaterShore {
    pub origin: [f32; 2],
    pub seaward: [f32; 2],
    /// Vertical descent for one horizontal metre in the seaward direction.
    pub slope: f32,
}

impl WaterShore {
    fn valid(self) -> bool {
        valid_position(&self.origin)
            && self.seaward.iter().all(|v| v.is_finite() && v.abs() <= 1.0)
            && Vec2::from_array(self.seaward).length_squared() >= 0.0001
            && (0.01..=3.0).contains(&self.slope)
    }

    pub fn gradient(self) -> Vec2 {
        Vec2::from_array(self.seaward).normalize_or_zero() * self.slope
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterSurface {
    pub id: ZoneId,
    pub center: [f32; 2],
    pub size: [f32; 2],
    pub rounded_edge: bool,
    pub shore: Option<WaterShore>,
    pub height: f32,
    pub floor_y: f32,
    pub current: [f32; 2],
    pub tint: [f32; 3],
    pub blend_mode: BlendMode,
    pub wave_height: f32,
    pub wave_hz: f32,
}

impl WaterSurface {
    pub fn contains_xz(self, position: Vec3) -> bool {
        let offset = Vec2::new(position.x - self.center[0], position.z - self.center[1]);
        let local = offset / (Vec2::from_array(self.size) * 0.5);
        let inside = if self.rounded_edge {
            local.length_squared() <= 1.0
        } else {
            local.abs().max_element() <= 1.0
        };
        inside && self.depth_at(Vec2::new(position.x, position.z)) > 0.001
    }

    pub fn floor_at(self, point: Vec2) -> f32 {
        self.shore.map_or(self.floor_y, |shore| {
            self.floor_y.max(-shore.gradient().dot(point - Vec2::from_array(shore.origin)))
        })
    }

    pub fn depth_at(self, point: Vec2) -> f32 {
        (self.height - self.floor_at(point)).max(0.0)
    }

    pub fn half_width_at_z(self, z: f32) -> f32 {
        let row = (z - self.center[1]) / (self.size[1] * 0.5);
        let extent = if self.rounded_edge {
            (1.0 - row * row).max(0.0).sqrt()
        } else {
            1.0
        };
        self.size[0] * 0.5 * extent
    }

    pub fn span_at_z(self, z: f32) -> Option<[f32; 2]> {
        if (z - self.center[1]).abs() > self.size[1] * 0.5 {
            return None;
        }
        let half_width = self.half_width_at_z(z);
        let [mut left, mut right] = [self.center[0] - half_width, self.center[0] + half_width];
        if let Some(shore) = self.shore {
            let gradient = shore.gradient();
            let offset = gradient.y * (z - shore.origin[1]) + self.height;
            if gradient.x.abs() > 1e-6 {
                let edge = shore.origin[0] - offset / gradient.x;
                if gradient.x > 0.0 {
                    left = left.max(edge);
                } else {
                    right = right.min(edge);
                }
            } else if offset <= 0.0 {
                return None;
            }
        }
        (right > left).then_some([left, right])
    }

    /// The visual wave oscillates around the level used by physics.
    pub fn wave_at(self, x: f32, z: f32, seconds: f64) -> f32 {
        let t = phase_at(seconds, self.wave_hz);
        let modulation = (x * 0.41 - z * 0.24 + t).sin() * 0.72;
        let amplitude = self.wave_height.min(self.depth_at(Vec2::new(x, z)) * 0.45);
        self.height
            + amplitude * (0.60 * (x * 1.17 + z * 0.67 - t + modulation).sin()
                + 0.31 * (x * -0.54 + z * 1.33 + t - modulation * 0.83).sin()
                + 0.09 * (x * 0.23 - z * 2.16 + t * 2.0).sin())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterSample {
    pub zone: ZoneId,
    pub surface_y: f32,
    pub immersion: f32,
    pub current: Vec3,
    pub linear_drag: f32,
    pub buoyancy: f32,
}

impl WaterSample {
    /// Applies liquid forces to a velocity after the physics engine has applied gravity.
    pub fn apply_to_velocity(self, velocity: Vec3, gravity: f32, dt: f32) -> Vec3 {
        if !dt.is_finite() || dt <= 0.0 || !gravity.is_finite() || !velocity.is_finite() {
            return velocity;
        }
        let acceleration = Vec3::Y * (gravity.abs() * self.buoyancy * self.immersion);
        let damping = self.linear_drag * self.immersion;
        if damping <= 1e-6 {
            return velocity + acceleration * dt;
        }
        let blend = -(-damping * dt).exp_m1();
        velocity.lerp(self.current, blend) + acceleration * (blend / damping)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splash {
    pub zone: ZoneId,
    pub position: Vec3,
    pub strength: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct BodyInput {
    pub center: Vec3,
    pub height: f32,
    /// Velocity after gravity has been applied by the body solver.
    pub velocity: Vec3,
    /// Drag in inverse seconds. Zero disables coupling to air.
    pub wind_drag: f32,
    pub buoyant: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Environment {
    pub time: DayCycle,
    pub weather: WeatherTrack,
    pub ground_mist_height: f32,
    pub prevailing_direction: [f32; 3],
    pub winds: Vec<WindZone>,
    pub waters: Vec<WaterZone>,
    next_zone: ZoneId,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            time: DayCycle::default(),
            weather: WeatherTrack::default(),
            ground_mist_height: 0.0,
            prevailing_direction: [0.6, 0.0, 0.8],
            winds: Vec::new(),
            waters: Vec::new(),
            next_zone: 1,
        }
    }
}

impl Environment {
    pub fn validate(&self) -> Result<(), ImportError> {
        if !self.time.valid() || !valid_direction(self.prevailing_direction) {
            return Err(ImportError::new(
                "Время суток и ветер",
                "Некорректное время суток или направление общего ветра.",
            ));
        }
        if !(0.0..=8.0).contains(&self.ground_mist_height) {
            return Err(ImportError::new(
                "Погода, приземный туман",
                "Высота приземного тумана должна быть от 0 до 8 м.",
            ));
        }
        self.weather
            .validate()
            .map_err(|error| ImportError::new("Погода, таймлайн", error))?;
        let mut ids = HashSet::new();
        if self.winds.len() > MAX_ZONES
            || self.waters.len() > MAX_ZONES
            || self
                .winds
                .iter()
                .any(|zone| zone.id == 0 || !ids.insert(zone.id) || !zone.valid())
            || self
                .waters
                .iter()
                .any(|zone| zone.id == 0 || !ids.insert(zone.id) || !zone.valid())
        {
            return Err(ImportError::new(
                "Объёмы ветра и воды",
                "Некорректные границы, параметры или номера объёмов.",
            ));
        }
        Ok(())
    }

    pub fn add_wind(&mut self, center: [f32; 3]) -> Option<ZoneId> {
        if self.winds.len() >= MAX_ZONES || !valid_position(&center) {
            return None;
        }
        let id = self.reserve_zone()?;
        self.winds.push(WindZone {
            id,
            name: format!("Ветер {id}"),
            center,
            size: [8.0, 4.0, 8.0],
            direction: [1.0, 0.0, 0.0],
            speed: 4.0,
            gust: 1.4,
            gust_hz: 0.33,
            feather: 0.8,
        });
        Some(id)
    }

    pub fn add_water(&mut self, center: [f32; 2], floor_y: f32) -> Option<ZoneId> {
        if self.waters.len() >= MAX_ZONES
            || !valid_position(&center)
            || !valid_position(&[floor_y, floor_y + 2.0])
        {
            return None;
        }
        let id = self.reserve_zone()?;
        self.waters.push(WaterZone {
            id,
            name: format!("Вода {id}"),
            center,
            size: [5.0, 5.0],
            rounded_edge: false,
            shore: None,
            floor_y,
            max_level: floor_y + 2.0,
            level: floor_y + 0.5,
            current: [0.0, 0.45],
            buoyancy: 1.55,
            linear_drag: 3.4,
            tint: [0.06, 0.33, 0.43],
            blend_mode: BlendMode::Normal,
            wave_height: 0.055,
            wave_hz: 0.70,
        });
        Some(id)
    }

    fn reserve_zone(&mut self) -> Option<ZoneId> {
        let last = self
            .winds
            .iter()
            .map(|zone| zone.id)
            .chain(self.waters.iter().map(|zone| zone.id))
            .max()
            .unwrap_or(0);
        let id = last.checked_add(1)?.max(self.next_zone);
        self.next_zone = id.checked_add(1)?;
        Some(id)
    }

    pub fn sample(&self, seconds: f64) -> Atmosphere {
        self.sample_weather(seconds, self.weather.at(seconds), self.weather.surface_at(seconds))
    }

    pub fn weather_events_between(&self, before: f64, after: f64) -> Vec<Event> {
        self.weather.events_between(before, after)
    }

    pub fn wind_at(&self, point: Vec3, seconds: f64) -> Vec3 {
        self.wind_for(point, seconds, self.weather.at(seconds))
    }

    pub fn basins(&self) -> impl Iterator<Item = WaterSurface> + '_ {
        self.waters.iter().map(|zone| zone.basin(zone.level))
    }

    fn wind_for(&self, point: Vec3, seconds: f64, weather: Weather) -> Vec3 {
        if !point.is_finite() || !seconds.is_finite() {
            return Vec3::ZERO;
        }
        let prevailing =
            Vec3::from_array(self.prevailing_direction).normalize_or_zero() * weather.wind;
        self.winds
            .iter()
            .fold(prevailing, |wind, zone| wind + zone.sample(point, seconds))
            .clamp_length_max(70.0)
    }

    fn body_water(
        &self,
        center: Vec3,
        height: f32,
        level: impl Fn(&WaterZone) -> f32,
    ) -> Option<WaterSample> {
        if !center.is_finite() || !height.is_finite() || height <= 0.0001 {
            return None;
        }
        self.waters
            .iter()
            .filter_map(|zone| zone.sample_body(level(zone), center, height))
            .max_by(|left, right| {
                left.immersion
                    .total_cmp(&right.immersion)
                    .then_with(|| left.surface_y.total_cmp(&right.surface_y))
                    .then_with(|| right.zone.cmp(&left.zone))
            })
    }

    fn sample_weather(&self, seconds: f64, weather: Weather, surface: SurfaceWeather) -> Atmosphere {
        let hour = self.time.hour_at(seconds);
        let arc = (hour - 6.0) * TAU / 24.0;
        let altitude = arc.sin() * 0.85;
        let bearing = self.time.noon_bearing.to_radians() + (hour - 12.0) * TAU / 24.0;
        let horizontal = (1.0 - altitude * altitude).sqrt();
        let sun_direction = Vec3::new(
            horizontal * bearing.sin(),
            altitude.max(0.035),
            horizontal * bearing.cos(),
        )
        .normalize();

        const NIGHT: Vec3 = Vec3::new(0.018, 0.024, 0.071);
        const SUNSET: Vec3 = Vec3::new(0.96, 0.38, 0.19);
        const DAY: Vec3 = Vec3::new(0.49, 0.72, 0.94);
        let daylight = smooth01((altitude + 0.16) / 0.44);
        let high_sun = smooth01((altitude + 0.015) / 0.62);
        let twilight = daylight * (1.0 - high_sun);
        let sky_top = NIGHT.lerp(Vec3::new(0.085, 0.29, 0.66), daylight).lerp(
            Vec3::new(0.28, 0.31, 0.38) * (0.25 + 0.75 * daylight),
            weather.cloud * 0.78,
        );
        let sky_horizon = NIGHT
            .lerp(DAY, daylight)
            .lerp(SUNSET, twilight * 0.84)
            .lerp(
                Vec3::new(0.51, 0.55, 0.59) * (0.28 + 0.72 * daylight),
                weather.cloud * 0.64,
            );
        let sun_color = Vec3::new(1.0, 0.43, 0.20).lerp(Vec3::new(1.0, 0.96, 0.88), high_sun);
        let sun_strength = smooth01((altitude + 0.036) / 0.47) * weather.sunlight;
        let ambient = Vec3::new(0.09, 0.125, 0.23)
            .lerp(
                Vec3::new(0.34, 0.41, 0.48),
                daylight * (1.0 - weather.cloud * 0.42),
            )
            .lerp(Vec3::new(0.32, 0.19, 0.21), twilight * 0.48);
        Atmosphere {
            hour,
            sun_direction,
            sun_color,
            sun_strength,
            ambient,
            sky_top,
            sky_horizon,
            fog_color: sky_horizon.lerp(
                Vec3::new(0.63, 0.68, 0.72) * (0.32 + daylight * 0.68),
                weather.cloud * 0.67,
            ),
            fog_density: weather.fog,
            ground_mist_height: self.ground_mist_height,
            rain: weather.rain,
            snow: weather.snow,
            surface_wet: surface.wet,
            surface_snow: surface.snow,
            cloud_cover: weather.cloud,
            wind_speed: weather.wind,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    pub hour: f32,
    /// Direction from the ground toward the light. Y stays positive for shadow projection.
    pub sun_direction: Vec3,
    pub sun_color: Vec3,
    pub sun_strength: f32,
    pub ambient: Vec3,
    pub sky_top: Vec3,
    pub sky_horizon: Vec3,
    pub fog_color: Vec3,
    pub fog_density: f32,
    pub ground_mist_height: f32,
    pub rain: f32,
    pub snow: f32,
    pub surface_wet: f32,
    pub surface_snow: f32,
    pub cloud_cover: f32,
    pub wind_speed: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind {
    WeatherStarted {
        preset: WeatherPreset,
        cue: Option<u32>,
    },
    WeatherSettled {
        preset: WeatherPreset,
        cue: Option<u32>,
    },
    WaterLevelReached {
        zone: ZoneId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    pub seconds: f64,
    pub kind: EventKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WeatherChange {
    start: f64,
    duration: f32,
    previous: Weather,
    target: WeatherPreset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_surface: Option<SurfaceWeather>,
}

impl WeatherChange {
    fn at(self, seconds: f64) -> Weather {
        self.previous.blend(
            self.target.values(),
            ramp(self.start, self.duration, seconds),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct WaterChange {
    start: f64,
    duration: f32,
    previous: f32,
    target: f32,
}

impl WaterChange {
    fn at(self, seconds: f64) -> f32 {
        lerp(
            self.previous,
            self.target,
            ramp(self.start, self.duration, seconds),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    elapsed: f64,
    weather_override: Option<WeatherChange>,
    water_overrides: BTreeMap<ZoneId, WaterChange>,
}

impl Snapshot {
    pub fn validate(&self) -> Result<(), ImportError> {
        if !self.elapsed.is_finite()
            || self.elapsed < 0.0
            || self.water_overrides.len() > MAX_ZONES
            || self.weather_override.is_some_and(|state| {
                !valid_timestamp(state.start, self.elapsed)
                    || !valid_blend(state.duration)
                    || ![
                        state.previous.cloud,
                        state.previous.sunlight,
                        state.previous.fog,
                        state.previous.rain,
                        state.previous.snow,
                    ]
                    .iter()
                    .all(|value| (0.0..=1.0).contains(value))
                    || !(0.0..=70.0).contains(&state.previous.wind)
                    || state.previous_surface.is_some_and(|surface| !surface.valid())
            })
            || self.water_overrides.iter().any(|(&id, state)| {
                id == 0
                    || !valid_timestamp(state.start, self.elapsed)
                    || !valid_blend(state.duration)
                    || !valid_position(&[state.previous, state.target])
            })
        {
            return Err(ImportError::new(
                "Сохранение, погода",
                "Некорректное состояние времени, погоды или воды.",
            ));
        }
        Ok(())
    }
}

/// A fresh run starts from the authored clock, weather and water levels.
#[derive(Clone, Debug, Default)]
pub struct Session {
    elapsed: f64,
    weather_override: Option<WeatherChange>,
    water_overrides: BTreeMap<ZoneId, WaterChange>,
    events: Vec<Event>,
}

impl Session {
    pub fn new(config: &Environment) -> Self {
        Self {
            events: config.weather_events_between(-1.0, 0.0),
            ..Self::default()
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            elapsed: self.elapsed,
            weather_override: self.weather_override,
            water_overrides: self.water_overrides.clone(),
        }
    }

    pub fn restore(config: &Environment, snapshot: Snapshot) -> Result<Self, ImportError> {
        snapshot.validate()?;
        if snapshot.water_overrides.iter().any(|(&id, state)| {
            config
                .waters
                .iter()
                .find(|zone| zone.id == id)
                .is_none_or(|zone| {
                    !(zone.floor_y..=zone.max_level).contains(&state.previous)
                        || !(zone.floor_y..=zone.max_level).contains(&state.target)
                })
        }) {
            return Err(ImportError::new(
                "Сохранение, объёмы воды",
                "Настроенные объёмы воды изменились после сохранения.",
            ));
        }
        Ok(Self {
            elapsed: snapshot.elapsed,
            weather_override: snapshot.weather_override,
            water_overrides: snapshot.water_overrides,
            events: Vec::new(),
        })
    }

    pub fn elapsed(&self) -> f64 {
        self.elapsed
    }

    pub fn tick(&mut self, config: &Environment, dt: f64) {
        let after = self.elapsed + dt;
        if !after.is_finite() || after <= self.elapsed {
            return;
        }
        if let Some(active) = self.weather_override {
            let end = active.start + f64::from(active.duration);
            if self.elapsed < end && end <= after {
                self.events.push(Event {
                    seconds: end,
                    kind: EventKind::WeatherSettled {
                        preset: active.target,
                        cue: None,
                    },
                });
            }
        } else {
            self.events
                .extend(config.weather_events_between(self.elapsed, after));
        }
        for (&zone, active) in &self.water_overrides {
            let end = active.start + f64::from(active.duration);
            if self.elapsed < end && end <= after {
                self.events.push(Event {
                    seconds: end,
                    kind: EventKind::WaterLevelReached { zone },
                });
            }
        }
        self.elapsed = after;
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        let mut events = std::mem::take(&mut self.events);
        events.sort_by(|left, right| left.seconds.total_cmp(&right.seconds));
        events
    }

    /// A rule override takes control of weather for the current playthrough.
    pub fn set_weather(
        &mut self,
        config: &Environment,
        preset: WeatherPreset,
        seconds: f32,
    ) -> Result<bool, String> {
        if !valid_blend(seconds) {
            return Err("Переход погоды должен длиться от 0 до 600 секунд.".into());
        }
        if self
            .weather_override
            .is_some_and(|active| active.target == preset)
        {
            return Ok(false);
        }
        let previous = self.weather(config);
        let changing = previous != preset.values();
        self.weather_override = Some(WeatherChange {
            start: self.elapsed,
            duration: if changing { seconds } else { 0.0 },
            previous,
            target: preset,
            previous_surface: Some(self.surface(config)),
        });
        if !changing {
            return Ok(false);
        }
        self.events.push(Event {
            seconds: self.elapsed,
            kind: EventKind::WeatherStarted { preset, cue: None },
        });
        if seconds == 0.0 {
            self.events.push(Event {
                seconds: self.elapsed,
                kind: EventKind::WeatherSettled { preset, cue: None },
            });
        }
        Ok(true)
    }

    pub fn set_water_level(
        &mut self,
        config: &Environment,
        id: ZoneId,
        level: f32,
        seconds: f32,
    ) -> Result<bool, String> {
        let zone = config
            .waters
            .iter()
            .find(|zone| zone.id == id)
            .ok_or("Объём воды отсутствует.")?;
        if !(zone.floor_y..=zone.max_level).contains(&level) || !valid_blend(seconds) {
            return Err("Уровень или длительность изменения воды вне диапазона.".into());
        }
        if self
            .water_overrides
            .get(&id)
            .is_some_and(|active| active.target == level)
        {
            return Ok(false);
        }
        let previous = self.level_for(zone);
        if previous == level {
            self.water_overrides.remove(&id);
            if zone.level != level {
                self.water_overrides.insert(
                    id,
                    WaterChange {
                        start: self.elapsed,
                        duration: 0.0,
                        previous,
                        target: level,
                    },
                );
            }
            return Ok(false);
        }
        self.water_overrides.insert(
            id,
            WaterChange {
                start: self.elapsed,
                duration: seconds,
                previous,
                target: level,
            },
        );
        if seconds == 0.0 {
            self.events.push(Event {
                seconds: self.elapsed,
                kind: EventKind::WaterLevelReached { zone: id },
            });
        }
        Ok(true)
    }

    pub fn sample(&self, config: &Environment) -> Atmosphere {
        config.sample_weather(self.elapsed, self.weather(config), self.surface(config))
    }

    fn surface(&self, config: &Environment) -> SurfaceWeather {
        if let Some(state) = self.weather_override {
            let mut surface = state.previous_surface.unwrap_or_else(|| config.weather.surface_at(state.start));
            surface.transition(state.previous, state.target.values(), f64::from(state.duration), self.elapsed - state.start);
            surface
        } else { config.weather.surface_at(self.elapsed) }
    }

    pub fn wind_at(&self, config: &Environment, point: Vec3) -> Vec3 {
        config.wind_for(point, self.elapsed, self.weather(config))
    }

    pub fn body_velocity(
        &self,
        config: &Environment,
        body: BodyInput,
        gravity: f32,
        dt: f32,
        seconds: f64,
    ) -> Vec3 {
        if !body.velocity.is_finite()
            || !body.center.is_finite()
            || !seconds.is_finite()
            || !dt.is_finite()
            || dt <= 0.0
        {
            return body.velocity;
        }
        let water = if body.buoyant {
            config.body_water(body.center, body.height, |zone| {
                self.level_at(zone, seconds)
            })
        } else {
            None
        };
        let mut velocity = body.velocity;
        if body.wind_drag.is_finite() && body.wind_drag > 0.0 {
            let exposure = water.map_or(1.0, |water| 1.0 - water.immersion);
            let wind = config.wind_for(body.center, seconds, self.weather_at(config, seconds));
            let blend = -(-body.wind_drag * exposure * dt).exp_m1();
            velocity = velocity.lerp(wind, blend);
        }
        water.map_or(velocity, |water| {
            water.apply_to_velocity(velocity, gravity, dt)
        })
    }

    pub fn level(&self, config: &Environment, id: ZoneId) -> Option<f32> {
        config
            .waters
            .iter()
            .find(|zone| zone.id == id)
            .map(|zone| self.level_for(zone))
    }

    pub fn basins<'a>(
        &'a self,
        config: &'a Environment,
    ) -> impl Iterator<Item = WaterSurface> + 'a {
        config
            .waters
            .iter()
            .map(|zone| zone.basin(self.level_for(zone)))
    }

    pub fn surfaces<'a>(
        &'a self,
        config: &'a Environment,
    ) -> impl Iterator<Item = WaterSurface> + 'a {
        config
            .waters
            .iter()
            .filter_map(|zone| zone.sample_surface(self.level_for(zone)))
    }

    #[cfg(test)]
    pub fn water_at(&self, config: &Environment, center: Vec3, height: f32) -> Option<WaterSample> {
        config.body_water(center, height, |zone| self.level_for(zone))
    }

    pub fn splash_between(
        &self,
        config: &Environment,
        before: Vec3,
        after: Vec3,
        height: f32,
        dt: f32,
    ) -> Option<Splash> {
        if !before.is_finite()
            || !after.is_finite()
            || !height.is_finite()
            || height <= 0.0
            || !dt.is_finite()
            || dt <= 0.0
        {
            return None;
        }
        let fall = before.y - after.y;
        let speed = fall / dt;
        if speed < 0.85 {
            return None;
        }
        let half = height * 0.5;
        self.surfaces(config)
            .filter_map(|surface| {
                let fraction = (before.y - half - surface.height) / fall;
                if !(0.0..=1.0).contains(&fraction) || fraction == 0.0 {
                    return None;
                }
                let point = before.lerp(after, fraction).with_y(surface.height);
                surface.contains_xz(point).then_some((
                    fraction,
                    Splash {
                        zone: surface.id,
                        position: point,
                        strength: ((speed - 0.6) / 7.0).clamp(0.05, 1.0),
                    },
                ))
            })
            .min_by(|left, right| {
                left.0
                    .total_cmp(&right.0)
                    .then_with(|| left.1.zone.cmp(&right.1.zone))
            })
            .map(|(_, splash)| splash)
    }

    fn weather(&self, config: &Environment) -> Weather {
        self.weather_at(config, self.elapsed)
    }

    fn weather_at(&self, config: &Environment, seconds: f64) -> Weather {
        self.weather_override
            .map_or_else(|| config.weather.at(seconds), |active| active.at(seconds))
    }

    fn level_for(&self, zone: &WaterZone) -> f32 {
        self.level_at(zone, self.elapsed)
    }

    fn level_at(&self, zone: &WaterZone, seconds: f64) -> f32 {
        self.water_overrides
            .get(&zone.id)
            .map_or(zone.level, |active| active.at(seconds))
            .clamp(zone.floor_y, zone.max_level)
    }
}

fn valid_position<const N: usize>(values: &[f32; N]) -> bool {
    values
        .iter()
        .all(|value| (-10_000.0..=10_000.0).contains(value))
}

fn valid_direction(direction: [f32; 3]) -> bool {
    direction.iter().all(|axis| (-1.0..=1.0).contains(axis))
        && Vec3::from_array(direction).length_squared() > 1e-8
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 64 && name.chars().all(|ch| !ch.is_control())
}

fn valid_timestamp(start: f64, elapsed: f64) -> bool {
    (0.0..=elapsed).contains(&start)
}

fn phase_at(seconds: f64, hz: f32) -> f32 {
    if !seconds.is_finite() {
        return 0.0;
    }
    let hz = f64::from(hz);
    (seconds.rem_euclid(1.0 / hz) * hz * std::f64::consts::TAU) as f32
}

fn valid_blend(seconds: f32) -> bool {
    (0.0..=600.0).contains(&seconds)
}

fn lerp(from: f32, to: f32, factor: f32) -> f32 {
    // Exact endpoints also let repeated rule actions recognize a settled level.
    if factor <= 0.0 {
        from
    } else if factor >= 1.0 {
        to
    } else {
        from + (to - from) * factor
    }
}

fn smooth01(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn ramp(start: f64, duration: f32, now: f64) -> f32 {
    if duration <= 0.0 {
        1.0
    } else {
        smooth01(((now - start) / f64::from(duration)) as f32)
    }
}

/// Distinct from physics body identifiers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MillVolumes {
    pub wind: ZoneId,
    pub water: ZoneId,
    pub sluice_level: f32,
}

pub fn mill_demo() -> (Environment, MillVolumes) {
    let mut config = Environment::default();
    config.time.start_hour = 15.5;
    config.time.hours_per_second = 0.19;
    let wind = config
        .add_wind([2.7, 1.3, 0.9])
        .expect("finite mill fixture");
    let airflow = &mut config.winds[0];
    airflow.name = "Ветер у мельницы".into();
    airflow.size = [5.0, 3.4, 4.0];
    airflow.direction = [0.87, 0.0, 0.48];
    airflow.speed = 5.0;
    airflow.gust = 1.4;
    airflow.gust_hz = 0.47;
    let water = config
        .add_water([0.0, 3.5], -0.40)
        .expect("finite mill fixture");
    let channel = &mut config.waters[0];
    channel.name = "Канал шлюза".into();
    channel.size = [4.0, 3.0];
    channel.level = -0.20;
    channel.max_level = 0.85;
    channel.current = [0.23, 0.0];
    channel.wave_height = 0.042;
    (
        config,
        MillVolumes {
            wind,
            water,
            sluice_level: 0.35,
        },
    )
}
#[derive(Clone, Debug)]
pub struct Frame {
    pub atmosphere: Atmosphere,
    pub water: Vec<WaterSurface>,
    pub elapsed: f64,
    pub wind: Vec3,
    pub splashes: Vec<(Splash, f32)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_water_blend_survives_loading_and_level_changes() -> Result<(), Box<dyn std::error::Error>> {
        let mut authored = Environment::default();
        let id = authored
            .add_water([0.0, 0.0], -1.0)
            .ok_or("Не удалось создать водный объём.")?;
        let legacy = serde_json::to_value(&authored)?;
        assert!(legacy["waters"][0].get("blend_mode").is_none());
        let reopened: Environment = serde_json::from_value(legacy.clone())?;
        assert_eq!(
            reopened.basins().next().map(|water| water.blend_mode),
            Some(BlendMode::Normal)
        );
        assert_eq!(serde_json::to_value(reopened)?, legacy);

        authored.waters[0].blend_mode = BlendMode::Add;
        let saved = serde_json::to_value(authored)?;
        assert_eq!(saved["waters"][0]["blend_mode"], "Add");
        let reopened: Environment = serde_json::from_value(saved)?;
        reopened.validate()?;
        assert_eq!(
            reopened.basins().next().map(|water| water.blend_mode),
            Some(BlendMode::Add)
        );
        let mut session = Session::new(&reopened);
        session.set_water_level(&reopened, id, 0.5, 1.0)?;
        session.tick(&reopened, 1.0);
        let surface = session
            .surfaces(&reopened)
            .next()
            .ok_or("Вода не отрисована.")?;
        assert_eq!(surface.height, 0.5);
        assert_eq!(surface.blend_mode, BlendMode::Add);
        Ok(())
    }

    #[test]
    fn surface_wetness_outlasts_rain_and_survives_an_override_checkpoint() {
        let mut config = Environment::default();
        config.weather.initial = WeatherPreset::Rain;
        let mut session = Session::new(&config);
        session.set_weather(&config, WeatherPreset::Clear, 0.0).unwrap();
        session.tick(&config, 2.0);
        let damp = session.sample(&config);
        assert_eq!(damp.rain, 0.0);
        assert!(damp.surface_wet > 0.65);
        let mut resumed = Session::restore(&config, session.snapshot()).unwrap();
        assert_eq!(resumed.sample(&config), damp);
        resumed.tick(&config, 120.0);
        assert!(resumed.sample(&config).surface_wet < 0.01);

        config.weather.initial = WeatherPreset::Clear;
        config.weather.add_cue(WeatherPreset::Snow).unwrap();
        assert!(config.sample(5.0).surface_snow < config.sample(8.0).surface_snow);
    }

    #[test]
    fn coastal_profile_shapes_the_outline_depth_and_buoyancy() {
        let mut config = Environment::default();
        let id = config.add_water([1.0, 0.0], -1.0).unwrap();
        let zone = config.waters.iter_mut().find(|zone| zone.id == id).unwrap();
        zone.size = [8.0, 8.0];
        zone.rounded_edge = true;
        zone.level = 0.0;
        zone.shore = Some(WaterShore {
            origin: [0.0, 0.0],
            seaward: [1.0, 0.0],
            slope: 0.12,
        });
        let surface = zone.basin(zone.level);
        assert_eq!(surface.span_at_z(0.0), Some([0.0, 5.0]));
        assert_eq!(surface.span_at_z(4.01), None);
        assert!(!surface.contains_xz(Vec3::new(-0.2, 0.0, 0.0)));
        assert!(surface.contains_xz(Vec3::new(0.25, 0.0, 0.0)));
        assert!((surface.depth_at(Vec2::new(0.25, 0.0)) - 0.03).abs() < 1e-6);
        assert!((surface.wave_at(0.25, 0.0, 1.2) - surface.height).abs() <= 0.01351);
        let session = Session::new(&config);
        assert!(session.water_at(&config, Vec3::new(-0.2, 0.0, 0.0), 0.5).is_none());
        let shallow = session
            .water_at(&config, Vec3::new(0.25, 0.0, 0.0), 0.5)
            .unwrap();
        let deep = session
            .water_at(&config, Vec3::new(4.0, 0.0, 0.0), 0.5)
            .unwrap();
        assert!((shallow.immersion - 0.06).abs() < 1e-6);
        assert!((deep.immersion - 0.5).abs() < 1e-6);
        let saved = serde_json::to_string(&config).unwrap();
        let restored: Environment = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored, config);
    }

    #[test]
    fn snowfall_blends_independently_from_rain_and_survives_resume() {
        let mut config = Environment::default();
        config.weather.initial = WeatherPreset::Rain;
        let mut session = Session::new(&config);
        session.set_weather(&config, WeatherPreset::Snow, 4.0).unwrap();
        session.tick(&config, 2.0);
        let middle = session.sample(&config);
        assert!((middle.snow - 0.5).abs() < 1e-5);
        assert!((middle.rain - 0.5).abs() < 1e-5);
        let encoded = serde_json::to_vec(&session.snapshot()).unwrap();
        let mut resumed = Session::restore(&config, serde_json::from_slice(&encoded).unwrap()).unwrap();
        assert_eq!(resumed.sample(&config), middle);
        resumed.tick(&config, 2.0);
        let settled = resumed.sample(&config);
        assert_eq!((settled.snow, settled.rain), (1.0, 0.0));
    }

    #[test]
    fn afternoon_to_sunset_moves_shadows_and_warms_sun_and_sky_together() {
        let mut config = Environment::default();
        config.time.start_hour = 12.0;
        config.time.hours_per_second = 0.5;
        let noon = config.sample(0.0);
        let sunset = config.sample(11.6);
        let night = config.sample(24.0);
        assert!((sunset.hour - 17.8).abs() < 1e-4);
        assert!(noon.sun_direction.y > 0.8 && sunset.sun_direction.y < 0.1);
        assert!(noon.sun_direction.dot(sunset.sun_direction) < 0.80);
        assert!(sunset.sky_horizon.x > sunset.sky_horizon.z);
        assert!(sunset.sun_color.z < noon.sun_color.z * 0.5);
        assert!(sunset.sun_strength < noon.sun_strength * 0.25);
        assert!(sunset.ambient.length() < noon.ambient.length());
        assert_eq!(night.sun_strength, 0.0);
        assert!(night.sun_direction.y > 0.0 && night.sun_direction.is_normalized());
        assert!(config.sample(f64::MAX).hour.is_finite());
    }

    #[test]
    fn authored_weather_blends_and_emits_ordered_events_only_once() {
        let mut config = Environment::default();
        config.weather.cues = vec![
            WeatherCue {
                id: 7,
                start_seconds: 1.0,
                preset: WeatherPreset::Rain,
                blend_seconds: 2.0,
            },
            WeatherCue {
                id: 9,
                start_seconds: 5.0,
                preset: WeatherPreset::Fog,
                blend_seconds: 1.0,
            },
        ];
        config.validate().unwrap();
        let mut session = Session::new(&config);
        let clear = session.sample(&config);
        assert_eq!(clear.rain, 0.0);
        session.tick(&config, 1.0);
        assert_eq!(
            session.take_events(),
            [Event {
                seconds: 1.0,
                kind: EventKind::WeatherStarted {
                    preset: WeatherPreset::Rain,
                    cue: Some(7)
                },
            }]
        );
        assert_eq!(session.sample(&config), clear);
        session.tick(&config, 1.0);
        assert!((session.sample(&config).rain - 0.5).abs() < 1e-6);
        session.tick(&config, 1.0);
        let rain = session.sample(&config);
        assert_eq!(rain.rain, 1.0);
        assert!(rain.sun_strength < clear.sun_strength);
        session.tick(&config, 3.0);
        assert_eq!(
            session
                .take_events()
                .into_iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            [
                EventKind::WeatherSettled {
                    preset: WeatherPreset::Rain,
                    cue: Some(7)
                },
                EventKind::WeatherStarted {
                    preset: WeatherPreset::Fog,
                    cue: Some(9)
                },
                EventKind::WeatherSettled {
                    preset: WeatherPreset::Fog,
                    cue: Some(9)
                },
            ]
        );
        let fog = session.sample(&config);
        assert_eq!(fog.rain, 0.0);
        assert!(fog.fog_density > rain.fog_density * 3.0);
        session.tick(&config, 9.0);
        assert!(session.take_events().is_empty());
    }

    #[test]
    fn rule_begins_weather_blend_from_visible_state_and_can_override_schedule() {
        let mut config = Environment::default();
        config.weather.cues.push(WeatherCue {
            id: 2,
            start_seconds: 0.0,
            preset: WeatherPreset::Rain,
            blend_seconds: 4.0,
        });
        let mut session = Session::new(&config);
        assert_eq!(session.take_events().len(), 1);
        session.tick(&config, 2.0);
        let halfway = session.sample(&config);
        assert!(
            session
                .set_weather(&config, WeatherPreset::Fog, 2.0)
                .unwrap()
        );
        assert_eq!(session.sample(&config), halfway);
        assert!(
            !session
                .set_weather(&config, WeatherPreset::Fog, 9.0)
                .unwrap()
        );
        assert_eq!(
            session.take_events()[0].kind,
            EventKind::WeatherStarted {
                preset: WeatherPreset::Fog,
                cue: None,
            }
        );
        session.tick(&config, 2.0);
        assert_eq!(
            session.take_events(),
            [Event {
                seconds: 4.0,
                kind: EventKind::WeatherSettled {
                    preset: WeatherPreset::Fog,
                    cue: None
                },
            }]
        );

        let mut neutral = Session::new(&Environment::default());
        assert!(
            !neutral
                .set_weather(&Environment::default(), WeatherPreset::Clear, 2.0)
                .unwrap()
        );
        neutral.tick(&Environment::default(), 3.0);
        assert!(neutral.take_events().is_empty());
    }

    #[test]
    fn local_airflow_is_deterministic_and_fades_at_zone_edge() {
        let mut config = Environment::default();
        config.add_wind([0.0, 1.5, 0.0]).unwrap();
        let center = Vec3::new(0.0, 1.5, 0.0);
        let outside = Vec3::new(4.1, 1.5, 0.0);
        let edge = Vec3::new(4.0, 1.5, 0.0);
        let baseline = config.wind_at(outside, 2.1);
        let local = config.wind_at(center, 2.1);
        assert_eq!(baseline, config.wind_at(edge, 2.1));
        assert!(local.x > baseline.x + 2.0);
        assert_eq!(local, config.wind_at(center, 2.1));
        assert_ne!(local, config.wind_at(center, 2.8));
        assert!(config.wind_at(center, f64::MAX).is_finite());
        let near_edge = config.wind_at(Vec3::new(3.9, 1.5, 0.0), 2.1);
        assert!(near_edge.x - baseline.x < local.x * 0.12);
    }

    #[test]
    fn rounded_puddle_leaves_its_four_corners_dry() {
        let mut config = Environment::default();
        config.add_water([0.0; 2], -0.04).unwrap();
        let water = &mut config.waters[0];
        water.size = [4.0, 2.0];
        water.level = 0.02;
        let corner = Vec3::new(1.75, 0.0, 0.88);
        assert!(water.basin(water.level).contains_xz(corner));
        water.rounded_edge = true;
        let surface = water.basin(water.level);
        assert!(!surface.contains_xz(corner));
        assert!(surface.contains_xz(Vec3::ZERO));
        assert!(surface.half_width_at_z(corner.z) < corner.x);
    }

    #[test]
    fn water_level_current_waves_and_splash_share_a_volume() {
        let mut config = Environment::default();
        let id = config.add_water([0.0, 0.0], 0.0).unwrap();
        config.waters[0].level = 0.0;
        config.waters[0].current = [0.85, 0.0];
        let mut session = Session::new(&config);
        assert_eq!(session.surfaces(&config).count(), 0);
        let basin = session.basins(&config).next().unwrap();
        assert_eq!(basin.height, basin.floor_y);
        assert_eq!(basin.wave_height, 0.0);
        session.set_water_level(&config, id, 1.4, 2.0).unwrap();
        session.tick(&config, 1.0);
        assert!((session.level(&config, id).unwrap() - 0.7).abs() < 1e-6);
        assert!(session.take_events().is_empty());
        assert!(
            session
                .water_at(&config, Vec3::new(0.0, 1.0, 0.0), 0.4)
                .is_none()
        );
        session.tick(&config, 1.0);
        assert_eq!(
            session.take_events()[0].kind,
            EventKind::WaterLevelReached { zone: id }
        );
        assert_eq!(session.level(&config, id), Some(1.4));
        let surface = session.surfaces(&config).next().unwrap();
        assert!((surface.wave_at(0.4, 0.7, 3.5) - surface.height).abs() <= surface.wave_height);
        assert_ne!(
            surface.wave_at(0.4, 0.7, 3.5),
            surface.wave_at(0.4, 0.7, 3.9)
        );
        let water = session
            .water_at(&config, Vec3::new(0.0, 1.4, 0.0), 0.4)
            .unwrap();
        assert!((water.immersion - 0.5).abs() < 1e-6);
        let impulse = water.apply_to_velocity(Vec3::ZERO, 9.81, 0.15);
        assert!(impulse.x > 0.0 && impulse.y > 0.0);
        let splash = session
            .splash_between(
                &config,
                Vec3::new(0.0, 1.9, 0.0),
                Vec3::new(0.0, 1.3, 0.0),
                0.4,
                0.08,
            )
            .unwrap();
        assert_eq!(splash.zone, id);
        assert_eq!(splash.position.y, 1.4);
        assert!(splash.strength > 0.7);
        assert!(
            session
                .splash_between(
                    &config,
                    Vec3::new(4.0, 1.9, 0.0),
                    Vec3::new(4.0, 1.3, 0.0),
                    0.4,
                    0.08,
                )
                .is_none()
        );
    }

    #[test]
    fn rising_water_lifts_and_drifts_a_body_under_fixed_step_gravity() {
        let mut config = Environment::default();
        let id = config.add_water([0.0, 0.0], 0.0).unwrap();
        config.waters[0].level = 0.0;
        config.waters[0].size = [10.0, 10.0];
        config.waters[0].current = [0.25, 0.0];
        let mut session = Session::new(&config);
        let dt = 1.0_f32 / 120.0;
        let mut center = Vec3::new(0.0, 0.25, 0.0);
        let mut velocity = Vec3::ZERO;
        session.set_water_level(&config, id, 1.2, 2.0).unwrap();
        for _ in 0..960 {
            session.tick(&config, f64::from(dt));
            velocity -= Vec3::Y * (9.81 * dt);
            if let Some(water) = session.water_at(&config, center, 0.50) {
                velocity = water.apply_to_velocity(velocity, 9.81, dt);
            }
            center += velocity * dt;
            if center.y < 0.25 {
                center.y = 0.25;
                velocity.y = velocity.y.max(0.0);
            }
        }
        assert!((1.06..1.19).contains(&center.y), "body {:?}", center);
        assert!(center.x > 1.0 && center.x < 2.0);
        assert!(velocity.y.abs() < 0.12);
    }

    #[test]
    fn body_force_opt_in_respects_fixed_step_time_during_a_blend() {
        let mut config = Environment::default();
        let id = config.add_water([0.0, 0.0], 0.0).unwrap();
        config.waters[0].level = 0.0;
        let mut session = Session::new(&config);
        session.set_water_level(&config, id, 1.2, 1.0).unwrap();
        session.tick(&config, 2.0);
        let body = BodyInput {
            center: Vec3::new(0.0, 0.65, 0.0),
            height: 0.4,
            velocity: Vec3::ZERO,
            wind_drag: 0.0,
            buoyant: true,
        };
        let dry = session.body_velocity(&config, body, 9.81, 0.02, 0.1);
        let wet = session.body_velocity(&config, body, 9.81, 0.02, 1.1);
        let disabled = session.body_velocity(
            &config,
            BodyInput {
                buoyant: false,
                ..body
            },
            9.81,
            0.02,
            1.1,
        );
        assert_eq!(dry, Vec3::ZERO);
        assert!(wet.y > 0.25 && wet.z > 0.0);
        assert_eq!(disabled, Vec3::ZERO);
    }

    #[test]
    fn authored_and_running_environment_roundtrip_with_blends_in_progress() {
        let mut config = Environment::default();
        config.time.hours_per_second = 0.17;
        let wind = config.add_wind([0.0, 0.0, 0.0]).unwrap();
        let water = config.add_water([0.0, 0.0], 0.0).unwrap();
        let config: Environment =
            serde_json::from_slice(&serde_json::to_vec(&config).unwrap()).unwrap();
        config.validate().unwrap();
        assert_ne!(wind, water);
        let mut session = Session::new(&config);
        session
            .set_weather(&config, WeatherPreset::Rain, 4.0)
            .unwrap();
        session.set_water_level(&config, water, 1.5, 2.0).unwrap();
        session.tick(&config, 1.0);
        session.take_events();
        let snapshot: Snapshot =
            serde_json::from_slice(&serde_json::to_vec(&session.snapshot()).unwrap()).unwrap();
        let mut restored = Session::restore(&config, snapshot).unwrap();
        assert_eq!(restored.sample(&config), session.sample(&config));
        assert_eq!(
            restored.level(&config, water),
            session.level(&config, water)
        );
        assert!(restored.take_events().is_empty());
        session.tick(&config, 3.0);
        restored.tick(&config, 3.0);
        assert_eq!(session.take_events(), restored.take_events());
    }

    #[test]
    fn validation_rejects_duplicate_volume_ids_and_overlapping_weather() {
        let mut config = Environment::default();
        let wind = config.add_wind([0.0, 0.0, 0.0]).unwrap();
        config.add_water([0.0, 0.0], 0.0).unwrap();
        config.waters[0].id = wind;
        assert!(config.validate().is_err());
        config.waters.clear();
        config.weather.cues = vec![
            WeatherCue {
                id: 1,
                start_seconds: 1.0,
                preset: WeatherPreset::Rain,
                blend_seconds: 5.0,
            },
            WeatherCue {
                id: 2,
                start_seconds: 4.0,
                preset: WeatherPreset::Fog,
                blend_seconds: 3.0,
            },
        ];
        assert!(config.validate().is_err());
    }
}
