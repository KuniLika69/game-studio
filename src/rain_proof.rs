use std::{fs, path::Path};

use eframe::egui::{Key, Modifiers};
use serde::Serialize;
use serde_json::json;

use super::{Driver, FPS, SIZE, Sink, io_error, proof_input, quarter_proof, staged_proof};
use crate::{
    Motion, Studio, configure_context, game_export::GamePackage, offscreen::Offscreen,
    proof_video::Video, quality::Profile, rain_workshop, viewport3d,
};

#[derive(Serialize)]
struct Route {
    sprint_m_s: f32,
    step_height_m: f32,
    wet_frames: usize,
    door: Option<crate::studio_door_hand::ContactMeasure>,
    indoor_conversation: bool,
    frames_at_60_hz: usize,
}

pub(super) fn author(output: &Path, releases: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    fs::create_dir_all(releases).map_err(io_error)?;
    let staging = output.join("rain-export-stage");
    fs::create_dir_all(&staging).map_err(io_error)?;
    let scene = output.join("rain-workshop.robot.json");
    let staged = staging.join("RainWorkshop.exe");
    let mut display = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(display.context());
    let mut app = Studio::default();
    app.desk.recovery_root = Some(output.join("rain-recovery"));
    let mut video = Video::start(
        &output.join("rain-workshop-editor.mp4"),
        SIZE[0],
        SIZE[1],
        FPS,
    )?;
    {
        let mut sink = Sink::Video(&mut video);
        let mut d = Driver::new(app, &mut display, &mut sink, Some((&scene, &staged)));
        d.settle(6)?;
        if d.point("Попробовать пример", 0).is_ok() {
            d.click("Попробовать пример")?;
        } else {
            d.click("Новый…")?;
        }
        d.click(rain_workshop::NAME)?;
        d.settle(6)?;
        d.app.scene.validate()?;
        if d.app.scene.avatar.is_none() {
            return Err(d.app.status.clone());
        }
        d.hold(18)?;
        png(&d, &output.join("rain-workshop-editor.png"), SIZE)?;
        staged_proof::panel_click(&mut d, "Сохранить как…")?;
        staged_proof::reload(&mut d)?;
        staged_proof::panel_click(&mut d, "▶ Играть")?;
        play(&mut d, false, None, |_| Ok(()))?;
        staged_proof::panel_click(&mut d, "■ Стоп")?;
        staged_proof::panel_click(&mut d, "Собрать игру для Windows")?;
    }
    video.finish()?;
    let destination = releases.join("RainWorkshop.exe");
    if destination.exists() {
        return Err("Файл RainWorkshop.exe уже существует.".into());
    }
    fs::rename(staged, destination).map_err(io_error)?;
    legacy_fixture(&scene, &output.join("rain-workshop-legacy.robot.json"))?;
    Ok(())
}

fn legacy_fixture(project: &Path, older: &Path) -> Result<(), String> {
    let source = fs::read(project).map_err(io_error)?;
    let mut scene: serde_json::Value =
        serde_json::from_slice(&source).map_err(|e| e.to_string())?;
    scene["version"] = json!(19);
    for model in scene["world"]["models"].as_array_mut().unwrap() {
        model.as_object_mut().unwrap().remove("light");
    }
    for conversation in scene["story"]["conversations"].as_array_mut().unwrap() {
        for node in conversation["nodes"].as_array_mut().unwrap() {
            let node = node.as_object_mut().unwrap();
            node.remove("voice");
            node.remove("face");
        }
    }
    fs::write(
        older,
        serde_json::to_vec_pretty(&scene).map_err(|e| e.to_string())?,
    )
    .map_err(io_error)?;
    let stable_assets = project.with_extension("assets");
    let legacy_assets = older.with_extension("assets");
    fs::create_dir_all(&legacy_assets).map_err(io_error)?;
    for asset in fs::read_dir(stable_assets).map_err(io_error)? {
        let asset = asset.map_err(io_error)?;
        fs::copy(asset.path(), legacy_assets.join(asset.file_name())).map_err(io_error)?;
    }
    Ok(())
}

