use crate::scene::{Part, PartMotion, Shape};
use eframe::egui;

#[derive(Clone)]
pub struct Profile {
    height: f32,
    shoulders: f32,
    skin: [f32; 3],
    shirt: [f32; 3],
    trousers: [f32; 3],
    hair: [f32; 3],
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            height: 1.90,
            shoulders: 0.53,
            skin: [0.71, 0.45, 0.32],
            shirt: [0.22, 0.50, 0.71],
            trousers: [0.23, 0.25, 0.35],
            hair: [0.24, 0.15, 0.11],
        }
    }
}

pub fn controls(ui: &mut egui::Ui, profile: &mut Profile) {
    ui.add(egui::Slider::new(&mut profile.height, 0.9..=3.0).text("Рост, м"));
    ui.add(egui::Slider::new(&mut profile.shoulders, 0.32..=0.92).text("Ширина плеч"));
    for (name, color) in [
        ("Кожа", &mut profile.skin),
        ("Куртка", &mut profile.shirt),
        ("Брюки", &mut profile.trousers),
        ("Волосы", &mut profile.hair),
    ] {
        ui.horizontal(|ui| {
            ui.color_edit_button_rgb(color);
            ui.label(name);
        });
    }
}

pub fn parts(profile: &Profile) -> Vec<Part> {
    use PartMotion::{LeftArm, LeftFoot, LeftLeg, RightArm, RightFoot, RightLeg, Static};
    use Shape::{Box, Cylinder, Sphere};

    let h = profile.height / 1.9;
    let w = profile.shoulders / 0.53;
    let dark = [0.12, 0.13, 0.15];
    let pale = [0.86, 0.87, 0.82];
    let mut parts = Vec::new();
    let mut add = |name: &str, shape, position: [f32; 3], size: [f32; 3], color, motion| {
        parts.push(Part {
            id: parts.len() as u64 + 1,
            name: name.into(),
            shape,
            position: [position[0] * w * h, position[1] * h, position[2] * h],
            rotation: [0.0; 3],
            size: [size[0] * w * h, size[1] * h, size[2] * h],
            color,
            motion,
        });
    };

    add(
        "Куртка",
        Sphere,
        [0.0, 1.27, 0.0],
        [0.52, 0.60, 0.31],
        profile.shirt,
        Static,
    );
    add(
        "Таз",
        Box,
        [0.0, 0.99, 0.0],
        [0.41, 0.20, 0.25],
        profile.trousers,
        Static,
    );
    add(
        "Шея",
        Cylinder,
        [0.0, 1.56, 0.0],
        [0.17, 0.18, 0.15],
        profile.skin,
        Static,
    );
    add(
        "Голова",
        Sphere,
        [0.0, 1.72, 0.027],
        [0.32, 0.34, 0.29],
        profile.skin,
        Static,
    );
    add(
        "Волосы",
        Sphere,
        [0.0, 1.812, -0.016],
        [0.33, 0.19, 0.28],
        profile.hair,
        Static,
    );
    add(
        "Нос",
        Sphere,
        [0.0, 1.68, 0.165],
        [0.057, 0.072, 0.069],
        profile.skin,
        Static,
    );
    for (side, arm, leg, foot, label) in [
        (-1.0, LeftArm, LeftLeg, LeftFoot, "Л"),
        (1.0, RightArm, RightLeg, RightFoot, "П"),
    ] {
        add(
            &format!("Глаз {label}"),
            Sphere,
            [side * 0.067, 1.732, 0.151],
            [0.065, 0.038, 0.035],
            pale,
            Static,
        );
        add(
            &format!("Зрачок {label}"),
            Sphere,
            [side * 0.064, 1.734, 0.169],
            [0.024, 0.03, 0.016],
            dark,
            Static,
        );
        add(
            &format!("Рукав {label}"),
            Cylinder,
            [side * 0.29, 1.275, 0.0],
            [0.16, 0.43, 0.16],
            profile.shirt,
            arm,
        );
        add(
            &format!("Кисть {label}"),
            Sphere,
            [side * 0.29, 1.031, 0.009],
            [0.126, 0.17, 0.12],
            profile.skin,
            arm,
        );
        add(
            &format!("Нога {label}"),
            Cylinder,
            [side * 0.117, 0.572, 0.0],
            [0.17, 0.79, 0.195],
            profile.trousers,
            leg,
        );
        add(
            &format!("Ботинок {label}"),
            Sphere,
            [side * 0.117, 0.107, 0.067],
            [0.19, 0.215, 0.38],
            dark,
            foot,
        );
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Game, Scene};
    use glam::Vec3;

    #[test]
    fn assembled_hand_moves_with_its_sleeve() {
        let mut scene = Scene::empty();
        scene.parts = parts(&Profile::default());
        let arm_distance = |parts: &[Part]| {
            let mut arm = parts
                .iter()
                .filter(|part| part.motion == PartMotion::LeftArm);
            let a = Vec3::from_array(arm.next().unwrap().position);
            let b = Vec3::from_array(arm.next().unwrap().position);
            a.distance(b)
        };
        let mut game = Game::default();
        game.phase = std::f32::consts::FRAC_PI_2;
        game.move_blend = 1.0;
        assert!(
            (arm_distance(&scene.parts) - arm_distance(&scene.sample_game(0.0, &game))).abs()
                < 1e-5
        );
    }
}
