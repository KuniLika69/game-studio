use std::collections::{BTreeMap, HashSet};

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::{
    avatar::{Avatar, Motion},
    buildings::Building,
    scene::Game,
    world::{Collider, Door, InteractionState, World},
};

pub const FIRST_NPC_ID: u64 = 1 << 59;
const LAST_NPC_ID: u64 = FIRST_NPC_ID + (1 << 30) - 1;
pub const MAX_NPCS: usize = 24;
pub const MAX_WAYPOINTS: usize = 64;
pub const MOTION_TRANSITION_SECONDS: f32 = 0.19;
const WALK_SPEED: f32 = 3.15;
const WAYPOINT_RADIUS: f32 = 0.13;
const NAV_PADDING: f32 = 0.38;
const MAX_NEAR_OBSTACLES: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Waypoint {
    pub position: [f32; 3],
    #[serde(default)]
    pub wait: f32,
}

impl Waypoint {
    pub fn at(position: [f32; 3]) -> Self {
        Self {
            position,
            wait: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Npc {
    pub id: u64,
    pub avatar: Avatar,
    pub route: Vec<Waypoint>,
    pub speed: f32,
    #[serde(default)]
    pub decorative: bool,
}

impl Npc {
    pub fn new(id: u64, avatar: Avatar, route: Vec<Waypoint>) -> Self {
        Self {
            id,
            avatar,
            route,
            speed: 1.3,
            decorative: false,
        }
    }

    pub fn validate_all(npcs: &[Self]) -> Result<(), String> {
        if npcs.len() > MAX_NPCS {
            return Err(format!(
                "В сцене допускается до {MAX_NPCS} персонажей с маршрутами"
            ));
        }
        let mut ids = HashSet::with_capacity(npcs.len());
        for npc in npcs {
            npc.avatar.validate()?;
            if !(FIRST_NPC_ID..=LAST_NPC_ID).contains(&npc.id)
                || !ids.insert(npc.id)
                || !(0.35..=WALK_SPEED).contains(&npc.speed)
                || !(1..=MAX_WAYPOINTS).contains(&npc.route.len())
                || npc.route.iter().any(|point| {
                    let [x, y, z] = point.position;
                    !(-100.0..=100.0).contains(&x)
                        || !(0.0..=100.0).contains(&y)
                        || !(-100.0..=100.0).contains(&z)
                        || !(0.0..=60.0).contains(&point.wait)
                })
            {
                return Err(format!(
                    "Некорректный маршрут персонажа «{}»",
                    npc.avatar.name
                ));
            }
        }
        Ok(())
    }
}

pub fn next_id(npcs: &[Npc]) -> Option<u64> {
    let next = npcs
        .iter()
        .map(|npc| npc.id.saturating_add(1))
        .max()
        .unwrap_or(FIRST_NPC_ID);
    (npcs.len() < MAX_NPCS && next <= LAST_NPC_ID).then_some(next)
}

/// A short round trip that walks through the actual exterior and first interior openings.
pub fn doorway_route(building: &Building) -> Vec<Waypoint> {
    let opening = building.door_bounds();
    let [x, floor, z] = door_center(opening);
    let center = Vec2::new(x, z);
    let outward =
        [Vec2::Y, Vec2::X, Vec2::NEG_Y, Vec2::NEG_X][usize::from(building.quarter_turns % 4)];
    let tangent = Vec2::new(-outward.y, outward.x);
    let outside = center + outward * 1.22;
    let near_room = center - outward * 1.05;
    let waypoint = |point: Vec2| Waypoint::at([point.x, floor, point.y]);
    let mut route = vec![
        waypoint(outside + tangent * 1.35),
        waypoint(outside),
        waypoint(near_room),
    ];
    if let Some(partition) = building.interior_doors().next() {
        let [px, _, pz] = door_center(partition);
        let opening = Vec2::new(px, pz);
        let axis = if partition.max[0] - partition.min[0] < partition.max[2] - partition.min[2] {
            Vec2::X
        } else {
            Vec2::Y
        };
        let normal = if (near_room - opening).dot(axis) < 0.0 {
            -axis
        } else {
            axis
        };
        route.extend([
            waypoint(opening + normal * 0.88),
            waypoint(opening - normal * 0.88),
        ]);
    }
    let end = route.len() - 1;
    route[end].wait = 1.0;
    route.extend_from_within(1..end);
    route[end + 1..].reverse();
    route[0].wait = 0.55;
    route
}

fn door_center(bounds: Collider) -> [f32; 3] {
    [
        (bounds.min[0] + bounds.max[0]) * 0.5,
        bounds.min[1],
        (bounds.min[2] + bounds.max[2]) * 0.5,
    ]
}

#[derive(Default)]
pub struct NpcSimulation {
    agents: BTreeMap<u64, NpcState>,
}

pub struct NpcState {
    pub elapsed: f32,
    pub motion: Motion,
    pub previous: Option<(Motion, f32)>,
    pub yaw: f32,
    /// Index of the waypoint the actor is currently approaching.
    pub waypoint: usize,
    pub blocked: bool,
    body: Game,
    wait: f32,
    stalled: f32,
    replan: f32,
    path: Vec<Vec2>,
}

impl NpcState {
    fn new(npc: &Npc) -> Option<Self> {
        let first = npc.route.first()?;
        let mut body = Game::default();
        [body.x, body.height, body.z] = first.position;
        let waypoint = usize::from(npc.route.len() > 1);
        let facing = xz(npc.route[waypoint].position) - xz(first.position);
        let yaw = facing.x.atan2(facing.y).to_degrees();
        Some(Self {
            elapsed: 0.0,
            motion: Motion::Idle,
            previous: None,
            yaw,
            waypoint,
            blocked: false,
            body,
            wait: first.wait,
            stalled: 0.0,
            replan: 0.0,
            path: Vec::new(),
        })
    }

    pub fn position(&self) -> [f32; 3] {
        [self.body.x, self.body.height, self.body.z]
    }

    pub(crate) fn grounded(&self) -> bool {
        self.body.grounded
    }

    fn animate(&mut self, motion: Motion, dt: f32) {
        if self.motion != motion {
            self.previous = Some((self.motion, self.elapsed));
            self.elapsed = 0.0;
            self.motion = motion;
        } else {
            self.elapsed = (self.elapsed + dt).rem_euclid(3_600.0);
            if self.elapsed >= MOTION_TRANSITION_SECONDS {
                self.previous = None;
            } else if let Some((_, clock)) = &mut self.previous {
                *clock = (*clock + dt).rem_euclid(3_600.0);
            }
        }
    }

    fn idle(&mut self, dt: f32, colliders: &[Collider]) {
        self.body.update(dt, [0.0; 2], false, false, colliders);
        self.animate(Motion::Idle, dt);
    }

    fn advance(&mut self, npc: &Npc, frame: &mut Frame<'_>) {
        let dt = frame.dt;
        if frame.paused == Some(npc.id) || npc.route.len() <= 1 {
            self.body.stop_horizontal();
            self.idle(dt, frame.colliders);
            return;
        }
        if self.wait > 0.0 {
            self.wait = (self.wait - dt).max(0.0);
            self.idle(dt, frame.colliders);
            return;
        }

        self.waypoint %= npc.route.len();
        let target = npc.route[self.waypoint];
        let from = xz(self.position());
        let goal = xz(target.position);
        if from.distance_squared(goal) <= WAYPOINT_RADIUS * WAYPOINT_RADIUS {
            self.wait = target.wait;
            self.waypoint = (self.waypoint + 1) % npc.route.len();
            self.path.clear();
            self.replan = 0.0;
            self.stalled = 0.0;
            self.blocked = false;
            self.idle(dt, frame.colliders);
            return;
        }

        let solids = nav_boxes(frame.fixed, self.body.height, NAV_PADDING);
        self.replan = (self.replan - dt).max(0.0);
        while self
            .path
            .last()
            .is_some_and(|point| from.distance_squared(*point) < 0.10_f32.powi(2))
        {
            self.path.pop();
        }
        if self.replan == 0.0 {
            if self.stalled >= 0.32 {
                // A fully open leaf can still obstruct a diagonal path; navigate around it.
                let physical = nav_boxes(frame.colliders, self.body.height, NAV_PADDING);
                self.path = detour(from, goal, &physical)
                    .or_else(|| detour(from, goal, &solids))
                    .unwrap_or_default();
            } else if !clear(from, goal, &solids) && self.path.is_empty() {
                self.path = detour(from, goal, &solids).unwrap_or_default();
            }
            self.replan = 0.25;
        }

        let point = self.path.last().copied().unwrap_or(goal);
        let opening = request_doors(from, point, self.body.height, &solids, frame);
        let offset = point - from;
        let distance = offset.length();
        let braking_speed = (34.0 * (distance - 0.04).max(0.004)).sqrt();
        let speed = npc
            .speed
            .clamp(0.35, WALK_SPEED)
            .min(distance / dt)
            .min(braking_speed);
        let desired = offset.normalize_or_zero() * speed;
        let velocity =
            avoid_people(npc.id, self.position(), desired, frame).clamp_length_max(speed);
        self.body.update(
            dt,
            (velocity / WALK_SPEED).to_array(),
            false,
            false,
            frame.colliders,
        );
        let movement = xz(self.position()) - from;
        if movement.length_squared() > (dt * 0.10).powi(2) {
            let heading = movement.x.atan2(movement.y).to_degrees();
            let turn = (heading - self.yaw + 180.0).rem_euclid(360.0) - 180.0;
            self.yaw += turn * (1.0 - (-10.0 * dt).exp());
            self.stalled = 0.0;
            self.blocked = false;
            self.animate(Motion::Walk, dt);
        } else {
            self.stalled = if opening {
                0.0
            } else {
                (self.stalled + dt).min(2.0)
            };
            self.blocked = self.stalled > 0.70;
            self.animate(Motion::Idle, dt);
        }
    }
}

struct Frame<'a> {
    dt: f32,
    doors: &'a [Door<'a>],
    state: &'a mut InteractionState,
    colliders: &'a [Collider],
    fixed: &'a [Collider],
    people: &'a [(u64, [f32; 3])],
    player: Option<[f32; 3]>,
    paused: Option<u64>,
}

impl NpcSimulation {
    pub fn reset(&mut self, defs: &[Npc]) {
        self.agents = defs
            .iter()
            .filter_map(|npc| Some((npc.id, NpcState::new(npc)?)))
            .collect();
    }

