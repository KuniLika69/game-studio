use std::f32::consts::{PI, TAU};

use eframe::egui::{self, Event, Key, Modifiers, PointerButton};

use super::{Driver, Projection};
use crate::{
    body_physics::{BodyPose, SensorKind},
    proof_input,
    rules::{Event as RuleEvent, TraceOutcome},
    scene::PLAYER_RADIUS,
    simulation_rules::{Contact, Signal},
    story::Objective,
};

const CRATE: u64 = 1;
const PLATE: u64 = 3;
const FLOAT: u64 = 4;
const TITLE: &str = "Мельница у реки";
const RESULT: &str = "Выполнено · Мельница у реки";

pub(super) fn play(d: &mut Driver<'_, '_>, preview: bool) -> Result<(), String> {
    if preview && !d.app.game_running {
        d.click("▶ Играть")?;
    }
    if !d.app.game_running {
        return Err("Мельница не запущена.".into());
    }
    d.settle(6)?;

    let quest = d
        .app
        .scene
        .story
        .quests
        .iter()
        .find(|quest| quest.title == TITLE)
        .ok_or("У мельницы нет цели.")?;
    let Objective::Rules { completion_stage } = quest.objective else {
        return Err("Мельница должна завершаться правилами.".into());
    };
    let quest_id = quest.id;
    let gate = d
        .app
        .scene
        .wind_rotor
        .as_ref()
        .and_then(|rotor| rotor.enabled_by)
        .ok_or("Мельница должна запускаться через флаг.")?;
    let lamp = d
        .app
        .scene
        .world
        .objects
        .iter()
        .find(|object| object.name == "Лампа мельницы")
        .ok_or("У мельницы нет лампы окончания.")?
        .id;
    let (water, old_level) = d
        .app
        .scene
        .environment
        .as_ref()
        .and_then(|config| config.waters.first())
        .map(|zone| (zone.id, zone.level))
        .ok_or("В сцене нет канала.")?;
    let old_float = body(d, FLOAT)?;
    let start = d.app.game_position();

    if start.x.abs() > 0.1
        || start.z.abs() > 0.1
        || d.app.physics_runtime.plate_pressed(PLATE) != Some(false)
        || d.app.rule_session.flag(&d.app.scene.rules, gate) != Some(false)
        || d.app.rule_session.object_enabled(lamp)
        || d.app.rotor_angle.abs() > 0.0001
        || (water_level(d, water)? - old_level).abs() > 0.002
        || completed(d, quest_id, completion_stage)
    {
        return Err(format!("Некорректное начало мельницы. Игрок {start:?}."));
    }

    push_until_pressed(d)?;
    d.hold(15)?;

    let rotor_turn = RuleEvent::Simulation(Signal::RotorTurned);
    let mut finish_frame = 0;
    for frame in 1..=480 {
        d.settle(1)?;
        let raised = body(d, FLOAT)?.center[1] - old_float.center[1];
        if completed(d, quest_id, completion_stage) && raised > 0.20 {
            finish_frame = frame;
            break;
        }
    }
    let new_float = body(d, FLOAT)?;
    let new_level = water_level(d, water)?;
    if finish_frame == 0
        || (new_level - old_level) < 0.24
        || d.app.rule_session.flag(&d.app.scene.rules, gate) != Some(true)
        || !d.app.rule_session.object_enabled(lamp)
        || fired(
            d,
            RuleEvent::Simulation(Signal::WaterReached { zone: water }),
        ) == 0
        || fired(d, rotor_turn) != 1
    {
        return Err(format!(
            "Мельница не закончила цепочку. Цель {:?}, вода {old_level:.3} → {new_level:.3}, поплавок {:?} → {:?}, фаза ротора {:.3}, срабатываний {}.",
            d.app.rule_session.quest_stage(quest_id),
            old_float.center,
            new_float.center,
            d.app.rotor_angle,
            fired(d, rotor_turn)
        ));
    }
    d.expect_hud(RESULT)?;
    d.hold(36)?;
    eprintln!(
        "Мельница · итог через {finish_frame} кадров после плиты, вода y {old_level:.3} → {new_level:.3}, поплавок y {:.3} → {:.3}.",
        old_float.center[1], new_float.center[1]
    );
    Ok(())
}

