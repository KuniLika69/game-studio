//! Fixed-step translation of box bodies and physical sensor edges.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::world::Collider;

#[path = "physics_snapshot.rs"]
mod snapshot;
pub use snapshot::Snapshot;

mod bounds_serde {
    use super::{Collider, Deserialize, Serialize};
    use serde::{Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        bounds: &Collider,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        [bounds.min, bounds.max].serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Collider, D::Error> {
        let [min, max] = <[[f32; 3]; 2]>::deserialize(deserializer)?;
        Ok(Collider { min, max })
    }
}

const FIXED_STEP: f64 = 1.0 / 120.0;
const MAX_STEPS: u8 = 8;
pub const MAX_BODIES: usize = 32;
pub const MAX_SENSORS: usize = 64;
const MAX_IMPACTS: usize = MAX_BODIES * 2;
const CONTACT_SLOP: f32 = 0.00002;
const MIN_BOUNCE_SPEED: f32 = 0.5;
const MAX_SPEED: f32 = 80.0;
pub const GRAVITY: f32 = 9.81;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(usize)]
pub enum Axis {
    X = 0,
    Y = 1,
    Z = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub friction: f32,
    pub restitution: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            friction: 0.65,
            restitution: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxisJoint {
    pub axis: Axis,
    /// The lower and upper translation, relative to the body's starting center.
    pub travel: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum BodyKind {
    Dynamic {
        mass: f32,
        joint: Option<AxisJoint>,
    },
    Platform {
        axis: Axis,
        travel: f32,
        period: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodySpec {
    pub id: u64,
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
    pub material: Material,
    pub kind: BodyKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solid {
    pub bounds: Collider,
    pub material: Material,
}

impl From<Collider> for Solid {
    fn from(bounds: Collider) -> Self {
        Self {
            bounds,
            material: Material::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SensorKind {
    Overlap,
    /// Press when the combined mass resting on the sensor's top reaches the limit.
    Pressure {
        minimum_mass: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sensor {
    pub id: u64,
    #[serde(with = "bounds_serde")]
    pub bounds: Collider,
    pub kind: SensorKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorContact {
    pub id: u64,
    pub bounds: Collider,
    pub mass: f32,
    pub grounded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ContactSource {
    Body(u64),
    Actor(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Entered(ContactSource),
    Exited(ContactSource),
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerEvent {
    pub tick: u64,
    pub sensor_id: u64,
    pub kind: Trigger,
}

#[derive(Debug, Default, PartialEq)]
pub struct StepReport {
    pub substeps: u8,
    pub discarded_seconds: f64,
    pub saturated_substeps: u8,
    pub events: Vec<TriggerEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyPose {
    pub center: [f32; 3],
    pub velocity: [f32; 3],
    pub bounds: Collider,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    TooManyComponents,
    DuplicateId(u64),
    InvalidBody(u64),
    InvalidSensor(u64),
    InvalidId,
}

#[derive(Debug, Default)]
pub struct Simulation {
    bodies: Vec<Body>,
    sensors: Vec<SensorState>,
    clock: u64,
    accumulator: f64,
}

#[derive(Debug)]
struct Body {
    spec: BodySpec,
    center: [f32; 3],
    velocity: [f32; 3],
}

#[derive(Debug)]
struct SensorState {
    spec: Sensor,
    overlapping: BTreeSet<ContactSource>,
    pressed: bool,
}

impl Simulation {
    pub fn new(bodies: &[BodySpec], sensors: &[Sensor]) -> Result<Self, LayoutError> {
        if bodies.len() > MAX_BODIES || sensors.len() > MAX_SENSORS {
            return Err(LayoutError::TooManyComponents);
        }
        validate_ids(bodies.iter().map(|body| body.id))?;
        validate_ids(sensors.iter().map(|sensor| sensor.id))?;
        if let Some(body) = bodies.iter().find(|spec| !valid_body(spec)) {
            return Err(LayoutError::InvalidBody(body.id));
        }
        if let Some(sensor) = sensors.iter().find(|sensor| {
            !valid_bounds(sensor.bounds)
                || matches!(sensor.kind, SensorKind::Pressure { minimum_mass }
                    if !(0.01..=30_000.0).contains(&minimum_mass))
        }) {
            return Err(LayoutError::InvalidSensor(sensor.id));
        }

        let mut bodies: Vec<_> = bodies
            .iter()
            .map(|&spec| Body {
                center: spec.center,
                velocity: [0.0; 3],
                spec,
            })
            .collect();
        bodies.sort_unstable_by_key(|body| body.spec.id);
        let mut sensors: Vec<_> = sensors
            .iter()
            .map(|&spec| SensorState {
                spec,
                overlapping: BTreeSet::new(),
                pressed: false,
            })
            .collect();
        sensors.sort_unstable_by_key(|state| state.spec.id);
        Ok(Self {
            bodies,
            sensors,
            ..Self::default()
        })
    }

    pub fn body(&self, id: u64) -> Option<BodyPose> {
        self.bodies
            .binary_search_by_key(&id, |body| body.spec.id)
            .ok()
            .map(|index| self.bodies[index].pose())
    }

    pub fn poses(&self) -> impl Iterator<Item = (u64, BodyPose)> + '_ {
        self.bodies.iter().map(|body| (body.spec.id, body.pose()))
    }

    pub fn elapsed_seconds(&self) -> f64 {
        self.clock as f64 * FIXED_STEP + self.accumulator
    }

    pub fn supporting_platform(&self, bounds: Collider) -> Option<(u64, BodyPose)> {
        self.bodies
            .iter()
            .filter(|body| matches!(body.spec.kind, BodyKind::Platform { .. }))
            .filter(|body| supports(body.bounds(), bounds))
            .max_by(|left, right| left.bounds().max[1].total_cmp(&right.bounds().max[1]))
            .map(|body| (body.spec.id, body.pose()))
    }

    pub fn plate_pressed(&self, id: u64) -> Option<bool> {
        self.sensors
            .iter()
            .find(|state| state.spec.id == id)
            .filter(|state| matches!(state.spec.kind, SensorKind::Pressure { .. }))
            .map(|state| state.pressed)
    }

    /// Add a world-space impulse in kilogram metres per second.
    #[cfg(test)]
    pub fn push_impulse(&mut self, id: u64, impulse: [f32; 3]) -> bool {
        let Ok(index) = self.bodies.binary_search_by_key(&id, |body| body.spec.id) else {
            return false;
        };
        let body = &mut self.bodies[index];
        if !body.is_dynamic() || !impulse.iter().all(|value| value.is_finite()) {
            return false;
        }
        for (axis, impulse) in impulse.into_iter().enumerate() {
            body.velocity[axis] = (body.velocity[axis] + impulse * body.inverse_mass(axis))
                .clamp(-MAX_SPEED, MAX_SPEED);
        }
        true
    }

    /// Legacy static colliders use Solid::from. Actors supplied here participate
    /// in sensors only. Their movement controller can query poses separately.
    #[cfg(test)]
    pub fn step(&mut self, seconds: f64, solids: &[Solid], actors: &[ActorContact]) -> StepReport {
        self.step_with(seconds, solids, actors, |_, pose, _, _| pose.velocity)
    }

    /// After gravity, apply optional scene forces to each dynamic body in stable ID order.
    pub fn step_with(
        &mut self,
        seconds: f64,
        solids: &[Solid],
        actors: &[ActorContact],
        mut adjust: impl FnMut(u64, BodyPose, f32, f64) -> [f32; 3],
    ) -> StepReport {
        let mut report = StepReport::default();
        if !seconds.is_finite() || seconds <= 0.0 {
            return report;
        }
        self.accumulator += seconds;
        let steps_pending = (self.accumulator / FIXED_STEP + 1.0e-9).floor();
        report.substeps = steps_pending.min(f64::from(MAX_STEPS)) as u8;
        if steps_pending <= 0.0 {
            return report;
        }
        let accrued = self.accumulator;
        self.accumulator = self.accumulator.rem_euclid(FIXED_STEP);
        if self.accumulator > FIXED_STEP - 1.0e-10 {
            self.accumulator = 0.0;
        }

        if steps_pending > f64::from(MAX_STEPS) {
            report.discarded_seconds =
                accrued - self.accumulator - f64::from(MAX_STEPS) * FIXED_STEP;
        }
        for _ in 0..report.substeps {
            self.clock = self.clock.saturating_add(1);
            if self.fixed_step(solids, actors, &mut report.events, &mut adjust) {
                report.saturated_substeps += 1;
            }
        }
        report
    }

    fn fixed_step(
        &mut self,
        solids: &[Solid],
        actors: &[ActorContact],
        events: &mut Vec<TriggerEvent>,
        adjust: &mut impl FnMut(u64, BodyPose, f32, f64) -> [f32; 3],
    ) -> bool {
        let dt = FIXED_STEP as f32;
        for body in &mut self.bodies {
            body.update_velocity(self.clock, dt);
            if body.is_dynamic() {
                let velocity = adjust(
                    body.spec.id,
                    body.pose(),
                    dt,
                    self.clock as f64 * FIXED_STEP,
                );
                for (axis, velocity) in velocity.into_iter().enumerate() {
                    if velocity.is_finite() && body.inverse_mass(axis) > 0.0 {
                        body.velocity[axis] = velocity.clamp(-MAX_SPEED, MAX_SPEED);
                    }
                }
            }
        }
        self.separate_overlaps(solids);
        let mut swept = BTreeSet::new();
        let mut remaining = dt;
        for _ in 0..MAX_IMPACTS {
            let Some(impact) = self.first_impact(solids, remaining) else {
                self.advance_positions(remaining, &mut swept);
                remaining = 0.0;
                break;
            };
            let used = remaining * impact.fraction;
            self.advance_positions(used, &mut swept);
            remaining -= used;
            self.respond(impact, solids);
            if remaining <= 1.0e-8 {
                break;
            }
        }
        self.update_sensors(actors, &swept, events);
        // Do not advance through a blocker after exhausting the contact budget.
        remaining > 1.0e-8
    }

    fn advance_positions(&mut self, duration: f32, swept: &mut BTreeSet<(u64, ContactSource)>) {
        if duration <= 0.0 {
            return;
        }
        for body in &mut self.bodies {
            let delta = body.velocity.map(|velocity| velocity * duration);
            let bounds = body.bounds();
            for state in &self.sensors {
                if state.spec.kind == SensorKind::Overlap
                    && (intersects(bounds, state.spec.bounds)
                        || sweep(bounds, state.spec.bounds, delta, |_| true).is_some())
                {
                    swept.insert((state.spec.id, ContactSource::Body(body.spec.id)));
                }
            }
            for (position, delta) in body.center.iter_mut().zip(delta) {
                *position += delta;
            }
        }
    }

    fn first_impact(&self, solids: &[Solid], remaining: f32) -> Option<Impact> {
        let mut best: Option<Impact> = None;
        let mut inspect = |impact: Impact| {
            if best.is_none_or(|best| best.fraction > impact.fraction) {
                best = Some(impact);
            }
        };
        for (index, body) in self.bodies.iter().enumerate() {
            let bounds = body.bounds();
            if let Some((axis, boundary)) = body.joint_hit(remaining) {
                let fraction = (boundary - body.center[axis]) / (body.velocity[axis] * remaining);
                inspect(Impact {
                    body: index,
                    target: Target::Joint(boundary),
                    fraction: fraction.clamp(0.0, 1.0),
                    axis,
                    normal: -body.velocity[axis].signum(),
                });
            }
            if body.is_dynamic() {
                let delta = body.velocity.map(|velocity| velocity * remaining);
                for (other, solid) in solids.iter().enumerate() {
                    if let Some(hit) = sweep(bounds, solid.bounds, delta, |axis| {
                        body.inverse_mass(axis) > 0.0
                    }) {
                        inspect(hit.as_impact(index, Target::Solid(other)));
                    }
                }
            }
            for (other, obstacle) in self.bodies.iter().enumerate().skip(index + 1) {
                if !body.is_dynamic() && !obstacle.is_dynamic() {
                    continue;
                }
                let delta = std::array::from_fn(|axis| {
                    (body.velocity[axis] - obstacle.velocity[axis]) * remaining
                });
                if let Some(hit) = sweep(bounds, obstacle.bounds(), delta, |axis| {
                    body.inverse_mass(axis) + obstacle.inverse_mass(axis) > 0.0
                }) {
                    inspect(hit.as_impact(index, Target::Body(other)));
                }
            }
        }
        best
    }

    fn respond(&mut self, impact: Impact, solids: &[Solid]) {
        match impact.target {
            Target::Joint(boundary) => {
                let body = &mut self.bodies[impact.body];
                body.center[impact.axis] = boundary;
                body.velocity[impact.axis] = 0.0;
            }
            Target::Solid(index) => {
                let body = &mut self.bodies[impact.body];
                resolve_contact(
                    body,
                    None,
                    solids[index].material,
                    impact.axis,
                    impact.normal,
                );
            }
            Target::Body(other) => {
                let (first, second) = self.bodies.split_at_mut(other);
                let body = &mut first[impact.body];
                let obstacle = &mut second[0];
                let material = obstacle.spec.material;
                resolve_contact(body, Some(obstacle), material, impact.axis, impact.normal);
            }
        }
    }

    fn separate_overlaps(&mut self, solids: &[Solid]) {
        for index in 0..self.bodies.len() {
            let body = &mut self.bodies[index];
            if body.is_dynamic() {
                for solid in solids {
                    if let Some((axis, offset)) = separation(body, solid.bounds, None) {
                        body.shift(axis, offset);
                    }
                }
            }
            let (first, rest) = self.bodies.split_at_mut(index + 1);
            let body = &mut first[index];
            for obstacle in rest {
                if !body.is_dynamic() && !obstacle.is_dynamic() {
                    continue;
                }
                if let Some((axis, offset)) = separation(body, obstacle.bounds(), Some(obstacle)) {
                    let weight_a = body.inverse_mass(axis);
                    let weight_b = obstacle.inverse_mass(axis);
                    let share = weight_a / (weight_a + weight_b);
                    body.shift(axis, offset * share);
                    obstacle.shift(axis, -offset * (1.0 - share));
                }
            }
        }
    }

    fn update_sensors(
        &mut self,
        actors: &[ActorContact],
        swept: &BTreeSet<(u64, ContactSource)>,
        events: &mut Vec<TriggerEvent>,
    ) {
        for state in &mut self.sensors {
            let sensor_id = state.spec.id;
            let tick = self.clock;
            let mut emit = |kind| {
                events.push(TriggerEvent {
                    tick,
                    sensor_id,
                    kind,
                })
            };
            let zone = state.spec.bounds;
            match state.spec.kind {
                SensorKind::Overlap => {
                    let present = overlappers(zone, &self.bodies, actors);
                    for &contact in state.overlapping.difference(&present) {
                        emit(Trigger::Exited(contact));
                    }
                    for &contact in present.difference(&state.overlapping) {
                        emit(Trigger::Entered(contact));
                    }
                    let first = (sensor_id, ContactSource::Body(0));
                    for &(_, contact) in swept
                        .range(first..)
                        .take_while(|entry| entry.0 == sensor_id)
                    {
                        if !present.contains(&contact) && !state.overlapping.contains(&contact) {
                            emit(Trigger::Entered(contact));
                            emit(Trigger::Exited(contact));
                        }
                    }
                    state.overlapping = present;
                }
                SensorKind::Pressure { minimum_mass } => {
                    let object_mass: f32 = self
                        .bodies
                        .iter()
                        .filter_map(|body| match body.spec.kind {
                            BodyKind::Dynamic { mass, .. }
                                if body.velocity[1].abs() <= 0.15
                                    && supports(zone, body.bounds()) =>
                            {
                                Some(mass)
                            }
                            _ => None,
                        })
                        .sum();
                    let mut actors_by_id = BTreeMap::<u64, f32>::new();
                    for actor in actors.iter().filter(|actor| {
                        actor.id != 0
                            && actor.grounded
                            && actor.mass.is_finite()
                            && actor.mass > 0.0
                            && supports(zone, actor.bounds)
                    }) {
                        let mass = actors_by_id.entry(actor.id).or_default();
                        *mass = mass.max(actor.mass);
                    }
                    let actor_mass: f32 = actors_by_id.values().sum();
                    let pressed = object_mass + actor_mass >= minimum_mass;
                    if pressed != state.pressed {
                        emit(if pressed {
                            Trigger::Pressed
                        } else {
                            Trigger::Released
                        });
                        state.pressed = pressed;
                    }
                }
            }
        }
    }
}

impl Body {
    fn pose(&self) -> BodyPose {
        BodyPose {
            center: self.center,
            velocity: self.velocity,
            bounds: self.bounds(),
        }
    }

    fn bounds(&self) -> Collider {
        Collider {
            min: std::array::from_fn(|axis| self.center[axis] - self.spec.half_extents[axis]),
            max: std::array::from_fn(|axis| self.center[axis] + self.spec.half_extents[axis]),
        }
    }

    fn is_dynamic(&self) -> bool {
        matches!(self.spec.kind, BodyKind::Dynamic { .. })
    }

    fn inverse_mass(&self, axis: usize) -> f32 {
        match self.spec.kind {
            BodyKind::Dynamic { mass, joint }
                if joint.is_none_or(|joint| joint.axis as usize == axis) =>
            {
                mass.recip()
            }
            _ => 0.0,
        }
    }

    fn joint_bounds(&self) -> Option<(usize, [f32; 2])> {
        let BodyKind::Dynamic {
            joint: Some(joint), ..
        } = self.spec.kind
        else {
            return None;
        };
        let axis = joint.axis as usize;
        Some((
            axis,
            joint.travel.map(|offset| self.spec.center[axis] + offset),
        ))
    }

    fn joint_hit(&self, dt: f32) -> Option<(usize, f32)> {
        let (axis, [min, max]) = self.joint_bounds()?;
        let speed = self.velocity[axis];
        let target = self.center[axis] + speed * dt;
        if speed < 0.0 && target < min {
            Some((axis, min))
        } else if speed > 0.0 && target > max {
            Some((axis, max))
        } else {
            None
        }
    }

    fn shift(&mut self, axis: usize, offset: f32) {
        if self.inverse_mass(axis) == 0.0 {
            return;
        }
        self.center[axis] += offset;
        if let Some((_, [min, max])) = self.joint_bounds() {
            self.center[axis] = self.center[axis].clamp(min, max);
        }
    }

    fn update_velocity(&mut self, clock: u64, dt: f32) {
        match self.spec.kind {
            BodyKind::Dynamic { .. } => {
                if self.inverse_mass(Axis::Y as usize) > 0.0 {
                    self.velocity[1] = (self.velocity[1] - GRAVITY * dt).max(-MAX_SPEED);
                }
            }
            BodyKind::Platform {
                axis,
                travel,
                period,
            } => {
                let seconds = clock as f64 * FIXED_STEP;
                let phase = (seconds / f64::from(period)).fract();
                let progress = (1.0 - (2.0 * phase - 1.0).abs()) as f32;
                let axis = axis as usize;
                let target = self.spec.center[axis] + travel * progress;
                self.velocity[axis] =
                    ((target - self.center[axis]) / dt).clamp(-MAX_SPEED, MAX_SPEED);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Target {
    Solid(usize),
    Body(usize),
    Joint(f32),
}

#[derive(Clone, Copy)]
struct Impact {
    body: usize,
    target: Target,
    fraction: f32,
    axis: usize,
    normal: f32,
}

struct Hit {
    fraction: f32,
    axis: usize,
    normal: f32,
}

impl Hit {
    fn as_impact(self, body: usize, target: Target) -> Impact {
        Impact {
            body,
            target,
            fraction: self.fraction,
            axis: self.axis,
            normal: self.normal,
        }
    }
}

fn sweep(
    a: Collider,
    b: Collider,
    delta: [f32; 3],
    accepts: impl Fn(usize) -> bool,
) -> Option<Hit> {
    let mut enter = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    let mut axis_normal = None;
    for (axis, delta) in delta.into_iter().enumerate() {
        let low = b.min[axis] - a.max[axis];
        let high = b.max[axis] - a.min[axis];
        if delta.abs() <= f32::EPSILON {
            if low >= -CONTACT_SLOP || high <= CONTACT_SLOP {
                return None;
            }
            continue;
        }
        let (near, far, sign) = if delta > 0.0 {
            (low, high, -1.0)
        } else {
            (high, low, 1.0)
        };
        let first = if near.abs() <= CONTACT_SLOP {
            0.0
        } else {
            near / delta
        };
        if first > enter || first == enter && axis_normal.is_none() {
            enter = first;
            axis_normal = accepts(axis).then_some((axis, sign));
        }
        exit = exit.min(far / delta);
        if enter > exit {
            return None;
        }
    }
    let (axis, normal) = axis_normal?;
    (enter >= 0.0 && enter <= 1.0 && exit >= 0.0).then_some(Hit {
        fraction: enter,
        axis,
        normal,
    })
}

fn separation(body: &Body, other: Collider, partner: Option<&Body>) -> Option<(usize, f32)> {
    let bounds = body.bounds();
    if !intersects(bounds, other) {
        return None;
    }
    (0..3)
        .filter(|&axis| {
            body.inverse_mass(axis) + partner.map_or(0.0, |body| body.inverse_mass(axis)) > 0.0
        })
        .map(|axis| {
            let negative = other.min[axis] - bounds.max[axis] - CONTACT_SLOP;
            let positive = other.max[axis] - bounds.min[axis] + CONTACT_SLOP;
            let offset = if -negative < positive {
                negative
            } else {
                positive
            };
            (axis, offset)
        })
        .min_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
}

fn resolve_contact(
    first: &mut Body,
    mut other: Option<&mut Body>,
    other_material: Material,
    axis: usize,
    normal: f32,
) {
    let opposite_velocity = other.as_ref().map_or([0.0; 3], |body| body.velocity);
    let relative: [f32; 3] =
        std::array::from_fn(|axis| first.velocity[axis] - opposite_velocity[axis]);
    let closing = -relative[axis] * normal;
    let inv_mass: [f32; 3] = std::array::from_fn(|axis| {
        first.inverse_mass(axis) + other.as_ref().map_or(0.0, |body| body.inverse_mass(axis))
    });
    if closing <= 0.0 || inv_mass[axis] <= 0.0 {
        return;
    }
    let bounce = if closing >= MIN_BOUNCE_SPEED {
        first
            .spec
            .material
            .restitution
            .max(other_material.restitution)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let normal_impulse = (1.0 + bounce) * closing / inv_mass[axis];
    let mut impulse: [f32; 3] = std::array::from_fn(|tangent| {
        if tangent != axis && inv_mass[tangent] > 0.0 {
            -relative[tangent] / inv_mass[tangent]
        } else {
            0.0
        }
    });
    let tangent_length = impulse
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    let friction = (first.spec.material.friction * other_material.friction.max(0.0)).sqrt();
    let max_tangent = normal_impulse * friction;
    if tangent_length > max_tangent {
        let ratio = max_tangent / tangent_length;
        impulse = impulse.map(|value| value * ratio);
    }
    impulse[axis] = normal * normal_impulse;
    for (axis, impulse) in impulse.into_iter().enumerate() {
        first.velocity[axis] = (first.velocity[axis] + impulse * first.inverse_mass(axis))
            .clamp(-MAX_SPEED, MAX_SPEED);
        if let Some(obstacle) = other.as_deref_mut() {
            obstacle.velocity[axis] = (obstacle.velocity[axis]
                - impulse * obstacle.inverse_mass(axis))
            .clamp(-MAX_SPEED, MAX_SPEED);
        }
    }
}

fn supports(zone: Collider, shape: Collider) -> bool {
    let foot_gap = shape.min[1] - zone.max[1];
    foot_gap.abs() <= 0.025 && intersects_axis(zone, shape, 0) && intersects_axis(zone, shape, 2)
}

fn overlappers(
    zone: Collider,
    bodies: &[Body],
    actors: &[ActorContact],
) -> BTreeSet<ContactSource> {
    bodies
        .iter()
        .filter(|body| intersects(zone, body.bounds()))
        .map(|body| ContactSource::Body(body.spec.id))
        .chain(
            actors
                .iter()
                .filter(|actor| actor.id != 0 && intersects(zone, actor.bounds))
                .map(|actor| ContactSource::Actor(actor.id)),
        )
        .collect()
}

fn intersects(a: Collider, b: Collider) -> bool {
    (0..3).all(|axis| intersects_axis(a, b, axis))
}

fn intersects_axis(a: Collider, b: Collider, axis: usize) -> bool {
    a.min[axis] < b.max[axis] - CONTACT_SLOP && b.min[axis] < a.max[axis] - CONTACT_SLOP
}

fn validate_ids(ids: impl Iterator<Item = u64>) -> Result<(), LayoutError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if id == 0 {
            return Err(LayoutError::InvalidId);
        }
        if !seen.insert(id) {
            return Err(LayoutError::DuplicateId(id));
        }
    }
    Ok(())
}

fn valid_bounds(bounds: Collider) -> bool {
    (0..3).all(|axis| {
        (-10_000.0..=10_000.0).contains(&bounds.min[axis])
            && (-10_000.0..=10_000.0).contains(&bounds.max[axis])
            && bounds.max[axis] - bounds.min[axis] >= 0.001
    })
}

fn valid_body(spec: &BodySpec) -> bool {
    spec.center
        .iter()
        .all(|axis| (-10_000.0..=10_000.0).contains(axis))
        && spec
            .half_extents
            .iter()
            .all(|extent| (0.015..=32.0).contains(extent))
        && (0.0..=2.0).contains(&spec.material.friction)
        && (0.0..=1.0).contains(&spec.material.restitution)
        && match spec.kind {
            BodyKind::Dynamic { mass, joint } => {
                (0.01..=30_000.0).contains(&mass)
                    && joint.is_none_or(|joint| {
                        let [low, high] = joint.travel;
                        (-100.0..=0.0).contains(&low)
                            && (0.0..=100.0).contains(&high)
                            && high - low >= 0.01
                    })
            }
            BodyKind::Platform { travel, period, .. } => {
                (0.05..=100.0).contains(&travel.abs())
                    && (0.25..=600.0).contains(&period)
                    && 2.0 * travel.abs() / period <= 12.0
            }
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_at(center: [f32; 3], half: [f32; 3]) -> Collider {
        Collider {
            min: std::array::from_fn(|axis| center[axis] - half[axis]),
            max: std::array::from_fn(|axis| center[axis] + half[axis]),
        }
    }

    fn crate_at(id: u64, center: [f32; 3], half: f32, mass: f32) -> BodySpec {
        BodySpec {
            id,
            center,
            half_extents: [half; 3],
            material: Material::default(),
            kind: BodyKind::Dynamic { mass, joint: None },
        }
    }

    fn platform_at(id: u64, center: [f32; 3], axis: Axis) -> BodySpec {
        BodySpec {
            id,
            center,
            half_extents: [0.8, 0.12, 0.8],
            material: Material::default(),
            kind: BodyKind::Platform {
                axis,
                travel: 2.0,
                period: 4.0,
            },
        }
    }

    fn floor(top: f32) -> Solid {
        box_at([0.0, top - 0.2, 0.0], [50.0, 0.2, 50.0]).into()
    }

    #[test]
    fn fixed_steps_match_across_render_rates_and_drop_expensive_backlog() {
        let body = crate_at(1, [0.0, 1.2, 0.0], 0.2, 2.0);
        let platform = platform_at(2, [0.0, 0.0, 0.0], Axis::X);
        let mut slow = Simulation::new(&[body, platform], &[]).unwrap();
        let mut fast = Simulation::new(&[platform, body], &[]).unwrap();
        for _ in 0..60 {
            assert_eq!(slow.step(1.0 / 60.0, &[], &[]).substeps, 2);
        }
        for _ in 0..120 {
            assert_eq!(fast.step(1.0 / 120.0, &[], &[]).substeps, 1);
        }
        assert_eq!(
            slow.poses().collect::<Vec<_>>(),
            fast.poses().collect::<Vec<_>>()
        );
        let stall = slow.step(3.0, &[], &[]);
        assert_eq!(stall.substeps, MAX_STEPS);
        assert!(stall.discarded_seconds > 2.9);
        assert_eq!(slow.step(FIXED_STEP, &[], &[]).substeps, 1);
    }

    #[test]
    fn fast_box_cannot_tunnel_through_thin_solid() {
        let wall = box_at([0.42, 1.0, 0.0], [0.005, 0.85, 2.0]).into();
        let mut sim = Simulation::new(&[crate_at(1, [0.0, 1.0, 0.0], 0.05, 1.0)], &[]).unwrap();
        assert!(sim.push_impulse(1, [80.0, 0.0, 0.0]));
        let report = sim.step(1.0 / 30.0, &[floor(0.0), wall], &[]);
        let body = sim.body(1).unwrap();
        assert!(body.bounds.max[0] <= 0.415_01 && body.bounds.max[0] > 0.414);
        assert!(body.velocity[0].abs() < 0.0001);
        assert_eq!(report.saturated_substeps, 0);
    }

    #[test]
    fn unequal_masses_exchange_correct_momentum_at_an_elastic_impact() {
        let test_body = |id, x, mass| {
            let mut body = crate_at(id, [x, 1.0, 0.0], 0.1, mass);
            body.material.restitution = 1.0;
            body.kind = BodyKind::Dynamic {
                mass,
                joint: Some(AxisJoint {
                    axis: Axis::X,
                    travel: [-4.0, 4.0],
                }),
            };
            body
        };
        let lighter = test_body(1, -0.3, 1.0);
        let heavier = test_body(2, 0.3, 3.0);
        let mut sim = Simulation::new(&[lighter, heavier], &[]).unwrap();
        assert!(sim.push_impulse(1, [4.0, 0.0, 0.0]));
        for _ in 0..24 {
            assert_eq!(sim.step(FIXED_STEP, &[], &[]).saturated_substeps, 0);
        }
        let light = sim.body(1).unwrap();
        let heavy = sim.body(2).unwrap();
        assert!((light.velocity[0] + 2.0).abs() < 1.0e-5);
        assert!((heavy.velocity[0] - 2.0).abs() < 1.0e-5);
        assert!((light.velocity[0] + 3.0 * heavy.velocity[0] - 4.0).abs() < 1.0e-5);
    }

    #[test]
    fn floor_friction_stops_sliding_and_restitution_rebounds() {
        let sliding = crate_at(1, [0.0, 0.2, 0.0], 0.2, 4.0);
        let mut bouncing = crate_at(2, [0.0, 1.2, 2.0], 0.2, 4.0);
        bouncing.material.restitution = 0.8;
        let mut sim = Simulation::new(&[sliding, bouncing], &[]).unwrap();
        assert!(sim.push_impulse(1, [8.0, 0.0, 0.0]));
        let mut rebound: f32 = 0.0;
        for _ in 0..120 {
            sim.step(FIXED_STEP, &[floor(0.0)], &[]);
            rebound = rebound.max(sim.body(2).unwrap().velocity[1]);
        }
        let sliding = sim.body(1).unwrap();
        assert!(sliding.center[0] < 2.0);
        assert!(sliding.velocity[0].abs() < 1.0e-4);
        assert!(rebound > 3.4 && rebound < 3.9);
    }

    #[test]
    fn platform_carries_a_box_by_friction_and_height() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let platform = platform_at(1, [0.0, 0.0, 0.0], axis);
            let load = crate_at(2, [0.0, 0.32, 0.0], 0.2, 5.0);
            let mut sim = Simulation::new(&[platform, load], &[]).unwrap();
            for _ in 0..120 {
                assert_eq!(sim.step(FIXED_STEP, &[], &[]).saturated_substeps, 0);
            }
            let carrier = sim.body(1).unwrap();
            let load = sim.body(2).unwrap();
            assert!((carrier.center[axis as usize] - 1.0).abs() < 1.0e-4);
            assert!((carrier.bounds.max[1] - load.bounds.min[1]).abs() < 1.0e-4);
            assert!((carrier.center[0] - load.center[0]).abs() < 0.1);
            assert!((carrier.center[2] - load.center[2]).abs() < 0.1);
        }
    }

    #[test]
    fn plate_combines_weight_and_emits_only_changed_edges() {
        let body = crate_at(5, [0.0, 0.2, 0.0], 0.1, 4.0);
        let sensor = Sensor {
            id: 8,
            bounds: box_at([0.0, 0.05, 0.0], [0.5, 0.05, 0.4]),
            kind: SensorKind::Pressure { minimum_mass: 8.0 },
        };
        let mut sim = Simulation::new(&[body], &[sensor]).unwrap();
        let actor = ActorContact {
            id: 7,
            bounds: box_at([0.25, 0.6, 0.0], [0.1, 0.5, 0.1]),
            mass: 5.0,
            grounded: true,
        };
        let solids = [floor(0.1)];
        assert!(sim.step(FIXED_STEP, &solids, &[]).events.is_empty());
        assert!(
            sim.step(
                FIXED_STEP,
                &solids,
                &[ActorContact {
                    grounded: false,
                    ..actor
                }]
            )
            .events
            .is_empty()
        );
        let step = sim.step(FIXED_STEP, &solids, &[actor, actor]);
        assert_eq!(
            step.events,
            [TriggerEvent {
                tick: 3,
                sensor_id: 8,
                kind: Trigger::Pressed
            }]
        );
        assert_eq!(sim.plate_pressed(8), Some(true));
        assert!(sim.step(FIXED_STEP, &solids, &[actor]).events.is_empty());
        assert_eq!(
            sim.step(FIXED_STEP, &solids, &[]).events[0].kind,
            Trigger::Released
        );
        assert_eq!(sim.plate_pressed(8), Some(false));
    }

    #[test]
    fn trigger_records_a_crossing_between_two_simulation_samples() {
        let body = crate_at(1, [0.0, 1.0, 0.0], 0.02, 1.0);
        let trigger = Sensor {
            id: 2,
            bounds: box_at([0.3, 1.0, 0.0], [0.005, 0.4, 0.1]),
            kind: SensorKind::Overlap,
        };
        let mut sim = Simulation::new(&[body], &[trigger]).unwrap();
        assert!(sim.push_impulse(1, [80.0, 0.0, 0.0]));
        let step = sim.step(FIXED_STEP, &[], &[]);
        assert!(sim.body(1).unwrap().bounds.min[0] > trigger.bounds.max[0]);
        let kind = step
            .events
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kind,
            [
                Trigger::Entered(ContactSource::Body(1)),
                Trigger::Exited(ContactSource::Body(1))
            ]
        );
        assert!(sim.step(FIXED_STEP, &[], &[]).events.is_empty());
    }

    #[test]
    fn axis_joint_stays_on_rail_and_catches_limits_with_a_large_impulse() {
        let mut slider = crate_at(1, [0.0, 1.5, 2.2], 0.1, 1.0);
        slider.kind = BodyKind::Dynamic {
            mass: 1.0,
            joint: Some(AxisJoint {
                axis: Axis::X,
                travel: [-0.2, 0.35],
            }),
        };
        let mut sim = Simulation::new(&[slider], &[]).unwrap();
        assert!(sim.push_impulse(1, [100.0, 70.0, 70.0]));
        assert_eq!(sim.step(FIXED_STEP, &[], &[]).saturated_substeps, 0);
        assert_eq!(sim.body(1).unwrap().center, [0.35, 1.5, 2.2]);
        assert!(sim.push_impulse(1, [-100.0, -70.0, -70.0]));
        sim.step(FIXED_STEP, &[], &[]);
        assert_eq!(sim.body(1).unwrap().center, [-0.2, 1.5, 2.2]);
        assert_eq!(sim.body(1).unwrap().velocity, [0.0; 3]);
    }

    #[test]
    fn external_forces_are_applied_in_tick_order_before_collision() {
        let body = crate_at(1, [0.0, 1.0, 0.0], 0.1, 1.0);
        let wall = box_at([0.42, 1.0, 0.0], [0.005, 0.85, 2.0]).into();
        let mut sim = Simulation::new(&[body], &[]).unwrap();
        let mut calls = Vec::new();
        let report = sim.step_with(1.0 / 60.0, &[wall], &[], |id, pose, dt, secs| {
            calls.push((id, dt, secs));
            [80.0, pose.velocity[1] + GRAVITY * dt, 0.0]
        });
        assert_eq!(report.substeps, 2);
        assert_eq!(
            calls,
            [
                (1, FIXED_STEP as f32, FIXED_STEP),
                (1, FIXED_STEP as f32, 2.0 * FIXED_STEP)
            ]
        );
        assert_eq!(sim.body(1).unwrap().center[1], 1.0);
        assert!(sim.body(1).unwrap().bounds.max[0] <= 0.415_01);
    }

    #[test]
    fn duplicate_or_invalid_body_and_sensor_data_fail_before_simulation() {
        let first = crate_at(9, [0.0, 0.4, 0.0], 0.4, 3.0);
        assert!(matches!(
            Simulation::new(&[first, first], &[]),
            Err(LayoutError::DuplicateId(9))
        ));
        let broken = BodySpec {
            center: [f32::NAN, 0.0, 0.0],
            ..first
        };
        assert!(matches!(
            Simulation::new(&[broken], &[]),
            Err(LayoutError::InvalidBody(9))
        ));
        let trigger = Sensor {
            id: 10,
            bounds: box_at([0.0, 0.0, 0.0], [0.2, 0.2, 0.2]),
            kind: SensorKind::Pressure { minimum_mass: 0.0 },
        };
        assert!(matches!(
            Simulation::new(&[], &[trigger]),
            Err(LayoutError::InvalidSensor(10))
        ));
    }
}
