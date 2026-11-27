use std::{collections::BTreeSet, fs, path::Path, sync::Arc};

use eframe::egui;
use glam::Vec3;
use serde_json::json;

use super::{io_error, proof_input};
use crate::{
    assets::Library, configure_context, material_graph,
    model::{Model, MorphWeight, SecondaryPose},
    offscreen::Offscreen,
    viewport3d::{self, Camera, ModelDraw, TransparentLayer, ViewportScene},
};

const SIZE: [u32; 2] = [1000, 1000];
const SCALE: f32 = 2.25;

pub(super) fn record(output: &Path, path: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut library = Library::default();
    let source = library.import(path)?;
    let model = &library.get(&source).ok_or("Импортированный VRM не найден.")?.model;
    let decals = model.primitives.iter().enumerate().filter_map(|(index, primitive)| {
        primitive.material.facial_decal.then_some(index)
    }).collect::<Vec<_>>();
    if decals.is_empty() {
        return Err("В VRM не найдено прозрачных слоёв лица.".into());
    }
    let morphs = [("Fcl_EYE_Close", 0.56), ("Fcl_ALL_Joy", 0.52)].into_iter().map(|(name, weight)| {
        if model.morph_names().iter().any(|target| target == name) {
            Ok(MorphWeight { target: name.into(), weight })
        } else {
            Err(format!("В VRM нет морфа {name}."))
        }
    }).collect::<Result<Vec<_>, _>>()?;
    let pose = model.pose_blended_with_morphs(None, 0.0, None, &[], SecondaryPose::default(), &[], &morphs)
        .ok_or("VRM не вернул геометрию морфов.")?;
    let moved = decals.iter().map(|&index| {
        let primitive = &model.primitives[index];
        primitive.indices.iter().copied().collect::<BTreeSet<_>>().into_iter().filter(|&vertex| {
            let vertex = vertex as usize;
            let start = Vec3::from_array(primitive.vertices[vertex].position);
            let end = Vec3::from_array(pose[index][vertex].position);
            start.distance_squared(end) > 1e-8
        }).count()
    }).sum::<usize>();
    if moved < 10 {
        return Err(format!("Моргание и улыбка сдвинули только {moved} вершин слоя лица."));
    }
    let (low, high) = decals.iter().flat_map(|&index| {
        let primitive = &model.primitives[index];
        primitive.indices.iter().map(|&vertex| &primitive.vertices[vertex as usize])
    }).map(|vertex| Vec3::from_array(vertex.position) * SCALE)
        .fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(low, high), point| {
            (low.min(point), high.max(point))
        });
    let camera = Camera { yaw: 0.0, pitch: 0.0, distance: 0.72,
        target: ((low + high) * 0.5 - Vec3::Y * 0.04).to_array() };
    let mut hidden = (**model).clone();
    let mut emissive = (**model).clone();
    for &index in &decals {
        hidden.primitives[index].material.base_color[3] = 0.0;
        let material = &mut emissive.primitives[index].material;
        material.emissive = [0.14, 0.28, 0.55];
        material.emissive_texture = None;
    }
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut input = proof_input::Driver::new(SIZE);
    let rest = frame(&mut display, &mut input, model, 16, camera, &[])?;
    display.measure_transparency();
    let visible = frame(&mut display, &mut input, model, 16, camera, &morphs)?;
    let samples = display.take_transparency_samples();
    let sorted = samples.last().ok_or("Вьюпорт не записал слои прозрачности.")?;
    if decals.iter().any(|&primitive| !sorted.order.contains(&TransparentLayer::Model { model: 0, primitive })) {
        return Err("В проходе прозрачности пропущена часть декалей VRM.".into());
    }
    let absent = frame(&mut display, &mut input, &Arc::new(hidden), 17, camera, &morphs)?;
    let lit = frame(&mut display, &mut input, &Arc::new(emissive), 18, camera, &morphs)?;
    let morph_pixels = differences(&rest, &visible);
    let decal_pixels = differences(&absent, &visible);
    let emission_pixels = differences(&visible, &lit);
    for (name, rgb) in [("rest", &rest), ("blink-smile", &visible), ("blink-smile-no-decals", &absent), ("blink-smile-emissive", &lit)] {
        image::save_buffer(output.join(format!("{name}.png")), rgb, SIZE[0], SIZE[1], image::ColorType::Rgb8)
            .map_err(|error| error.to_string())?;
    }
    let report = json!({
        "vrm": path.file_name().map(|name| name.to_string_lossy()),
        "gpu": display.adapter_info().name, "resolution": SIZE,
        "morphs": morphs, "moving_decal_vertices": moved, "decal_primitives": decals,
        "sorted_layers": sorted.order.iter().map(|layer| format!("{layer:?}")).collect::<Vec<_>>(),
        "sort_ms": sorted.ms, "significant_channel_delta": 3,
        "changed_rgb_pixels": {"morph": morph_pixels, "decal": decal_pixels, "emissive_variant": emission_pixels},
        "emissive_variant": {"linear_rgb": [0.14, 0.28, 0.55], "uses_authored_emissive_texture": false},
    });
    fs::write(output.join("vrm-decals.json"), report.to_string()).map_err(io_error)?;
    if morph_pixels < 64 || decal_pixels < 64 || emission_pixels < 64 {
        return Err(format!("Недостаточное отличие RGB: морф {morph_pixels}, декали {decal_pixels}, эмиссия {emission_pixels}."));
    }
    Ok(())
}

fn differences(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(3).zip(b.chunks_exact(3))
        .filter(|(a, b)| a.iter().zip(*b).any(|(&a, &b)| a.abs_diff(b) > 3)).count()
}

fn frame(
    display: &mut Offscreen, input: &mut proof_input::Driver, model: &Arc<Model>,
    token: u64, camera: Camera, morphs: &[MorphWeight],
) -> Result<Vec<u8>, String> {
    let draw = ModelDraw {
        token, model, position: [0.0; 3], rotation: [0.0; 3], scale: [SCALE; 3], selected: false,
        tuning: Default::default(), vegetation: [0.0; 4], receives_weather: false,
        graph: None, material_preview: material_graph::PreviewLight::Scene,
        clip: None, elapsed: 0.0, blend: None, bone_offsets: Vec::new(), bone_translations: Vec::new(),
        morph_weights: morphs.to_vec(), secondary: SecondaryPose::default(),
    };
    display.frame(input.raw(Vec::new()), |ctx| {
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
            viewport3d::paint(ui, ui.available_rect_before_wrap(), ViewportScene {
                robot: &[], environment: &[], architecture: &[], models: std::slice::from_ref(&draw),
                camera, selected: None, sparks: &[], glow: 0.0, playing: false,
                lighting: Default::default(), lights: &[], natural: None,
                effects: &[], post_process: Default::default(),
            });
        });
    })
}
