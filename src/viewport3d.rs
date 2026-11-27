use std::{collections::HashMap, ops::Range, sync::Arc};

use bytemuck::{Pod, Zeroable};
use eframe::{egui, egui_wgpu};
use glam::{EulerRot, Mat4, Quat, Vec3};
use wgpu::util::DeviceExt;
use studio_core::render_settings::{BlendMode, PostProcess, ShadowQuality};

#[path = "hdr_post.rs"]
mod hdr_post;
use hdr_post::{HDR, PostRenderer};
#[path = "shadow_cascades.rs"]
mod shadow_cascades;
#[path = "transparent.rs"]
mod transparent;
pub(crate) use transparent::{SortSample, TransparencyProbe, TransparentLayer, transparent_blend};
use transparent::{LayerDepth, sort_layers};

use crate::assets::{
    Placement,
    materials::{Lighting, Tuning},
};
use crate::buildings::{self, Surface};
use crate::effects::{self, Particle};
use crate::environment::{Atmosphere, MAX_ZONES, WaterSurface};
use crate::material_graph::{Compiled, PreviewLight};
use crate::model::{
    BoneRotation, BoneTranslation, Material, Model, MorphWeight, SecondaryPose,
    Vertex as ModelVertex,
};
use crate::plan2d::{key_marker_id, marker_key_id};
use crate::scene::{Part, PartMotion, Shape};
use crate::world::{InteractionState, World};

const MESH_COUNT: usize = 4;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;
const SHADOW_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADOW_SIZE: u32 = 2048;
const LOW_SHADOW_SIZE: u32 = 1024;
const KEY_DIRECTION: Vec3 = Vec3::new(-0.53, 0.74, 0.43);
const INTERIOR_LIGHTS: usize = 6;
const INTERIOR_ROOMS: usize = 4;
const CONTACT_SHADOWS: usize = 8;
pub const VIEWPORT_MSAA: u16 = 4;

pub fn shadow_size(samples: u16) -> u32 {
    if samples == 1 {
        LOW_SHADOW_SIZE
    } else {
        SHADOW_SIZE
    }
}

#[derive(Clone, Copy)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: [f32; 3],
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: 0.65,
            pitch: 0.26,
            distance: 5.1,
            target: [0.0, 1.1, 0.0],
        }
    }
}

impl Camera {
    pub(crate) fn projection(self, rect: egui::Rect) -> (Vec3, Mat4) {
        let target = Vec3::from_array(self.target);
        let pitch = self.pitch.clamp(-1.34, 1.34);
        let mut eye = target
            + Vec3::new(
                self.yaw.sin() * pitch.cos(),
                pitch.sin(),
                self.yaw.cos() * pitch.cos(),
            ) * self.distance.max(0.65);
        eye.y = eye.y.max(0.12);
        let projection = Mat4::perspective_rh(
            40.0_f32.to_radians(),
            (rect.width() / rect.height()).max(0.01),
            0.04,
            160.0,
        );
        (eye, projection * Mat4::look_at_rh(eye, target, Vec3::Y))
    }

    pub fn ground_point(
        self,
        rect: egui::Rect,
        point: egui::Pos2,
        height: f32,
    ) -> Option<[f32; 2]> {
        if !rect.is_positive() || !rect.contains(point) || !height.is_finite() {
            return None;
        }
        let (_, projection) = self.projection(rect);
        let inverse = projection.inverse();
        let x = 2.0 * (point.x - rect.left()) / rect.width() - 1.0;
        let y = 1.0 - 2.0 * (point.y - rect.top()) / rect.height();
        let near = inverse.project_point3(Vec3::new(x, y, 0.0));
        let far = inverse.project_point3(Vec3::new(x, y, 1.0));
        let fraction = (height - near.y) / (far.y - near.y);
        if !(0.0..=1.0).contains(&fraction) {
            return None;
        }
        let position = near.lerp(far, fraction);
        position.is_finite().then_some([position.x, position.z])
    }

    pub fn screen_point(self, rect: egui::Rect, world: [f32; 3]) -> Option<egui::Pos2> {
        if !rect.is_positive() {
            return None;
        }
        let (_, projection) = self.projection(rect);
        let clip = projection * Vec3::from_array(world).extend(1.0);
        if clip.w <= 0.0 || !clip.is_finite() {
            return None;
        }
        let point = clip.truncate() / clip.w;
        let projected = egui::pos2(
            rect.left() + (point.x + 1.0) * 0.5 * rect.width(),
            rect.top() + (1.0 - point.y) * 0.5 * rect.height(),
        );
        ((0.0..=1.0).contains(&point.z) && rect.contains(projected)).then_some(projected)
    }
}

#[cfg(test)]
mod camera_tests {
    use super::*;

    #[test]
    fn ground_click_inverts_the_rendered_position_at_different_camera_angles() {
        let rect = egui::Rect::from_min_size(egui::pos2(115.0, 42.0), egui::vec2(780.0, 492.0));
        let expected = Vec3::new(3.35, 0.65, -1.78);
        for (yaw, pitch, distance) in [(-1.31, 0.28, 4.2), (1.12, 0.68, 6.15), (0.0, -0.1, 4.6)] {
            let camera = Camera {
                yaw,
                pitch,
                distance,
                target: [3.0, 1.28, -2.0],
            };
            let mouse = camera.screen_point(rect, expected.to_array()).unwrap();
            let [x, z] = camera.ground_point(rect, mouse, expected.y).unwrap();
            assert!((x - expected.x).abs() < 0.001);
            assert!((z - expected.z).abs() < 0.001);
            assert!(
                camera
                    .ground_point(rect, egui::Pos2::ZERO, expected.y)
                    .is_none()
            );
        }
    }
}

/// Editor-only guides for the authored collision box and the pair of seat anchors.
pub fn placement_guides(ui: &egui::Ui, rect: egui::Rect, camera: Camera, model: &Placement) {
    let painter = ui.painter().with_clip_rect(rect);
    if let Some(collider) = model.custom_collider() {
        let corners: [_; 8] = std::array::from_fn(|index| {
            let point = std::array::from_fn(|axis| {
                if index & (1 << axis) == 0 {
                    collider.min[axis]
                } else {
                    collider.max[axis]
                }
            });
            camera.screen_point(rect, point)
        });
        let stroke = egui::Stroke::new(1.35_f32, egui::Color32::from_rgb(91, 214, 219));
        for (index, point) in corners.iter().enumerate() {
            for axis in 0..3 {
                let bit = 1 << axis;
                if index & bit == 0
                    && let (Some(from), Some(to)) = (point, corners[index | bit])
                {
                    painter.line_segment([*from, to], stroke);
                }
            }
        }
    }
    let Some(seat) = model.seat_pose() else {
        return;
    };
    let labels = [
        (
            seat.position,
            "Сесть",
            egui::Color32::from_rgb(255, 210, 113),
        ),
        (seat.stand, "Встать", egui::Color32::from_rgb(153, 236, 179)),
    ];
    let [sit, stand] = labels.map(|(point, _, _)| camera.screen_point(rect, point));
    if let (Some(sit), Some(stand)) = (sit, stand) {
        painter.line_segment(
            [sit, stand],
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(215, 203, 167)),
        );
    }
    for (screen, (_, label, color)) in [sit, stand].into_iter().zip(labels) {
        let Some(point) = screen else { continue };
        painter.circle(
            point,
            5.1,
            egui::Color32::from_gray(27),
            egui::Stroke::new(1.65_f32, color),
        );
        painter.text(
            point + egui::vec2(9.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
    }
}

pub fn key_parts(world: &World, session: Option<&InteractionState>) -> Vec<Part> {
    let mut parts = Vec::with_capacity(world.keys.len() * 11);
    for key in &world.keys {
        if session.is_some_and(|session| session.has_key(key.id)) {
            continue;
        }
        let origin = Vec3::from_array(key.position) + Vec3::Y * 0.36;
        let yaw = -20.0_f32;
        let rotation = Quat::from_rotation_y(yaw.to_radians());
        let mut add = |center: Vec3, size: [f32; 3], angle: f32| {
            parts.push(Part {
                id: key_marker_id(key.id),
                name: String::new(),
                shape: Shape::Box,
                position: (origin + rotation * center).to_array(),
                size,
                rotation: [0.0, yaw, angle],
                color: [0.94, 0.67, 0.25],
                motion: PartMotion::Static,
            });
        };
        for side in 0..8 {
            let angle = side as f32 * 45.0;
            let (sin, cos) = angle.to_radians().sin_cos();
            add(
                Vec3::new(-0.155 + 0.090 * cos, 0.040 + 0.090 * sin, 0.0),
                [0.091, 0.045, 0.052],
                angle + 90.0,
            );
        }
        add(Vec3::new(0.08, 0.040, 0.0), [0.30, 0.047, 0.052], 0.0);
        add(Vec3::new(0.116, 0.015, 0.0), [0.044, 0.065, 0.052], 0.0);
        add(Vec3::new(0.208, 0.003, 0.0), [0.044, 0.084, 0.052], 0.0);
    }
    parts
}

pub struct Spark {
    pub position: [f32; 3],
    pub color: [f32; 3],
    pub size: f32,
}

pub struct ModelDraw<'a> {
    pub token: u64,
    pub model: &'a Arc<Model>,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub selected: bool,
    pub tuning: Tuning,
    pub vegetation: [f32; 4],
    pub receives_weather: bool,
    pub graph: Option<Compiled>,
    pub material_preview: PreviewLight,
    pub clip: Option<usize>,
    pub elapsed: f32,
    pub blend: Option<BlendPose>,
    pub bone_offsets: Vec<BoneRotation>,
    pub bone_translations: Vec<BoneTranslation>,
    pub morph_weights: Vec<MorphWeight>,
    pub secondary: SecondaryPose,
}