    pub fn state(&self, id: u64) -> Option<&NpcState> {
        self.agents.get(&id)
    }

    pub fn advance(
        &mut self,
        defs: &[Npc],
        world: &World,
        state: &mut InteractionState,
        colliders: &[Collider],
        player: Option<[f32; 3]>,
        dt: f32,
        paused: Option<u64>,
    ) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.agents.retain(|id, _| {
            defs.iter()
                .any(|npc| npc.id == *id && !npc.route.is_empty())
        });
        if defs.is_empty() {
            return;
        }
        for npc in defs {
            if let std::collections::btree_map::Entry::Vacant(entry) = self.agents.entry(npc.id)
                && let Some(actor) = NpcState::new(npc)
            {
                entry.insert(actor);
            }
        }

        let doors: Vec<_> = world.buildings.iter().flat_map(Building::doors).collect();
        let mut leaves = Vec::new();
        for door in &doors {
            leaves.extend(door.colliders(state.fraction(door)));
        }
        let fixed: Vec<_> = colliders
            .iter()
            .copied()
            .filter(|bounds| !leaves.contains(bounds))
            .collect();
        let people: Vec<_> = self
            .agents
            .iter()
            .map(|(&id, actor)| (id, actor.position()))
            .collect();
        let mut frame = Frame {
            dt: dt.min(0.075),
            doors: &doors,
            state,
            colliders,
            fixed: &fixed,
            people: &people,
            player,
            paused,
        };
        for npc in defs {
            if let Some(agent) = self.agents.get_mut(&npc.id) {
                agent.advance(npc, &mut frame);
            }
        }
    }
}

