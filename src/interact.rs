use std::collections::{BTreeSet, HashMap, HashSet};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::scene::{PLAYER_HEIGHT, PLAYER_RADIUS, Part, PartMotion, Shape};

use super::{Collider, KeyPickup, MAX_KEY_PICKUPS, World};

const DOOR_THICKNESS: f32 = 0.075;
const DOOR_TRAVEL_SECONDS: f32 = 0.48;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoorSettings {
    pub initially_open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_key: Option<u32>,
    #[serde(skip_serializing_if = "is_false")]
    pub rule_lock: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub hand_contact: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum LockMode {
    Rule,
    Key(u32),
}

impl DoorSettings {
    fn lock(self) -> Option<LockMode> {
        if self.rule_lock {
            Some(LockMode::Rule)
        } else {
            self.required_key.map(LockMode::Key)
        }
    }
}

pub(crate) fn configure_door(
    ui: &mut egui::Ui,
    settings: &mut Option<DoorSettings>,
    keys: &[KeyPickup],
) -> bool {
    let mut present = settings.is_some();
    let mut changed = ui
        .checkbox(&mut present, "Дверная створка")
        .on_hover_text("В игре нажми E рядом с дверью, чтобы открыть или закрыть её.")
        .changed();
    if changed {
        *settings = present.then(DoorSettings::default);
    }
    if let Some(settings) = settings {
        changed |= ui.checkbox(&mut settings.hand_contact, "Открывать рукой")
            .on_hover_text("В 3D герой с совместимым скелетом подходит к ручке. Створка начинает движение после касания.")
            .changed();
        changed |= ui
            .checkbox(&mut settings.rule_lock, "Заперта до выполнения правила")
            .changed();
        if settings.rule_lock {
            ui.weak("В разделе «Правила» добавь действие «Отпереть дверь».");
        } else {
            changed |= crate::inspector::door_lock(ui, settings, keys);
            if settings.required_key.is_none() {
                changed |= ui
                    .checkbox(&mut settings.initially_open, "Открыта при запуске")
                    .changed();
            }
        }
    }
    changed
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seat {
    pub id: u64,
    /// Position of the top of the seat in world coordinates.
    pub position: [f32; 3],
    /// Clear standing position in front of or beside the seat.
    pub stand: [f32; 3],
    pub yaw: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interaction {
    Door { id: u64, opening: bool },
    LockedDoor { id: u64, key: u32 },
    RuleLockedDoor { id: u64 },
    Key { id: u32 },
    Seat(Seat),
}

impl Interaction {
    pub fn door_id(self) -> Option<u64> {
        match self {
            Self::Door { id, .. } | Self::LockedDoor { id, .. } | Self::RuleLockedDoor { id } => {
                Some(id)
            }
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct InteractionState {
    doors: HashMap<u64, DoorMotion>,
    keys: BTreeSet<u32>,
    disabled_objects: HashSet<u64>,
}

impl InteractionState {
    pub(crate) fn set_disabled_objects(&mut self, ids: impl Iterator<Item = u64>) {
        self.disabled_objects.clear();
        self.disabled_objects.extend(ids);
    }

    pub(crate) fn object_active(&self, id: u64) -> bool {
        !self.disabled_objects.contains(&id)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InteractionSnapshot {
    keys: Vec<u32>,
    doors: Vec<(u64, DoorMotion)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct DoorMotion {
    phase: f32,
    opening: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unlocked_with: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unlocked_by_rule: Option<LockMode>,
}

pub(crate) struct Door<'a> {
    pub id: u64,
    pub latch_id: u64,
    pub name: &'a str,
    pub hinge: [f32; 3],
    pub width: f32,
    pub height: f32,
    pub yaw: f32,
    pub settings: DoorSettings,
}

impl InteractionState {
    pub fn has_key(&self, id: u32) -> bool {
        self.keys.contains(&id)
    }

    pub fn key_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.keys.iter().copied()
    }

    pub(super) fn collect_key(&mut self, id: u32) {
        self.keys.insert(id);
    }

    pub fn snapshot(&self) -> InteractionSnapshot {
        let mut doors: Vec<_> = self
            .doors
            .iter()
            .map(|(&id, &motion)| (id, motion))
            .collect();
        doors.sort_unstable_by_key(|entry| entry.0);
        InteractionSnapshot {
            keys: self.key_ids().collect(),
            doors,
        }
    }

    pub fn restore(&mut self, world: &World, snapshot: &InteractionSnapshot) {
        self.keys = snapshot
            .keys
            .iter()
            .copied()
            .filter(|&id| world.key(id).is_some())
            .collect();
        self.doors = snapshot
            .doors
            .iter()
            .filter_map(|&(id, mut motion)| {
                let door = world.door(id)?;
                let lock = door.settings.lock();
                let key_unlock = match lock {
                    Some(LockMode::Key(key))
                        if motion.unlocked_with == Some(key) && self.has_key(key) =>
                    {
                        Some(key)
                    }
                    _ => None,
                };
                if !valid_motion(motion)
                    || (lock.is_some() && motion.unlocked_by_rule != lock && key_unlock.is_none())
                {
                    return None;
                }
                motion.unlocked_with = key_unlock;
                motion.unlocked_by_rule =
                    motion.unlocked_by_rule.filter(|&mode| Some(mode) == lock);
                Some((id, motion))
            })
            .collect();
    }

    pub(crate) fn locked(&self, door: &Door<'_>) -> bool {
        let Some(mode) = door.settings.lock() else {
            return false;
        };
        !self.doors.get(&door.id).is_some_and(|motion| {
            motion.unlocked_by_rule == Some(mode)
                || matches!(mode, LockMode::Key(key)
                if motion.unlocked_with == Some(key) && self.has_key(key))
        })
    }

    pub(super) fn unlock_by_rule(&mut self, door: &Door<'_>) -> bool {
        if !self.locked(door) {
            return false;
        }
        let mut motion = self.motion(door);
        motion.unlocked_by_rule = door.settings.lock();
        self.doors.insert(door.id, motion);
        true
    }

    pub(crate) fn next_opening(&self, door: &Door<'_>) -> bool {
        !self.motion(door).opening
    }

    pub(super) fn door_interaction(&self, door: &Door<'_>) -> Interaction {
        if self.locked(door) {
            match door.settings.lock() {
                Some(LockMode::Rule) => return Interaction::RuleLockedDoor { id: door.id },
                // Старые проекты сохранили прямую привязку ключа к замку.
                Some(LockMode::Key(key)) if !self.has_key(key) => {
                    return Interaction::LockedDoor { id: door.id, key };
                }
                _ => {}
            }
        }
        Interaction::Door { id: door.id, opening: self.next_opening(door) }
    }

    pub(crate) fn toggle(&mut self, door: &Door<'_>) {
        if !self.locked(door) {
            self.toggle_unlocked(door, false);
        }
    }

    pub(crate) fn toggle_player(&mut self, door: &Door<'_>) {
        if !self.locked(door)
            || matches!(door.settings.lock(), Some(LockMode::Key(key)) if self.has_key(key))
        {
            self.toggle_unlocked(door, true);
        }
    }

    fn toggle_unlocked(&mut self, door: &Door<'_>, player: bool) {
        let mut motion = self.motion(door);
        motion.opening = !motion.opening;
        if player
            && motion.opening
            && let Some(LockMode::Key(key)) = door.settings.lock()
            && self.has_key(key)
        {
            motion.unlocked_with = Some(key);
        }
        self.doors.insert(door.id, motion);
    }

    pub(crate) fn fraction(&self, door: &Door<'_>) -> f32 {
        eased(self.motion(door).phase)
    }

    pub(crate) fn advance(&mut self, door: &Door<'_>, seconds: f32, player: [f32; 3]) {
        if self.locked(door) {
            return;
        }
        let Some(motion) = self.doors.get_mut(&door.id) else {
            return;
        };
        let step = seconds.clamp(0.0, 0.075) / DOOR_TRAVEL_SECONDS;
        let phase = if motion.opening {
            (motion.phase + step).min(1.0)
        } else {
            (motion.phase - step).max(0.0)
        };
        if phase != motion.phase
            && (motion.opening
                || door.closing_path_is_clear(eased(motion.phase), eased(phase), player))
        {
            motion.phase = phase;
        }
    }

    fn motion(&self, door: &Door<'_>) -> DoorMotion {
        if self.locked(door) {
            return DoorMotion {
                phase: 0.0,
                opening: false,
                unlocked_with: None,
                unlocked_by_rule: None,
            };
        }
        let opening = door.settings.initially_open && door.settings.lock().is_none();
        self.doors.get(&door.id).copied().unwrap_or(DoorMotion {
            phase: f32::from(u8::from(opening)),
            opening,
            unlocked_with: None,
            unlocked_by_rule: None,
        })
    }
}

impl InteractionSnapshot {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let mut keys = HashSet::new();
        let mut doors = HashSet::new();
        if self.keys.len() > MAX_KEY_PICKUPS
            || self.keys.iter().any(|&id| id == 0 || !keys.insert(id))
            || self.doors.len() > 1024
            || self.doors.iter().any(|&(id, motion)| {
                id == 0
                    || !doors.insert(id)
                    || !valid_motion(motion)
                    || motion.unlocked_with.is_some_and(|key| !keys.contains(&key))
            })
        {
            return Err("Некорректное состояние инвентаря или дверей.".into());
        }
        Ok(())
    }
}

fn valid_motion(motion: DoorMotion) -> bool {
    (0.0..=1.0).contains(&motion.phase) && motion.unlocked_by_rule != Some(LockMode::Key(0))
}

fn eased(phase: f32) -> f32 {
    phase * phase * (3.0 - 2.0 * phase)
}

impl Door<'_> {
    pub(crate) fn lever(&self, fraction: f32, side: f32) -> glam::Vec3 {
        glam::Vec3::from_array(self.point(fraction, self.width - 0.11, 1.02, 0.0775 * side))
    }

    pub(crate) fn approach_side(&self, from: glam::Vec3, fraction: f32) -> f32 {
        let plane = glam::Vec3::from_array(self.hinge);
        (from - plane).dot(self.lever_normal(fraction, 1.0)).signum()
    }

    pub(crate) fn lever_normal(&self, fraction: f32, side: f32) -> glam::Vec3 {
        let (sin, cos) = (self.yaw + 90.0 * fraction).to_radians().sin_cos();
        side * glam::Vec3::new(sin, 0.0, cos)
    }
    pub(crate) fn interaction_position(&self) -> [f32; 3] {
        self.point(0.0, self.width * 0.5, 0.0, 0.0)
    }

    fn point(&self, fraction: f32, along: f32, y: f32, perpendicular: f32) -> [f32; 3] {
        let (sin, cos) = (self.yaw + 90.0 * fraction).to_radians().sin_cos();
        [
            self.hinge[0] + along * cos + perpendicular * sin,
            self.hinge[1] + y,
            self.hinge[2] - along * sin + perpendicular * cos,
        ]
    }

    pub(crate) fn parts(&self, fraction: f32, plan: bool) -> impl Iterator<Item = Part> {
        let make = |id, position, size, color| Part {
            id,
            name: self.name.into(),
            shape: Shape::Box,
            position,
            size,
            rotation: [0.0, self.yaw + fraction * 90.0, 0.0],
            color,
            motion: PartMotion::Static,
        };
        let leaf = make(
            self.id,
            self.point(fraction, self.width * 0.5, self.height * 0.5, 0.0),
            [self.width, self.height, DOOR_THICKNESS],
            [0.51, 0.31, 0.17],
        );
        let latch = (!plan).then(|| {
            make(
                self.latch_id,
                self.point(fraction, self.width - 0.11, 1.02, 0.0),
                [0.094, 0.047, 0.155],
                [0.71, 0.57, 0.33],
            )
        });
        [Some(leaf), latch].into_iter().flatten()
    }

    pub(crate) fn colliders(&self, fraction: f32) -> impl Iterator<Item = Collider> + '_ {
        // At intermediate angles short pieces bound the moving leaf without covering the entire doorway.
        let count = if fraction <= 0.001 || fraction >= 0.999 {
            1
        } else {
            6
        };
        let segment = self.width / count as f32;
        let angle = (self.yaw + fraction * 90.0).to_radians();
        let (sin, cos) = angle.sin_cos();
        let x_radius = cos.abs() * segment * 0.5 + sin.abs() * DOOR_THICKNESS * 0.5;
        let z_radius = sin.abs() * segment * 0.5 + cos.abs() * DOOR_THICKNESS * 0.5;
        (0..count).map(move |index| {
            let [x, y, z] = self.point(fraction, (index as f32 + 0.5) * segment, 0.0, 0.0);
            Collider {
                min: [x - x_radius, y, z - z_radius],
                max: [x + x_radius, y + self.height, z + z_radius],
            }
        })
    }

    fn obstructed(&self, fraction: f32, player: [f32; 3]) -> bool {
        self.colliders(fraction).any(|collider| {
            player[1] + PLAYER_HEIGHT > collider.min[1]
                && player[1] < collider.max[1]
                && [0, 2].into_iter().all(|axis| {
                    player[axis] + PLAYER_RADIUS > collider.min[axis]
                        && player[axis] - PLAYER_RADIUS < collider.max[axis]
                })
        })
    }

    fn closing_path_is_clear(&self, from: f32, to: f32, player: [f32; 3]) -> bool {
        let travel = (from - to) * std::f32::consts::FRAC_PI_2 * self.width;
        let steps = ((travel / 0.14).ceil() as u32).max(1);
        (1..=steps).all(|step| {
            let fraction = from + (to - from) * step as f32 / steps as f32;
            !self.obstructed(fraction, player)
        })
    }
}