pub(super) fn player(
    package: GamePackage,
    output: &Path,
    profile: Profile,
    record: bool,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let size = match profile {
        Profile::High => SIZE,
        Profile::Low => [1920, 1080],
    };
    let samples = profile.samples();
    let mut display = Offscreen::new(size, samples)?;
    configure_context(display.context());
    let mut app = Studio::from_package(package, display.context());
    app.low_fx = profile == Profile::Low;
    let mut video = record
        .then(|| {
            Video::start(
                &output.join("rain-workshop-play-muted.mp4"),
                size[0],
                size[1],
                FPS,
            )
        })
        .transpose()?;
    if record {
        app.set_audio_recording(output.join("rain-workshop-stereo.wav"))?;
    }
    let mut sink = video.as_mut().map_or(Sink::None, Sink::Video);
    let mut d = Driver::new(app, &mut display, &mut sink, None);
    d.input = proof_input::Driver::new(size);
    d.settle(80)?;
    if record {
        png(&d, &output.join("rain-comparison-022.png"), size)?;
    }
    d.display.start_timing();
    let route = play(&mut d, true, record.then_some((output, size)), |_| Ok(()))?;
    let adapter = d.display.adapter_info().clone();
    let mut timings = d.display.stop_timing();
    timings.sort_by(f32::total_cmp);
    let p95 = timings[((timings.len() - 1) as f32 * 0.95).ceil() as usize];
    let audio = d.app.audio_recording_stats();
    if let Some(audio) = audio {
        if audio.dialogue < 1
            || audio.door_contacts != 1
            || audio.footsteps[crate::audio::Surface::Water as usize] == 0
            || audio.footsteps[crate::audio::Surface::Wood as usize] == 0
        {
            return Err(format!("Не все звуки прозвучали в EXE. {audio:?}"));
        }
        d.app.write_recorded_audio()?;
    }
    let suffix = if profile == Profile::High {
        "high"
    } else {
        "low"
    };
    let report = json!({
        "profile": profile.name(), "resolution": size, "msaa": samples,
        "shadow_map": viewport3d::shadow_size(samples), "gpu": adapter.name,
        "backend": format!("{:?}", adapter.backend), "driver": adapter.driver_info,
        "frames": timings.len(), "mean_ms": timings.iter().sum::<f32>() / timings.len() as f32,
        "p95_ms": p95, "route": route, "audio": audio,
        "method": "whole offscreen frame through WGPU sync and RGB readback; graphics present and FFmpeg excluded",
    });
    fs::write(
        output.join(format!("rain-workshop-{suffix}.json")),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(io_error)?;
    if let Some(video) = video {
        video.finish()?;
    }
    Ok(())
}

fn play(
    d: &mut Driver<'_, '_>,
    long: bool,
    shots: Option<(&Path, [u32; 2])>,
    intro: impl FnOnce(&mut Driver<'_, '_>) -> Result<(), String>,
) -> Result<Route, String> {
    let start_frame = d.phase;
    quarter_proof::turn(d, 0.0, 0.24)?;
    let mut wet_frames = 0;
    let start = d.app.game_position();
    hold_move(d, Key::W, 34, Modifiers::NONE, &mut wet_frames)?;
    if d.app.game_position().distance(start) < 0.25 || d.app.avatar_motion != Motion::Walk {
        return Err("Старт и ходьба героя не воспроизвелись.".into());
    }
    hold_move(d, Key::D, 36, Modifiers::NONE, &mut wet_frames)?;
    let heading_90 = d.app.avatar_heading;
    hold_move(d, Key::A, 42, Modifiers::NONE, &mut wet_frames)?;
    let heading_180 = d.app.avatar_heading;
    if (heading_delta(heading_90, heading_180).abs() - 180.0).abs() > 30.0 {
        return Err(format!(
            "Разворот не достиг 180° ({heading_90} → {heading_180})."
        ));
    }
    d.settle(28)?;
    if d.app.game.horizontal_speed() >= 0.12 {
        return Err("При отпускании клавиш герой не остановился.".into());
    }

    quarter_proof::walk(d, [-0.55, -1.58])?;
    d.key(Key::E)?;
    require_speaker(d)?;
    intro(d)?;
    if long {
        d.settle(145)?;
        if let Some((folder, size)) = shots {
            png(d, &folder.join("rain-dialogue-street-normal.png"), size)?;
        }
        d.key(Key::F8)?;
        if !d.app.dialogue_closeup {
            return Err("Крупный план разговора не включился.".into());
        }
        d.settle(155)?;
        if let Some((folder, size)) = shots {
            png(d, &folder.join("rain-dialogue-street.png"), size)?;
        }
        d.settle(402)?;
    } else {
        d.settle(14)?;
    }
    live_click(d, rain_workshop::ACCEPT)?;
    require_stage(d, 1)?;
    quarter_proof::walk(d, [0.0, -2.87])?;
    quarter_proof::turn(d, 0.0, 0.24)?;
    let run_start = d.app.game_position();
    let mut run_speed: f32 = 0.0;
    d.input.set_modifiers(Modifiers::SHIFT);
    d.frame(vec![proof_input::key_event(Key::W, Modifiers::SHIFT, true)])?;
    for _ in 0..90 {
        d.frame(vec![])?;
        run_speed = run_speed.max(d.app.game.horizontal_speed());
        if !d.app.environment_splashes.is_empty() {
            wet_frames += 1;
            if let Some((folder, size)) = shots
                && wet_frames == 5
            {
                png(d, &folder.join("rain-splash.png"), size)?;
            }
        }
        if d.app.game_position().z <= -5.45 {
            break;
        }
    }
    release(d, Key::W)?;
    if run_speed < 2.4 || wet_frames == 0 || d.app.game_position().distance(run_start) < 1.0 {
        return Err(format!(
            "Бег или брызги не проявились. Скорость {run_speed:.2}, кадров с водой {wet_frames}. Позиция {:?}",
            d.app.game_position()
        ));
    }
    d.settle(24)?;
    quarter_proof::walk(d, [0.36, -6.50])?;
    let step_height = d.app.game_position().y;
    if !(0.055..=0.105).contains(&step_height) {
        return Err(format!(
            "Герой не поднялся на бордюр. Высота {step_height:.3}."
        ));
    }
    if long {
        quarter_proof::turn(d, 0.68, 0.19)?;
    }
    d.key(Key::E)?;
    let uses_hand_contact = d.app.scene.world.buildings.first()
        .and_then(|building| building.door)
        .is_some_and(|door| door.hand_contact);
    let contact = if uses_hand_contact {
        for _ in 0..240 {
            d.frame(vec![])?;
            if d.app.door_contact.is_some() {
                break;
            }
        }
        let contact = d.app.door_contact.ok_or_else(|| {
            format!(
                "Герой не коснулся ручки. Положение {:?}, сообщение {:?}.",
                d.app.game_position(),
                d.app.interaction_message
            )
        })?;
        if contact.gap_m > 0.020 || contact.leaf_fraction_at_contact != 0.0 {
            return Err(format!(
                "Створка движется до контакта или есть зазор. {contact:?}"
            ));
        }
        Some(contact)
    } else {
        None
    };
    if let Some((folder, size)) = shots {
        png(d, &folder.join("rain-door-contact.png"), size)?;
    }
    d.settle(46)?;
    if long {
        quarter_proof::turn(d, 0.0, 0.24)?;
    }
    quarter_proof::walk(d, [0.08, -8.24])?;
    require_stage(d, 2)?;
    quarter_proof::walk(d, [-0.59, -11.58])?;
    d.key(Key::F)?;
    require_stage(d, 3)?;
    if let Some((folder, size)) = shots {
        png(d, &folder.join("rain-interior.png"), size)?;
    }
    let mut inside_conversation = false;
    if long {
        // Лида проходит через тот же физический дверной проём по обычному маршруту NPC.
        quarter_proof::walk(d, [0.25, -10.97])?;
        for _ in 0..86 {
            if d.app
                .npc_position(rain_workshop::NPC_ID)
                .is_some_and(|p| p.z < -9.8)
            {
                break;
            }
            d.settle(30)?;
        }
        if d.app
            .npc_position(rain_workshop::NPC_ID)
            .is_some_and(|p| p.z < -9.8)
        {
            d.key(Key::E)?;
            require_speaker(d)?;
            inside_conversation = true;
            d.settle(145)?;
            d.key(Key::F8)?;
            d.settle(171)?;
            if let Some((folder, size)) = shots {
                png(d, &folder.join("rain-dialogue-interior.png"), size)?;
            }
            d.settle(382)?;
            live_click(d, rain_workshop::DECLINE)?;
        }
        live_click(d, "Свободная камера")?;
        if !d.app.free_camera {
            return Err("Свободная камера в игре не включилась.".into());
        }
        quarter_proof::turn(d, 2.88, 0.12)?;
        hold_move(d, Key::PageUp, 35, Modifiers::NONE, &mut wet_frames)?;
        d.settle(160)?;
        live_click(d, "Свободная камера")?;
        quarter_proof::turn(d, 0.78, 0.25)?;
        let elapsed = d.phase.saturating_sub(start_frame);
        if elapsed < 4260 {
            d.settle(4260 - elapsed)?;
        }
    }
    Ok(Route {
        sprint_m_s: run_speed,
        step_height_m: step_height,
        wet_frames,
        door: contact,
        indoor_conversation: inside_conversation,
        frames_at_60_hz: d.phase.saturating_sub(start_frame),
    })
}

fn heading_delta(a: f32, b: f32) -> f32 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

pub(super) fn play_short(d: &mut Driver<'_, '_>) -> Result<(), String> {
    play(d, false, None, |_| Ok(())).map(|_| ())
}

pub(super) fn play_with_intro(
    d: &mut Driver<'_, '_>,
    intro: impl FnOnce(&mut Driver<'_, '_>) -> Result<(), String>,
) -> Result<(), String> {
    play(d, false, None, intro).map(|_| ())
}

fn require_speaker(d: &Driver<'_, '_>) -> Result<(), String> {
    if d.app.story_session.active_npc() == Some(rain_workshop::NPC_ID) {
        Ok(())
    } else {
        Err(format!(
            "Не открыт разговор с Лидой. Герой {:?}, Лида {:?}.",
            d.app.game_position(),
            d.app.npc_position(rain_workshop::NPC_ID)
        ))
    }
}

fn require_stage(d: &Driver<'_, '_>, expected: u16) -> Result<(), String> {
    let quest = d.app.scene.story.quests[0].id;
    let stage = d.app.rule_session.quest_stage(quest);
    if stage == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "У мастерской ожидался этап {expected}, получен {stage:?}. Положение {:?}.",
            d.app.game_position()
        ))
    }
}