fn request_doors(
    from: Vec2,
    target: Vec2,
    floor: f32,
    solids: &[NavBox],
    frame: &mut Frame<'_>,
) -> bool {
    let ahead = from + (target - from).clamp_length_max(1.64);
    let mut waiting = false;
    for door in frame.doors {
        let center = door.interaction_position();
        let center_xz = xz(center);
        if (center[1] - floor).abs() > 0.28
            || center_xz.distance_squared(from) > 1.34_f32.powi(2)
            || !clear(from, center_xz, solids)
            || !door
                .colliders(0.0)
                .any(|leaf| NavBox::from_collider(leaf, 0.04).hits(from, ahead))
        {
            continue;
        }
        // These doors are opened by the player's authored hand-contact sequence.
        if frame.state.next_opening(door) && !door.settings.hand_contact {
            frame.state.toggle(door);
        }
        waiting |= frame.state.fraction(door) < 0.97;
    }
    waiting
}

fn avoid_people(id: u64, world_pos: [f32; 3], desired: Vec2, frame: &Frame<'_>) -> Vec2 {
    let at = xz(world_pos);
    let direction = desired.normalize_or_zero();
    let mut velocity = desired;
    let right = Vec2::new(direction.y, -direction.x);
    let speed = desired.length();
    let neighbors = frame
        .people
        .iter()
        .filter(|&&(other, _)| other != id)
        .map(|&(_, position)| position)
        .chain(frame.player);
    for other in neighbors {
        if (other[1] - world_pos[1]).abs() > 0.55 {
            continue;
        }
        let delta = xz(other) - at;
        let distance = delta.length();
        if !(0.001..1.18).contains(&distance) {
            continue;
        }
        let toward = delta / distance;
        let approach = direction.dot(toward).max(0.0);
        let proximity = (1.0 - distance / 1.18).max(0.0);
        velocity += right * (1.5 * proximity * approach * speed);
        let clearance = (1.0 - distance / 0.82).clamp(0.0, 1.0);
        velocity -= toward * (2.6 * clearance * speed);
        if distance < 0.60 {
            velocity -= toward * velocity.dot(toward).max(0.0);
        }
    }
    velocity
}

