use super::*;
use crate::environment::{self, Event, Frame, Session, Splash};

const MAX_SPLASHES: usize = 32;
const SPLASH_SECONDS: f64 = 0.8;

pub(super) struct TimedSplash {
    splash: Splash,
    started: f64,
}

impl Studio {
    pub(super) fn environment_properties(&mut self, ctx: &egui::Context) -> bool {
        if self.editor_tab != EditorTab::Environment {
            return false;
        }
        let anchor = self.scene.map_2d.as_ref().map_or_else(
            || match self.view {
                View::Two if self.overhead => [self.plan_offset.x, 0.0, self.plan_offset.y],
                _ => [self.camera.target[0], 0.0, self.camera.target[2]],
            },
            |map| map.to_world(map.spawn),
        );
        egui::SidePanel::right("environment_properties")
            .default_width(305.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let edited = ui
                        .add_enabled_ui(!self.game_running, |ui| {
                            self.environment_editor
                                .show(ui, &mut self.scene.environment, anchor)
                                | self.rotor_inspector(ui)
                        })
                        .inner;
                    if edited {
                        self.changed();
                    }
                    ui.separator();
                    ui.weak(&self.status);
                });
            });
        true
    }

    pub(super) fn environment_start(&mut self, resume: bool) {
        self.environment_stop();
        let Some(config) = &self.scene.environment else {
            return;
        };
        let saved = resume
            .then_some(self.scene.checkpoint.as_ref())
            .flatten()
            .and_then(|checkpoint| checkpoint.environment.clone());
        self.environment_session = Some(match saved {
            Some(saved) => match Session::restore(config, saved) {
                Ok(session) => session,
                Err(error) => {
                    let session = Session::new(config);
                    let error = match &self.project {
                        Some(path) => error.with_file(path),
                        None => error,
                    };
                    self.record_import_problem("Погода и вода сохранения", error);
                    session
                }
            },
            None => Session::new(config),
        });
    }

    pub(super) fn environment_stop(&mut self) {
        self.environment_session = None;
        self.environment_splashes.clear();
        self.snow_tracks = crate::snow_tracks::Tracks::default();
    }

    pub(super) fn advance_environment(&mut self, dt: f32) -> Vec<Event> {
        let (Some(config), Some(session)) =
            (&self.scene.environment, &mut self.environment_session)
        else {
            return Vec::new();
        };
        session.tick(config, f64::from(dt));
        let seconds = session.elapsed();
        self.environment_splashes
            .retain(|entry| seconds - entry.started < SPLASH_SECONDS);
        session.take_events()
    }

    pub(super) fn environment_frame(&self, focus: Vec3) -> Option<Frame> {
        let config = self.scene.environment.as_ref()?;
        if let Some(session) = self
            .environment_session
            .as_ref()
            .filter(|_| self.game_running)
        {
            let elapsed = session.elapsed();
            Some(Frame {
                atmosphere: session.sample(config),
                water: session.basins(config).collect(),
                elapsed,
                wind: session.wind_at(config, focus),
                splashes: self
                    .environment_splashes
                    .iter()
                    .map(|entry| (entry.splash, (elapsed - entry.started) as f32))
                    .collect(),
            })
        } else {
            let elapsed = self.environment_editor.preview_seconds();
            Some(Frame {
                atmosphere: config.sample(elapsed),
                water: config.basins().collect(),
                elapsed,
                wind: config.wind_at(focus, elapsed),
                splashes: Vec::new(),
            })
        }
    }

    pub(super) fn environment_wind(&self, position: Vec3) -> Vec3 {
        let Some(config) = &self.scene.environment else {
            return Vec3::ZERO;
        };
        if let Some(session) = self
            .environment_session
            .as_ref()
            .filter(|_| self.game_running)
        {
            session.wind_at(config, position)
        } else {
            config.wind_at(position, self.environment_editor.preview_seconds())
        }
    }

    pub(super) fn add_environment_splash(&mut self, splash: Splash) {
        let Some(session) = &self.environment_session else {
            return;
        };
        if self.environment_splashes.len() >= MAX_SPLASHES {
            self.environment_splashes.remove(0);
        }
        self.environment_splashes.push(TimedSplash {
            splash,
            started: session.elapsed(),
        });
    }

    pub(super) fn harvest_wet_footsteps(&mut self, plants: [Option<Vec3>; 2]) {
        self.harvest_snow_footsteps(plants);
        let (Some(config), Some(session)) = (&self.scene.environment, &self.environment_session)
        else {
            return;
        };
        if !self.game_running
            || self.free_camera
            || self.story_session.is_open()
            || !self.game.grounded
            || !matches!(self.avatar_motion, Motion::Walk | Motion::Run)
        {
            return;
        }
        let splashes = plants
            .into_iter()
            .flatten()
            .filter_map(|foot| {
                let water = session.surfaces(config).find(|water| {
                    let depth = water.depth_at(glam::Vec2::new(foot.x, foot.z));
                    if !(0.008..=0.18).contains(&depth) || (foot.y - water.height).abs() > 0.115 {
                        return false;
                    }
                    if water.shore.is_some() {
                        return water.contains_xz(foot);
                    }
                    let local = (foot.x - water.center[0]) / (water.size[0] * 0.5);
                    let forward = (foot.z - water.center[1]) / (water.size[1] * 0.5);
                    local * local + forward * forward <= 0.83
                })?;
                let strength = 0.18 + 0.13 * self.game.horizontal_speed().clamp(0.0, 3.6);
                Some(Splash {
                    zone: water.id,
                    position: Vec3::new(foot.x, water.height, foot.z),
                    strength,
                })
            })
            .collect::<Vec<_>>();
        for splash in splashes {
            self.add_environment_splash(splash);
        }
    }

    fn harvest_snow_footsteps(&mut self, plants: [Option<Vec3>; 2]) {
        let (Some(config), Some(session)) = (&self.scene.environment, &self.environment_session)
        else {
            return;
        };
        if !self.game_running
            || self.view != View::Three
            || self.free_camera
            || self.story_session.is_open()
        {
            return;
        }
        let snow = session.sample(config).surface_snow;
        if snow < 0.20 {
            return;
        }
        let now = session.elapsed();
        let moving = |motion| matches!(motion, Motion::Walk | Motion::Run);
        let above_water = |point: Vec3| {
            !session
                .surfaces(config)
                .any(|water| water.height > point.y + 0.011 && water.contains_xz(point))
        };
        if let Some(avatar) = &self.scene.avatar {
            let height = avatar.height;
            if self.game.grounded && moving(self.avatar_motion) {
                if avatar
                    .generated
                    .is_some_and(|recipe| recipe.kind == creature_mesh::Kind::Quadruped)
                {
                    let paws = self.quadruped_contact_report(0);
                    self.snow_tracks.update_paws(
                        0,
                        paws,
                        self.avatar_heading,
                        height,
                        now,
                        snow,
                        above_water,
                    );
                } else if let Some(feet) = self.rigid_snow_boots() {
                    self.snow_tracks.update_rigid_boots(
                        feet,
                        self.avatar_heading,
                        height,
                        now,
                        snow,
                        above_water,
                    );
                } else {
                    for point in plants.into_iter().flatten() {
                        let ground = self.route_surface_height(
                            glam::Vec2::new(point.x, point.z),
                            self.game.height,
                        );
                        let point = Vec3::new(point.x, ground, point.z);
                        if above_water(point) {
                            self.snow_tracks.push_boot(
                                point,
                                self.avatar_heading,
                                height,
                                now,
                                snow,
                            );
                        }
                    }
                }
            } else {
                self.snow_tracks.remove_actor(0);
            }
        }
        for npc in &self.scene.npcs {
            if !npc
                .avatar
                .generated
                .is_some_and(|recipe| recipe.kind == creature_mesh::Kind::Quadruped)
            {
                continue;
            }
            if let Some(state) = self
                .npc_simulation
                .state(npc.id)
                .filter(|state| state.grounded() && moving(state.motion))
            {
                let paws = self.quadruped_contact_report(npc.id);
                self.snow_tracks.update_paws(
                    npc.id,
                    paws,
                    state.yaw,
                    npc.avatar.height,
                    now,
                    snow,
                    above_water,
                );
            } else {
                self.snow_tracks.remove_actor(npc.id);
            }
        }
    }

    fn rigid_snow_boots(&self) -> Option<[(Vec3, f32, f32); 2]> {
        let (model, _) = self.avatar_asset()?;
        if !model.has_named_material("Yuna | Hood") {
            return None;
        }
        let draw = self.avatar_draw_without_foot()?;
        let [x, y, z] = draw.rotation.map(f32::to_radians);
        let model_to_world = glam::Mat4::from_scale_rotation_translation(
            Vec3::from_array(draw.scale),
            Quat::from_euler(glam::EulerRot::XYZ, x, y, z),
            Vec3::from_array(draw.position),
        );
        let contacts = model.rigid_boot_contacts(
            draw.clip,
            draw.elapsed,
            draw.blend
                .map(|blend| (blend.clip, blend.elapsed, blend.weight)),
            &draw.bone_offsets,
        )?;
        Some(contacts.map(|mark| {
            let model_point = model_to_world.transform_point3(mark);
            let floor = self.route_surface_height(
                glam::Vec2::new(model_point.x, model_point.z),
                self.game.height,
            );
            (
                Vec3::new(model_point.x, floor, model_point.z),
                model_point.y - floor,
                model_point.y - self.game.height,
            )
        }))
    }

    pub(super) fn append_environment_splashes(&self, parts: &mut Vec<Part>) {
        let Some(session) = &self.environment_session else {
            return;
        };
        let count = if self.low_fx { 5 } else { 12 };
        for entry in &self.environment_splashes {
            let age = (session.elapsed() - entry.started) as f32;
            if !(0.0..SPLASH_SECONDS as f32).contains(&age) {
                continue;
            }
            let strength = entry.splash.strength;
            let air = self.environment_wind(entry.splash.position) * (0.09 * age * age);
            for index in 0..count {
                let variation = (index % 5) as f32 * 0.11;
                let lift = age * (1.5 + 2.0 * strength + variation) - age * age * 4.9;
                if lift <= 0.0 {
                    continue;
                }
                let (sin, cos) = (index as f32 * 2.4).sin_cos();
                let spread = age * (0.45 + strength * 0.8 + variation);
                let position =
                    entry.splash.position + Vec3::new(cos * spread, lift, sin * spread) + air;
                let size = 0.024 + strength * 0.022;
                parts.push(Part {
                    id: 0,
                    name: String::new(),
                    shape: Shape::Sphere,
                    position: position.to_array(),
                    rotation: [0.0; 3],
                    size: [size, size * 1.3, size],
                    color: [0.36, 0.75, 0.86],
                    motion: scene::PartMotion::Static,
                });
            }
        }
    }

    pub(super) fn environment_snapshot(&self) -> Option<environment::Snapshot> {
        self.environment_session.as_ref().map(Session::snapshot)
    }
}