fn home_key(key: Key, modifiers: Modifiers, pressed: bool) -> Vec<eframe::egui::Event> {
    vec![proof_input::key_event(key, modifiers, pressed)]
}

fn hold_move(
    d: &mut Driver<'_, '_>,
    key: Key,
    frames: usize,
    modifiers: Modifiers,
    wet: &mut usize,
) -> Result<(), String> {
    d.input.set_modifiers(modifiers);
    d.frame(home_key(key, modifiers, true))?;
    for _ in 1..frames {
        d.frame(vec![])?;
        *wet += usize::from(!d.app.environment_splashes.is_empty());
    }
    release(d, key)
}

fn release(d: &mut Driver<'_, '_>, key: Key) -> Result<(), String> {
    d.input.set_modifiers(Modifiers::NONE);
    d.frame(home_key(key, Modifiers::NONE, false))
}

fn live_click(d: &mut Driver<'_, '_>, text: &str) -> Result<(), String> {
    let point = d.point(text, 0)?;
    d.frame(proof_input::click(point))?;
    d.settle(4)
}

fn png(d: &Driver<'_, '_>, path: &Path, [w, h]: [u32; 2]) -> Result<(), String> {
    image::save_buffer(path, &d.last, w, h, image::ColorType::Rgb8).map_err(|e| e.to_string())
}