fn xz(position: [f32; 3]) -> Vec2 {
    Vec2::new(position[0], position[2])
}

#[derive(Clone, Copy)]
struct NavBox {
    min: Vec2,
    max: Vec2,
}

impl NavBox {
    fn from_collider(collider: Collider, padding: f32) -> Self {
        Self {
            min: xz(collider.min) - Vec2::splat(padding),
            max: xz(collider.max) + Vec2::splat(padding),
        }
    }

    fn contains(self, point: Vec2) -> bool {
        point.cmpge(self.min).all() && point.cmple(self.max).all()
    }

    fn hits(self, from: Vec2, to: Vec2) -> bool {
        let delta = to - from;
        let mut enter = 0.0_f32;
        let mut exit = 1.0_f32;
        for axis in 0..2 {
            if delta[axis].abs() < 1e-6 {
                if from[axis] < self.min[axis] || from[axis] > self.max[axis] {
                    return false;
                }
            } else {
                let a = (self.min[axis] - from[axis]) / delta[axis];
                let b = (self.max[axis] - from[axis]) / delta[axis];
                enter = enter.max(a.min(b));
                exit = exit.min(a.max(b));
                if enter > exit {
                    return false;
                }
            }
        }
        true
    }

    fn permits(self, from: Vec2, to: Vec2) -> bool {
        // A physical actor may touch a wall just inside the extra navigation margin.
        // Allow going out from that contact without allowing a leg across the obstacle.
        if self.contains(from) {
            for axis in 0..2 {
                if (from[axis] - self.min[axis] < 0.04 && to[axis] < self.min[axis])
                    || (self.max[axis] - from[axis] < 0.04 && to[axis] > self.max[axis])
                {
                    return true;
                }
            }
        }
        !self.hits(from, to)
    }
}

fn nav_boxes(colliders: &[Collider], floor: f32, padding: f32) -> Vec<NavBox> {
    colliders
        .iter()
        .copied()
        .filter(|bounds| {
            bounds.max[1] > floor + crate::scene::STEP_HEIGHT && bounds.min[1] < floor + 2.246
        })
        .map(|bounds| NavBox::from_collider(bounds, padding))
        .collect()
}

