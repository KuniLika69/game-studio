use eframe::egui;
use glam::{Vec2, Vec3};

use crate::{
    Studio,
    body_physics::{
        ActorContact, BodyKind, BodyPose, GRAVITY, SensorKind, Simulation, Snapshot, Solid,
        TriggerEvent,
    },
    environment::BodyInput,
    physics_project::{NamedBody, PhysicsScene, bounds, center_half},
    scene::{PLAYER_HEIGHT, PLAYER_RADIUS, Part, PartMotion, Shape},
    world::Collider,
};

const PART_ID_PREFIX: u64 = 1 << 61;
const PLAYER_MASS: f32 = 70.0;
const PUSH_FORCE: f32 = 420.0;
const PUSH_SPEED: f32 = 2.15;

#[derive(Default)]
pub(crate) struct Runtime {
    simulation: Option<Simulation>,
    pending: Vec<TriggerEvent>,
    environment_clock_offset: f64,
}

impl Runtime {
    fn begin(&mut self, project: &PhysicsScene, saved: Option<&Snapshot>) -> Result<bool, String> {
        *self = Self::default();
        if project.is_empty() {
            return Ok(true);
        }
        let mut simulation = project.simulation()?;
        let restored = saved.is_none_or(|saved| simulation.restore(saved).is_ok());
        self.simulation = Some(simulation);
        Ok(restored)
    }

    pub(crate) fn take_events(&mut self) -> Vec<TriggerEvent> {
        std::mem::take(&mut self.pending)
    }

    pub(crate) fn plate_pressed(&self, id: u64) -> Option<bool> {
        self.simulation.as_ref()?.plate_pressed(id)
    }

    pub(crate) fn body(&self, id: u64) -> Option<BodyPose> {
        self.simulation.as_ref()?.body(id)
    }

    pub(crate) fn snapshot(&self) -> Option<Snapshot> {
        self.simulation.as_ref().map(Simulation::snapshot)
    }

