//! Parametric characters assembled as self-contained glTF meshes and named animation nodes.
use std::f32::consts::{PI, TAU};

use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod continuous_coat;
mod groom;

/// The anatomy built by the generator, independent of its surface palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    #[default]
    Robot,
    Quadruped,
}

impl Kind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Robot => "Робот",
            Self::Quadruped => "Четвероногое существо",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Draft {
    pub(crate) kind: Kind,
    pub(crate) head: f32,
    pub(crate) body: f32,
    pub(crate) legs: f32,
    pub(crate) tail: f32,
    pub(crate) primary: [u8; 3],
    pub(crate) secondary: [u8; 3],
    pub(crate) accent: [u8; 3],
}

impl Default for Draft {
    fn default() -> Self {
        Self::for_kind(Kind::Robot)
    }
}

impl Draft {
    pub(crate) fn for_kind(kind: Kind) -> Self {
        let (primary, secondary, accent) = match kind {
            Kind::Robot => ([80, 143, 188], [45, 54, 79], [144, 237, 211]),
            Kind::Quadruped => ([187, 106, 79], [243, 206, 164], [127, 214, 199]),
        };
        Self {
            kind,
            head: 1.0,
            body: 1.0,
            legs: 1.0,
            tail: 1.0,
            primary,
            secondary,
            accent,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if [self.head, self.body, self.legs, self.tail]
            .iter()
            .any(|value| !(0.55..=1.55).contains(value))
        {
            return Err(
                "Размеры головы, тела, ног и хвоста должны быть в диапазоне 0,55–1,55.".into(),
            );
        }
        Ok(())
    }
}

pub(crate) fn build(draft: &Draft) -> Result<Vec<u8>, String> {
    draft.validate()?;
    let mut output = Glb::new(draft);
    let rig = match draft.kind {
        Kind::Robot => robot(&mut output, draft),
        Kind::Quadruped => animal(&mut output, draft),
    };
    for gait in [Gait::Idle, Gait::Walk, Gait::Run] {
        animate(&mut output, &rig, draft.kind, gait);
    }
    output.finish()
}

const PRIMARY: usize = 0;
const SECONDARY: usize = 1;
const ACCENT: usize = 2;
const DARK: usize = 3;
const LIGHT: usize = 4;
const PANEL: usize = 5;

struct Glb {
    blob: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    meshes: Vec<Value>,
    materials: Vec<Value>,
    nodes: Vec<Value>,
    animations: Vec<Value>,
    images: Vec<Value>,
    skin: Option<Value>,
}

impl Glb {
    fn new(draft: &Draft) -> Self {
        let metal = if draft.kind == Kind::Robot { 0.35 } else { 0.0 };
        let panel = draft.primary.map(|value| value.saturating_add(30));
        let mut glb = Self {
            blob: Vec::new(),
            views: Vec::new(),
            accessors: Vec::new(),
            meshes: Vec::new(),
            nodes: Vec::new(),
            animations: Vec::new(),
            images: Vec::new(),
            skin: None,
            materials: vec![
                surface("Creator | Primary", draft.primary, metal, 0.56, 0.0),
                surface(
                    "Creator | Secondary",
                    draft.secondary,
                    metal * 0.45,
                    0.69,
                    0.0,
                ),
                surface(
                    "Creator | Accent",
                    draft.accent,
                    0.1,
                    0.48,
                    if metal > 0.0 { 0.43 } else { 0.12 },
                ),
                surface("Creator | Dark", [30, 35, 45], 0.12, 0.53, 0.0),
                surface("Creator | Light", [248, 232, 202], 0.03, 0.58, 0.04),
                surface("Creator | Panels", panel, metal, 0.59, 0.0),
            ],
        };
        if draft.kind == Kind::Quadruped {
            groom::materials(&mut glb, draft);
        }
        glb
    }

    fn node(&mut self, parent: Option<usize>, name: &str, at: Vec3) -> usize {
        let id = self.nodes.len();
        self.nodes
            .push(json!({"name": name, "translation": at.to_array(), "children": []}));
        if let Some(parent) = parent {
            self.nodes[parent]["children"]
                .as_array_mut()
                .unwrap()
                .push(json!(id));
        }
        id
    }

    fn bytes(&mut self, data: impl Iterator<Item = u8>) -> usize {
        pad(&mut self.blob, 0);
        let start = self.blob.len();
        self.blob.extend(data);
        let index = self.views.len();
        self.views
            .push(json!({"buffer": 0, "byteOffset": start, "byteLength": self.blob.len() - start}));
        index
    }

    fn floats<const N: usize>(&mut self, data: &[[f32; N]], shape: &str, bounds: bool) -> usize {
        let view = self.bytes(data.iter().flatten().flat_map(|value| value.to_le_bytes()));
        let mut accessor =
            json!({"bufferView": view, "componentType": 5126, "count": data.len(), "type": shape});
        if bounds {
            let mut lo = [f32::INFINITY; N];
            let mut hi = [f32::NEG_INFINITY; N];
            for values in data {
                for (i, &value) in values.iter().enumerate() {
                    lo[i] = lo[i].min(value);
                    hi[i] = hi[i].max(value);
                }
            }
            accessor["min"] = json!(lo.as_slice());
            accessor["max"] = json!(hi.as_slice());
        }
        let index = self.accessors.len();
        self.accessors.push(accessor);
        index
    }