fn clear(from: Vec2, to: Vec2, obstacles: &[NavBox]) -> bool {
    obstacles.iter().all(|bounds| bounds.permits(from, to))
}

fn detour(from: Vec2, goal: Vec2, obstacles: &[NavBox]) -> Option<Vec<Vec2>> {
    if clear(from, goal, obstacles) {
        return Some(vec![goal]);
    }
    if obstacles.iter().any(|bounds| bounds.contains(goal)) {
        return None;
    }
    let low = from.min(goal) - Vec2::splat(4.0);
    let high = from.max(goal) + Vec2::splat(4.0);
    let mut relevant: Vec<_> = obstacles
        .iter()
        .filter(|bounds| bounds.max.cmpge(low).all() && bounds.min.cmple(high).all())
        .collect();
    let middle = (from + goal) * 0.5;
    relevant.sort_by(|a, b| {
        let key = |bounds: &NavBox| (bounds.min + bounds.max).distance_squared(middle * 2.0);
        key(a).total_cmp(&key(b))
    });
    let mut nodes = vec![from, goal];
    for bounds in relevant.into_iter().take(MAX_NEAR_OBSTACLES) {
        let min = bounds.min - Vec2::splat(0.035);
        let max = bounds.max + Vec2::splat(0.035);
        for corner in [min, Vec2::new(min.x, max.y), Vec2::new(max.x, min.y), max] {
            if obstacles.iter().all(|bounds| !bounds.contains(corner)) {
                nodes.push(corner);
            }
        }
    }

    let mut costs = vec![f32::INFINITY; nodes.len()];
    let mut previous = vec![None; nodes.len()];
    let mut closed = vec![false; nodes.len()];
    costs[0] = 0.0;
    loop {
        let current = (0..nodes.len())
            .filter(|&index| !closed[index] && costs[index].is_finite())
            .min_by(|&a, &b| {
                (costs[a] + nodes[a].distance(goal))
                    .total_cmp(&(costs[b] + nodes[b].distance(goal)))
            })?;
        if current == 1 {
            let mut path = Vec::new();
            let mut next = current;
            while let Some(parent) = previous[next] {
                path.push(nodes[next]);
                next = parent;
            }
            return Some(path);
        }
        closed[current] = true;
        for next in (1..nodes.len()).filter(|&index| !closed[index]) {
            let cost = costs[current] + nodes[current].distance(nodes[next]);
            if cost < costs[next] && clear(nodes[current], nodes[next], obstacles) {
                costs[next] = cost;
                previous[next] = Some(current);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{assets::Source, model::decode_glb, world::DoorSettings};

    use super::*;

    fn actor() -> Avatar {
        let mesh = decode_glb(include_bytes!("../demo_assets/character.glb")).unwrap();
        Avatar::new(Source::Knight, "Гость".into(), &mesh)
    }

    #[test]
    fn routes_are_validated_and_preserve_waits_after_roundtrip() {
        let mut npc = Npc::new(FIRST_NPC_ID, actor(), vec![Waypoint::at([1.0, 0.0, 2.0])]);
        npc.route[0].wait = 4.0;
        Npc::validate_all(std::slice::from_ref(&npc)).unwrap();
        let copy: Npc = serde_json::from_slice(&serde_json::to_vec(&npc).unwrap()).unwrap();
        assert_eq!(npc, copy);
        assert!(Npc::validate_all(&[npc, copy]).is_err());
    }

    #[test]
    fn animation_transition_keeps_and_advances_the_old_clock() {
        let npc = Npc::new(FIRST_NPC_ID, actor(), vec![Waypoint::at([0.0; 3])]);
        let mut state = NpcState::new(&npc).unwrap();
        state.animate(Motion::Idle, 0.12);
        state.animate(Motion::Walk, 0.05);
        assert_eq!(state.previous, Some((Motion::Idle, 0.12)));
        assert_eq!(state.elapsed, 0.0);
        state.animate(Motion::Walk, 0.05);
        let (motion, clock) = state.previous.unwrap();
        assert_eq!(motion, Motion::Idle);
        assert!((clock - 0.17).abs() < 1e-6);
        state.animate(Motion::Walk, 0.15);
        assert_eq!(state.previous, None);
    }

    #[test]
    fn avoidance_route_moves_around_wall_and_can_pause_for_dialogue() {
        let mut simulation = NpcSimulation::default();
        let mut doors = InteractionState::default();
        let wall = Collider {
            min: [-0.29, 0.0, -0.9],
            max: [0.29, 2.2, 0.9],
        };
        let npc = Npc::new(
            FIRST_NPC_ID,
            actor(),
            vec![
                Waypoint::at([-2.0, 0.0, 0.0]),
                Waypoint::at([2.0, 0.0, 0.0]),
            ],
        );
        let world = World::default();
        simulation.reset(std::slice::from_ref(&npc));
        for _ in 0..20 {
            simulation.advance(
                std::slice::from_ref(&npc),
                &world,
                &mut doors,
                &[wall],
                None,
                0.05,
                Some(npc.id),
            );
        }
        assert_eq!(
            simulation.state(npc.id).unwrap().position(),
            npc.route[0].position
        );
        let mut made_detour = false;
        let mut reached = false;
        for _ in 0..160 {
            simulation.advance(
                std::slice::from_ref(&npc),
                &world,
                &mut doors,
                &[wall],
                None,
                0.05,
                None,
            );
            let [x, _, z] = simulation.state(npc.id).unwrap().position();
            made_detour |= z.abs() > 1.23;
            reached |= x > 1.7;
            assert!(x.abs() > 0.63 || z.abs() > 1.23);
        }
        assert!(made_detour && reached);
    }

    #[test]
    fn city_guest_opens_two_native_doors_and_returns_outside() {
        let mut world = World::default();
        world.add_building_at([-7.4, 0.0, -2.8]).unwrap();
        let house = &mut world.buildings[0];
        house.size = [7.3, 3.45, 5.9];
        house.quarter_turns = 1;
        house.furnish_demo();
        house.door = Some(DoorSettings::default());
        house.interior.partitions[0].opening.as_mut().unwrap().door = Some(DoorSettings::default());
        let npc = Npc::new(FIRST_NPC_ID, actor(), doorway_route(house));
        let mut simulation = NpcSimulation::default();
        let mut doors = InteractionState::default();
        let mut entered_back_room = false;
        let mut left_afterwards = false;
        for _ in 0..520 {
            world.advance_interactions(&mut doors, 0.05, [50.0, 0.0, 50.0]);
            let colliders = world.colliders_with(&doors);
            simulation.advance(
                std::slice::from_ref(&npc),
                &world,
                &mut doors,
                &colliders,
                None,
                0.05,
                None,
            );
            let [x, _, _] = simulation.state(npc.id).unwrap().position();
            entered_back_room |= x < -8.30;
            left_afterwards |= entered_back_room && x > -4.0;
        }
        assert!(
            world.buildings[0]
                .doors()
                .all(|door| doors.fraction(&door) > 0.99)
        );
        assert!(entered_back_room && left_afterwards);
    }

    #[test]
    fn a_wall_in_front_of_an_adjacent_door_blocks_automatic_opening() {
        let mut world = World::default();
        world.add_building_at([0.0; 3]).unwrap();
        world.buildings[0].door = Some(DoorSettings::default());
        let doors: Vec<_> = world.buildings[0].doors().collect();
        let center = xz(doors[0].interaction_position());
        let from = center + Vec2::Y;
        let target = center - Vec2::Y;
        let wall = NavBox {
            min: center + Vec2::new(-0.8, 0.32),
            max: center + Vec2::new(0.8, 0.67),
        };
        let mut state = InteractionState::default();
        let mut frame = Frame {
            dt: 0.05,
            doors: &doors,
            state: &mut state,
            colliders: &[],
            fixed: &[],
            people: &[],
            player: None,
            paused: None,
        };
        assert!(!request_doors(from, target, 0.0, &[wall], &mut frame));
        assert!(frame.state.next_opening(&doors[0]));
        assert!(request_doors(from, target, 0.0, &[], &mut frame));
        assert!(!frame.state.next_opening(&doors[0]));
    }
}