    pub(crate) fn colliders(&self) -> impl Iterator<Item = Collider> + '_ {
        self.simulation
            .iter()
            .flat_map(|simulation| simulation.poses().map(|(_, pose)| pose.bounds))
    }

    fn advance(
        &mut self,
        project: &PhysicsScene,
        dt: f32,
        geometry: &[Collider],
        actor: ActorContact,
        motion: [f32; 2],
        carry: bool,
        mut adjust: impl FnMut(&NamedBody, BodyPose, f32, f64) -> [f32; 3],
    ) -> [f32; 3] {
        let Some(simulation) = &mut self.simulation else {
            return [0.0; 3];
        };
        let platform = if carry && actor.grounded {
            simulation.supporting_platform(actor.bounds)
        } else {
            None
        };
        let mut solids: Vec<_> = geometry.iter().copied().map(Solid::from).collect();
        if project.implicit_floor {
            solids.push(
                Collider {
                    min: [-512.0, -8.0, -512.0],
                    max: [512.0, 0.0, 512.0],
                }
                .into(),
            );
        }
        solids.push(actor.bounds.into());
        let intent = Vec2::from_array(motion).clamp_length_max(1.0);
        let report =
            simulation.step_with(f64::from(dt), &solids, &[actor], |id, pose, step, time| {
                let Some(body) = project.bodies.iter().find(|body| body.spec.id == id) else {
                    return pose.velocity;
                };
                let mut velocity = adjust(body, pose, step, time);
                if let BodyKind::Dynamic { mass, .. } = body.spec.kind
                    && let Some((axis, direction)) = push_contact(actor, intent, pose, geometry)
                {
                    let speed_in_direction = velocity[axis] * direction.signum();
                    if speed_in_direction < PUSH_SPEED {
                        let increase = (direction.abs() * PUSH_FORCE / mass * step)
                            .min(PUSH_SPEED - speed_in_direction);
                        velocity[axis] += increase * direction.signum();
                    }
                }
                velocity
            });
        self.pending.extend(report.events);
        platform
            .and_then(|(id, before)| {
                simulation.body(id).map(|after| {
                    std::array::from_fn(|axis| after.center[axis] - before.center[axis])
                })
            })
            .unwrap_or([0.0; 3])
    }

    fn render(&self, project: &PhysicsScene, running: bool) -> Vec<Part> {
        let mut output = Vec::new();
        for body in &project.bodies {
            let collider = self.body(body.spec.id).filter(|_| running).map_or_else(
                || bounds(body.spec.center, body.spec.half_extents),
                |pose| pose.bounds,
            );
            let color = match body.spec.kind {
                BodyKind::Platform { .. } => [0.36, 0.62, 0.72],
                BodyKind::Dynamic { .. } if body.buoyant => [0.93, 0.52, 0.16],
                BodyKind::Dynamic { .. } => [0.65, 0.37, 0.20],
            };
            output.push(proxy(body.spec.id, 0, &body.name, collider, color));
        }
        for sensor in &project.sensors {
            let id = sensor.spec.id;
            let name = &sensor.name;
            let collider = sensor.spec.bounds;
            match sensor.spec.kind {
                SensorKind::Pressure { .. } => {
                    let pressed = running && self.plate_pressed(id) == Some(true);
                    let color = if pressed {
                        [0.29, 0.86, 0.48]
                    } else {
                        [0.91, 0.65, 0.21]
                    };
                    let (mut center, mut half) = center_half(collider);
                    half[1] = if pressed { 0.010 } else { 0.022 };
                    center[1] = collider.max[1] + half[1];
                    output.push(proxy(id, 0, name, bounds(center, half), color));
                }
                SensorKind::Overlap if !running => {
                    let (center, half) = center_half(collider);
                    for (index, [x, z]) in [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]]
                        .into_iter()
                        .enumerate()
                    {
                        output.push(proxy(
                            id,
                            index as u64,
                            name,
                            bounds(
                                [center[0] + x * half[0], center[1], center[2] + z * half[2]],
                                [0.015, half[1], 0.015],
                            ),
                            [0.37, 0.75, 0.93],
                        ));
                    }
                }
                SensorKind::Overlap => {}
            }
        }
        output
    }
}

fn proxy(id: u64, piece: u64, name: &str, collider: Collider, color: [f32; 3]) -> Part {
    let (position, half) = center_half(collider);
    Part {
        id: part_id(id, piece),
        name: name.into(),
        shape: Shape::Box,
        position,
        rotation: [0.0; 3],
        size: half.map(|half| half * 2.0),
        color,
        motion: PartMotion::Static,
    }
}

fn part_id(id: u64, piece: u64) -> u64 {
    PART_ID_PREFIX | (id << 3) | piece
}

fn push_contact(
    actor: ActorContact,
    intent: Vec2,
    pose: BodyPose,
    geometry: &[Collider],
) -> Option<(usize, f32)> {
    let collider = pose.bounds;
    if !actor.grounded
        || !intent.is_finite()
        || actor.bounds.min[1] >= collider.max[1] - 0.008
        || actor.bounds.max[1] <= collider.min[1] + 0.008
    {
        return None;
    }
    let mut axis = [0, 2];
    if intent.y.abs() > intent.x.abs() {
        axis.swap(0, 1);
    }
    let selected = axis.into_iter().find_map(|axis| {
        let direction = intent[axis / 2];
        let cross = 2 - axis;
        if direction.abs() < 0.05
            || actor.bounds.min[cross] >= collider.max[cross] - 0.015
            || actor.bounds.max[cross] <= collider.min[cross] + 0.015
        {
            return None;
        }
        let gap = if direction > 0.0 {
            collider.min[axis] - actor.bounds.max[axis]
        } else {
            actor.bounds.min[axis] - collider.max[axis]
        };
        (-0.035..=0.08).contains(&gap).then_some((axis, direction))
    })?;
    let (axis, direction) = selected;
    let cross = 2 - axis;
    let point = (actor.bounds.min[cross].max(collider.min[cross])
        + actor.bounds.max[cross].min(collider.max[cross]))
        * 0.5;
    let mid_y =
        (actor.bounds.min[1].max(collider.min[1]) + actor.bounds.max[1].min(collider.max[1])) * 0.5;
    let mut from = [0.0, mid_y, 0.0];
    from[cross] = point;
    from[axis] = if direction > 0.0 {
        actor.bounds.max[axis]
    } else {
        actor.bounds.min[axis]
    };
    let mut to = from;
    to[axis] = if direction > 0.0 {
        collider.min[axis]
    } else {
        collider.max[axis]
    };
    geometry
        .iter()
        .all(|solid| !solid.intersects_segment(from, to))
        .then_some(selected)
}

