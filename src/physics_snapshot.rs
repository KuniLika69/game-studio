use super::*;

const MAX_PHYSICS_TICKS: u64 = 10_000_000_000;
const MAX_SENSOR_SOURCES: usize = MAX_BODIES + 128;
const BOUNDS_ERROR: f32 = 0.002;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    tick: u64,
    remainder: f64,
    bodies: Vec<BodyState>,
    sensors: Vec<SensorState>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct BodyState {
    id: u64,
    center: [f32; 3],
    velocity: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SensorState {
    id: u64,
    pressed: bool,
    overlaps: Vec<ContactSource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreError {
    InvalidData,
    DifferentLayout,
    InvalidBodyPose(u64),
    InvalidSensorState(u64),
}

impl Snapshot {
    pub fn validate(&self) -> Result<(), RestoreError> {
        if self.tick > MAX_PHYSICS_TICKS
            || !(0.0..FIXED_STEP).contains(&self.remainder)
            || self.bodies.len() > MAX_BODIES
            || self.sensors.len() > MAX_SENSORS
            || validate_ids(self.bodies.iter().map(|body| body.id)).is_err()
            || validate_ids(self.sensors.iter().map(|sensor| sensor.id)).is_err()
        {
            return Err(RestoreError::InvalidData);
        }
        for body in &self.bodies {
            if body
                .center
                .iter()
                .any(|axis| !(-1.0e6..=1.0e6).contains(axis))
                || body
                    .velocity
                    .iter()
                    .any(|axis| !(-MAX_SPEED..=MAX_SPEED).contains(axis))
            {
                return Err(RestoreError::InvalidBodyPose(body.id));
            }
        }
        let body_ids = self
            .bodies
            .iter()
            .map(|body| body.id)
            .collect::<BTreeSet<_>>();
        for sensor in &self.sensors {
            let mut contacts = BTreeSet::new();
            if sensor.overlaps.len() > MAX_SENSOR_SOURCES
                || sensor.overlaps.iter().any(|&contact| {
                    !contacts.insert(contact)
                        || match contact {
                            ContactSource::Body(id) => !body_ids.contains(&id),
                            ContactSource::Actor(id) => id == 0,
                        }
                })
            {
                return Err(RestoreError::InvalidSensorState(sensor.id));
            }
        }
        Ok(())
    }
}

impl Simulation {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.clock,
            remainder: self.accumulator,
            bodies: self
                .bodies
                .iter()
                .map(|body| BodyState {
                    id: body.spec.id,
                    center: body.center,
                    velocity: body.velocity,
                })
                .collect(),
            sensors: self
                .sensors
                .iter()
                .map(|sensor| SensorState {
                    id: sensor.spec.id,
                    pressed: sensor.pressed,
                    overlaps: sensor.overlapping.iter().copied().collect(),
                })
                .collect(),
        }
    }

    /// Confirm the checkpoint is compatible before mutating any live state.
    pub fn restore(&mut self, snapshot: &Snapshot) -> Result<(), RestoreError> {
        snapshot.validate()?;
        if self.bodies.len() != snapshot.bodies.len()
            || self.sensors.len() != snapshot.sensors.len()
        {
            return Err(RestoreError::DifferentLayout);
        }
        let bodies = snapshot
            .bodies
            .iter()
            .map(|state| (state.id, state))
            .collect::<BTreeMap<_, _>>();
        let sensors = snapshot
            .sensors
            .iter()
            .map(|state| (state.id, state))
            .collect::<BTreeMap<_, _>>();

        for body in &self.bodies {
            let saved = bodies
                .get(&body.spec.id)
                .ok_or(RestoreError::DifferentLayout)?;
            if !compatible_pose(body, saved) {
                return Err(RestoreError::InvalidBodyPose(body.spec.id));
            }
        }
        for sensor in &self.sensors {
            let saved = sensors
                .get(&sensor.spec.id)
                .ok_or(RestoreError::DifferentLayout)?;
            let compatible = match sensor.spec.kind {
                SensorKind::Overlap => !saved.pressed,
                SensorKind::Pressure { .. } => saved.overlaps.is_empty(),
            };
            if !compatible {
                return Err(RestoreError::InvalidSensorState(sensor.spec.id));
            }
        }

        self.clock = snapshot.tick;
        self.accumulator = snapshot.remainder;
        for body in &mut self.bodies {
            let saved = bodies[&body.spec.id];
            body.center = saved.center;
            body.velocity = saved.velocity;
        }
        for sensor in &mut self.sensors {
            let saved = sensors[&sensor.spec.id];
            sensor.pressed = saved.pressed;
            sensor.overlapping = saved.overlaps.iter().copied().collect();
        }
        Ok(())
    }
}