#[derive(Clone, Copy)]
pub struct BlendPose {
    pub clip: Option<usize>,
    pub elapsed: f32,
    /// Weight of the current pose. Zero keeps this previous pose.
    pub weight: f32,
}

pub struct ViewportScene<'a> {
    pub robot: &'a [Part],
    pub environment: &'a [Part],
    pub architecture: &'a [Part],
    pub models: &'a [ModelDraw<'a>],
    pub camera: Camera,
    pub selected: Option<u64>,
    pub sparks: &'a [Spark],
    pub glow: f32,
    pub playing: bool,
    pub lighting: Lighting,
    pub lights: &'a [(Vec3, crate::assets::materials::PointLight)],
    pub natural: Option<NaturalScene<'a>>,
    pub effects: &'a [Particle],
    pub post_process: PostProcess,
}

pub struct NaturalScene<'a> {
    pub atmosphere: &'a Atmosphere,
    pub water: &'a [WaterSurface],
    pub elapsed: f64,
    pub wind: Vec3,
    pub(crate) snow_tracks: &'a [crate::snow_tracks::RenderTrack],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    // Ground, selected, emission and environment material are independent channels.
    style: [f32; 4],
}

impl Instance {
    fn new(position: Vec3, size: Vec3, rotation: Quat, rgb: [f32; 3], style: [f32; 4]) -> Self {
        Self {
            model: Mat4::from_scale_rotation_translation(size, rotation, position)
                .to_cols_array_2d(),
            color: [rgb[0], rgb[1], rgb[2], 1.0],
            style,
        }
    }

