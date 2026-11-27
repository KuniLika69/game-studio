//! A short scripted capture of the packaged actor at the same door from two viewpoints.
use std::{fs, path::Path};

use eframe::egui::{Key, Modifiers};
use glam::Vec3;
use serde_json::json;

use super::{Driver, FPS, SIZE, Sink, io_error, proof_input};
use crate::{Studio, configure_context, game_export::GamePackage, offscreen::Offscreen, proof_video::Video, viewport3d};

pub(super) fn record(package: GamePackage, output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut render = Offscreen::new(SIZE, viewport3d::VIEWPORT_MSAA)?;
    configure_context(render.context());
    let mut app = Studio::from_package(package, render.context());
    app.set_audio_recording(output.join("actor-audition.wav"))?;
    let mut video = Video::start(&output.join("actor-motion-native.mp4"), SIZE[0], SIZE[1], FPS)?;
    let mut sink = Sink::Video(&mut video);
    let mut d = Driver::new(app, &mut render, &mut sink, None);
    d.settle(6)?;
    let mut contacts = Vec::new();
    for (index, angle) in [0.62_f32, 1.36].into_iter().enumerate() {
        d.app.start_play(d.display.context(), false);
        let door_id = position_at_door(&mut d, angle)?;
        d.settle(50)?;
        let action_seconds = d.phase as f64 / 60.0;
        d.key(Key::E)?;
        let mut checked_closed = 0_u32;
        let mut measured = None;
        for _ in 0..150 {
            d.frame(Vec::new())?;
            let door = d.app.scene.world.door(door_id).ok_or("Створка удалена.")?;
            let fraction = d.app.interactions.fraction(&door);
            if let Some(sample) = d.app.door_contact {
                if measured.is_none() {
                    measured = Some(sample);
                }
                if fraction == 0.0 && checked_closed == 4 {
                    screenshot(&d, &output.join(format!("grip-angle-{}.png", index + 1)))?;
                }
                if fraction > 0.0 {
                    if checked_closed < 4 || sample.pressure_seconds.is_none_or(|delay| delay < 0.075)
                        || sample.index_tip.is_none() || sample.thumb_tip.is_none() {
                        return Err("Створка не ждала движения пальцев после контакта.".into());
                    }
                    contacts.push(json!({
                        "angle":angle, "contact":sample, "closed_frames_after_palm":checked_closed,
                        "action_seconds":action_seconds, "swing_seconds":d.phase as f64 / 60.0
                    }));
                    break;
                }
                checked_closed += 1;
            } else if fraction > 0.0 {
                return Err("Створка начала движение без контакта.".into());
            }
        }
        if contacts.len() != index + 1 {
            return Err(format!("Рука не привела створку в движение. Позиция {:?}, последние данные {measured:?}.", d.app.actor_position()));
        }
        d.settle(52)?;
    }
    d.app.start_play(d.display.context(), false);
    let tuning = d.app.scene.avatar.as_ref().ok_or("Нет героя.")?.actor_tuning;
    d.app.camera.yaw = 0.45;
    d.app.camera.pitch = 0.03;
    d.app.camera.distance = 1.98;
    d.app.scene.play_settings.camera.distance = Some(1.98);
    d.settle(226)?;
    screenshot(&d, &output.join("breath-idle.png"))?;
    let before = d.app.player_secondary.breath(tuning);
    let breathing_rest_seconds = d.phase as f64 / 60.0;
    d.input.set_modifiers(Modifiers::SHIFT);
    d.frame(vec![proof_input::key_event(Key::D, Modifiers::SHIFT, true)])?;
    let mut peak_speed = 0.0_f32;
    for _ in 0..87 {
        d.frame(Vec::new())?;
        peak_speed = peak_speed.max(d.app.game.horizontal_speed());
    }
    d.input.set_modifiers(Modifiers::NONE);
    d.frame(vec![proof_input::key_event(Key::D, Modifiers::NONE, false)])?;
    d.settle(24)?;
    let recovering = d.app.player_secondary.breath(tuning);
    let breathing_recovery_seconds = d.phase as f64 / 60.0;
    screenshot(&d, &output.join("breath-recovery.png"))?;
    d.settle(180)?;
    let after = d.app.player_secondary.breath(tuning);
    if peak_speed < 2.4 || recovering.exertion <= before.exertion + 0.25 || after.exertion >= recovering.exertion {
        return Err(format!("Не измерено восстановление дыхания. Бег {peak_speed:.3}, до {:?}, после {:?}", recovering, after));
    }
    let transfers = capture_transfers(&mut d, output)?;
    d.app.write_recorded_audio()?;
    let report = json!({
        "two_angles":contacts,
        "peak_speed_m_s":peak_speed,
        "exertion_rest":before.exertion,
        "exertion_after_stop":recovering.exertion,
        "exertion_recovery":after.exertion,
        "breathing_rest_seconds":breathing_rest_seconds,
        "breathing_recovery_seconds":breathing_recovery_seconds,
        "keyboard_transfers":transfers,
        "audio":d.app.audio_recording_stats(),
        "fixture":"exported scene; proof positions the actor near the door and pre-authorizes it, then uses live E and ordinary game updates."
    });
    fs::write(output.join("actor-motion.json"), serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?).map_err(io_error)?;
    video.finish()?;
    Ok(())
}

