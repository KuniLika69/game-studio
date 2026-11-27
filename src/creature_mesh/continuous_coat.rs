//! Fuses the torso and legs in the bind pose, then binds the resulting skin and groom.
use std::collections::HashMap;

use glam::{Vec3, Vec3Swizzles};

use super::{Draft, Geometry, Glb, Limb, PRIMARY, groom, v};

pub(super) struct Binding {
    pub(super) joints: Vec<[u8; 4]>,
    pub(super) weights: Vec<[f32; 4]>,
}

struct Volume {
    center: Vec3,
    inverse_radii: Vec3,
    distance_scale: f32,
}

impl Volume {
    fn new(center: Vec3, radii: Vec3) -> Self {
        Self {
            center,
            inverse_radii: radii.recip(),
            distance_scale: radii.min_element(),
        }
    }

    fn distance(&self, point: Vec3) -> f32 {
        (((point - self.center) * self.inverse_radii).length() - 1.0) * self.distance_scale
    }
}

fn soft_min(a: f32, b: f32, distance: f32) -> f32 {
    let blend = (1.0 - (a - b).abs() / distance).max(0.0);
    a.min(b) - distance * 0.25 * blend * blend
}

fn smoothstep(from: f32, to: f32, value: f32) -> f32 {
    let t = ((value - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

struct Coat<'a> {
    draft: &'a Draft,
    torso: u8,
    legs: &'a [Limb],
    hips: [Vec3; 4],
    volumes: Vec<(Volume, f32)>,
}

impl<'a> Coat<'a> {
    fn new(draft: &'a Draft, torso: usize, legs: &'a [Limb], height: f32) -> Self {
        let b = draft.body;
        let stride = draft.legs;
        let origin = Vec3::Y * height;
        let hips = [(-1., 1.), (1., 1.), (-1., -1.), (1., -1.)]
            .map(|(side, end)| origin + v(side * 0.134 * b, 0.0, end * 0.236 * b));
        let mut volumes = vec![
            (
                Volume::new(
                    origin + v(0., 0.042, -0.041 * b),
                    v(0.161 * b, 0.166 * b, 0.367 * b),
                ),
                0.042,
            ),
            (
                Volume::new(
                    origin + v(0., 0.088, 0.177 * b),
                    v(0.171 * b, 0.174 * b, 0.174 * b),
                ),
                0.059,
            ),
            (
                Volume::new(
                    origin + v(0., 0.054, -0.205 * b),
                    v(0.171 * b, 0.169 * b, 0.185 * b),
                ),
                0.049,
            ),
            (
                Volume::new(
                    origin + v(0., 0.163, 0.299 * b),
                    v(0.114 * b, 0.149, 0.115 * b),
                ),
                0.071,
            ),
        ];
        for (index, hip) in hips.into_iter().enumerate() {
            let fore = index < 2;
            let width = if fore { 0.064 } else { 0.084 };
            volumes.extend([
                (
                    Volume::new(
                        hip + v(0., -0.071 * stride, if fore { 0.011 } else { -0.028 }),
                        v(width, 0.173 * stride, width * 1.08),
                    ),
                    0.053,
                ),
                (
                    Volume::new(
                        hip + v(0., -0.208 * stride, 0.013),
                        v(0.044, (0.069 * stride).max(0.035), 0.044),
                    ),
                    0.027,
                ),
                (
                    Volume::new(
                        hip + v(0., -0.329 * stride, 0.008),
                        v(0.031, 0.132 * stride, 0.038),
                    ),
                    0.025,
                ),
                (
                    Volume::new(
                        hip + v(0., -0.44 * stride - 0.018, 0.025),
                        v(0.047, 0.043, 0.063),
                    ),
                    0.023,
                ),
            ]);
        }
        Self {
            draft,
            torso: torso as u8,
            legs,
            hips,
            volumes,
        }
    }

    fn distance(&self, point: Vec3) -> f32 {
        let field = self
            .volumes
            .iter()
            .fold(f32::INFINITY, |field, (volume, blend)| {
                soft_min(field, volume.distance(point), *blend)
            });
        // The pads are the lower ground-contact surface and stay fully paw-local.
        field.max(0.019 - point.y)
    }

    fn normal(&self, point: Vec3) -> Vec3 {
        let component =
            |axis| self.distance(point + axis * 0.0032) - self.distance(point - axis * 0.0032);
        v(component(Vec3::X), component(Vec3::Y), component(Vec3::Z))
            .try_normalize()
            .unwrap_or(Vec3::Y)
    }

    fn influence(&self, point: Vec3) -> ([u8; 4], [f32; 4]) {
        let index = usize::from(point.x >= 0.0) + 2 * usize::from(point.z < 0.0);
        let hip = self.hips[index];
        let leg = &self.legs[index];
        let paw = leg.paw.as_ref().unwrap();
        let [upper, lower] = paw.lengths;
        let knee_y = hip.y - upper;
        let ankle_y = knee_y - lower;
        let near = 1.0
            - smoothstep(
                0.047,
                0.13 * self.draft.body + 0.045,
                (point.xz() - hip.xz()).length(),
            );
        let above_knee = smoothstep(knee_y + 0.017, hip.y + 0.043, point.y);
        let shoulder = 1.0 - smoothstep(hip.y - 0.035, hip.y + 0.151, point.y);
        let quadrant = smoothstep(0.014, 0.084 * self.draft.body, point.x.abs())
            * smoothstep(0.022, 0.136 * self.draft.body, point.z.abs());
        let limb = (1.0 - above_knee * (1.0 - near * shoulder)) * quadrant;
        let knee = 1.0 - smoothstep(knee_y - 0.055, knee_y + 0.051, point.y);
        let ankle = 1.0 - smoothstep(ankle_y - 0.015, ankle_y + 0.058, point.y);
        (
            [self.torso, leg.upper as u8, leg.lower as u8, paw.node as u8],
            [
                1.0 - limb,
                limb * (1.0 - knee),
                limb * knee * (1.0 - ankle),
                limb * knee * ankle,
            ],
        )
    }

    fn mesh(&self) -> Geometry {
        let mut mesh = Geometry::default();
        let b = self.draft.body;
        let height = self.hips[0].y;
        let low = v(-0.29 * b - 0.08, -0.037, -0.43 * b - 0.045);
        let high = v(-low.x, height + 0.36 + 0.12 * b, -low.z + 0.10);
        let step = 0.022;
        let [nx, ny, nz] = ((high - low) / step)
            .ceil()
            .to_array()
            .map(|count| count as usize + 1);
        let stride = nx * ny;
        let point = |slot: usize| {
            low + step
                * v(
                    (slot % nx) as f32,
                    (slot / nx % ny) as f32,
                    (slot / stride) as f32,
                )
        };
        let samples: Vec<_> = (0..stride * nz)
            .map(|slot| self.distance(point(slot)))
            .collect();
        let mut edges = HashMap::with_capacity(12_000);
        let mut edge = |a: usize, b: usize, mesh: &mut Geometry| {
            let key = (a.min(b), a.max(b));
            *edges.entry(key).or_insert_with(|| {
                let t = (samples[a] / (samples[a] - samples[b])).clamp(0.0, 1.0);
                let p = point(a).lerp(point(b), t);
                let index = u16::try_from(mesh.position.len()).expect("bounded coat tessellation");
                mesh.vertex(p, self.normal(p));
                mesh.uv
                    .push([p.x * 1.35 + p.y * 0.41, p.z * 1.54 + p.y * 0.56]);
                index
            })
        };
        const TETS: [[usize; 4]; 6] = [
            [0, 5, 1, 6],
            [0, 1, 2, 6],
            [0, 2, 3, 6],
            [0, 3, 7, 6],
            [0, 7, 4, 6],
            [0, 4, 5, 6],
        ];
        for z in 0..nz - 1 {
            for y in 0..ny - 1 {
                for x in 0..nx - 1 {
                    let a = x + y * nx + z * stride;
                    let b = a + stride;
                    let corners = [a, a + 1, a + nx + 1, a + nx, b, b + 1, b + nx + 1, b + nx];
                    let mask = corners.map(|corner| samples[corner] <= 0.0);
                    if mask.iter().all(|&part| part == mask[0]) {
                        continue;
                    }
                    for tet in TETS {
                        let mut inner = [0; 4];
                        let mut outer = [0; 4];
                        let mut count = 0;
                        for (index, &corner) in tet.iter().enumerate() {
                            if mask[corner] {
                                inner[count] = corners[corner];
                                count += 1;
                            } else {
                                outer[index - count] = corners[corner];
                            }
                        }
                        match count {
                            1 | 3 => {
                                let (one, three) = if count == 1 {
                                    (inner[0], outer)
                                } else {
                                    (outer[0], inner)
                                };
                                let tri = std::array::from_fn(|j| edge(one, three[j], &mut mesh));
                                add_triangle(&mut mesh, tri);
                            }
                            2 => {
                                let a = edge(inner[0], outer[0], &mut mesh);
                                let b = edge(inner[0], outer[1], &mut mesh);
                                let c = edge(inner[1], outer[1], &mut mesh);
                                let d = edge(inner[1], outer[0], &mut mesh);
                                add_triangle(&mut mesh, [a, b, c]);
                                add_triangle(&mut mesh, [a, c, d]);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        mesh
    }

    fn fibers(&self, coat: &Geometry) -> [(Geometry, Binding); 2] {
        let (mut total, mut areas) = (0.0, Vec::with_capacity(coat.indices.len() / 3));
        for triangle in coat.indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|i| Vec3::from_array(coat.position[i as usize]));
            total += (b - a).cross(c - a).length();
            areas.push(total);
        }
        let mut groups = std::array::from_fn(|_| {
            (
                Geometry::default(),
                Binding {
                    joints: Vec::new(),
                    weights: Vec::new(),
                },
            )
        });
        for index in 0..2750u32 {
            let noise = |channel| groom::noise(27_291 + index * 13 + channel);
            let sample = areas
                .partition_point(|&a| a < total * noise(0))
                .min(areas.len() - 1);
            let triangle = &coat.indices[sample * 3..][..3];
            let b = 1.0 - noise(1).sqrt();
            let c = (1.0 - b) * noise(2);
            let mix = |input: &[[f32; 3]]| {
                [b, c, 1.0 - b - c]
                    .into_iter()
                    .zip(triangle)
                    .map(|(w, &i)| Vec3::from_array(input[i as usize]) * w)
                    .sum::<Vec3>()
            };
            let point = mix(&coat.position);
            let normal = mix(&coat.normal).normalize_or_zero();
            if point.y < 0.048
                || (point.y > self.hips[0].y + 0.080 && point.z > 0.27 * self.draft.body)
            {
                continue;
            }
            let lower = 1.0 - smoothstep(self.hips[0].y - 0.115, self.hips[0].y + 0.051, point.y);
            let direction = v(0.0, -0.40 - 0.60 * lower, -0.88 + 0.71 * lower);
            let flow = (direction - direction.dot(normal) * normal).normalize_or_zero();
            let side = normal.cross(flow);
            if side.length_squared() < 0.1 {
                continue;
            }
            let length = (0.029 - 0.014 * lower) * (0.55 + 0.85 * noise(3));
            let width = length * (0.043 + noise(4) * 0.038);
            let root = point - normal * width * 0.71;
            let middle = root + length * (flow * 0.56 + normal * 0.23);
            let tip =
                root + length * (flow * 0.92 + normal * 0.35 + side * 0.28 * (noise(5) - 0.5));
            let (mesh, weights) = &mut groups[usize::from(noise(6) > 0.55)];
            let (joints, influence) = self.influence(point);
            let offset = mesh.position.len() as u16;
            for (p, u, depth) in [
                (root - side * width, 0.0, 0.0),
                (root + side * width, 1.0, 0.0),
                (middle - side * width * 0.60, 0.17, 0.50),
                (middle + side * width * 0.60, 0.83, 0.50),
                (tip, 0.5, 1.0),
            ] {
                mesh.vertex(p, (normal - flow * (0.18 * depth)).normalize());
                mesh.uv.push([u, depth]);
                weights.joints.push(joints);
                weights.weights.push(influence);
            }
            mesh.indices
                .extend([0, 2, 1, 1, 2, 3, 2, 4, 3].map(|index| index + offset));
        }
        groups
    }
}

fn add_triangle(mesh: &mut Geometry, mut tri: [u16; 3]) {
    let [a, b, c] = tri.map(|i| Vec3::from_array(mesh.position[i as usize]));
    let cross = (b - a).cross(c - a);
    if cross.length_squared() < 1e-20 {
        return;
    }
    let [n0, n1, n2] = tri.map(|i| Vec3::from_array(mesh.normal[i as usize]));
    if cross.dot(n0 + n1 + n2) < 0.0 {
        tri.swap(1, 2);
    }
    mesh.indices.extend(tri);
}

pub(super) fn build(out: &mut Glb, draft: &Draft, torso: usize, legs: &[Limb], height: f32) {
    let coat = Coat::new(draft, torso, legs, height);
    let mesh = coat.mesh();
    let (joints, weights) = mesh
        .position
        .iter()
        .map(|&p| coat.influence(Vec3::from_array(p)))
        .unzip();
    let fibers = coat.fibers(&mesh);
    let node = out.skin_node();
    out.add_primitive(node, PRIMARY, mesh, Some(Binding { joints, weights }));
    for ((mesh, binding), material) in fibers
        .into_iter()
        .zip([groom::ROOT_COLOR, groom::TIP_COLOR])
    {
        if !mesh.position.is_empty() {
            out.add_primitive(node, material, mesh, Some(binding));
        }
    }
}