fn compatible_pose(body: &Body, saved: &BodyState) -> bool {
    let restriction = match body.spec.kind {
        BodyKind::Dynamic { joint: None, .. } => return true,
        BodyKind::Dynamic { joint: Some(_), .. } => body.joint_bounds(),
        BodyKind::Platform { axis, travel, .. } => {
            let axis = axis as usize;
            let origin = body.spec.center[axis];
            Some((axis, [origin + travel.min(0.0), origin + travel.max(0.0)]))
        }
    };
    let Some((allowed, [lower, upper])) = restriction else {
        return false;
    };
    (lower - BOUNDS_ERROR..=upper + BOUNDS_ERROR).contains(&saved.center[allowed])
        && (0..3).filter(|&axis| axis != allowed).all(|axis| {
            (saved.center[axis] - body.spec.center[axis]).abs() <= BOUNDS_ERROR
                && saved.velocity[axis].abs() <= BOUNDS_ERROR
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_project::PhysicsScene;

    #[test]
    fn saved_physics_preserves_mechanical_phase_and_contact_edges() {
        let demo = PhysicsScene::mill_demo();
        let mut original = demo.scene.simulation().unwrap();
        let solids = [demo.floor];
        let grounded = ActorContact {
            id: 8,
            bounds: Collider {
                min: [-0.2, 0.0, -0.2],
                max: [0.2, 1.8, 0.2],
            },
            mass: 65.0,
            grounded: true,
        };
        for _ in 0..90 {
            original.step(FIXED_STEP, &solids, &[grounded]);
        }
        assert!(
            original
                .step(FIXED_STEP / 2.0, &solids, &[grounded])
                .events
                .is_empty()
        );
        assert_eq!(original.plate_pressed(demo.plate), Some(true));
        let bytes = serde_json::to_vec(&original.snapshot()).unwrap();
        let saved: Snapshot = serde_json::from_slice(&bytes).unwrap();

        let mut restored = demo.scene.simulation().unwrap();
        restored.restore(&saved).unwrap();
        assert_eq!(restored.snapshot(), original.snapshot());
        let uninterrupted = original.step(FIXED_STEP / 2.0, &solids, &[]);
        let resumed = restored.step(FIXED_STEP / 2.0, &solids, &[]);
        assert_eq!(uninterrupted, resumed);
        assert_eq!(restored.snapshot(), original.snapshot());
        assert!(
            resumed
                .events
                .iter()
                .any(|event| event.sensor_id == demo.plate && event.kind == Trigger::Released)
        );
    }

    #[test]
    fn invalid_or_foreign_checkpoint_never_overwrites_live_state() {
        let demo = PhysicsScene::mill_demo();
        let mut live = demo.scene.simulation().unwrap();
        let untouched = live.snapshot();
        let mut wrong = untouched.clone();
        wrong.bodies[0].velocity[0] = f32::INFINITY;
        assert!(matches!(
            live.restore(&wrong),
            Err(RestoreError::InvalidBodyPose(_))
        ));
        let mut wrong = untouched.clone();
        wrong.bodies[0].id += 400;
        assert_eq!(live.restore(&wrong), Err(RestoreError::DifferentLayout));
        let mut wrong = untouched.clone();
        let platform = wrong
            .bodies
            .iter_mut()
            .find(|state| state.id == demo.platform)
            .unwrap();
        platform.center[0] += 2.0;
        assert!(matches!(
            live.restore(&wrong),
            Err(RestoreError::InvalidBodyPose(_))
        ));
        assert_eq!(live.snapshot(), untouched);
    }
}