    fn add(&mut self, node: usize, material: usize, geometry: Geometry) {
        self.add_primitive(node, material, geometry, None);
    }

    fn add_primitive(
        &mut self,
        node: usize,
        material: usize,
        geometry: Geometry,
        binding: Option<continuous_coat::Binding>,
    ) {
        let position = self.floats(&geometry.position, "VEC3", true);
        let normal = self.floats(&geometry.normal, "VEC3", false);
        let view = self.bytes(
            geometry
                .indices
                .iter()
                .flat_map(|value| value.to_le_bytes()),
        );
        let indices = self.accessors.len();
        self.accessors
            .push(json!({"bufferView": view, "componentType": 5123,
                                  "count": geometry.indices.len(), "type": "SCALAR"}));
        let mut attributes = json!({"POSITION": position, "NORMAL": normal});
        if !geometry.uv.is_empty() && !self.images.is_empty() {
            attributes["TEXCOORD_0"] = json!(self.floats(&geometry.uv, "VEC2", false));
        }
        if let Some(binding) = binding {
            let joints = self.accessors.len();
            let view = self.bytes(binding.joints.iter().flatten().copied());
            self.accessors
                .push(json!({"bufferView": view, "componentType": 5121,
                "count": binding.joints.len(), "type": "VEC4"}));
            let weights = self.floats(&binding.weights, "VEC4", false);
            attributes["JOINTS_0"] = json!(joints);
            attributes["WEIGHTS_0"] = json!(weights);
        }
        let primitive = json!({"attributes": attributes, "indices": indices, "material": material});
        match self.nodes[node]["mesh"].as_u64() {
            Some(mesh) => self.meshes[mesh as usize]["primitives"]
                .as_array_mut()
                .unwrap()
                .push(primitive),
            None => {
                self.nodes[node]["mesh"] = json!(self.meshes.len());
                self.meshes
                    .push(json!({"name": self.nodes[node]["name"], "primitives": [primitive]}));
            }
        }
    }

    fn skin_node(&mut self) -> usize {
        // Bind positions are all measured in asset coordinates. Joint indices are the
        // original node IDs, including rigid detail nodes used as attachment points.
        let mut bind = vec![Vec3::ZERO; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            let local = Vec3::from_array(std::array::from_fn(|axis| {
                node["translation"][axis].as_f64().unwrap() as f32
            }));
            bind[index] += local;
            if let Some(children) = node["children"].as_array() {
                for child in children {
                    bind[child.as_u64().unwrap() as usize] = bind[index];
                }
            }
        }
        let matrices: Vec<_> = bind
            .iter()
            .map(|&p| Mat4::from_translation(-p).to_cols_array())
            .collect();
        let accessor = self.floats(&matrices, "MAT4", false);
        self.skin = Some(
            json!({"skeleton": 0, "joints": (0..bind.len()).collect::<Vec<_>>(),
            "inverseBindMatrices": accessor}),
        );
        let node = self.node(Some(0), "Continuous coat", Vec3::ZERO);
        self.nodes[node]["skin"] = json!(0);
        node
    }

    fn ball(&mut self, node: usize, material: usize, center: Vec3, radii: Vec3) {
        self.add(node, material, ellipsoid(center, radii, Quat::IDENTITY));
    }

    fn box_at(&mut self, node: usize, material: usize, center: Vec3, half: Vec3, edge: f32) {
        self.add(node, material, rounded_box(center, half, edge));
    }

    fn oblong(
        &mut self,
        node: usize,
        material: usize,
        bottom: Vec3,
        top: Vec3,
        width: f32,
        depth: f32,
    ) {
        let axis = top - bottom;
        self.add(
            node,
            material,
            ellipsoid(
                (bottom + top) * 0.5,
                v(width, axis.length() * 0.60, depth),
                Quat::from_rotation_arc(Vec3::Y, axis.normalize()),
            ),
        );
    }