impl Studio {
    pub(super) fn start_body_physics(&mut self, resume: bool) {
        let saved = resume
            .then_some(self.scene.checkpoint.as_ref())
            .flatten()
            .and_then(|checkpoint| checkpoint.physics.as_ref());
        match self.physics_runtime.begin(&self.scene.physics, saved) {
            Ok(true) => {}
            Ok(false) => {
                self.status = "Физические объекты изменились. Их состояние начато заново.".into()
            }
            Err(error) => self.status = error,
        }
        if let (Some(environment), Some(simulation)) = (
            self.environment_session.as_ref(),
            self.physics_runtime.simulation.as_ref(),
        ) {
            self.physics_runtime.environment_clock_offset =
                environment.elapsed() - simulation.elapsed_seconds();
        }
    }

    pub(super) fn advance_body_physics(&mut self, dt: f32, motion: [f32; 2], jump: bool) {
        if self.physics_runtime.simulation.is_none() {
            return;
        }
        let [x, y, z] = self.actor_position().to_array();
        let grounded = self.game.grounded && !jump;
        let actor = ActorContact {
            id: 1,
            bounds: Collider {
                min: [x - PLAYER_RADIUS, y, z - PLAYER_RADIUS],
                max: [x + PLAYER_RADIUS, y + PLAYER_HEIGHT, z + PLAYER_RADIUS],
            },
            mass: PLAYER_MASS,
            grounded,
        };
        let active = self.seating.is_none() && !self.story_session.is_open();
        let motion = if active { motion } else { [0.0; 2] };
        let blockers = self.static_colliders();
        let climate = self
            .scene
            .environment
            .as_ref()
            .zip(self.environment_session.as_ref());
        let clock_offset = self.physics_runtime.environment_clock_offset;
        let before: Vec<_> = self
            .scene
            .physics
            .bodies
            .iter()
            .filter(|body| climate.is_some() && body.buoyant)
            .filter_map(|body| {
                self.physics_runtime
                    .body(body.spec.id)
                    .map(|pose| (body.spec.id, pose))
            })
            .collect();
        let travel = self.physics_runtime.advance(
            &self.scene.physics,
            dt,
            &blockers,
            actor,
            motion,
            active,
            |body, pose, step, time| {
                climate.map_or(pose.velocity, |(config, session)| {
                    session
                        .body_velocity(
                            config,
                            BodyInput {
                                center: Vec3::from_array(pose.center),
                                height: pose.bounds.max[1] - pose.bounds.min[1],
                                velocity: Vec3::from_array(pose.velocity),
                                wind_drag: body.wind_drag,
                                buoyant: body.buoyant,
                            },
                            GRAVITY,
                            step,
                            (time + clock_offset).max(0.0),
                        )
                        .to_array()
                })
            },
        );
        let splashes: Vec<_> = before
            .into_iter()
            .filter_map(|(id, before)| {
                let (config, session) = climate?;
                let after = self.physics_runtime.body(id)?;
                session.splash_between(
                    config,
                    Vec3::from_array(before.center),
                    Vec3::from_array(after.center),
                    after.bounds.max[1] - after.bounds.min[1],
                    dt,
                )
            })
            .collect();
        for splash in splashes {
            self.add_environment_splash(splash);
        }
        if travel != [0.0; 3] {
            self.game.carry_motion(travel, &blockers);
        }
    }

