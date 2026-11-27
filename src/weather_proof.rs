use std::{fs, path::Path};

use eframe::egui::Key;
use serde_json::json;

use super::{Driver, Sink, io_error};
use crate::{
    Studio, configure_context,
    effect_rules::{Command, Origin},
    effects::Kind,
    environment::WeatherPreset,
    offscreen::Offscreen,
    play_settings::CameraMode,
    rain_workshop::{self, LIGHT_CAMERA, Style},
    rules::{Action, Event, GameAction, RuleSet},
};

const SIZE: [u32; 2] = [1920, 1080];

pub(super) fn record(output: &Path) -> Result<(), String> {
    fs::create_dir_all(output).map_err(io_error)?;
    let mut display = Offscreen::new(SIZE, 1)?;
    configure_context(display.context());
    let gpu = display.adapter_info().name.clone();
    let mut rows = Vec::new();
    for (name, preset, height, smoke, wind) in [
        ("fog-uniform", WeatherPreset::Fog, 0.0, false, false),
        ("fog-ground", WeatherPreset::Fog, 0.75, false, false),
        ("smoke-still", WeatherPreset::Clear, 0.0, true, false),
        ("smoke-wind", WeatherPreset::Clear, 0.0, true, true),
        ("rain", WeatherPreset::Rain, 0.0, false, false),
    ] {
        let mut studio = Studio {
            standalone: true,
            low_fx: true,
            ..Studio::default()
        };
        let mut scene = rain_workshop::create(&mut studio.assets, Style::BlueHour)?;
        scene.play_settings.camera.mode = CameraMode::Fixed;
        scene.rules = RuleSet::default();
        let environment = scene.environment.as_mut().ok_or("В сцене нет среды.")?;
        environment.weather.initial = preset;
        environment.weather.cues.clear();
        environment.time.start_hour = 16.0;
        environment.ground_mist_height = height;
        environment.winds.clear();
        if wind {
            let bounds = [0.0, 0.8, 0.0];
            environment
                .add_wind(bounds)
                .ok_or("Не создана зона ветра.")?;
            let zone = &mut environment.winds[0];
            zone.size = [30.0, 6.0, 30.0];
            zone.direction = [1.0, 0.0, 0.0];
            zone.speed = 8.0;
            zone.gust = 3.0;
        }
        if smoke {
            let effect = scene
                .effects
                .create(Kind::Smoke)
                .ok_or("Нет пресета дыма.")?;
            scene.rules.add_rule().ok_or("Нет правила дыма.")?;
            let rule = &mut scene.rules.rules[0];
            rule.name = "Запустить дым".into();
            rule.event = Some(Event::GameAction {
                action: GameAction::Mark,
            });
            rule.actions.push(Action::PlayEffect(Command {
                preset: effect,
                origin: Origin::Player,
            }));
        }
        let project = output.join(format!("{name}.robot.json"));
        studio
            .assets
            .save_files(crate::asset_sources(&scene), &project)?;
        scene.save(&project)?;
        studio.set_scene(crate::Scene::load(&project)?, Some(project), "");
        studio.begin_play(display.context(), false);
        studio.camera = LIGHT_CAMERA;
        let mut sink = Sink::None;
        let mut driver = Driver::new(studio, &mut display, &mut sink, None);
        driver.input = crate::proof_input::Driver::new(SIZE);
        driver.settle(20)?;
        if smoke {
            driver.key(Key::F)?;
        }
        driver.display.start_timing();
        driver.settle(55)?;
        let particles = driver.app.effect_particles();
        if smoke
            && !particles
                .iter()
                .any(|particle| particle.kind == Kind::Smoke)
        {
            return Err("Дым не запустился по правилу F.".into());
        }
        let position = driver.app.game_position();
        let air = driver
            .app
            .environment_frame(position)
            .ok_or("Нет погоды в игре.")?;
        if preset == WeatherPreset::Rain && (air.atmosphere.rain < 0.95 || air.water.is_empty()) {
            return Err("В дождевом тесте не запустились осадки или вода.".into());
        }
        if wind && air.wind.length() < 4.0 {
            return Err("Зона порыва не действует на игрока.".into());
        }
        image::save_buffer(
            output.join(format!("{name}.png")),
            &driver.last,
            SIZE[0],
            SIZE[1],
            image::ColorType::Rgb8,
        )
        .map_err(|error| error.to_string())?;
        rows.push(json!({
            "name":name, "weather":preset, "fog_density":air.atmosphere.fog_density,
            "mist_height_m":air.atmosphere.ground_mist_height, "rain":air.atmosphere.rain,
            "wind_m_per_s":air.wind.to_array(), "smoke_particles":particles.len(),
            "smoke_mean_x_offset_m": if particles.is_empty() { None } else {
                Some(particles.iter().map(|particle| particle.position.x - position.x).sum::<f32>() / particles.len() as f32)
            },
            "frame_ms":driver.display.stop_timing(),
        }));
    }
    let data =
        serde_json::to_vec_pretty(&json!({"adapter":gpu, "size":SIZE, "msaa":1, "scenes":rows}))
            .map_err(|error| error.to_string())?;
    fs::write(output.join("weather.json"), data).map_err(io_error)
}