    fn for_part(part: &Part, selected: Option<u64>, emission: f32, surface: f32) -> (usize, Self) {
        let mesh = match part.shape {
            Shape::Box => 0,
            Shape::Sphere => 1,
            Shape::Cylinder => 2,
        };
        let [x, y, z] = part.rotation.map(f32::to_radians);
        let instance = Self::new(
            Vec3::from_array(part.position),
            Vec3::from_array(part.size).max(Vec3::splat(0.01)),
            Quat::from_euler(EulerRot::XYZ, x, y, z),
            part.color,
            [
                0.0,
                f32::from(u8::from(selected == Some(part.id))),
                emission,
                surface,
            ],
        );
        (mesh, instance)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    view_projection: [[f32; 4]; 4],
    inverse_view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    contact_shadows: [[f32; 4]; CONTACT_SHADOWS],
    atmosphere: [f32; 4],
    light_view_projection: [[f32; 4]; 4],
    shadow_map: [f32; 4],
    far_light_view_projection: [[f32; 4]; 4],
    shadow_cascade: [f32; 4],
    interior_lights: [InteriorLight; INTERIOR_LIGHTS],
    interior_rooms: [InteriorRoom; INTERIOR_ROOMS],
    effect_origin_and_radius: [f32; 4],
    effect_color_and_strength: [f32; 4],
    natural: NaturalUniform,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct NaturalUniform {
    sun_direction_strength: [f32; 4],
    sun_color_enabled: [f32; 4],
    sky_top: [f32; 4],
    sky_horizon: [f32; 4],
    ambient: [f32; 4],
    fog: [f32; 4],
    weather: [f32; 4],
    precipitation: [f32; 4],
    surface_weather: [f32; 4],
    sky_drift: [f32; 4],
    basin_count: [u32; 4],
    basin_bounds: [[f32; 4]; MAX_ZONES],
    basin_floor: [[f32; 4]; MAX_ZONES],
    track_bounds: [f32; 4],
    track_positions: [[f32; 4]; crate::snow_tracks::MAX_VISIBLE],
    track_directions: [[f32; 4]; crate::snow_tracks::MAX_VISIBLE],
}

impl NaturalUniform {
    fn new(
        scene: Option<&NaturalScene<'_>>,
        eye: Vec3,
        target: Vec3,
        yaw: f32,
        rooms: &[InteriorRoom],
    ) -> Self {
        let mut uniform = Self::zeroed();
        uniform.sun_direction_strength = KEY_DIRECTION.normalize().extend(1.0).to_array();
        if let Some(scene) = scene {
            let sky = scene.atmosphere;
            uniform.sun_direction_strength = sky.sun_direction.extend(sky.sun_strength).to_array();
            uniform.sun_color_enabled = sky.sun_color.extend(1.0).to_array();
            uniform.sky_top = sky.sky_top.extend(0.0).to_array();
            uniform.sky_horizon = sky.sky_horizon.extend(0.0).to_array();
            uniform.ambient = sky.ambient.extend(sky.ground_mist_height).to_array();
            uniform.fog = sky.fog_color.extend(sky.fog_density).to_array();
            let exposed = !rooms
                .iter()
                .any(|room| room.low[3] > 0.0 && (room.contains(eye) || room.contains(target)));
            let screen_right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
            uniform.weather = [
                sky.rain,
                if exposed { sky.rain } else { 0.0 },
                (scene.elapsed * 1.25).rem_euclid(64.0) as f32,
                scene.wind.dot(screen_right) * 0.034,
            ];
            uniform.precipitation = [
                sky.snow,
                if exposed { sky.snow } else { 0.0 },
                sky.cloud_cover,
                scene.elapsed.rem_euclid(4096.0) as f32,
            ];
            uniform.surface_weather = [sky.surface_wet, sky.surface_snow, 0.0, 0.0];
            let (mut low, mut high) = (
                glam::Vec2::splat(f32::INFINITY),
                glam::Vec2::splat(f32::NEG_INFINITY),
            );
            for (index, track) in scene
                .snow_tracks
                .iter()
                .take(crate::snow_tracks::MAX_VISIBLE)
                .enumerate()
            {
                let [x, _, z, size] = track.position_and_size;
                let point = glam::Vec2::new(x, z);
                let spread = glam::Vec2::splat(size.abs() * 1.40);
                low = low.min(point - spread);
                high = high.max(point + spread);
                uniform.track_positions[index] = track.position_and_size;
                uniform.track_directions[index] = track.direction_and_strength;
                uniform.basin_count[2] += 1;
            }
            if uniform.basin_count[2] != 0 {
                uniform.track_bounds = [low.x, low.y, high.x, high.y];
            }
            let phase = scene.elapsed * 0.022;
            uniform.sky_drift = [
                (phase.sin() * 1.8) as f32,
                ((phase * 0.71).cos() * 1.3) as f32,
                scene.wind.x,
                scene.wind.z,
            ];
        }
        uniform
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WaterInstance {
    bounds: [f32; 4],
    level: [f32; 4],
    tint: [f32; 4],
    flow: [f32; 4],
    shore: [f32; 4],
}

impl WaterInstance {
    fn new(surface: &WaterSurface, elapsed: f64) -> Self {
        let hz = f64::from(surface.wave_hz);
        let phase = (elapsed.rem_euclid(1.0 / hz) * hz * std::f64::consts::TAU) as f32;
        let time = elapsed.rem_euclid(4096.0) as f32;
        Self {
            bounds: [
                surface.center[0],
                surface.center[1],
                surface.size[0],
                surface.size[1],
            ],
            level: [surface.height, surface.wave_height, phase, surface.floor_y],
            tint: Vec3::from_array(surface.tint)
                .extend(if surface.rounded_edge { 1.0 } else { 0.0 })
                .to_array(),
            flow: [
                surface.current[0] * time,
                surface.current[1] * time,
                surface.blend_mode.index() as f32,
                0.0,
            ],
            shore: surface.shore.map_or([0.0; 4], |shore| {
                let gradient = shore.gradient();
                [
                    gradient.x,
                    gradient.y,
                    gradient.dot(glam::Vec2::from_array(shore.origin)),
                    1.0,
                ]
            }),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InteriorLight {
    position_radius: [f32; 4],
    tint_room: [f32; 4],
    strength: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InteriorRoom {
    low: [f32; 4],
    high: [f32; 4],
}

impl InteriorRoom {
    fn contains(self, position: Vec3) -> bool {
        position.cmpge(Vec3::from_slice(&self.low)).all()
            && position.cmple(Vec3::from_slice(&self.high)).all()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParticleInstance {
    center_radius: [f32; 4],
    velocity_kind: [f32; 4],
    tint_alpha: [f32; 4],
}

impl From<&Particle> for ParticleInstance {
    fn from(particle: &Particle) -> Self {
        let code = match particle.kind {
            effects::Kind::Smoke => 0.0,
            effects::Kind::Dust => 1.0,
            effects::Kind::Rain => 2.0,
            effects::Kind::Splash => 3.0,
            effects::Kind::Trail => 4.0,
        };
        Self {
            center_radius: particle.position.extend(particle.radius).to_array(),
            velocity_kind: particle.velocity.extend(code).to_array(),
            tint_alpha: Vec3::from_array(particle.color)
                .extend(particle.opacity)
                .to_array(),
        }
    }
}

struct Renderer {
    pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    water_pipelines: [wgpu::RenderPipeline; 4],
    rain_pipeline: wgpu::RenderPipeline,
    particle_pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    mesh_ranges: [Range<u32>; MESH_COUNT],
    instances: wgpu::Buffer,
    water_instances: wgpu::Buffer,
    particle_instances: wgpu::Buffer,
    water_grid: u32,
    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,
    far_view_buffer: wgpu::Buffer,
    far_view_bind_group: wgpu::BindGroup,
    post: PostRenderer,
    models: ModelRenderer,
    shadow: ShadowRenderer,
}

struct Viewports {
    main: Renderer,
    preview: Option<Renderer>,
    format: wgpu::TextureFormat,
    samples: u16,
}

pub(crate) struct GpuTimingQueries(pub wgpu::QuerySet);

pub fn install(cc: &eframe::CreationContext<'_>, samples: u16) {
    if let Some(state) = &cc.wgpu_render_state {
        install_renderer(
            &mut state.renderer.write(),
            &state.device,
            &state.queue,
            state.target_format,
            samples,
        );
    }
}

pub(crate) fn install_renderer(
    renderer: &mut egui_wgpu::Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    samples: u16,
) {
    renderer.callback_resources.insert(Viewports {
        main: Renderer::new(device, queue, format, samples),
        preview: None,
        format,
        samples,
    });
}

pub fn paint(ui: &egui::Ui, rect: egui::Rect, scene: ViewportScene<'_>) {
    paint_scene(ui, rect, scene, false);
}

pub(crate) fn paint_preview(ui: &egui::Ui, rect: egui::Rect, scene: ViewportScene<'_>) {
    paint_scene(ui, rect, scene, true);
}

fn paint_scene(ui: &egui::Ui, rect: egui::Rect, scene: ViewportScene<'_>, preview: bool) {
    if !rect.is_positive() {
        return;
    }

    let ViewportScene {
        robot,
        environment,
        architecture,
        models,
        camera,
        selected,
        sparks,
        glow,
        playing,
        lighting,
        lights,
        natural,
        effects,
        post_process,
    } = scene;
    let target = Vec3::from_array(camera.target);
    let (eye, view_projection) = camera.projection(rect);
    let mut groups: [Vec<Instance>; MESH_COUNT] = std::array::from_fn(|_| Vec::new());
    groups[0].push(Instance::new(
        Vec3::new(target.x, -0.042, target.z),
        Vec3::new(240.0, 0.08, 240.0),
        Quat::IDENTITY,
        lighting.ground_tint.unwrap_or([0.23, 0.25, 0.25]),
        [1.0, 0.0, 0.0, 0.0],
    ));

    for part in environment {
        let emission = if marker_key_id(part.id).is_some() {
            0.28
        } else {
            0.0
        };
        let (mesh, instance) = Instance::for_part(part, selected, emission, 1.0);
        groups[mesh].push(instance);
    }
    for part in architecture {
        let surface = buildings::surface(part.id).unwrap_or(Surface::Floor);
        let tag = if surface == Surface::Floor && part.size[1] <= 0.03 {
            9.0
        } else {
            1.0 + f32::from(surface as u8)
        };
        let highlight = if selected.is_some() && selected == buildings::building_id(part.id) {
            Some(part.id)
        } else {
            selected
        };
        let (mesh, instance) = Instance::for_part(part, highlight, 0.0, tag);
        groups[mesh].push(instance);
    }
    let (interior_rooms, interior_lights) = interior_lighting(architecture, lights, target);

    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for part in robot {
        let position = Vec3::from_array(part.position);
        let size = Vec3::from_array(part.size).max(Vec3::splat(0.01));
        min = min.min(position - size * 0.5);
        max = max.max(position + size * 0.5);
        let (mesh, instance) = Instance::for_part(part, selected, glow.clamp(0.0, 2.0) * 0.28, 0.0);
        groups[mesh].push(instance);
    }
    let mut effect_center = Vec3::ZERO;
    for (index, spark) in sparks.iter().enumerate() {
        let size = spark.size.clamp(0.003, 0.3);
        let position = Vec3::from_array(spark.position);
        effect_center += position;
        groups[3].push(Instance::new(
            position,
            Vec3::new(size * 0.69, size * 1.85, size * 0.69),
            Quat::from_rotation_z((index as f32 * 1.84).sin()),
            spark.color,
            [0.0, 0.0, 2.2, 0.0],
        ));
    }
    let (effect_origin_and_radius, effect_color_and_strength) =
        sparks.first().map_or(([0.0; 4], [0.0; 4]), |spark| {
            let [x, y, z] = (effect_center / sparks.len() as f32 - Vec3::Y * 0.13).to_array();
            let [r, g, b] = spark.color;
            let strength = glow.clamp(0.0, 1.0);
            (
                [x, y, z, 1.05 + strength * 0.86],
                [r, g, b, strength * 1.08],
            )
        });

    let robot_contact = (!robot.is_empty()).then(|| {
        let center = (min + max) * 0.5;
        let dimensions = max - min;
        let radius = (dimensions.x.max(dimensions.z) * 0.50).max(0.38);
        [center.x, min.y, center.z, radius]
    });

    let mut natural_uniform =
        NaturalUniform::new(natural.as_ref(), eye, target, camera.yaw, &interior_rooms);
    if let Some(natural) = &natural {
        for surface in natural.water.iter().take(MAX_ZONES) {
            if surface.floor_y < -0.008 && (!surface.rounded_edge || surface.shore.is_some()) {
                let index = natural_uniform.basin_count[0] as usize;
                natural_uniform.basin_bounds[index] = [
                    surface.center[0],
                    surface.center[1],
                    surface.size[0] * 0.5,
                    surface.size[1] * 0.5,
                ];
                natural_uniform.basin_count[0] += 1;
                if let Some(shore) = surface.shore {
                    let gradient = shore.gradient();
                    natural_uniform.basin_floor[index] = [
                        gradient.x,
                        gradient.y,
                        gradient.dot(glam::Vec2::from_array(shore.origin)) + 0.008,
                        if surface.rounded_edge { 2.0 } else { 1.0 },
                    ];
                } else {
                    basin_geometry(surface, &mut groups[0]);
                }
            }
        }
    }
    let mut instances =
        Vec::with_capacity(robot.len() + environment.len() + architecture.len() + sparks.len() + 1);
    let instance_ranges = groups.map(|group| {
        let start = instances.len() as u32;
        instances.extend(group);
        start..instances.len() as u32
    });
    let mut shadow_ranges = instance_ranges.clone();
    shadow_ranges[0].start += 1; // Ground receives shadows but doesn't cast any.
    shadow_ranges[3].end = shadow_ranges[3].start; // Emissive particles don't cast shadows.
    let painter = ui.painter().with_clip_rect(rect);
    let water = natural.map_or_else(Vec::new, |natural| {
        natural
            .water
            .iter()
            .take(MAX_ZONES)
            .filter(|surface| surface.height - surface.floor_y > 0.001)
            .map(|surface| WaterInstance::new(surface, natural.elapsed))
            .collect()
    });
    let particles: Vec<_> = effects
        .iter()
        .take(effects::MAX_PARTICLES)
        .map(ParticleInstance::from)
        .collect();
    painter.add(egui_wgpu::Callback::new_paint_callback(
        rect,
        SceneCallback {
            rect,
            preview,
            post_process,
            uniform: ViewUniform {
                view_projection: view_projection.to_cols_array_2d(),
                inverse_view_projection: view_projection.inverse().to_cols_array_2d(),
                eye: [eye.x, eye.y, eye.z, 0.0],
                contact_shadows: contact_shadows(models, robot_contact, target),
                atmosphere: [
                    f32::from(u8::from(playing)),
                    lighting.daylight,
                    lighting.interior_gain,
                    0.0,
                ],
                light_view_projection: [[0.0; 4]; 4],
                shadow_map: [0.0; 4],
                far_light_view_projection: [[0.0; 4]; 4],
                shadow_cascade: [0.0; 4],
                interior_lights,
                interior_rooms,
                effect_origin_and_radius,
                effect_color_and_strength,
                natural: natural_uniform,
            },
            instances,
            instance_ranges,
            shadow_ranges,
            shadow_camera: (target, camera.distance, rect.aspect_ratio()),
            models: models
                .iter()
                .map(|draw| {
                    let mut model = PreparedModel::from(draw);
                    let height =
                        (draw.model.bounds_max[1] - draw.model.bounds_min[1]) * draw.scale[1];
                    let depth = eye.distance(Vec3::from_array(draw.position)).max(0.1);
                    model.guard_detail =
                        draw.selected || (height * rect.height()) / (0.728 * depth) >= 82.0;
                    model
                })
                .collect(),
            water,
            particles,
        },
    ));
}

fn basin_geometry(surface: &WaterSurface, instances: &mut Vec<Instance>) {
    let [x, z] = surface.center;
    let [width, depth] = surface.size;
    let y = surface.floor_y;
    let height = -0.002 - y;
    let mut add = |position, size, rgb| {
        instances.push(Instance::new(
            position,
            size,
            Quat::IDENTITY,
            rgb,
            [0.0, 0.0, 0.0, 1.0],
        ));
    };
    add(
        Vec3::new(x, y - 0.035, z),
        Vec3::new(width, 0.07, depth),
        [0.26, 0.26, 0.23],
    );
    let edge = 0.012;
    let middle = y + height * 0.5;
    let rock = [0.31, 0.29, 0.24];
    for side in [-1.0, 1.0] {
        add(
            Vec3::new(x + side * (width * 0.5 + edge * 0.5), middle, z),
            Vec3::new(edge, height, depth + edge * 2.0),
            rock,
        );
        add(
            Vec3::new(x, middle, z + side * (depth * 0.5 + edge * 0.5)),
            Vec3::new(width, height, edge),
            rock,
        );
    }
}

fn contact_shadows(
    models: &[ModelDraw<'_>],
    robot: Option<[f32; 4]>,
    focus: Vec3,
) -> [[f32; 4]; CONTACT_SHADOWS] {
    let mut contacts = [[0.0; 4]; CONTACT_SHADOWS];
    let first = usize::from(robot.is_some());
    if let Some(robot) = robot {
        contacts[0] = robot;
    }
    let mut animated: Vec<_> = models
        .iter()
        .filter(|draw| !draw.model.animations.is_empty())
        .collect();
    let distance = |draw: &ModelDraw<'_>| Vec3::from_array(draw.position).distance_squared(focus);
    animated.sort_unstable_by(|a, b| distance(a).total_cmp(&distance(b)));
    let animated_count = animated.len().min(4 - first);
    for (slot, draw) in contacts[first..]
        .iter_mut()
        .take(animated_count)
        .zip(animated)
    {
        let [x, y, z] = draw.position;
        let low = draw.model.bounds_min[1] * draw.scale[1];
        let high = draw.model.bounds_max[1] * draw.scale[1];
        let radius = ((high - low).abs() * 0.24).clamp(0.25, 0.65);
        *slot = [x, y + low.min(high), z, radius];
    }
    let mut grounded: Vec<_> = models
        .iter()
        .filter_map(|draw| {
            if !draw.model.animations.is_empty() {
                return None;
            }
            let min = Vec3::from_array(draw.model.bounds_min) * Vec3::from_array(draw.scale);
            let max = Vec3::from_array(draw.model.bounds_max) * Vec3::from_array(draw.scale);
            let size = max - min;
            if size.y < 0.23 || size.x.max(size.z) > 7.5 {
                return None;
            }
            let yaw = Quat::from_rotation_y(draw.rotation[1].to_radians());
            let center = Vec3::from_array(draw.position)
                + yaw * Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5);
            let radius = if draw.vegetation[3] > 0.0 {
                size.x.min(size.z) * 0.20
            } else {
                size.x.min(size.z) * 0.72
            };
            Some((center, radius.clamp(0.19, 2.25)))
        })
        .collect();
    grounded.sort_unstable_by(|(a, _), (b, _)| {
        a.distance_squared(focus)
            .total_cmp(&b.distance_squared(focus))
    });
    for (slot, (center, radius)) in contacts[first + animated_count..].iter_mut().zip(grounded) {
        *slot = center.extend(radius).to_array();
    }
    contacts
}

struct SceneCallback {
    rect: egui::Rect,
    preview: bool,
    post_process: PostProcess,
    uniform: ViewUniform,
    instances: Vec<Instance>,
    instance_ranges: [Range<u32>; MESH_COUNT],
    shadow_ranges: [Range<u32>; MESH_COUNT],
    shadow_camera: (Vec3, f32, f32),
    models: Vec<PreparedModel>,
    water: Vec<WaterInstance>,
    particles: Vec<ParticleInstance>,
}

fn interior_lighting(
    architecture: &[Part],
    authored_lights: &[(Vec3, crate::assets::materials::PointLight)],
    target: Vec3,
) -> (
    [InteriorRoom; INTERIOR_ROOMS],
    [InteriorLight; INTERIOR_LIGHTS],
) {
    let mut roofs = HashMap::<u64, &Part>::new();
    for part in architecture
        .iter()
        .filter(|part| buildings::surface(part.id) == Some(Surface::Roof))
    {
        if let Some(id) = buildings::building_id(part.id) {
            let roof = roofs.entry(id).or_insert(part);
            if part.size[0] * part.size[2] > roof.size[0] * roof.size[2] {
                *roof = part;
            }
        }
    }
    let distance = |part: &Part| Vec3::from_array(part.position).distance_squared(target);
    let mut floors: Vec<_> = architecture
        .iter()
        .filter(|part| {
            buildings::building_id(part.id) == Some(part.id) && roofs.contains_key(&part.id)
        })
        .collect();
    floors.sort_unstable_by(|a, b| distance(a).total_cmp(&distance(b)));
    let mut rooms = [InteriorRoom::zeroed(); INTERIOR_ROOMS];
    let mut room_ids = [None; INTERIOR_ROOMS];
    for (index, floor) in floors.into_iter().take(INTERIOR_ROOMS).enumerate() {
        let roof = roofs[&floor.id];
        let [x, y, z] = floor.position;
        let [width, height, depth] = floor.size;
        let half_x = (width * 0.5 - 0.13).max(0.25);
        let half_z = (depth * 0.5 - 0.13).max(0.25);
        rooms[index] = InteriorRoom {
            low: [x - half_x, y + height * 0.5 - 0.045, z - half_z, 1.0],
            high: [
                x + half_x,
                roof.position[1] - roof.size[1] * 0.5,
                z + half_z,
                0.0,
            ],
        };
        room_ids[index] = Some(floor.id);
    }

    let mut candidates: Vec<_> = architecture
        .iter()
        .filter(|part| buildings::surface(part.id) == Some(Surface::Light))
        .map(|part| {
            let position = Vec3::from_array(part.position) - Vec3::Y * 0.08;
            let room = room_ids.iter().enumerate().position(|(index, id)| {
                id.is_some()
                    && *id == buildings::building_id(part.id)
                    && rooms[index].contains(position)
            });
            light_uniform(
                position,
                part.color,
                if room.is_some() { 5.3 } else { 3.1 },
                1.0,
                room,
            )
        })
        .collect();
    for &(position, lamp) in authored_lights {
        let room = rooms
            .iter()
            .position(|room| room.low[3] > 0.0 && room.contains(position));
        candidates.push(light_uniform(
            position,
            lamp.color,
            lamp.radius,
            lamp.intensity,
            room,
        ));
    }
    let distance =
        |lamp: &InteriorLight| Vec3::from_slice(&lamp.position_radius).distance_squared(target);
    candidates.sort_unstable_by(|a, b| distance(a).total_cmp(&distance(b)));
    let mut lights = [InteriorLight::zeroed(); INTERIOR_LIGHTS];
    for (slot, lamp) in lights.iter_mut().zip(candidates) {
        *slot = lamp;
    }
    (rooms, lights)
}

fn light_uniform(
    position: Vec3,
    [r, g, b]: [f32; 3],
    radius: f32,
    intensity: f32,
    room: Option<usize>,
) -> InteriorLight {
    InteriorLight {
        position_radius: position.extend(radius).to_array(),
        tint_room: [r, g, b, room.map_or(0.0, |index| index as f32 + 1.0)],
        strength: [intensity, 0.0, 0.0, 0.0],
    }
}

fn light_view(
    target: Vec3,
    camera_distance: f32,
    aspect: f32,
    resolution: u32,
    light_direction: Vec3,
) -> (Mat4, f32) {
    let extent = (camera_distance * 1.6 + aspect.clamp(0.5, 3.0) * 2.0).clamp(14.0, 48.0);
    shadow_cascades::light_projection(target, extent, resolution, light_direction)
}

impl egui_wgpu::CallbackTrait for SceneCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let pixels = egui::PaintCallbackInfo {
            viewport: self.rect,
            clip_rect: self.rect,
            pixels_per_point: screen.pixels_per_point,
            screen_size_px: screen.size_in_pixels,
        }
        .viewport_in_pixels();
        if pixels.width_px == 0 || pixels.height_px == 0 {
            return Vec::new();
        }
        let timing = (!self.preview)
            .then(|| resources.get::<GpuTimingQueries>())
            .flatten()
            .map(|timing| timing.0.clone());
        let measure_sort = !self.preview && resources.get::<TransparencyProbe>().is_some();
        let mut sort_sample = None;
        if let Some(viewports) = resources.get_mut::<Viewports>() {
            let renderer = if self.preview {
                viewports.preview.get_or_insert_with(|| {
                    Renderer::new(device, queue, viewports.format, viewports.samples)
                })
            } else {
                &mut viewports.main
            };
            renderer.post.configure(
                device,
                queue,
                [pixels.width_px as u32, pixels.height_px as u32],
                self.post_process,
            );
            let mut uniform = self.uniform;
            let (target, distance, aspect) = self.shadow_camera;
            let resolution = renderer.shadow.resolution;
            let direction = Vec3::from_slice(&uniform.natural.sun_direction_strength);
            let cascaded = self.post_process.shadow_quality == ShadowQuality::Cascaded;
            let (projection, world_texel) = if cascaded {
                let eye = Vec3::from_slice(&uniform.eye);
                let [near, (far_projection, far_texel)] =
                    shadow_cascades::projections(eye, target, aspect, resolution, direction);
                uniform.far_light_view_projection = far_projection.to_cols_array_2d();
                uniform.shadow_cascade = [
                    far_texel,
                    shadow_cascades::SPLIT,
                    shadow_cascades::TRANSITION,
                    1.0,
                ];
                let far_uniform = ViewUniform {
                    light_view_projection: uniform.far_light_view_projection,
                    ..uniform
                };
                queue.write_buffer(
                    &renderer.far_view_buffer,
                    0,
                    bytemuck::bytes_of(&far_uniform),
                );
                renderer.shadow.ensure_cascades(device);
                near
            } else {
                renderer.shadow.cascades = None;
                light_view(target, distance, aspect, resolution, direction)
            };
            uniform.light_view_projection = projection.to_cols_array_2d();
            uniform.shadow_map = [1.0 / resolution as f32, world_texel, 0.88, 0.0];
            uniform.natural.basin_count[1] = renderer.water_grid;
            queue.write_buffer(&renderer.view_buffer, 0, bytemuck::bytes_of(&uniform));
            let bytes = bytemuck::cast_slice(&self.instances);
            if bytes.len() as u64 > renderer.instances.size() {
                renderer.instances =
                    instance_buffer(device, bytes.len().next_power_of_two() as u64);
            }
            queue.write_buffer(&renderer.instances, 0, bytes);
            if !self.water.is_empty() {
                queue.write_buffer(
                    &renderer.water_instances,
                    0,
                    bytemuck::cast_slice(&self.water),
                );
            }
            renderer.models.prepare(device, queue, &self.models);
            let sort_start = measure_sort.then(std::time::Instant::now);
            let layers = self.transparent_layers(renderer);
            sort_sample = sort_start.map(|start| SortSample {
                ms: start.elapsed().as_secs_f32() * 1000.0,
                order: layers.iter().map(|layer| layer.layer).collect(),
            });
            if !self.particles.is_empty() {
                let sorted_particles = layers
                    .iter()
                    .filter_map(|layer| match layer.layer {
                        TransparentLayer::Particle(index) => Some(self.particles[index]),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                queue.write_buffer(
                    &renderer.particle_instances,
                    0,
                    bytemuck::cast_slice(&sorted_particles),
                );
            }
            if let Some(timing) = &timing {
                encoder.write_timestamp(timing, 0);
                encoder.write_timestamp(timing, 5);
            }
            if uniform.natural.sun_direction_strength[3] > 0.005 {
                renderer.render_shadow(
                    encoder,
                    &renderer.shadow.view,
                    &renderer.view_bind_group,
                    &self.shadow_ranges,
                    &self.models,
                );
                if let Some(far) = renderer.shadow.cascades.as_ref().filter(|_| cascaded) {
                    renderer.render_shadow(
                        encoder,
                        &far.view,
                        &renderer.far_view_bind_group,
                        &self.shadow_ranges,
                        &self.models,
                    );
                }
            }
            if let Some(timing) = &timing {
                encoder.write_timestamp(timing, 6);
            }
            if let Some(mut pass) = renderer.post.scene_pass(encoder) {
                self.draw_geometry(&mut pass, renderer, &layers);
            }
            if let Some(timing) = &timing {
                encoder.write_timestamp(timing, 1);
            }
            if self.post_process.bloom {
                renderer.post.bloom(encoder);
            }
            if let Some(timing) = &timing {
                encoder.write_timestamp(timing, 2);
            }
        }
        if let Some(sample) = sort_sample
            && let Some(probe) = resources.get_mut::<TransparencyProbe>()
        {
            probe.0.push(sample);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(viewports) = resources.get::<Viewports>() {
            let renderer = if self.preview {
                viewports.preview.as_ref()
            } else {
                Some(&viewports.main)
            };
            if let Some(renderer) = renderer {
                let timing = (!self.preview)
                    .then(|| resources.get::<GpuTimingQueries>())
                    .flatten();
                if let Some(timing) = timing {
                    pass.write_timestamp(&timing.0, 3);
                }
                renderer.post.composite(pass);
                if let Some(timing) = timing {
                    pass.write_timestamp(&timing.0, 4);
                }
            }
        }
    }
}

impl SceneCallback {
    fn draw_geometry(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        renderer: &Renderer,
        layers: &[LayerDepth],
    ) {
        let shadows = renderer.shadow.bindings(self.post_process.shadow_quality);
        pass.set_pipeline(&renderer.pipeline);
        pass.set_bind_group(0, &renderer.view_bind_group, &[]);
        pass.set_bind_group(1, shadows, &[]);
        pass.set_vertex_buffer(0, renderer.vertices.slice(..));
        pass.set_vertex_buffer(1, renderer.instances.slice(..));
        for (mesh, instances) in renderer.mesh_ranges.iter().zip(&self.instance_ranges) {
            if !instances.is_empty() {
                pass.draw(mesh.clone(), instances.clone());
            }
        }
        renderer.models.paint(pass, &self.models, None);
        // Draw far sky after the depth-tested meshes so covered pixels can be skipped.
        pass.set_pipeline(&renderer.sky_pipeline);
        pass.draw(0..3, 0..1);
        let mut layers = layers.iter().map(|layer| layer.layer).peekable();
        let mut particle_end = 0;
        while let Some(layer) = layers.next() {
            match layer {
                TransparentLayer::Water(index) => {
                    let water = &self.water[index];
                    pass.set_pipeline(&renderer.water_pipelines[water.flow[2] as usize]);
                    pass.set_bind_group(1, shadows, &[]);
                    pass.set_vertex_buffer(0, renderer.water_instances.slice(..));
                    let cells = renderer.water_grid;
                    let sides = if water.level[0] + water.level[1] > -0.005 {
                        cells * 4
                    } else {
                        0
                    };
                    pass.draw(
                        0..((cells * cells + sides) * 6),
                        index as u32..index as u32 + 1,
                    );
                }
                TransparentLayer::Model { model, primitive } => {
                    pass.set_bind_group(1, shadows, &[]);
                    renderer
                        .models
                        .draw_transparent(pass, &self.models, model, primitive);
                }
                TransparentLayer::Particle(_) => {
                    let first = particle_end;
                    particle_end += 1;
                    while layers
                        .next_if(|layer| matches!(layer, TransparentLayer::Particle(_)))
                        .is_some()
                    {
                        particle_end += 1;
                    }
                    pass.set_pipeline(&renderer.particle_pipeline);
                    pass.set_vertex_buffer(0, renderer.particle_instances.slice(..));
                    pass.draw(0..6, first..particle_end);
                }
            }
        }
        if self.uniform.natural.weather[1] > 0.005 || self.uniform.natural.precipitation[1] > 0.005
        {
            pass.set_pipeline(&renderer.rain_pipeline);
            pass.draw(0..3, 0..1);
        }
    }

    fn transparent_layers(&self, renderer: &Renderer) -> Vec<LayerDepth> {
        let mut layers = Vec::new();
        let eye = Vec3::from_slice(&self.uniform.eye);
        renderer
            .models
            .transparent_layers(&self.models, eye, &mut layers);
        layers.extend(
            self.water
                .iter()
                .enumerate()
                .map(|(index, water)| LayerDepth {
                    layer: TransparentLayer::Water(index),
                    distance_squared: eye.distance_squared(Vec3::new(
                        water.bounds[0],
                        water.level[0],
                        water.bounds[1],
                    )),
                    decal: false,
                }),
        );
        layers.extend(
            self.particles
                .iter()
                .enumerate()
                .map(|(index, particle)| LayerDepth {
                    layer: TransparentLayer::Particle(index),
                    distance_squared: eye
                        .distance_squared(Vec3::from_slice(&particle.center_radius)),
                    decal: false,
                }),
        );
        sort_layers(&mut layers);
        layers
    }
}

fn instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Robot viewport instances"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ModelInstance {
    transform: [[f32; 4]; 4],
    style: [f32; 4],
    gains: [f32; 4],
    emission: [f32; 4],
    graph_tint_metallic: [f32; 4],
    graph_emission_preview: [f32; 4],
    vegetation: [f32; 4],
}

struct PreparedModel {
    token: u64,
    source: Arc<Model>,
    instance: ModelInstance,
    clip: Option<usize>,
    elapsed: f32,
    blend: Option<BlendPose>,
    bone_offsets: Vec<BoneRotation>,
    bone_translations: Vec<BoneTranslation>,
    morph_weights: Vec<MorphWeight>,
    secondary: SecondaryPose,
    guard_detail: bool,
    blend_mode: BlendMode,
}

impl From<&ModelDraw<'_>> for PreparedModel {
    fn from(draw: &ModelDraw<'_>) -> Self {
        let [x, y, z] = draw.rotation.map(f32::to_radians);
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::from_array(draw.scale).max(Vec3::splat(0.001)),
            Quat::from_euler(EulerRot::XYZ, x, y, z),
            Vec3::from_array(draw.position),
        );
        let tuning = draw.tuning;
        let graph = draw.graph.unwrap_or_default();
        let [u, v] = graph.uv_scale.unwrap_or(tuning.uv_scale);
        let [r, g, b] = tuning.emissive_color.unwrap_or([0.0; 3]);
        let [red, green, blue] = graph.base_color.unwrap_or([1.0; 3]);
        let [glow_r, glow_g, glow_b] = graph.emission.unwrap_or([0.0; 3]);
        Self {
            token: draw.token,
            source: Arc::clone(draw.model),
            instance: ModelInstance {
                transform: transform.to_cols_array_2d(),
                style: [
                    f32::from(u8::from(draw.selected)),
                    u,
                    v,
                    graph.roughness.or(tuning.roughness).unwrap_or(-1.0),
                ],
                gains: [
                    graph.normal_strength.unwrap_or(tuning.normal_strength),
                    tuning.emissive_strength,
                    f32::from(u8::from(draw.receives_weather)),
                    tuning.blend_mode.index() as f32,
                ],
                emission: [
                    r,
                    g,
                    b,
                    f32::from(u8::from(tuning.emissive_color.is_some())),
                ],
                graph_tint_metallic: [red, green, blue, graph.metallic.unwrap_or(-1.0)],
                graph_emission_preview: [
                    glow_r,
                    glow_g,
                    glow_b,
                    f32::from(draw.material_preview as u8),
                ],
                vegetation: draw.vegetation,
            },
            clip: draw.clip,
            elapsed: draw.elapsed,
            blend: draw
                .blend
                .filter(|blend| blend.weight.is_finite() && blend.weight < 1.0),
            bone_offsets: draw.bone_offsets.clone(),
            bone_translations: draw.bone_translations.clone(),
            morph_weights: draw.morph_weights.clone(),
            secondary: if draw.model.has_secondary_pose() {
                draw.secondary
            } else {
                SecondaryPose::default()
            },
            guard_detail: true,
            blend_mode: tuning.blend_mode,
        }
    }
}

struct ModelRenderer {
    pipelines: [wgpu::RenderPipeline; 2],
    transparent: [[wgpu::RenderPipeline; 2]; 4],
    decals: [[wgpu::RenderPipeline; 2]; 4],
    masked_shadow_pipeline: wgpu::RenderPipeline,
    shadow_empty: wgpu::BindGroup,
    material_layout: wgpu::BindGroupLayout,
    instances: wgpu::Buffer,
    cache: HashMap<u64, Vec<GpuPrimitive>>,
    animated: Vec<Option<AnimatedVertices>>,
    sampler: wgpu::Sampler,
    white_srgb: wgpu::TextureView,
    white_linear: wgpu::TextureView,
    flat_normal: wgpu::TextureView,
}

struct GpuPrimitive {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    material: wgpu::BindGroup,
    double_sided: bool,
    masked: bool,
    guard_fur: bool,
    alpha_blend: bool,
    facial_decal: bool,
    center: Vec3,
}

struct AnimatedVertices {
    token: u64,
    key: AnimationKey,
    buffers: Vec<Option<wgpu::Buffer>>,
}

#[derive(PartialEq)]
struct AnimationKey {
    clip: Option<usize>,
    elapsed: u32,
    previous: Option<(Option<usize>, u32, u32)>,
    bone_offsets: Vec<BoneRotation>,
    bone_translations: Vec<BoneTranslation>,
    morph_weights: Vec<MorphWeight>,
    secondary: SecondaryPose,
    guard_detail: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
    // Metallic and roughness factors, normal strength, has normal texture.
    surface: [f32; 4],
    emission_alpha: [f32; 4],
    illustration: [f32; 4],
}

struct TextureViews {
    linear: Option<wgpu::TextureView>,
    srgb: Option<wgpu::TextureView>,
}

#[derive(Clone, Copy, Default)]
struct TextureUse {
    linear: bool,
    srgb: bool,
}

impl TextureViews {
    fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba: &[u8],
        color_use: TextureUse,
    ) -> Self {
        let create = |format| upload_texture(device, queue, width, height, rgba, format);
        Self {
            linear: color_use
                .linear
                .then(|| create(wgpu::TextureFormat::Rgba8Unorm)),
            srgb: color_use
                .srgb
                .then(|| create(wgpu::TextureFormat::Rgba8UnormSrgb)),
        }
    }
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    rgba: &[u8],
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let mip_count = width.max(height).ilog2() + 1;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("GLB texture"),
        size,
        mip_level_count: mip_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let srgb_lookup = format.is_srgb().then(|| {
        std::array::from_fn(|index| {
            let value = index as f32 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        })
    });
    let mut width = width;
    let mut height = height;
    let mut pixels = std::borrow::Cow::Borrowed(rgba);
    for mip_level in 0..mip_count {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        if mip_level + 1 < mip_count {
            let next_width = (width / 2).max(1);
            let next_height = (height / 2).max(1);
            pixels = std::borrow::Cow::Owned(mip_pixels(
                &pixels,
                width,
                height,
                next_width,
                next_height,
                srgb_lookup.as_ref(),
            ));
            width = next_width;
            height = next_height;
        }
    }
    texture.create_view(&Default::default())
}

fn mip_pixels(
    source: &[u8],
    width: u32,
    height: u32,
    next_width: u32,
    next_height: u32,
    srgb_lookup: Option<&[f32; 256]>,
) -> Vec<u8> {
    let mut output = Vec::with_capacity((next_width * next_height * 4) as usize);
    for y in 0..next_height {
        let rows = y * height / next_height..(y + 1) * height / next_height;
        for x in 0..next_width {
            let columns = x * width / next_width..(x + 1) * width / next_width;
            let count = (columns.len() * rows.len()) as f32;
            let mut average = [0.0; 4];
            for sy in rows.clone() {
                for sx in columns.clone() {
                    let offset = ((sy * width + sx) * 4) as usize;
                    for channel in 0..4 {
                        let value = source[offset + channel];
                        average[channel] += match srgb_lookup {
                            Some(table) if channel != 3 => table[value as usize],
                            _ => f32::from(value) / 255.0,
                        };
                    }
                }
            }
            for (channel, sum) in average.into_iter().enumerate() {
                let value = sum / count;
                let encoded = if srgb_lookup.is_some() && channel != 3 {
                    if value <= 0.0031308 {
                        value * 12.92
                    } else {
                        1.055 * value.powf(1.0 / 2.4) - 0.055
                    }
                } else {
                    value
                };
                output.push((encoded * 255.0).round() as u8);
            }
        }
    }
    output
}

impl ModelRenderer {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        shader: &wgpu::ShaderModule,
        view_layout: &wgpu::BindGroupLayout,
        shadow_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        samples: u16,
    ) -> Self {
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("GLB material layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                material_texture_layout(2),
                material_texture_layout(3),
                material_texture_layout(4),
                material_texture_layout(5),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("GLB model pipeline layout"),
            bind_group_layouts: &[view_layout, shadow_layout, &material_layout],
            push_constant_ranges: &[],
        });
        let create = |blend: Option<BlendMode>, decal| {
            [Some(wgpu::Face::Back), None].map(|cull_mode| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("GLB model pipeline"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some("vs_model"),
                        buffers: &model_vertex_layouts(),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some("fs_model"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: blend.map(transparent_blend),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        cull_mode,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: blend.is_none(),
                        depth_compare: if blend.is_some() {
                            wgpu::CompareFunction::LessEqual
                        } else {
                            wgpu::CompareFunction::Less
                        },
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState {
                            constant: if decal { -1 } else { 0 },
                            ..Default::default()
                        },
                    }),
                    multisample: color_samples(samples),
                    multiview: None,
                    cache: None,
                })
            })
        };
        let pipelines = create(None, false);
        let transparent = BlendMode::ALL.map(|mode| create(Some(mode), false));
        let decals = BlendMode::ALL.map(|mode| create(Some(mode), true));
        let shadow_empty_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Unused shadow caster resource group"),
                entries: &[],
            });
        let shadow_empty = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Unused shadow caster resources"),
            layout: &shadow_empty_layout,
            entries: &[],
        });
        let masked_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Masked GLB shadow layout"),
            bind_group_layouts: &[view_layout, &shadow_empty_layout, &material_layout],
            push_constant_ranges: &[],
        });
        let masked_shadow_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Masked GLB shadow pipeline"),
                layout: Some(&masked_layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_model_shadow_masked"),
                    buffers: &model_vertex_layouts(),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs_model_shadow_masked"),
                    targets: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(shadow_depth_state()),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("GLB material sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 4,
            ..Default::default()
        });
        Self {
            pipelines,
            transparent,
            decals,
            masked_shadow_pipeline,
            shadow_empty,
            material_layout,
            instances: instance_buffer(device, 4096),
            cache: HashMap::new(),
            animated: Vec::new(),
            sampler,
            white_srgb: upload_texture(
                device,
                queue,
                1,
                1,
                &[255; 4],
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ),
            white_linear: upload_texture(
                device,
                queue,
                1,
                1,
                &[255; 4],
                wgpu::TextureFormat::Rgba8Unorm,
            ),
            flat_normal: upload_texture(
                device,
                queue,
                1,
                1,
                &[128, 128, 255, 255],
                wgpu::TextureFormat::Rgba8Unorm,
            ),
        }
    }

    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, draws: &[PreparedModel]) {
        self.cache
            .retain(|token, _| draws.iter().any(|draw| draw.token == *token));
        self.animated.resize_with(draws.len(), || None);
        let mut instances = Vec::with_capacity(draws.len());
        for (index, model) in draws.iter().enumerate() {
            if !self.cache.contains_key(&model.token) {
                let primitives = self.upload(device, queue, &model.source);
                self.cache.insert(model.token, primitives);
            }
            instances.push(model.instance);
            update_animation(device, queue, model, &mut self.animated[index]);
        }
        let bytes = bytemuck::cast_slice(&instances);
        if bytes.len() as u64 > self.instances.size() {
            self.instances = instance_buffer(device, bytes.len().next_power_of_two() as u64);
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.instances, 0, bytes);
        }
    }

    fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        model: &Model,
    ) -> Vec<GpuPrimitive> {
        let mut color_uses = vec![TextureUse::default(); model.images.len()];
        for primitive in &model.primitives {
            let material = &primitive.material;
            for index in [material.base_color_texture, material.emissive_texture]
                .into_iter()
                .flatten()
            {
                if let Some(usage) = color_uses.get_mut(index) {
                    usage.srgb = true;
                }
            }
            for index in [material.metallic_roughness_texture, material.normal_texture]
                .into_iter()
                .flatten()
            {
                if let Some(usage) = color_uses.get_mut(index) {
                    usage.linear = true;
                }
            }
        }
        let images: Vec<_> = model
            .images
            .iter()
            .zip(color_uses)
            .map(|(image, color_use)| {
                TextureViews::upload(
                    device,
                    queue,
                    image.width,
                    image.height,
                    &image.pixels,
                    color_use,
                )
            })
            .collect();
        let resting = model.pose(None, 0.0);
        model
            .primitives
            .iter()
            .enumerate()
            .map(|(index, primitive)| {
                let source = resting
                    .as_ref()
                    .map_or(primitive.vertices.as_slice(), |pose| &pose[index]);
                let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("GLB mesh vertices"),
                    contents: bytemuck::cast_slice(source),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("GLB mesh indices"),
                    contents: bytemuck::cast_slice(&primitive.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                let (low, high) = primitive.indices
                    .iter()
                    .map(|&index| Vec3::from_array(source[index as usize].position))
                    .fold(
                        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                        |(low, high), point| (low.min(point), high.max(point)),
                    );
                GpuPrimitive {
                    vertices,
                    indices,
                    index_count: primitive.indices.len() as u32,
                    material: self.create_material(device, &primitive.material, &images),
                    double_sided: primitive.material.double_sided,
                    masked: primitive.material.alpha_cutoff.is_some(),
                    guard_fur: primitive.material.guard_fur,
                    alpha_blend: primitive.material.alpha_blend,
                    facial_decal: primitive.material.facial_decal,
                    center: (low + high) * 0.5,
                }
            })
            .collect()
    }

    fn create_material(
        &self,
        device: &wgpu::Device,
        material: &Material,
        images: &[TextureViews],
    ) -> wgpu::BindGroup {
        let base = material
            .base_color_texture
            .and_then(|index| images.get(index))
            .and_then(|image| image.srgb.as_ref());
        let metallic_roughness = material
            .metallic_roughness_texture
            .and_then(|index| images.get(index))
            .and_then(|image| image.linear.as_ref());
        let normal = material
            .normal_texture
            .and_then(|index| images.get(index))
            .and_then(|image| image.linear.as_ref());
        let emissive = material
            .emissive_texture
            .and_then(|index| images.get(index))
            .and_then(|image| image.srgb.as_ref());
        let [r, g, b] = material.emissive;
        let factors = MaterialUniform {
            emission_alpha: [r, g, b, material.alpha_cutoff.unwrap_or(-1.0)],
            base_color: material.base_color,
            illustration: [
                material.illustration,
                f32::from(u8::from(material.guard_fur)),
                f32::from(u8::from(material.alpha_blend)),
                0.0,
            ],
            surface: [
                material.metallic,
                material.roughness,
                material.normal_scale,
                f32::from(u8::from(normal.is_some())),
            ],
        };
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("GLB material factors"),
            contents: bytemuck::bytes_of(&factors),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GLB material bindings"),
            layout: &self.material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(base.unwrap_or(&self.white_srgb)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        metallic_roughness.unwrap_or(&self.white_linear),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(
                        normal.unwrap_or(&self.flat_normal),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(
                        emissive.unwrap_or(&self.white_srgb),
                    ),
                },
            ],
        })
    }

    fn paint(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        draws: &[PreparedModel],
        shadow_pipeline: Option<&wgpu::RenderPipeline>,
    ) {
        if draws.is_empty() {
            return;
        }
        pass.set_vertex_buffer(1, self.instances.slice(..));
        for (index, draw) in draws.iter().enumerate() {
            if let Some(primitives) = self.cache.get(&draw.token) {
                let animated = self.animated[index].as_ref();
                for (primitive_index, primitive) in primitives.iter().enumerate() {
                    if !draw.guard_detail && primitive.guard_fur {
                        continue;
                    }
                    if let Some(opaque) = shadow_pipeline {
                        if primitive.facial_decal
                            || (primitive.transparent(draw) && !primitive.masked)
                        {
                            continue;
                        }
                        if primitive.masked {
                            pass.set_pipeline(&self.masked_shadow_pipeline);
                            pass.set_bind_group(1, &self.shadow_empty, &[]);
                            pass.set_bind_group(2, &primitive.material, &[]);
                        } else {
                            pass.set_pipeline(opaque);
                        }
                    } else {
                        if primitive.transparent(draw) {
                            continue;
                        }
                        pass.set_pipeline(&self.pipelines[usize::from(primitive.double_sided)]);
                        pass.set_bind_group(2, &primitive.material, &[]);
                    }
                    let vertices = animated
                        .and_then(|pose| pose.buffers[primitive_index].as_ref())
                        .unwrap_or(&primitive.vertices);
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    pass.set_index_buffer(primitive.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..primitive.index_count, 0, index as u32..index as u32 + 1);
                }
            }
        }
    }

    fn transparent_layers(&self, draws: &[PreparedModel], eye: Vec3, layers: &mut Vec<LayerDepth>) {
        for (model, draw) in draws.iter().enumerate() {
            let Some(primitives) = self.cache.get(&draw.token) else {
                continue;
            };
            let transform = Mat4::from_cols_array_2d(&draw.instance.transform);
            for (primitive, mesh) in primitives.iter().enumerate() {
                if mesh.transparent(draw) && (draw.guard_detail || !mesh.guard_fur) {
                    let center = transform.transform_point3(mesh.center);
                    layers.push(LayerDepth {
                        layer: TransparentLayer::Model { model, primitive },
                        distance_squared: eye.distance_squared(center),
                        decal: mesh.facial_decal,
                    });
                }
            }
        }
    }

    fn draw_transparent(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        draws: &[PreparedModel],
        model: usize,
        mesh: usize,
    ) {
        let draw = &draws[model];
        let primitive = &self.cache[&draw.token][mesh];
        let pipelines = if primitive.facial_decal {
            &self.decals
        } else {
            &self.transparent
        };
        let pipeline = &pipelines[draw.blend_mode.index()][usize::from(primitive.double_sided)];
        let vertices = self.animated[model]
            .as_ref()
            .and_then(|pose| pose.buffers[mesh].as_ref())
            .unwrap_or(&primitive.vertices);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(2, &primitive.material, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_vertex_buffer(1, self.instances.slice(..));
        pass.set_index_buffer(primitive.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..primitive.index_count, 0, model as u32..model as u32 + 1);
    }
}