    fn finish(mut self) -> Result<Vec<u8>, String> {
        for node in &mut self.nodes {
            if node["children"].as_array().is_some_and(Vec::is_empty) {
                node.as_object_mut().unwrap().remove("children");
            }
        }
        pad(&mut self.blob, 0);
        let mut document = json!({
            "asset": {"version": "2.0", "generator": "Robot Studio Character Creator"},
            "scenes": [{"nodes": [0]}], "scene": 0, "nodes": self.nodes,
            "meshes": self.meshes, "materials": self.materials, "animations": self.animations,
            "buffers": [{"byteLength": self.blob.len()}], "bufferViews": self.views,
            "accessors": self.accessors,
        });
        if !self.images.is_empty() {
            let textures = self
                .images
                .iter()
                .enumerate()
                .map(|(index, _)| json!({"source": index}))
                .collect::<Vec<_>>();
            document["textures"] = json!(textures);
            document["images"] = json!(self.images);
        }
        if let Some(skin) = self.skin {
            document["skins"] = json!([skin]);
        }
        let mut meta = serde_json::to_vec(&document).map_err(|error| error.to_string())?;
        pad(&mut meta, b' ');
        let mut out = Vec::with_capacity(28 + meta.len() + self.blob.len());
        out.extend(b"glTF");
        for value in [
            2,
            (28 + meta.len() + self.blob.len()) as u32,
            meta.len() as u32,
            0x4e4f534a,
        ] {
            out.extend(value.to_le_bytes());
        }
        out.extend(meta);
        out.extend((self.blob.len() as u32).to_le_bytes());
        out.extend(0x004e4942_u32.to_le_bytes());
        out.extend(self.blob);
        Ok(out)
    }
}

fn surface(name: &str, color: [u8; 3], metal: f32, roughness: f32, glow: f32) -> Value {
    let rgb = color.map(|value| {
        let srgb = f32::from(value) / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    });
    json!({"name": name, "pbrMetallicRoughness": {"baseColorFactor": [rgb[0], rgb[1], rgb[2], 1.0],
          "metallicFactor": metal, "roughnessFactor": roughness}, "emissiveFactor": rgb.map(|value| value * glow)})
}

fn pad(blob: &mut Vec<u8>, fill: u8) {
    blob.resize(blob.len().next_multiple_of(4), fill);
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

#[derive(Default)]
struct Geometry {
    position: Vec<[f32; 3]>,
    normal: Vec<[f32; 3]>,
    indices: Vec<u16>,
    uv: Vec<[f32; 2]>,
}

impl Geometry {
    fn vertex(&mut self, position: Vec3, normal: Vec3) {
        self.position.push(position.to_array());
        self.normal.push(normal.to_array());
    }

    fn grid(&mut self, start: u16, width: u16, height: u16, flip: bool) {
        for row in 0..height - 1 {
            for col in 0..width - 1 {
                let a = start + row * width + col;
                let b = a + 1;
                let c = a + width;
                let d = c + 1;
                self.indices.extend(if flip {
                    [a, c, b, b, c, d]
                } else {
                    [a, b, c, b, d, c]
                });
            }
        }
    }
}

fn rounded_box(center: Vec3, half: Vec3, radius: f32) -> Geometry {
    let radius = radius.min(half.min_element() * 0.92);
    let inner = half - Vec3::splat(radius);
    let mut result = Geometry::default();
    let coordinates = |border: f32, core: f32| {
        [
            -border,
            (core - border) * 0.5 - core,
            -core,
            0.0,
            core,
            (border + core) * 0.5,
            border,
        ]
    };
    for [axis, horizontal, vertical] in [[0, 1, 2], [1, 2, 0], [2, 0, 1]] {
        for sign in [-1.0, 1.0] {
            let start = result.position.len() as u16;
            for y in coordinates(half[vertical], inner[vertical]) {
                for x in coordinates(half[horizontal], inner[horizontal]) {
                    let mut point = Vec3::ZERO;
                    point[axis] = sign * half[axis];
                    point[horizontal] = x;
                    point[vertical] = y;
                    let core = point.clamp(-inner, inner);
                    let normal = (point - core).normalize();
                    result.vertex(center + core + radius * normal, normal);
                }
            }
            result.grid(start, 7, 7, sign < 0.0);
        }
    }
    result
}

fn ellipsoid(center: Vec3, radii: Vec3, rotation: Quat) -> Geometry {
    const WIDTH: u16 = 23;
    const HEIGHT: u16 = 15;
    let mut result = Geometry::default();
    for row in 0..HEIGHT {
        let lat = (f32::from(row) / f32::from(HEIGHT - 1) - 0.5) * (PI - 0.0001);
        for col in 0..WIDTH {
            let yaw = f32::from(col) / f32::from(WIDTH - 1) * TAU;
            let unit = v(yaw.sin() * lat.cos(), lat.sin(), yaw.cos() * lat.cos());
            result.vertex(
                center + rotation * (unit * radii),
                rotation * (unit / radii).normalize(),
            );
            result.uv.push([
                f32::from(col) / f32::from(WIDTH - 1),
                f32::from(row) / f32::from(HEIGHT - 1),
            ]);
        }
    }
    result.grid(0, WIDTH, HEIGHT, false);
    result
}

struct Limb {
    upper: usize,
    lower: usize,
    phase: f32,
    paw: Option<Paw>,
}

struct Paw {
    node: usize,
    lengths: [f32; 2],
}

struct Rig {
    hips: usize,
    height: f32,
    torso: Option<usize>,
    head: usize,
    legs: Vec<Limb>,
    arms: Vec<usize>,
    tail: usize,
    ears: Vec<usize>,
    eyes: Vec<usize>,
    soft_parts: Vec<usize>,
}

fn robot(g: &mut Glb, d: &Draft) -> Rig {
    let root = g.node(None, "Root", Vec3::ZERO);
    let height = 0.14 + 0.50 * d.legs;
    let hips = g.node(Some(root), "Pelvis", v(0.0, height, 0.0));
    let torso = g.node(Some(hips), "Torso", v(0.0, 0.21, 0.0));
    let bw = d.body;
    g.box_at(
        torso,
        PRIMARY,
        Vec3::ZERO,
        v(0.226 * bw, 0.231, 0.163 * bw),
        0.065,
    );
    g.box_at(
        torso,
        SECONDARY,
        v(0.0, -0.174, 0.0),
        v(0.181 * bw, 0.065, 0.148 * bw),
        0.03,
    );
    g.box_at(
        torso,
        PANEL,
        v(0.0, 0.014, 0.144 * bw),
        v(0.166 * bw, 0.147, 0.035),
        0.028,
    );
    g.box_at(
        torso,
        DARK,
        v(0.0, 0.054, 0.173 * bw + 0.008),
        v(0.090 * bw, 0.060, 0.019),
        0.015,
    );
    for i in -1..=1 {
        g.box_at(
            torso,
            ACCENT,
            v(i as f32 * 0.043 * bw, 0.058, 0.173 * bw + 0.033),
            v(0.011, 0.019 + i as f32 * 0.008, 0.005),
            0.004,
        );
    }
    let head = g.node(Some(torso), "Head", v(0.0, 0.26, 0.0));
    let h = d.head;
    g.box_at(
        head,
        SECONDARY,
        v(0.0, -0.002, 0.0),
        v(0.070, 0.059, 0.070),
        0.025,
    );
    g.box_at(
        head,
        PRIMARY,
        v(0.0, 0.148 * h, 0.0),
        v(0.246 * h, 0.201 * h, 0.180 * h),
        0.077 * h,
    );
    g.box_at(
        head,
        DARK,
        v(0.0, 0.153 * h, 0.161 * h),
        v(0.194 * h, 0.126 * h, 0.039 * h),
        0.036 * h,
    );
    let mut eyes = Vec::new();
    for (side, label) in [(-1.0, "L"), (1.0, "R")] {
        let eye = g.node(
            Some(head),
            &format!("Eye.{label}"),
            v(0.082 * h * side, 0.164 * h, 0.202 * h),
        );
        eyes.push(eye);
        g.box_at(
            eye,
            ACCENT,
            Vec3::ZERO,
            v(0.047 * h, 0.041 * h, 0.0055 * h),
            0.0045 * h,
        );
        g.box_at(
            eye,
            LIGHT,
            v(-0.014 * h, 0.015 * h, 0.006 * h),
            v(0.011 * h, 0.007 * h, 0.002 * h),
            0.0015 * h,
        );
        g.ball(
            head,
            SECONDARY,
            v(0.231 * h * side, 0.13 * h, -0.016 * h),
            v(0.046, 0.070, 0.054) * h,
        );
        g.ball(
            head,
            ACCENT,
            v(0.262 * h * side, 0.13 * h, -0.016 * h),
            v(0.019, 0.037, 0.033) * h,
        );
    }
    g.box_at(
        head,
        ACCENT,
        v(0., 0.079 * h, 0.192 * h),
        v(0.042 * h, 0.006 * h, 0.005 * h),
        0.004 * h,
    );
    let antenna = g.node(Some(head), "Antenna", v(-0.112 * h, 0.322 * h, -0.035 * h));
    let tip = v(-0.045, 0.143, -0.016) * d.tail;
    g.oblong(antenna, SECONDARY, Vec3::ZERO, tip, 0.018, 0.016);
    g.ball(antenna, ACCENT, tip, v(0.035, 0.036, 0.030));
    let mut arms = Vec::new();
    let mut legs = Vec::new();
    for (side, label, phase) in [(-1.0, "L", 0.0), (1.0, "R", PI)] {
        let arm = g.node(
            Some(torso),
            &format!("Arm.{label}"),
            v(side * (0.226 * bw + 0.063), 0.125, 0.0),
        );
        arms.push(arm);
        g.ball(arm, SECONDARY, Vec3::ZERO, v(0.091, 0.092, 0.088));
        g.box_at(
            arm,
            PANEL,
            v(side * 0.027, -0.110, 0.0),
            v(0.075, 0.105, 0.082),
            0.032,
        );
        let hand = g.node(
            Some(arm),
            &format!("Hand.{label}"),
            v(side * 0.032, -0.231, 0.007),
        );
        g.box_at(
            hand,
            SECONDARY,
            v(0.0, -0.026, 0.001),
            v(0.068, 0.069, 0.060),
            0.029,
        );
        g.box_at(
            hand,
            ACCENT,
            v(0.0, 0.008, 0.056),
            v(0.043, 0.015, 0.009),
            0.008,
        );
        let upper = g.node(
            Some(hips),
            &format!("Leg.{label}"),
            v(side * 0.122 * bw, -0.018, 0.0),
        );
        let shin = g.node(
            Some(upper),
            &format!("Shin.{label}"),
            v(0.0, -0.244 * d.legs, 0.0),
        );
        legs.push(Limb {
            upper,
            lower: shin,
            phase,
            paw: None,
        });
        g.box_at(
            upper,
            SECONDARY,
            v(0., -0.112 * d.legs, 0.0),
            v(0.080, 0.141 * d.legs, 0.077),
            0.033,
        );
        g.box_at(
            shin,
            PRIMARY,
            v(0., -0.117 * d.legs, 0.013),
            v(0.080, 0.134 * d.legs, 0.079),
            0.037,
        );
        g.box_at(
            shin,
            ACCENT,
            v(0.0, -0.037, 0.085),
            v(0.044, 0.008, 0.012),
            0.006,
        );
        let foot = g.node(
            Some(shin),
            &format!("Foot.{label}"),
            v(0.0, -0.256 * d.legs, 0.034),
        );
        g.box_at(
            foot,
            SECONDARY,
            v(0.0, -0.080, 0.019),
            v(0.095, 0.042, 0.127),
            0.027,
        );
        g.box_at(
            foot,
            PANEL,
            v(0.0, -0.039, 0.022),
            v(0.091, 0.040, 0.113),
            0.030,
        );
    }
    Rig {
        hips,
        height,
        torso: None,
        head,
        legs,
        arms,
        tail: antenna,
        ears: Vec::new(),
        eyes,
        soft_parts: Vec::new(),
    }
}

fn animal(g: &mut Glb, d: &Draft) -> Rig {
    use groom::{EYE, INNER_EAR, IRIS, NOSE, PAW_PAD, Patch, Shape};
    let root = g.node(None, "Root", Vec3::ZERO);
    let height = 0.09 + 0.44 * d.legs;
    let hips = g.node(Some(root), "Pelvis", v(0., height, 0.));
    let torso = g.node(Some(hips), "Torso", Vec3::ZERO);
    let b = d.body;
    let fur = |g: &mut Glb, node, material, patch: Patch, count, length, seed| {
        patch.coat(g, node, material);
        patch.fibers(g, node, count, length, seed, material == SECONDARY);
    };
    let axis = |top: Vec3, bottom: Vec3, width, depth, shape| {
        let delta = top - bottom;
        Patch::new(
            (top + bottom) * 0.5,
            v(width, delta.length() * 0.65, depth),
            Quat::from_rotation_arc(Vec3::Y, delta.normalize()),
            shape,
        )
    };
    let ruff = g.node(Some(torso), "ChestRuff", v(0., 0.10, 0.235 * b));
    fur(
        g,
        ruff,
        SECONDARY,
        axis(
            v(0., 0.185, 0.15 * b),
            v(0., -0.09, 0.007 * b),
            0.143 * b,
            0.121 * b,
            Shape::Ruff,
        ),
        820,
        0.061 * b.sqrt(),
        2914,
    );
    let head = g.node(Some(torso), "Head", v(0., 0.269, 0.363 * b));
    let h = d.head;
    fur(
        g,
        head,
        PRIMARY,
        Patch::new(
            v(0., 0.031, 0.022) * h,
            v(0.154, 0.128, 0.186) * h,
            Quat::from_rotation_x(-0.13),
            Shape::Face,
        ),
        510,
        0.027 * h.sqrt(),
        11525,
    );
    g.ball(
        head,
        PRIMARY,
        v(0., -0.025, 0.158) * h,
        v(0.101, 0.060, 0.128) * h,
    );
    g.ball(
        head,
        NOSE,
        v(0., -0.061, 0.174) * h,
        v(0.075, 0.023, 0.103) * h,
    );
    for side in [-1., 1.] {
        fur(
            g,
            head,
            SECONDARY,
            Patch::new(
                v(0.050 * side, -0.046, 0.164) * h,
                v(0.068, 0.052, 0.125) * h,
                Quat::from_rotation_y(-0.15 * side),
                Shape::Face,
            ),
            125,
            0.018 * h.sqrt(),
            13137 + u32::from(side > 0.0),
        );
    }
    g.ball(
        head,
        NOSE,
        v(0., -0.027, 0.274) * h,
        v(0.045, 0.025, 0.035) * h,
    );
    groom::whiskers(g, head, h);
    let mut ears = Vec::new();
    let mut eyes = Vec::new();
    for (side, label) in [(-1., "L"), (1., "R")] {
        let ear = g.node(
            Some(head),
            &format!("Ear.{label}"),
            v(0.101 * side, 0.125, -0.022) * h,
        );
        ears.push(ear);
        groom::ear(g, ear, side, h, PRIMARY, INNER_EAR);
        let ear_patch = axis(
            v(0.094 * side, 0.111, -0.01) * h,
            v(0., 0.035, -0.010) * h,
            0.052 * h,
            0.018 * h,
            Shape::Limb,
        );
        ear_patch.fibers(
            g,
            ear,
            140,
            0.020 * h.sqrt(),
            19371 + u32::from(side > 0.0),
            false,
        );
        let eye = g.node(
            Some(head),
            &format!("Eye.{label}"),
            v(0.105 * side, 0.054, 0.125) * h,
        );
        eyes.push(eye);
        let facing = Quat::from_euler(EulerRot::YXZ, 0.42 * side, 0.05, 0.21 * side);
        for (mat, pivot, size) in [
            (NOSE, Vec3::ZERO, v(0.040, 0.031, 0.020)),
            (EYE, v(0., 0., 0.009), v(0.034, 0.026, 0.020)),
            (IRIS, v(0.002, -0.001, 0.026), v(0.019, 0.019, 0.007)),
            (EYE, v(0.003, -0.001, 0.032), v(0.008, 0.016, 0.004)),
            (LIGHT, v(-0.005, 0.007, 0.033), v(0.004, 0.004, 0.002)),
        ] {
            g.add(eye, mat, ellipsoid(facing * (pivot * h), size * h, facing));
        }
    }
    let tail = g.node(Some(torso), "Tail", v(0., 0.099, -0.304 * b));
    let t = d.tail;
    let base_end = v(0.044, 0.045, -0.319) * t;
    fur(
        g,
        tail,
        PRIMARY,
        axis(
            base_end,
            v(0., 0., 0.047) * t,
            0.103 * t.sqrt(),
            0.093 * t.sqrt(),
            Shape::Tail,
        ),
        690,
        0.063 * t.sqrt(),
        21179,
    );
    let tip = g.node(Some(tail), "TailTip", base_end);
    fur(
        g,
        tip,
        PANEL,
        axis(
            v(0.067, 0.074, -0.219) * t,
            v(-0.015, -0.01, 0.119) * t,
            0.110 * t.sqrt(),
            0.087 * t.sqrt(),
            Shape::Tail,
        ),
        710,
        0.074 * t.sqrt(),
        29435,
    );
    fur(
        g,
        tip,
        SECONDARY,
        axis(
            v(0.105, 0.099, -0.307) * t,
            v(0.039, 0.045, -0.140) * t,
            0.070 * t.sqrt(),
            0.046 * t.sqrt(),
            Shape::Tail,
        ),
        350,
        0.059 * t.sqrt(),
        33741,
    );
    let mut legs = Vec::new();
    for (z, label, rear) in [(0.236, "Front", false), (-0.236, "Rear", true)] {
        for (side, suffix) in [(-1., "L"), (1., "R")] {
            let leg = g.node(
                Some(hips),
                &format!("Leg.{label}.{suffix}"),
                v(0.134 * side * b, 0.0, z * b),
            );
            let knee = g.node(
                Some(leg),
                &format!("Knee.{label}.{suffix}"),
                v(0.0, -0.225 * d.legs, 0.0),
            );
            let paw = g.node(
                Some(knee),
                &format!("Paw.{label}.{suffix}"),
                v(0., -0.215 * d.legs, 0.0),
            );
            g.ball(paw, PAW_PAD, v(0., -0.051, 0.038), v(0.049, 0.039, 0.066));
            for toe in [-1.0, 0.0, 1.0] {
                let toe = toe as f32;
                g.ball(
                    paw,
                    PAW_PAD,
                    v(toe * 0.025, -0.052, 0.085 - toe.abs() * 0.011),
                    v(0.019, 0.027, 0.036),
                );
            }
            let phase = match (rear, side > 0.) {
                (false, false) => 0.0,
                (false, true) => PI,
                (true, false) => 1.5 * PI,
                (true, true) => 0.5 * PI,
            };
            legs.push(Limb {
                upper: leg,
                lower: knee,
                phase,
                paw: Some(Paw {
                    node: paw,
                    lengths: [0.225 * d.legs, 0.215 * d.legs],
                }),
            });
        }
    }
    continuous_coat::build(g, d, torso, &legs, height);
    Rig {
        hips,
        height,
        torso: Some(torso),
        head,
        legs,
        arms: Vec::new(),
        tail,
        ears,
        eyes,
        soft_parts: vec![ruff, tip],
    }
}

#[derive(Clone, Copy)]
enum Gait {
    Idle,
    Walk,
    Run,
}

impl Gait {
    fn timing(self) -> (&'static str, f32, f32) {
        match self {
            Self::Idle => ("Idle", 2.4, 0.0),
            Self::Walk => ("Walk", 0.94, 0.41),
            Self::Run => ("Run", 0.60, 0.68),
        }
    }
}

fn paw_pose(
    gait: Gait,
    phase: f32,
    pelvis_height: f32,
    [upper, lower]: [f32; 2],
    rear: bool,
) -> [f32; 3] {
    if matches!(gait, Gait::Idle) {
        return [0.0; 3];
    }
    let run = matches!(gait, Gait::Run);
    let cycle = (phase / TAU).rem_euclid(1.0);
    let contact = if run { 0.51 } else { 0.64 };
    let reach = upper + lower;
    let travel = reach * if run { 0.92 } else { 0.54 };
    let (z, lift, tip) = if cycle <= contact {
        (travel * (0.5 - cycle / contact), 0.0, 0.0)
    } else {
        let s = (cycle - contact) / (1.0 - contact);
        let forward = s * s * (3.0 - 2.0 * s);
        let arc = (PI * s).sin();
        (
            travel * (forward - 0.5),
            reach * arc * if run { 0.25 } else { 0.17 },
            0.16 * arc * (1.0 - 2.0 * s),
        )
    };
    let down = pelvis_height - (0.09 + lift);
    let cosine = ((down * down + z * z - upper * upper - lower * lower) / (2.0 * upper * lower))
        .clamp(-1.0, 1.0);
    let knee = cosine.acos() * if rear { -1.0 } else { 1.0 };
    let hip = (-z).atan2(down) - (lower * knee.sin()).atan2(upper + lower * knee.cos());
    [hip, knee, -hip - knee + tip]
}

fn animate(out: &mut Glb, rig: &Rig, kind: Kind, gait: Gait) {
    let (name, duration, stride) = gait.timing();
    let step = (0..=24).map(|i| i as f32 / 24.0).collect::<Vec<_>>();
    let times = out.floats(
        &step
            .iter()
            .map(|time| [time * duration])
            .collect::<Vec<_>>(),
        "SCALAR",
        true,
    );
    let phases = step.iter().map(|x| x * TAU).collect::<Vec<_>>();
    let mut samplers = Vec::new();
    let mut channels = Vec::new();
    let mut add = |node, path, data| {
        let index = samplers.len();
        samplers.push(json!({"input": times, "interpolation": "LINEAR", "output": data}));
        channels.push(json!({"sampler": index, "target": {"node": node, "path": path}}));
    };
    let pelvis = phases
        .iter()
        .map(|phase| {
            let bounce = if let Some(paw) = &rig.legs[0].paw {
                let reach: f32 = paw.lengths.iter().sum();
                match gait {
                    Gait::Idle => 0.0,
                    Gait::Walk => reach * (-0.103 + 0.012 * (1.0 - (phase * 2.0).cos())),
                    Gait::Run => reach * (-0.180 + 0.025 * (1.0 - (phase * 2.0).cos())),
                }
            } else if stride > 0.0 {
                stride * 0.043 * (1.0 - (phase * 2.0).cos())
            } else {
                0.004 * (1.0 - phase.cos())
            };
            [0.0, rig.height + bounce, 0.0]
        })
        .collect::<Vec<_>>();
    let pelvis_keys = out.floats(&pelvis, "VEC3", false);
    add(rig.hips, "translation", pelvis_keys);
    if matches!(gait, Gait::Idle) {
        let blink = step
            .iter()
            .map(|t| {
                [
                    1.0,
                    1.0 - 0.94 * (-((t - 0.625) / 0.031).powi(2)).exp(),
                    1.0,
                ]
            })
            .collect::<Vec<_>>();
        let keys = out.floats(&blink, "VEC3", false);
        for &eye in &rig.eyes {
            add(eye, "scale", keys);
        }
    }
    let mut rotate = |out: &mut Glb, node, rotations: Vec<[f32; 3]>| {
        let values = rotations
            .into_iter()
            .map(|[x, y, z]| Quat::from_euler(EulerRot::XYZ, x, y, z).to_array())
            .collect::<Vec<_>>();
        let data = out.floats(&values, "VEC4", false);
        add(node, "rotation", data);
    };
    if let Some(torso) = rig.torso {
        rotate(
            out,
            torso,
            phases
                .iter()
                .map(|&phase| {
                    [
                        0.020 * stride * (phase * 2.0 - 0.4).sin(),
                        0.047 * stride * (phase - 0.65).cos(),
                        0.105 * stride * (phase - 0.35).sin(),
                    ]
                })
                .collect(),
        );
    }
    rotate(
        out,
        rig.head,
        phases
            .iter()
            .map(|p| {
                [
                    0.028 * p.cos() + stride * 0.050,
                    0.052 * p.sin(),
                    0.019 * p.cos(),
                ]
            })
            .collect(),
    );
    rotate(
        out,
        rig.tail,
        phases
            .iter()
            .map(|p| {
                [
                    0.08 * p.sin(),
                    (0.15 + stride * 0.33) * p.cos(),
                    0.09 * p.sin(),
                ]
            })
            .collect(),
    );
    for (index, &ear) in rig.ears.iter().enumerate() {
        rotate(
            out,
            ear,
            phases
                .iter()
                .map(|p| {
                    [
                        0.033 * p.sin(),
                        0.0,
                        (0.072 + stride * 0.08) * (p + PI * index as f32).cos(),
                    ]
                })
                .collect(),
        );
    }
    for (index, &part) in rig.soft_parts.iter().enumerate() {
        let gain = 0.25 + stride * 0.90;
        rotate(
            out,
            part,
            phases
                .iter()
                .map(|&p| {
                    let phase = p - 0.7 - index as f32 * 0.67;
                    [
                        0.10 * gain * phase.sin(),
                        0.24 * index as f32 * gain * phase.cos(),
                        0.06 * gain * (phase * 2.0).sin(),
                    ]
                })
                .collect(),
        );
    }
    for (index, &arm) in rig.arms.iter().enumerate() {
        let phase = index as f32 * PI;
        rotate(
            out,
            arm,
            phases
                .iter()
                .map(|p| {
                    [
                        -stride * 0.80 * (p + phase).cos(),
                        0.0,
                        0.08 - 0.16 * index as f32,
                    ]
                })
                .collect(),
        );
    }
    for (index, leg) in rig.legs.iter().enumerate() {
        let shift = if kind == Kind::Quadruped && matches!(gait, Gait::Run) {
            if index == 1 || index == 2 { PI } else { 0.0 }
        } else {
            leg.phase
        };
        if let Some(paw) = &leg.paw {
            let poses = phases
                .iter()
                .zip(&pelvis)
                .map(|(p, height)| paw_pose(gait, p + shift, height[1], paw.lengths, index > 1))
                .collect::<Vec<_>>();
            for (axis, node) in [leg.upper, leg.lower, paw.node].into_iter().enumerate() {
                rotate(
                    out,
                    node,
                    poses.iter().map(|frame| [frame[axis], 0.0, 0.0]).collect(),
                );
            }
            continue;
        }
        rotate(
            out,
            leg.upper,
            phases
                .iter()
                .map(|p| [stride * (p + shift).cos(), 0.0, 0.0])
                .collect(),
        );
        rotate(
            out,
            leg.lower,
            phases
                .iter()
                .map(|p| [stride * 0.9 * (p + shift).sin().max(0.0), 0.0, 0.0])
                .collect(),
        );
    }
    out.animations
        .push(json!({"name": name, "samplers": samplers, "channels": channels}));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::decode_glb;

    #[test]
    fn creates_each_anatomy_with_editable_bounds_and_playable_rig() {
        for kind in [Kind::Robot, Kind::Quadruped] {
            let draft = Draft::for_kind(kind);
            let bytes = build(&draft).unwrap();
            assert_eq!(bytes, build(&draft).unwrap());
            let model = decode_glb(&bytes).unwrap();
            assert!(
                model.bounds_min[1].abs() < 1e-5,
                "{} floors at {:?}",
                kind.label(),
                model.bounds_min
            );
            assert!(model.bounds_max[1] > 0.9 && model.primitives.len() >= 22);
            let bones = model.attachment_bones();
            assert!(bones.contains(&"Head") && bones.contains(&"Eye.R"));
            assert!(bones.contains(&if kind == Kind::Robot {
                "Hand.R"
            } else {
                "Tail"
            }));
            if kind == Kind::Quadruped {
                let fibers = model
                    .primitives
                    .iter()
                    .filter(|part| part.material.guard_fur);
                let vertices = fibers.clone().map(|p| p.vertices.len()).sum::<usize>();
                assert!(vertices >= 20_000);
                assert!(
                    fibers
                        .clone()
                        .all(|p| p.material.double_sided && p.material.alpha_cutoff.is_some())
                );
                assert!(
                    fibers
                        .flat_map(|p| &p.vertices)
                        .any(|vertex| vertex.uv[1] == 1.0)
                );
                assert!(model.primitives.iter().any(|p| {
                    p.material.base_color_texture.is_some() && p.material.normal_texture.is_some()
                }));
                assert!(bones.contains(&"ChestRuff") && bones.contains(&"TailTip"));
            }
            assert_eq!(
                model
                    .animations
                    .iter()
                    .map(|clip| clip.name.as_str())
                    .collect::<Vec<_>>(),
                ["Idle", "Walk", "Run"]
            );
            let walk = model.pose(Some(1), 0.12).unwrap();
            let run = model.pose(Some(2), 0.12).unwrap();
            assert!(
                walk.iter()
                    .flatten()
                    .zip(run.iter().flatten())
                    .any(|(a, b)| {
                        Vec3::from_array(a.position).distance(Vec3::from_array(b.position)) > 0.05
                    })
            );
            let edited = decode_glb(
                &build(&Draft {
                    head: 1.4,
                    legs: 1.35,
                    primary: [210, 49, 91],
                    ..draft
                })
                .unwrap(),
            )
            .unwrap();
            assert!(edited.bounds_max[1] - model.bounds_max[1] > 0.14);
            assert!(
                model
                    .primitives
                    .iter()
                    .zip(&edited.primitives)
                    .any(|(before, after)| {
                        before.material.base_color != after.material.base_color
                    })
            );
        }
        assert!(
            build(&Draft {
                body: f32::NAN,
                ..Draft::default()
            })
            .is_err()
        );
    }

    #[test]
    fn authored_quadruped_keeps_support_near_the_floor_between_swing_steps() {
        let model = decode_glb(&build(&Draft::for_kind(Kind::Quadruped)).unwrap()).unwrap();
        for clip in [1, 2] {
            for frame in 0..48 {
                let t = frame as f32 / 48.0 * model.animations[clip].duration;
                let pose = model.pose(Some(clip), t).unwrap();
                let sole = pose
                    .iter()
                    .flatten()
                    .map(|vertex| vertex.position[1])
                    .fold(f32::INFINITY, f32::min);
                assert!(
                    (-0.009..=0.013).contains(&sole),
                    "support height {sole} for clip {clip} at {t}"
                );
            }
        }
    }

    #[test]
    fn generated_quadruped_has_one_connected_weighted_coat_at_proportion_limits() {
        for (body, legs) in [(1.0, 1.0), (1.55, 0.55), (0.55, 1.55)] {
            let draft = Draft {
                body,
                legs,
                ..Draft::for_kind(Kind::Quadruped)
            };
            let bytes = build(&draft).unwrap();
            let source = gltf::Gltf::from_slice(&bytes).unwrap();
            let node = source.nodes().find(|node| node.skin().is_some()).unwrap();
            let mesh = node.mesh().unwrap();
            let coat = mesh.primitives().next().unwrap();
            let reader = coat.reader(|_| source.blob.as_deref());
            let weights: Vec<_> = reader.read_weights(0).unwrap().into_f32().collect();
            assert!(
                weights
                    .iter()
                    .all(|w| (w.iter().sum::<f32>() - 1.0).abs() < 1e-5)
            );
            assert!(
                weights
                    .iter()
                    .filter(|w| w.iter().filter(|&&x| x > 0.1).count() > 1)
                    .count()
                    > 40
            );
            assert!(
                mesh.primitives()
                    .skip(1)
                    .all(|fiber| fiber.get(&gltf::Semantic::Weights(0)).is_some())
            );

            let mut neighbors = vec![Vec::new(); weights.len()];
            let indices: Vec<_> = reader
                .read_indices()
                .unwrap()
                .into_u32()
                .map(|i| i as usize)
                .collect();
            for &[a, b, c] in indices.as_chunks::<3>().0 {
                neighbors[a].extend([b, c]);
                neighbors[b].extend([a, c]);
                neighbors[c].extend([a, b]);
            }
            let mut visited = vec![false; weights.len()];
            let mut stack = vec![indices[0]];
            while let Some(index) = stack.pop() {
                if !std::mem::replace(&mut visited[index], true) {
                    stack.extend(&neighbors[index]);
                }
            }
            assert!(
                indices.iter().all(|&index| visited[index]),
                "coat disconnected at body {body} and legs {legs}"
            );
            let model = decode_glb(&bytes).unwrap();
            let names = model.bone_names();
            assert!(
                names.contains(&"Torso")
                    && names.contains(&"Paw.Front.L")
                    && names.contains(&"Paw.Rear.R")
            );
        }
    }
}
