use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const MAX_NODES: usize = 32;
pub type NodeId = u32;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum PreviewLight {
    #[default]
    Scene = 0,
    Outdoor = 1,
    Indoor = 2,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub output: Output,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub position: [f32; 2],
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    Color([f32; 3]),
    Float(f32),
    Multiply([Option<NodeId>; 2]),
    Mix([Option<NodeId>; 3]),
    TextureScale([Option<NodeId>; 2]),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Output {
    pub position: [f32; 2],
    pub inputs: [Option<NodeId>; 6],
}

impl Default for Output {
    fn default() -> Self {
        Self {
            position: [530.0, 102.0],
            inputs: [None; 6],
        }
    }
}

pub const OUTPUT_INPUTS: [&str; 6] = [
    "Цвет",
    "Шероховатость",
    "Свечение",
    "Масштаб UV",
    "Металличность",
    "Сила нормалей",
];

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Color(_) => "Цвет",
            Self::Float(_) => "Число",
            Self::Multiply(_) => "Умножить",
            Self::Mix(_) => "Смешать",
            Self::TextureScale(_) => "Масштаб текстуры",
        }
    }

    pub fn input_labels(self) -> &'static [&'static str] {
        match self {
            Self::Multiply(_) => &["A", "B"],
            Self::Mix(_) => &["A", "B", "Доля B"],
            Self::TextureScale(_) => &["По U", "По V"],
            Self::Color(_) | Self::Float(_) => &[],
        }
    }

    pub fn inputs(&self) -> &[Option<NodeId>] {
        match self {
            Self::Multiply(inputs) | Self::TextureScale(inputs) => inputs,
            Self::Mix(inputs) => inputs,
            Self::Color(_) | Self::Float(_) => &[],
        }
    }

    pub fn inputs_mut(&mut self) -> &mut [Option<NodeId>] {
        match self {
            Self::Multiply(inputs) | Self::TextureScale(inputs) => inputs,
            Self::Mix(inputs) => inputs,
            Self::Color(_) | Self::Float(_) => &mut [],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Compiled {
    // The shader consumes linear RGB. A missing port preserves the GLB/manual tuning.
    pub base_color: Option<[f32; 3]>,
    pub roughness: Option<f32>,
    pub emission: Option<[f32; 3]>,
    pub uv_scale: Option<[f32; 2]>,
    pub metallic: Option<f32>,
    pub normal_strength: Option<f32>,
}

impl Compiled {
    pub fn wgsl(self) -> String {
        let rgb = |[r, g, b]: [f32; 3]| format!("vec3<f32>({r:?}, {g:?}, {b:?})");
        let [u, v] = self.uv_scale.unwrap_or([1.0; 2]);
        let flags = u32::from(self.base_color.is_some())
            | u32::from(self.roughness.is_some()) << 1
            | u32::from(self.emission.is_some()) << 2
            | u32::from(self.uv_scale.is_some()) << 3
            | u32::from(self.metallic.is_some()) << 4
            | u32::from(self.normal_strength.is_some()) << 5;
        format!(
            "struct StudioMaterial {{\n    base_color: vec3<f32>,\n    roughness: f32,\n    emission: vec3<f32>,\n    flags: u32,\n    uv_scale: vec2<f32>,\n    metallic: f32,\n    normal_strength: f32,\n}}\n\nfn studio_material() -> StudioMaterial {{\n    return StudioMaterial({}, {:?}, {}, {flags}u, vec2<f32>({u:?}, {v:?}), {:?}, {:?});\n}}\n",
            rgb(self.base_color.unwrap_or([1.0; 3])),
            self.roughness.unwrap_or(0.5),
            rgb(self.emission.unwrap_or([0.0; 3])),
            self.metallic.unwrap_or(0.0),
            self.normal_strength.unwrap_or(1.0),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueType {
    Color,
    Float,
    Uv,
}

impl ValueType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Color => "цвет",
            Self::Float => "число",
            Self::Uv => "масштаб UV",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Issue {
    /// None refers to the material output card.
    pub node: Option<NodeId>,
    pub port: Option<usize>,
    pub message: String,
    pub blocking: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub compiled: Option<Compiled>,
    pub issues: Vec<Issue>,
    pub types: BTreeMap<NodeId, ValueType>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Preset {
    Skin,
    Cloth,
    Wood,
    Stone,
    Metal,
    Glass,
    Water,
    AmberSignal,
}

impl Preset {
    pub const ALL: [Self; 8] = [
        Self::Skin,
        Self::Cloth,
        Self::Wood,
        Self::Stone,
        Self::Metal,
        Self::Glass,
        Self::Water,
        Self::AmberSignal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Skin => "Кожа",
            Self::Cloth => "Ткань",
            Self::Wood => "Дерево",
            Self::Stone => "Камень",
            Self::Metal => "Металл",
            Self::Glass => "Стекло · стилизация",
            Self::Water => "Вода · стилизация",
            Self::AmberSignal => "Янтарное свечение",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Skin => "Тёплая матовая поверхность. Рисунок берётся из GLB.",
            Self::Cloth => "Матовая поверхность с уменьшенной силой карты нормалей.",
            Self::Wood => "Тёплый тон. Волокна берутся из текстуры GLB.",
            Self::Stone => "Холодный нейтральный тон с высокой шероховатостью.",
            Self::Metal => "Металличность 1 и низкая шероховатость для чёткого блика.",
            Self::Glass => "Непрозрачная глянцевая поверхность с голубым оттенком.",
            Self::Water => {
                "Непрозрачная глянцевая поверхность. Для волн и физики используйте зону воды."
            }
            Self::AmberSignal => "Самосветящаяся янтарная поверхность.",
        }
    }
}

impl Graph {
    pub fn add(&mut self, kind: Kind, position: [f32; 2]) -> Option<NodeId> {
        if self.nodes.len() >= MAX_NODES {
            return None;
        }
        let id = (1..=MAX_NODES as u32).find(|id| self.nodes.iter().all(|node| node.id != *id))?;
        self.nodes.push(Node { id, position, kind });
        Some(id)
    }

    pub fn remove(&mut self, id: NodeId) {
        self.nodes.retain(|node| node.id != id);
        for input in self
            .nodes
            .iter_mut()
            .flat_map(|node| node.kind.inputs_mut())
            .chain(&mut self.output.inputs)
        {
            if *input == Some(id) {
                *input = None;
            }
        }
    }

    pub fn storage_valid(&self) -> bool {
        let in_bounds = |position: [f32; 2]| {
            position
                .iter()
                .all(|value| (-4096.0..=4096.0).contains(value))
        };
        let mut seen = [false; MAX_NODES + 1];
        self.nodes.len() <= MAX_NODES
            && in_bounds(self.output.position)
            && self.nodes.iter().all(|node| {
                in_bounds(node.position)
                    && (1..=MAX_NODES as u32).contains(&node.id)
                    && !std::mem::replace(&mut seen[node.id as usize], true)
                    && match node.kind {
                        Kind::Color(rgb) => rgb.iter().all(|channel| (0.0..=1.0).contains(channel)),
                        Kind::Float(value) => (-64.0..=64.0).contains(&value),
                        _ => true,
                    }
            })
    }

    pub fn preset(preset: Preset) -> Self {
        let mut graph = Self::default();
        let (tint, roughness, metallic, normal_strength) = match preset {
            Preset::Skin => ([1.0, 0.88, 0.79], 0.56, 0.0, 0.75),
            Preset::Cloth => ([0.82, 0.86, 0.93], 0.93, 0.0, 0.67),
            Preset::Wood => ([0.93, 0.75, 0.57], 0.67, 0.0, 0.95),
            Preset::Stone => ([0.76, 0.78, 0.76], 0.86, 0.0, 1.1),
            Preset::Metal => ([0.76, 0.80, 0.83], 0.22, 1.0, 0.8),
            Preset::Glass => ([0.63, 0.82, 0.94], 0.095, 0.0, 0.25),
            Preset::Water => ([0.40, 0.69, 0.84], 0.13, 0.0, 0.4),
            Preset::AmberSignal => ([0.94, 0.54, 0.24], 0.32, 0.0, 0.7),
        };
        let color = graph.add(Kind::Color(tint), [34.0, 32.0]);
        let matte = graph.add(Kind::Float(roughness), [34.0, 132.0]);
        graph.output.inputs[0] = color;
        graph.output.inputs[1] = matte;
        graph.output.inputs[4] = graph.add(Kind::Float(metallic), [34.0, 232.0]);
        graph.output.inputs[5] = graph.add(Kind::Float(normal_strength), [34.0, 332.0]);
        if preset == Preset::AmberSignal {
            let gain = graph.add(Kind::Float(2.2), [264.0, 120.0]);
            graph.output.inputs[2] = graph.add(Kind::Multiply([color, gain]), [268.0, 220.0]);
        }
        graph
    }

    pub fn compile(&self) -> Option<Compiled> {
        self.evaluate(false).compiled
    }

    pub fn report(&self) -> Report {
        self.evaluate(true)
    }

    fn evaluate(&self, diagnose_unused: bool) -> Report {
        if !self.storage_valid() {
            return Report {
                issues: vec![Issue {
                    node: None,
                    port: None,
                    message: "Некорректные данные графа.".into(),
                    blocking: true,
                }],
                ..Report::default()
            };
        }
        let mut evaluator = Evaluator {
            nodes: self.nodes.iter().map(|node| (node.id, node.kind)).collect(),
            states: BTreeMap::new(),
            issues: Vec::new(),
            blocking: true,
        };
        let [
            base_color,
            roughness,
            emission,
            uv_scale,
            metallic,
            normal_strength,
        ] = self.output.inputs;
        let compiled = Compiled {
            base_color: evaluator.output(
                base_color,
                0,
                ValueType::Color,
                |value| match value {
                    Value::Color(color) if color.iter().all(|c| (0.0..=1.0).contains(c)) => {
                        Some(color)
                    }
                    _ => None,
                },
                "Диапазон цвета 0–1.",
            ),
            roughness: evaluator.output(
                roughness,
                1,
                ValueType::Float,
                |value| match value {
                    Value::Float(value) if (0.05..=1.0).contains(&value) => Some(value),
                    _ => None,
                },
                "Диапазон шероховатости 0,05–1.",
            ),
            emission: evaluator.output(
                emission,
                2,
                ValueType::Color,
                |value| match value {
                    Value::Color(color) if color.iter().all(|c| (0.0..=16.0).contains(c)) => {
                        Some(color)
                    }
                    _ => None,
                },
                "Диапазон линейного свечения 0–16.",
            ),
            uv_scale: evaluator.output(
                uv_scale,
                3,
                ValueType::Uv,
                |value| match value {
                    Value::Uv(uv) => Some(uv),
                    _ => None,
                },
                "Диапазон повторов UV 0,05–64.",
            ),
            metallic: evaluator.output_float(metallic, 4, 0.0..=1.0, "Диапазон металличности 0–1."),
            normal_strength: evaluator.output_float(
                normal_strength,
                5,
                0.0..=3.0,
                "Диапазон силы нормалей 0–3.",
            ),
        };
        let compiled = evaluator.issues.is_empty().then_some(compiled);
        if diagnose_unused {
            evaluator.blocking = false;
            for node in &self.nodes {
                if !evaluator.states.contains_key(&node.id) {
                    evaluator.node(node.id, None, 0);
                }
            }
        }
        let types = evaluator
            .states
            .into_iter()
            .filter_map(|(id, state)| match state {
                State::Done(Some(value)) => Some((id, value.kind())),
                _ => None,
            })
            .collect();
        Report {
            compiled,
            issues: evaluator.issues,
            types,
        }
    }
}

#[derive(Clone, Copy)]
enum Value {
    Float(f32),
    Color([f32; 3]),
    Uv([f32; 2]),
}

impl Value {
    fn kind(self) -> ValueType {
        match self {
            Self::Float(_) => ValueType::Float,
            Self::Color(_) => ValueType::Color,
            Self::Uv(_) => ValueType::Uv,
        }
    }

    fn finite(self) -> bool {
        match self {
            Self::Float(value) => value.is_finite(),
            Self::Color(value) => value.into_iter().all(f32::is_finite),
            Self::Uv(value) => value.into_iter().all(f32::is_finite),
        }
    }
}

#[derive(Clone, Copy)]
enum State {
    Visiting,
    Done(Option<Value>),
}

struct Evaluator {
    nodes: BTreeMap<NodeId, Kind>,
    states: BTreeMap<NodeId, State>,
    issues: Vec<Issue>,
    blocking: bool,
}

impl Evaluator {
    fn issue(&mut self, node: Option<NodeId>, port: Option<usize>, message: impl Into<String>) {
        self.issues.push(Issue {
            node,
            port,
            message: message.into(),
            blocking: self.blocking,
        });
    }

    fn output<T>(
        &mut self,
        target: Option<NodeId>,
        port: usize,
        expected: ValueType,
        mapper: impl FnOnce(Value) -> Option<T>,
        range_message: &str,
    ) -> Option<T> {
        let value = self.node(target?, None, port)?;
        if value.kind() != expected {
            self.issue(None, Some(port), format!("Требуется {}.", expected.label()));
            return None;
        }
        mapper(value).or_else(|| {
            self.issue(None, Some(port), range_message);
            None
        })
    }

    fn output_float(
        &mut self,
        target: Option<NodeId>,
        port: usize,
        range: std::ops::RangeInclusive<f32>,
        message: &str,
    ) -> Option<f32> {
        self.output(
            target,
            port,
            ValueType::Float,
            |value| match value {
                Value::Float(value) if range.contains(&value) => Some(value),
                _ => None,
            },
            message,
        )
    }

    fn input(&mut self, target: Option<NodeId>, from: NodeId, port: usize) -> Option<Value> {
        match target {
            Some(target) => self.node(target, Some(from), port),
            None => {
                self.issue(Some(from), Some(port), "Подключите вход.");
                None
            }
        }
    }

    fn node(&mut self, id: NodeId, from: Option<NodeId>, port: usize) -> Option<Value> {
        match self.states.get(&id).copied() {
            Some(State::Visiting) => {
                self.issue(from, Some(port), format!("Цикл через узел {id}."));
                return None;
            }
            Some(State::Done(value)) => return value,
            None => {}
        }
        let Some(kind) = self.nodes.get(&id).copied() else {
            self.issue(from, Some(port), format!("Узел {id} отсутствует."));
            return None;
        };
        self.states.insert(id, State::Visiting);
        let value = self.compute(id, kind).filter(|value| {
            if value.finite() {
                return true;
            }
            self.issue(Some(id), None, "Число вышло за допустимый диапазон.");
            false
        });
        self.states.insert(id, State::Done(value));
        value
    }

    fn compute(&mut self, id: NodeId, kind: Kind) -> Option<Value> {
        match kind {
            Kind::Color(color) => Some(Value::Color(color.map(srgb_to_linear))),
            Kind::Float(value) => Some(Value::Float(value)),
            Kind::Multiply([a, b]) => {
                let a = self.input(a, id, 0);
                let b = self.input(b, id, 1);
                match (a?, b?) {
                    (Value::Float(a), Value::Float(b)) => Some(Value::Float(a * b)),
                    (Value::Color(a), Value::Color(b)) => {
                        Some(Value::Color(std::array::from_fn(|i| a[i] * b[i])))
                    }
                    (Value::Float(factor), Value::Color(color))
                    | (Value::Color(color), Value::Float(factor)) => {
                        Some(Value::Color(color.map(|channel| channel * factor)))
                    }
                    _ => {
                        self.issue(Some(id), None, "Умножение принимает числа и цвета.");
                        None
                    }
                }
            }
            Kind::Mix([a, b, t]) => {
                let a = self.input(a, id, 0);
                let b = self.input(b, id, 1);
                let t = self.input(t, id, 2);
                let Value::Float(t) = t? else {
                    self.issue(Some(id), Some(2), "Доля B должна быть числом.");
                    return None;
                };
                if !(0.0..=1.0).contains(&t) {
                    self.issue(Some(id), Some(2), "Диапазон доли B 0–1.");
                    return None;
                }
                let mix = |a: f32, b: f32| a * (1.0 - t) + b * t;
                match (a?, b?) {
                    (Value::Float(a), Value::Float(b)) => Some(Value::Float(mix(a, b))),
                    (Value::Color(a), Value::Color(b)) => {
                        Some(Value::Color(std::array::from_fn(|i| mix(a[i], b[i]))))
                    }
                    _ => {
                        self.issue(Some(id), None, "Соедините два цвета или два числа.");
                        None
                    }
                }
            }
            Kind::TextureScale([u, v]) => {
                let u = self.scale(u, id, 0);
                let v = self.scale(v, id, 1);
                Some(Value::Uv([u?, v?]))
            }
        }
    }

    fn scale(&mut self, target: Option<NodeId>, id: NodeId, port: usize) -> Option<f32> {
        match self.input(target, id, port)? {
            Value::Float(value) if (0.05..=64.0).contains(&value) => Some(value),
            Value::Float(_) => {
                self.issue(Some(id), Some(port), "Диапазон повторов 0,05–64.");
                None
            }
            _ => {
                self.issue(Some(id), Some(port), "Для масштаба требуется число.");
                None
            }
        }
    }
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(graph: &mut Graph, kind: Kind) -> Option<NodeId> {
        graph.add(kind, [0.0; 2])
    }

    #[test]
    fn typed_graph_roundtrips_and_emits_deterministic_wgsl() {
        let mut graph = Graph::default();
        let red = add(&mut graph, Kind::Color([1.0, 0.0, 0.0]));
        let green = add(&mut graph, Kind::Color([0.0, 1.0, 0.0]));
        let t = add(&mut graph, Kind::Float(0.25));
        let scale = add(&mut graph, Kind::Float(2.6));
        let blend = add(&mut graph, Kind::Mix([red, green, t]));
        let gain = add(&mut graph, Kind::Multiply([scale, blend]));
        let uv = add(&mut graph, Kind::TextureScale([scale, t]));
        graph.output.inputs = [blend, t, gain, uv, None, None];
        let compiled = graph.compile().unwrap();
        assert_eq!(compiled.base_color, Some([0.75, 0.25, 0.0]));
        assert_eq!(compiled.roughness, Some(0.25));
        assert_eq!(compiled.uv_scale, Some([2.6, 0.25]));
        assert_eq!(compiled.emission, Some([0.75 * 2.6, 0.25 * 2.6, 0.0]));
        let wgsl = compiled.wgsl();
        assert!(wgsl.contains("15u, vec2<f32>(2.6, 0.25)"));
        let loaded: Graph = serde_json::from_slice(&serde_json::to_vec(&graph).unwrap()).unwrap();
        assert_eq!(loaded, graph);
        assert_eq!(loaded.compile().unwrap().wgsl(), wgsl);
    }

    #[test]
    fn cycles_type_mismatches_and_unused_drafts_have_clear_domains() {
        let mut graph = Graph::default();
        let roughness = add(&mut graph, Kind::Float(0.25));
        let unfinished = add(&mut graph, Kind::Mix([None; 3]));
        graph.output.inputs[1] = roughness;
        let report = graph.report();
        assert_eq!(report.compiled.unwrap().roughness, Some(0.25));
        assert!(
            report
                .issues
                .iter()
                .all(|issue| issue.node == unfinished && !issue.blocking)
        );
        graph.output.inputs[0] = roughness;
        let report = graph.report();
        assert!(report.compiled.is_none());
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.node.is_none() && issue.port == Some(0))
        );
        graph.output.inputs[0] = unfinished;
        graph.nodes[1].kind = Kind::Mix([unfinished, roughness, roughness]);
        let report = graph.report();
        assert!(report.compiled.is_none());
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.node == unfinished && issue.message.contains("Цикл"))
        );
        graph.remove(unfinished.unwrap());
        assert!(graph.compile().is_some());
    }

    #[test]
    fn storage_caps_nodes_rejects_duplicate_ids_and_keeps_missing_port_draft() {
        let mut graph = Graph::default();
        for _ in 0..MAX_NODES {
            assert!(add(&mut graph, Kind::Float(0.9)).is_some());
        }
        assert!(add(&mut graph, Kind::Float(0.5)).is_none());
        graph.output.inputs[1] = Some(80_000);
        assert!(graph.storage_valid() && graph.compile().is_none());
        graph.nodes[1].id = 1;
        assert!(!graph.storage_valid());
    }
}