impl GpuPrimitive {
    fn transparent(&self, draw: &PreparedModel) -> bool {
        self.facial_decal || self.alpha_blend || draw.blend_mode != BlendMode::Normal
    }
}

fn material_texture_layout(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn update_animation(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    model: &PreparedModel,
    current: &mut Option<AnimatedVertices>,
) {
    if model.clip.is_none()
        && model.blend.is_none()
        && model.bone_offsets.is_empty()
        && model.bone_translations.is_empty()
        && model.morph_weights.is_empty()
        && !model.secondary.active()
    {
        *current = None;
        return;
    }
    let key = AnimationKey {
        clip: model.clip,
        elapsed: model.elapsed.to_bits(),
        previous: model
            .blend
            .map(|blend| (blend.clip, blend.elapsed.to_bits(), blend.weight.to_bits())),
        bone_offsets: model.bone_offsets.clone(),
        bone_translations: model.bone_translations.clone(),
        morph_weights: model.morph_weights.clone(),
        secondary: model.secondary,
        guard_detail: model.guard_detail,
    };
    if current
        .as_ref()
        .is_some_and(|pose| pose.token == model.token && pose.key == key)
    {
        return;
    }
    let Some(pose) = blended_pose(model) else {
        *current = None;
        return;
    };
    let mut buffers = current
        .take()
        .filter(|pose| pose.token == model.token)
        .map_or_else(|| vec![None; pose.len()], |previous| previous.buffers);
    for (index, (buffer, stream)) in buffers.iter_mut().zip(&pose).enumerate() {
        if !model.guard_detail && model.source.primitives[index].material.guard_fur {
            continue;
        }
        if let Some(buffer) = buffer {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(stream));
        } else {
            *buffer = Some(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("GLB posed mesh vertices"),
                    contents: bytemuck::cast_slice(stream),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                }),
            );
        }
    }
    *current = Some(AnimatedVertices {
        token: model.token,
        key,
        buffers,
    });
}