pub(super) fn push_until_pressed(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let plate = d
        .app
        .scene
        .physics
        .sensors
        .iter()
        .find(|sensor| sensor.spec.id == PLATE)
        .filter(|sensor| matches!(sensor.spec.kind, SensorKind::Pressure { .. }))
        .ok_or("В сцене нет нужной нажимной плиты.")?
        .spec
        .bounds;
    let old_box = body(d, CRATE)?;
    align_camera(d)?;
    let right = Key::from_name(&d.app.scene.play_settings.controls.right.main)
        .ok_or("Нет клавиши движения вправо.")?;
    d.frame(vec![proof_input::key_event(right, Modifiers::NONE, true)])?;
    let mut contact_frame = 0;
    for frame in 1..=180 {
        if d.app.physics_runtime.plate_pressed(PLATE) == Some(true) {
            contact_frame = frame;
            break;
        }
        d.settle(1)?;
    }
    let at_contact = d.app.game_position();
    let box_at_contact = body(d, CRATE)?;
    d.frame(vec![proof_input::key_event(right, Modifiers::NONE, false)])?;
    let plate_rule = RuleEvent::Simulation(Signal::Contact {
        sensor_id: PLATE,
        state: Contact::Pressed,
    });
    if contact_frame == 0
        || at_contact.x + PLAYER_RADIUS >= plate.min[0] - 0.01
        || box_at_contact.center[0] - old_box.center[0] < 0.30
        || box_at_contact.bounds.max[0] < plate.min[0] - 0.01
        || fired(d, plate_rule) == 0
    {
        return Err(format!(
            "Ящик не активировал плиту самостоятельно. Игрок {at_contact:?}, ящик {:?} → {:?}, плита {:?}.",
            old_box.center, box_at_contact.center, plate
        ));
    }
    eprintln!(
        "Мельница · плита на кадре {contact_frame}, ящик x {:.3} → {:.3}.",
        old_box.center[0], box_at_contact.center[0]
    );
    Ok(())
}

fn body(d: &Driver<'_, '_>, id: u64) -> Result<BodyPose, String> {
    d.app
        .physics_runtime
        .body(id)
        .ok_or_else(|| format!("Физическое тело #{id} не запущено."))
}

fn water_level(d: &Driver<'_, '_>, water: u32) -> Result<f32, String> {
    d.app
        .scene
        .environment
        .as_ref()
        .zip(d.app.environment_session.as_ref())
        .and_then(|(config, session)| session.level(config, water))
        .ok_or_else(|| format!("Нет текущего уровня канала #{water}."))
}

fn completed(d: &Driver<'_, '_>, quest: u32, stage: u16) -> bool {
    d.app
        .rule_session
        .quest_stage(quest)
        .is_some_and(|current| current >= stage)
}

fn fired(d: &Driver<'_, '_>, event: RuleEvent) -> usize {
    d.app
        .rule_session
        .trace()
        .filter(|trace| trace.event == event && matches!(trace.outcome, TraceOutcome::Fired { .. }))
        .count()
}

fn align_camera(d: &mut Driver<'_, '_>) -> Result<(), String> {
    let rect = match d.app.proof.as_ref().and_then(|p| p.projection.as_ref()) {
        Some(Projection::World(_, rect)) => *rect,
        _ => return Err("Для проверки мельницы нужен вид 3D.".into()),
    };
    let offset =
        |d: &Driver<'_, '_>| (d.app.camera.yaw + d.app.camera_shoulder + PI).rem_euclid(TAU) - PI;
    let dx = offset(d) / 0.009;
    if dx.abs() >= 7.0 {
        let start = rect.center() - egui::vec2(dx * 0.5, 0.0);
        d.frame(proof_input::hover(start))?;
        d.frame(vec![proof_input::pointer_button(
            start,
            PointerButton::Primary,
            true,
        )])?;
        let steps = (dx.abs() / 26.0).ceil() as usize;
        for step in 1..=steps {
            let point = start + egui::vec2(dx * step as f32 / steps as f32, 0.0);
            d.frame(vec![Event::PointerMoved(point)])?;
        }
        d.frame(vec![proof_input::pointer_button(
            start + egui::vec2(dx, 0.0),
            PointerButton::Primary,
            false,
        )])?;
        d.settle(4)?;
    }
    let yaw = offset(d);
    if yaw.abs() > 0.14 {
        return Err(format!("Камера не выровнялась вдоль ящика. Угол {yaw:.3}."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Studio, configure_context, offscreen::Offscreen, viewport3d};

    #[test]
    #[ignore = "requires a local WGPU adapter for background rendering"]
    fn authored_mill_playthrough_through_real_input() -> Result<(), String> {
        let mut display = Offscreen::new(super::super::SIZE, viewport3d::VIEWPORT_MSAA)?;
        configure_context(display.context());
        let mut app = Studio::default();
        app.mill_demo(false);
        let mut sink = super::super::Sink::None;
        let mut driver = Driver::new(app, &mut display, &mut sink, None);
        driver.settle(4)?;
        play(&mut driver, true)
    }
}