    pub(super) fn physics_parts(&self) -> Vec<Part> {
        self.physics_runtime
            .render(&self.scene.physics, self.game_running)
    }

    pub(super) fn selected_physics_proxy(&self) -> Option<u64> {
        self.physics_editor
            .selected()
            .map(|selection| part_id(selection.id(), 0))
    }

    pub(super) fn physics_placement_height(&self) -> f32 {
        self.physics_editor
            .selected()
            .and_then(|selection| self.scene.physics.height_at(selection))
            .unwrap_or(0.0)
    }

    pub(super) fn place_physics_at(&mut self, x: f32, z: f32) {
        let height = self.physics_placement_height();
        if self
            .physics_editor
            .place(&mut self.scene.physics, [x, height, z])
        {
            self.invalidate_physics_checkpoint();
        }
    }

    pub(super) fn physics_editor_tools(&mut self, ui: &mut egui::Ui) {
        if self.scene.map_2d.is_some() {
            return;
        }
        let [x, _, z] = self.camera.target;
        let source = self.world_selection.and_then(|id| {
            self.scene
                .world
                .objects
                .iter()
                .find(|entry| entry.id == id)
                .map(|entry| entry.editor_bounds())
                .or_else(|| {
                    let entry = self
                        .scene
                        .world
                        .models
                        .iter()
                        .find(|entry| entry.id == id)?;
                    entry.custom_collider().or_else(|| {
                        self.assets
                            .get(&entry.source)
                            .map(|loaded| entry.bounds(&loaded.model))
                    })
                })
        });
        ui.collapsing("Физика", |ui| {
            ui.add_enabled_ui(!self.game_running, |ui| {
                if self
                    .physics_editor
                    .show(ui, &mut self.scene.physics, [x, 0.0, z - 1.8], source)
                {
                    self.invalidate_physics_checkpoint();
                }
            });
        });
    }

