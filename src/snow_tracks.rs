use std::collections::{HashMap, VecDeque};

use glam::{Vec2, Vec3};

use crate::model::PawContact;

pub(crate) const MAX_VISIBLE: usize = 16;
const HISTORY: usize = 96;
const MAX_AGE: f64 = 36.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RenderTrack {
    pub(crate) position_and_size: [f32; 4],
    pub(crate) direction_and_strength: [f32; 4],
}

struct Track {
    position: Vec3,
    direction: Vec2,
    size: f32,
    born: f64,
    strength: f32,
}

#[derive(Default)]
pub(super) struct Tracks {
    recent: VecDeque<Track>,
    paw_stance: HashMap<u64, [bool; 4]>,
    rigid_stance: [bool; 2],
}

impl Tracks {
    pub(super) fn update_rigid_boots(
        &mut self,
        feet: [(Vec3, f32, f32); 2],
        yaw: f32,
        height: f32,
        now: f64,
        snow: f32,
        clear: impl Fn(Vec3) -> bool,
    ) {
        for (index, (point, clearance, lift)) in feet.into_iter().enumerate() {
            if lift > height * 0.013 {
                self.rigid_stance[index] = false;
            }
            if !self.rigid_stance[index]
                && lift <= height * 0.0085
                && (-0.10..=0.046).contains(&clearance)
            {
                self.rigid_stance[index] = true;
                if clear(point) {
                    self.push_boot(point, yaw, height * 1.10, now, snow);
                }
            }
        }
    }

    pub(super) fn push_boot(&mut self, point: Vec3, yaw: f32, height: f32, now: f64, snow: f32) {
        self.push(point, yaw, (height * 0.090).clamp(0.075, 0.24), now, snow);
    }

    pub(super) fn update_paws(
        &mut self,
        id: u64,
        contacts: [Option<PawContact>; 4],
        yaw: f32,
        height: f32,
        now: f64,
        snow: f32,
        clear: impl Fn(Vec3) -> bool,
    ) {
        let mut stance = self.paw_stance.remove(&id).unwrap_or_default();
        for (held, contact) in stance.iter_mut().zip(contacts) {
            let Some(contact) = contact else {
                *held = false;
                continue;
            };
            if contact.stance < 0.50 {
                *held = false;
            }
            if !*held && contact.stance >= 0.86 && contact.solved_gap_m.abs() <= 0.047 {
                *held = true;
                let [x, _, z] = contact.solved_position_m;
                let point = Vec3::new(x, contact.ground_height_m, z);
                if clear(point) {
                    self.push(point, yaw, -(height * 0.075).clamp(0.047, 0.15), now, snow);
                }
            }
        }
        self.paw_stance.insert(id, stance);
    }

    pub(super) fn remove_actor(&mut self, id: u64) {
        self.paw_stance.remove(&id);
        if id == 0 {
            self.rigid_stance = [false; 2];
        }
    }

    fn push(&mut self, point: Vec3, yaw: f32, size: f32, now: f64, snow: f32) {
        self.recent.retain(|track| now - track.born < MAX_AGE);
        if self.recent.iter().rev().take(6).any(|track| {
            now - track.born < 0.28
                && track.position.distance_squared(point) < (size.abs() * 0.30).powi(2)
        }) {
            return;
        }
        if self.recent.len() == HISTORY {
            self.recent.pop_front();
        }
        let (sin, cos) = yaw.to_radians().sin_cos();
        self.recent.push_back(Track {
            position: point,
            direction: Vec2::new(sin, cos),
            size,
            born: now,
            strength: (0.56 + snow * 0.44).clamp(0.0, 1.0),
        });
    }

    pub(super) fn near(&self, focus: Vec3, now: f64) -> Vec<RenderTrack> {
        let mut visible = self
            .recent
            .iter()
            .filter_map(|track| {
                let age = now - track.born;
                let distance = (track.position - focus).length_squared();
                ((0.0..MAX_AGE).contains(&age) && distance < 22.0_f32.powi(2))
                    .then_some((track, age, distance))
            })
            .collect::<Vec<_>>();
        visible.sort_by(|a, b| a.2.total_cmp(&b.2).then_with(|| a.1.total_cmp(&b.1)));
        visible
            .into_iter()
            .take(MAX_VISIBLE)
            .map(|(track, age, _)| RenderTrack {
                position_and_size: track.position.extend(track.size).to_array(),
                direction_and_strength: [
                    track.direction.x,
                    track.direction.y,
                    track.strength
                        * (1.0 - ((age - 12.0) / (MAX_AGE - 12.0)).clamp(0.0, 1.0) as f32),
                    0.0,
                ],
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snow_track_history_caps_render_count_and_outlives_flurries() {
        let mut tracks = Tracks::default();
        for i in 0..64 {
            tracks.push_boot(Vec3::X * i as f32 * 0.2, 90.0, 1.5, i as f64 * 0.4, 0.8);
        }
        let marks = tracks.near(Vec3::X * 1.0, 30.0);
        assert_eq!(marks.len(), MAX_VISIBLE);
        assert_eq!(marks[0].position_and_size[0], 1.0);
        assert!(marks[0].direction_and_strength[0] > 0.99);
        assert!(marks[0].direction_and_strength[2] < 0.5);
        assert!(tracks.near(Vec3::ZERO, 100.0).is_empty());
    }

    #[test]
    fn yuna_snow_tracks_follow_the_imported_walk_clip() {
        let model =
            crate::model::decode_glb(include_bytes!("../demo_assets/chibi/yuna.glb")).unwrap();
        let clip = model
            .animations
            .iter()
            .position(|clip| clip.name == "Walk")
            .unwrap();
        let mut tracks = Tracks::default();
        for tick in 0..150 {
            let time = tick as f32 / 60.0;
            let boot_points = model
                .rigid_boot_contacts(Some(clip), time, None, &[])
                .unwrap();
            let marked = boot_points.map(|point| {
                let ground = model.bounds_min[1];
                let lift = (point.y - ground) * 1.3 / (model.bounds_max[1] - ground);
                (Vec3::new(point.x, ground, point.z + time), lift, lift)
            });
            tracks.update_rigid_boots(marked, 0.0, 1.3, f64::from(time), 0.7, |_| true);
        }
        assert!(
            (4..=14).contains(&tracks.recent.len()),
            "{}",
            tracks.recent.len()
        );
    }
}