fn capture_transfers(d: &mut Driver<'_, '_>, output: &Path) -> Result<Vec<serde_json::Value>, String> {
    use crate::actor_motion::TransferKind;

    d.app.start_play(d.display.context(), false);
    d.app.camera.yaw = std::f32::consts::PI;
    d.app.camera.pitch = 0.18;
    d.app.camera.distance = 3.7;
    d.app.scene.play_settings.camera.distance = Some(3.7);
    d.settle(55)?;

    let mut from = None;
    let mut transfers = Vec::new();
    for (key, expected, frames) in [
        (Some(Key::W), TransferKind::Start, 40),
        (Some(Key::D), TransferKind::QuarterTurn, 38),
        (Some(Key::A), TransferKind::HalfTurn, 43),
        (None, TransferKind::Brake, 42),
    ] {
        let action_seconds = d.phase as f64 / 60.0;
        let mut events = Vec::new();
        if let Some(old) = from {
            events.push(proof_input::key_event(old, Modifiers::NONE, false));
        }
        if let Some(new) = key {
            events.push(proof_input::key_event(new, Modifiers::NONE, true));
        }
        d.frame(events)?;
        let mut peak_speed = 0.0_f32;
        let mut captures = 0;
        let mut end_heading = 0.0;
        for _ in 0..frames {
            d.frame(Vec::new())?;
            peak_speed = peak_speed.max(d.app.game.horizontal_speed());
            end_heading = d.app.avatar_heading;
            if d.app.player_secondary.transfer.kind() == expected {
                captures += 1;
                if captures == 6 {
                    screenshot(d, &output.join(format!("gait-{expected:?}.png")))?;
                }
            }
        }
        if captures < 6 {
            return Err(format!("Движение {expected:?} проявилось только на {captures} кадрах."));
        }
        transfers.push(json!({
            "state":expected, "frames_in_state":captures, "peak_speed_m_s":peak_speed,
            "final_heading_degrees":end_heading, "action_seconds":action_seconds,
        }));
        from = key;
    }
    d.settle(24)?;
    Ok(transfers)
}

fn position_at_door(d: &mut Driver<'_, '_>, angle: f32) -> Result<u64, String> {
    let (id, at, normal) = d.app.scene.world.buildings.iter()
        .flat_map(crate::buildings::Building::doors)
        .find(|door| door.settings.hand_contact).map(|door| {
            (door.id, Vec3::from_array(door.interaction_position()), door.lever_normal(0.0,1.0))
        }).ok_or("В сцене отсутствует дверь с захватом.")?;
    d.app.scene.world.unlock_door_by_rule(&mut d.app.interactions, id);
    let stand = at + normal * 1.0;
    d.app.game.x = stand.x;
    d.app.game.z = stand.z;
    d.app.game.height = (at.y - 0.012).max(0.0);
    d.app.avatar_heading = 180.0 + normal.x.atan2(normal.z).to_degrees();
    d.app.game.yaw = d.app.avatar_heading;
    d.app.camera_follow = [stand.x, d.app.game.height * 0.42, stand.z].into();
    d.app.camera.yaw = angle + normal.x.atan2(normal.z);
    d.app.camera.pitch = 0.03;
    d.app.camera.distance = 1.87;
    d.app.scene.play_settings.camera.distance = Some(1.87);
    Ok(id)
}

fn screenshot(d: &Driver<'_, '_>, destination: &Path) -> Result<(), String> {
    image::save_buffer(destination, &d.last, SIZE[0], SIZE[1], image::ColorType::Rgb8).map_err(|e|e.to_string())
}