fn blended_pose(model: &PreparedModel) -> Option<Vec<Vec<ModelVertex>>> {
    model.source.pose_blended_with_morphs(
        model.clip,
        model.elapsed,
        model
            .blend
            .map(|from| (from.clip, from.elapsed, from.weight)),
        &model.bone_offsets,
        model.secondary,
        &model.bone_translations,
        &model.morph_weights,
    )
}

fn primitive_vertex_layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    const VERTEX: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
    const INSTANCE: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        2 => Float32x4, 3 => Float32x4, 4 => Float32x4,
        5 => Float32x4, 6 => Float32x4, 7 => Float32x4
    ];
    [
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX,
        },
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE,
        },
    ]
}

fn model_vertex_layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    const VERTEX: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4
    ];
    const INSTANCE: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
        4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
        7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4,
        11 => Float32x4, 12 => Float32x4, 13 => Float32x4
    ];
    [
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ModelVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX,
        },
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ModelInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE,
        },
    ]
}

fn color_samples(samples: u16) -> wgpu::MultisampleState {
    wgpu::MultisampleState {
        count: u32::from(samples),
        ..Default::default()
    }
}

fn shadow_depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: SHADOW_DEPTH_FORMAT,
        depth_write_enabled: true,
        depth_compare: wgpu::CompareFunction::Less,
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState {
            constant: 3,
            slope_scale: 1.9,
            clamp: 0.0,
        },
    }
}