    fn invalidate_physics_checkpoint(&mut self) {
        if let Some(checkpoint) = &mut self.scene.checkpoint {
            checkpoint.physics = None;
        }
        self.changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{physics_project::Addition, scene::Scene};

    #[test]
    fn walking_pushes_a_box_until_its_plate_fires() {
        let demo = PhysicsScene::mill_demo();
        let mut studio = Studio::default();
        studio.scene = Scene::empty();
        studio.scene.physics = demo.scene;
        studio.scene.physics.implicit_floor = true;
        studio.game_running = true;
        studio.game.x = -2.02;
        studio.start_body_physics(false);
        let mut pressed = false;
        for _ in 0..150 {
            let dt = 1.0 / 120.0;
            studio.advance_body_physics(dt, [1.0, 0.0], false);
            let colliders = studio.colliders();
            studio.game.update(dt, [1.0, 0.0], false, false, &colliders);
            pressed |= studio.physics_runtime.take_events().iter().any(|event| {
                event.sensor_id == demo.plate && event.kind == crate::body_physics::Trigger::Pressed
            });
            if pressed {
                break;
            }
        }
        assert!(pressed);
        assert!(studio.physics_runtime.body(demo.crate_body).unwrap().center[0] > -0.78);
        assert_eq!(studio.physics_runtime.plate_pressed(demo.plate), Some(true));
    }

    #[test]
    fn moving_box_remains_on_its_side_of_the_player() {
        let mut scene = PhysicsScene::default();
        let body = scene.add(Addition::Crate, [-1.2, 0.0, 0.0]).unwrap().id();
        let mut runtime = Runtime::default();
        runtime.begin(&scene, None).unwrap();
        let actor = ActorContact {
            id: 1,
            bounds: Collider {
                min: [-PLAYER_RADIUS, 0.0, -PLAYER_RADIUS],
                max: [PLAYER_RADIUS, PLAYER_HEIGHT, PLAYER_RADIUS],
            },
            mass: PLAYER_MASS,
            grounded: true,
        };
        for _ in 0..12 {
            runtime.advance(
                &scene,
                1.0 / 120.0,
                &[],
                actor,
                [0.0; 2],
                true,
                |_, pose, _, _| [15.0, pose.velocity[1], 0.0],
            );
        }
        let box_pose = runtime.body(body).unwrap();
        assert!((box_pose.bounds.max[0] - actor.bounds.min[0]).abs() < 0.0001);
        assert!(box_pose.velocity[0].abs() < 0.0001);
    }

    #[test]
    fn lift_carries_real_game_controller_and_resume_preserves_its_height() {
        let mut studio = Studio::default();
        studio.scene = Scene::empty();
        let lift = studio
            .scene
            .physics
            .add(Addition::Platform, [0.0; 3])
            .unwrap()
            .id();
        studio.start_body_physics(false);
        studio.game_running = true;
        studio.game.height = 0.25;
        for _ in 0..90 {
            let dt = 1.0 / 120.0;
            studio.advance_body_physics(dt, [0.0; 2], false);
            let colliders = studio.colliders();
            studio.game.update(dt, [0.0; 2], false, false, &colliders);
        }
        let top = studio.physics_runtime.body(lift).unwrap().bounds.max[1];
        assert!(top > 0.6);
        assert!((studio.game.height - top).abs() < 0.0001 && studio.game.grounded);
        studio.remember_checkpoint();
        let checkpoint = studio.scene.checkpoint.clone().unwrap();
        studio.physics_runtime = Runtime::default();
        studio.start_body_physics(true);
        assert_eq!(studio.physics_runtime.snapshot(), checkpoint.physics);
    }

    #[test]
    fn native_three_d_viewport_places_a_physical_plate() {
        use crate::proof_input::{Driver, click, find_label};
        use crate::{EditorTab, View, proof};

        let mut studio = Studio::default();
        studio.scene = Scene::empty();
        studio.view = View::Three;
        studio.editor_tab = EditorTab::World;
        studio.proof = Some(proof::Environment {
            scene: None,
            export: None,
            map_png: None,
            import_glb: None,
            effect_file: None,
            wav_file: None,
            character_file: None,
            sprite_canvas: None,
            projection: None,
        });
        let ctx = egui::Context::default();
        let mut driver = Driver::new([1280, 840]);
        let frame = |app: &mut Studio, driver: &mut Driver, events| {
            ctx.run(driver.raw(events), |ctx| app.run_frame(ctx))
        };
        let mut output = frame(&mut studio, &mut driver, Vec::new());
        for label in ["Физика", "+ Нажимная плита", "Переместить на плане"]
        {
            let point = find_label(&output, label, 0)
                .unwrap_or_else(|| panic!("missing native studio control {label}"));
            driver.skip_frames(45);
            frame(&mut studio, &mut driver, click(point));
            output = frame(&mut studio, &mut driver, Vec::new());
        }
        assert!(studio.physics_editor.placing());
        let Some(proof::Projection::World(camera, rect)) = studio
            .proof
            .as_ref()
            .and_then(|proof| proof.projection.as_ref())
        else {
            panic!("No native 3D projection");
        };
        let target = [0.85, 0.0, -0.6];
        let point = camera.screen_point(*rect, target).unwrap();
        driver.skip_frames(45);
        frame(&mut studio, &mut driver, click(point));
        assert!(!studio.physics_editor.placing());
        let sensor = studio.scene.physics.sensors[0].spec;
        let (center, _) = center_half(sensor.bounds);
        assert!((center[0] - target[0]).abs() < 0.002);
        assert!((center[2] - target[2]).abs() < 0.002);
        assert_eq!(sensor.bounds.max[1], 0.0);
        assert_eq!(
            studio.physics_parts()[0].name,
            studio.scene.physics.sensors[0].name
        );
    }
}