struct ShadowRenderer {
    resolution: u32,
    primitive_pipeline: wgpu::RenderPipeline,
    model_pipeline: wgpu::RenderPipeline,
    view: wgpu::TextureView,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    cascades: Option<FarCascade>,
}

struct FarCascade {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl ShadowRenderer {
    fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        pipeline_layout: &wgpu::PipelineLayout,
        resolution: u32,
    ) -> Self {
        let depth_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Directional shadow bindings layout"),
            entries: &[
                depth_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                depth_entry(2),
            ],
        });
        let view = shadow_view(device, resolution, "Directional shadow depth map");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Directional shadow PCF comparison"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let bind_group = directional_shadow_bind_group(device, &layout, &sampler, &view, &view);
        let pipeline = |entry_point, buffers: &[wgpu::VertexBufferLayout<'_>], cull_mode| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Directional shadow caster pipeline"),
                layout: Some(pipeline_layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(entry_point),
                    buffers,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: None,
                primitive: wgpu::PrimitiveState {
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(shadow_depth_state()),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        Self {
            resolution,
            primitive_pipeline: pipeline(
                "vs_shadow",
                &primitive_vertex_layouts(),
                Some(wgpu::Face::Back),
            ),
            model_pipeline: pipeline("vs_model_shadow", &model_vertex_layouts(), None),
            view,
            layout,
            bind_group,
            sampler,
            cascades: None,
        }
    }

    fn ensure_cascades(&mut self, device: &wgpu::Device) {
        self.cascades.get_or_insert_with(|| {
            let view = shadow_view(device, self.resolution, "Far directional shadow cascade");
            let bind_group = directional_shadow_bind_group(
                device,
                &self.layout,
                &self.sampler,
                &self.view,
                &view,
            );
            FarCascade { view, bind_group }
        });
    }

    fn bindings(&self, quality: ShadowQuality) -> &wgpu::BindGroup {
        self.cascades
            .as_ref()
            .filter(|_| quality == ShadowQuality::Cascaded)
            .map_or(&self.bind_group, |far| &far.bind_group)
    }
}

fn shadow_view(device: &wgpu::Device, resolution: u32, label: &str) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: resolution,
                height: resolution,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn directional_shadow_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    near: &wgpu::TextureView,
    far: &wgpu::TextureView,
) -> wgpu::BindGroup {
    use wgpu::BindingResource;
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Directional shadow bindings"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(near),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: BindingResource::TextureView(far),
            },
        ],
    })
}

impl Renderer {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        output: wgpu::TextureFormat,
        samples: u16,
    ) -> Self {
        let format = HDR;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Robot lighting"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("robot.wgsl"),
                    "\n",
                    include_str!("model.wgsl"),
                    "\n",
                    include_str!("natural.wgsl"),
                    "\n",
                    include_str!("effects.wgsl")
                )
                .into(),
            ),
        });
        let camera_buffer = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<ViewUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let view_buffer = camera_buffer("Robot viewport camera and floor shadow");
        let far_view_buffer = camera_buffer("Far sun cascade camera");
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Robot viewport uniform layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_group = |label, buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &view_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let view_bind_group = camera_group("Robot viewport uniform bindings", &view_buffer);
        let far_view_bind_group = camera_group("Far sun cascade camera bindings", &far_view_buffer);
        let view_only = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Robot sky and shadow caster layout"),
            bind_group_layouts: &[&view_layout],
            push_constant_ranges: &[],
        });
        let shadow = ShadowRenderer::new(device, &shader, &view_only, shadow_size(samples));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Robot viewport pipeline layout"),
            bind_group_layouts: &[&view_layout, &shadow.layout],
            push_constant_ranges: &[],
        });
        let water_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<WaterInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4],
        };
        let water_pipelines = BlendMode::ALL.map(|mode| {
            overlay_pipeline(
                device,
                &shader,
                &layout,
                format,
                samples,
                ("vs_water", "fs_water"),
                std::slice::from_ref(&water_layout),
                wgpu::CompareFunction::LessEqual,
                mode,
            )
        });
        let rain_pipeline = overlay_pipeline(
            device,
            &shader,
            &view_only,
            format,
            samples,
            ("vs_sky", "fs_rain"),
            &[],
            wgpu::CompareFunction::Always,
            BlendMode::Normal,
        );
        let particle_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ParticleInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4],
        };
        let particle_pipeline = overlay_pipeline(
            device,
            &shader,
            &view_only,
            format,
            samples,
            ("vs_particle", "fs_particle"),
            &[particle_layout],
            wgpu::CompareFunction::LessEqual,
            BlendMode::Normal,
        );
        let color_targets = [Some(format.into())];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Robot viewport pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &primitive_vertex_layouts(),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &color_targets,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: color_samples(samples),
            multiview: None,
            cache: None,
        });
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Robot viewport sky"),
            layout: Some(&view_only),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sky"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sky"),
                targets: &color_targets,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: color_samples(samples),
            multiview: None,
            cache: None,
        });

        let mut vertices = Vec::new();
        let mesh_ranges = [
            cube_vertices,
            sphere_vertices,
            cylinder_vertices,
            spark_vertices,
        ]
        .map(|build| {
            let start = vertices.len() as u32;
            build(&mut vertices);
            start..vertices.len() as u32
        });
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Robot viewport primitive meshes"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self {
            pipeline,
            sky_pipeline,
            water_pipelines,
            rain_pipeline,
            particle_pipeline,
            vertices,
            mesh_ranges,
            instances: instance_buffer(device, 8192),
            water_instances: instance_buffer(
                device,
                (MAX_ZONES * std::mem::size_of::<WaterInstance>()) as u64,
            ),
            water_grid: if samples < 2 { 8 } else { 18 },
            particle_instances: instance_buffer(
                device,
                (effects::MAX_PARTICLES * std::mem::size_of::<ParticleInstance>()) as u64,
            ),
            view_buffer,
            view_bind_group,
            far_view_buffer,
            far_view_bind_group,
            post: PostRenderer::new(device, output, samples),
            models: ModelRenderer::new(
                device,
                queue,
                &shader,
                &view_layout,
                &shadow.layout,
                format,
                samples,
            ),
            shadow,
        }
    }

    fn render_shadow(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        camera: &wgpu::BindGroup,
        ranges: &[Range<u32>; MESH_COUNT],
        models: &[PreparedModel],
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Directional shadow depth pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.shadow.primitive_pipeline);
        pass.set_bind_group(0, camera, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instances.slice(..));
        for (mesh, instances) in self.mesh_ranges.iter().zip(ranges) {
            if !instances.is_empty() {
                pass.draw(mesh.clone(), instances.clone());
            }
        }
        self.models
            .paint(&mut pass, models, Some(&self.shadow.model_pipeline));
    }
}

#[allow(clippy::too_many_arguments)]
fn overlay_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    samples: u16,
    (vertex, fragment): (&str, &str),
    buffers: &[wgpu::VertexBufferLayout<'_>],
    depth_compare: wgpu::CompareFunction,
    blend: BlendMode,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex),
            buffers,
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(transparent_blend(blend)),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: false,
            depth_compare,
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: color_samples(samples),
        multiview: None,
        cache: None,
    })
}

fn push_vertex(vertices: &mut Vec<Vertex>, position: Vec3, normal: Vec3) {
    vertices.push(Vertex {
        position: position.to_array(),
        normal: normal.to_array(),
    });
}

fn cube_vertices(vertices: &mut Vec<Vertex>) {
    for normal in [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
    ] {
        let u = if normal.y != 0.0 {
            Vec3::X
        } else {
            Vec3::Y.cross(normal)
        };
        let v = normal.cross(u);
        let a = (normal - u - v) * 0.5;
        let b = (normal + u - v) * 0.5;
        let c = (normal - u + v) * 0.5;
        let d = (normal + u + v) * 0.5;
        for position in [a, b, c, b, d, c] {
            push_vertex(vertices, position, normal);
        }
    }
}

fn sphere_vertices(vertices: &mut Vec<Vertex>) {
    const LATITUDE_BANDS: u32 = 10;
    const LONGITUDE_BANDS: u32 = 16;
    let normal_at = |band: u32, wedge: u32| {
        let latitude = std::f32::consts::PI * (band as f32 / LATITUDE_BANDS as f32 - 0.5);
        let longitude = std::f32::consts::TAU * wedge as f32 / LONGITUDE_BANDS as f32;
        let (sin_lat, cos_lat) = latitude.sin_cos();
        let (sin_lon, cos_lon) = longitude.sin_cos();
        Vec3::new(cos_lat * sin_lon, sin_lat, cos_lat * cos_lon)
    };
    for band in 0..LATITUDE_BANDS {
        for wedge in 0..LONGITUDE_BANDS {
            let a = normal_at(band, wedge);
            let b = normal_at(band, wedge + 1);
            let c = normal_at(band + 1, wedge);
            let d = normal_at(band + 1, wedge + 1);
            for normal in [a, b, c, b, d, c] {
                push_vertex(vertices, normal * 0.5, normal);
            }
        }
    }
}

fn cylinder_vertices(vertices: &mut Vec<Vertex>) {
    const SIDES: u32 = 20;
    let rim_at = |wedge: u32| {
        let angle = std::f32::consts::TAU * wedge as f32 / SIDES as f32;
        let (sin, cos) = angle.sin_cos();
        Vec3::new(sin, 0.0, cos)
    };
    for wedge in 0..SIDES {
        let n0 = rim_at(wedge);
        let n1 = rim_at(wedge + 1);
        let top = Vec3::Y * 0.5;
        let bottom = -top;
        let a = n0 * 0.5 + bottom;
        let b = n1 * 0.5 + bottom;
        let c = n0 * 0.5 + top;
        let d = n1 * 0.5 + top;
        for (position, normal) in [(a, n0), (b, n1), (c, n0), (b, n1), (d, n1), (c, n0)] {
            push_vertex(vertices, position, normal);
        }
        for position in [top, c, d] {
            push_vertex(vertices, position, Vec3::Y);
        }
        for position in [bottom, b, a] {
            push_vertex(vertices, position, Vec3::NEG_Y);
        }
    }
}

fn spark_vertices(vertices: &mut Vec<Vertex>) {
    let belt = [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z].map(|point| point * 0.5);
    for index in 0..belt.len() {
        let a = belt[index];
        let b = belt[(index + 1) % belt.len()];
        for face in [[Vec3::Y * 0.5, b, a], [Vec3::NEG_Y * 0.5, a, b]] {
            let normal = (face[1] - face[0]).cross(face[2] - face[0]).normalize();
            for point in face {
                push_vertex(vertices, point, normal);
            }
        }
    }
}
