use eframe::egui::{self, Color32, FontId, Id, Pos2, Rect, Sense, Stroke, Vec2};

use crate::material_graph::{
    Graph, Issue, Kind, MAX_NODES, NodeId, OUTPUT_INPUTS, Preset, Report, ValueType,
};

const WIDTH: f32 = 168.0;
const HEADER: f32 = 30.0;
const ROW: f32 = 24.0;
const GRID: f32 = 32.0;
const TEXT: Color32 = Color32::from_rgb(224, 235, 246);
const MUTED: Color32 = Color32::from_rgb(154, 179, 199);
const ACCENT: Color32 = Color32::from_rgb(101, 225, 197);
const ERROR: Color32 = Color32::from_rgb(244, 144, 115);
const WARNING: Color32 = Color32::from_rgb(223, 179, 103);

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
enum Card {
    Node(NodeId),
    Output,
}

#[derive(Clone)]
struct Canvas {
    pan: Vec2,
    selected: Card,
    source: Option<NodeId>,
    preset: Option<Preset>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            selected: Card::Output,
            source: None,
            preset: None,
        }
    }
}

/// Authored values are stored in the graph. Selection, panning and a pending wire are local UI state.
pub fn editor(ui: &mut egui::Ui, graph: &mut Graph) -> bool {
    let id = ui.id().with("material graph");
    let mut canvas: Canvas = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    if let Card::Node(id) = canvas.selected
        && !graph.nodes.iter().any(|node| node.id == id)
    {
        canvas.selected = Card::Output;
    }
    let mut changed = toolbar(ui, graph, &mut canvas);
    let report = graph.report();
    changed |= drawing(ui, id, graph, &report, &mut canvas);
    changed |= properties(ui, id, graph, &mut canvas);
    for issue in report
        .issues
        .iter()
        .filter(|issue| card_for_issue(issue) == canvas.selected)
        .take(4)
    {
        let port = issue.port.and_then(|port| match canvas.selected {
            Card::Output => OUTPUT_INPUTS.get(port),
            Card::Node(id) => graph
                .nodes
                .iter()
                .find(|node| node.id == id)?
                .kind
                .input_labels()
                .get(port),
        });
        let message = port.map_or_else(
            || issue.message.clone(),
            |port| format!("{port} · {}", issue.message),
        );
        ui.colored_label(if issue.blocking { ERROR } else { WARNING }, message);
    }
    if report.compiled.is_none() {
        ui.colored_label(ERROR, "В сцене действуют обычные настройки GLB.");
    }
    ui.data_mut(|data| data.insert_temp(id, canvas));
    changed
}

fn toolbar(ui: &mut egui::Ui, graph: &mut Graph, canvas: &mut Canvas) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("Добавить узел", |ui| {
            let kinds = [
                Kind::Color([0.75; 3]),
                Kind::Float(0.5),
                Kind::Multiply([None; 2]),
                Kind::Mix([None; 3]),
                Kind::TextureScale([None; 2]),
            ];
            for kind in kinds {
                if ui
                    .add_enabled(
                        graph.nodes.len() < MAX_NODES,
                        egui::Button::new(kind.label()),
                    )
                    .clicked()
                {
                    let position = open_position(graph, -canvas.pan);
                    if let Some(id) = graph.add(kind, position) {
                        canvas.selected = Card::Node(id);
                        changed = true;
                    }
                    ui.close();
                }
            }
        });
        ui.menu_button("Шаблоны", |ui| {
            for preset in Preset::ALL {
                if ui
                    .button(preset.label())
                    .on_hover_text(preset.description())
                    .clicked()
                {
                    canvas.preset = Some(preset);
                    ui.close();
                }
            }
        });
        if ui
            .button("Домой")
            .on_hover_text("Вернуть вид к началу схемы")
            .clicked()
        {
            canvas.pan = Vec2::ZERO;
        }
        let compiled = graph.compile();
        if ui
            .add_enabled(compiled.is_some(), egui::Button::new("Копировать WGSL"))
            .clicked()
            && let Some(compiled) = compiled
        {
            ui.ctx().copy_text(compiled.wgsl());
        }
        if canvas.source.is_some() {
            ui.label("Выберите вход");
            if ui.small_button("Отмена").clicked() {
                canvas.source = None;
            }
        }
    });
    if let Some(preset) = canvas.preset {
        ui.horizontal(|ui| {
            ui.label(format!("Заменить граф шаблоном «{}»?", preset.label()));
            if ui.button("Применить шаблон").clicked() {
                *graph = Graph::preset(preset);
                *canvas = Canvas::default();
                changed = true;
            }
            if ui.button("Отмена").clicked() {
                canvas.preset = None;
            }
        });
    }
    changed
}

fn open_position(graph: &Graph, origin: Vec2) -> [f32; 2] {
    for index in 0..MAX_NODES as u32 {
        let col = (index % 2) as f32;
        let row = (index / 2) as f32;
        let candidate = bounds([26.0 + origin.x + col * 228.0, 24.0 + origin.y + row * 140.0]);
        if graph.nodes.iter().all(|node| {
            (node.position[0] - candidate[0]).abs() > WIDTH
                || (node.position[1] - candidate[1]).abs() > 132.0
        }) {
            return candidate;
        }
    }
    bounds([origin.x + 20.0, origin.y + 20.0])
}

fn bounds(point: [f32; 2]) -> [f32; 2] {
    point.map(|value| value.clamp(-4096.0, 4096.0))
}

fn card_for_issue(issue: &Issue) -> Card {
    issue.node.map_or(Card::Output, Card::Node)
}

struct Layout {
    rect: Rect,
    title: &'static str,
    inputs: &'static [&'static str],
    card: Card,
}

impl Layout {
    fn new(
        origin: Pos2,
        position: [f32; 2],
        title: &'static str,
        inputs: &'static [&'static str],
        card: Card,
    ) -> Self {
        let point = origin + egui::vec2(position[0], position[1]);
        let rect = Rect::from_min_size(
            point,
            egui::vec2(WIDTH, HEADER + ROW * inputs.len().max(1) as f32 + 8.0),
        );
        Self {
            rect,
            title,
            inputs,
            card,
        }
    }

    fn input(&self, index: usize) -> Pos2 {
        egui::pos2(
            self.rect.left(),
            self.rect.top() + HEADER + ROW * (index as f32 + 0.5),
        )
    }

    fn source(&self) -> Pos2 {
        egui::pos2(self.rect.right(), self.rect.top() + HEADER + ROW * 0.5)
    }
}

fn drawing(
    ui: &mut egui::Ui,
    id: Id,
    graph: &mut Graph,
    report: &Report,
    canvas: &mut Canvas,
) -> bool {
    let width = ui.available_width().max(180.0);
    let height = ui.available_height().clamp(330.0, 470.0);
    let (rect, background) =
        ui.allocate_exact_size(egui::vec2(width, height), Sense::click_and_drag());
    if background.dragged_by(egui::PointerButton::Middle)
        || background.dragged_by(egui::PointerButton::Secondary)
    {
        canvas.pan += background.drag_delta();
        canvas.pan = Vec2::new(
            canvas.pan.x.clamp(-4100.0, 4100.0),
            canvas.pan.y.clamp(-4100.0, 4100.0),
        );
    }
    let pane = ui.painter().with_clip_rect(rect);
    pane.rect_filled(rect, 5.0, Color32::from_rgb(11, 19, 30));
    let grid = Stroke::new(1.0_f32, Color32::from_rgb(22, 34, 48));
    let mut x = rect.left() + canvas.pan.x.rem_euclid(GRID);
    while x < rect.right() {
        pane.vline(x, rect.y_range(), grid);
        x += GRID;
    }
    let mut y = rect.top() + canvas.pan.y.rem_euclid(GRID);
    while y < rect.bottom() {
        pane.hline(rect.x_range(), y, grid);
        y += GRID;
    }
    let origin = rect.min + canvas.pan;
    let output = Layout::new(
        origin,
        graph.output.position,
        "Выход материала",
        &OUTPUT_INPUTS,
        Card::Output,
    );
    let layouts: Vec<_> = graph
        .nodes
        .iter()
        .map(|node| {
            Layout::new(
                origin,
                node.position,
                node.kind.label(),
                node.kind.input_labels(),
                Card::Node(node.id),
            )
        })
        .collect();
    let source = |source| {
        layouts
            .iter()
            .find(|layout| layout.card == Card::Node(source))
            .map(Layout::source)
    };
    for (layout, inputs) in layouts
        .iter()
        .zip(graph.nodes.iter().map(|node| node.kind.inputs()))
        .chain(std::iter::once((&output, graph.output.inputs.as_slice())))
    {
        for (port, id) in inputs.iter().enumerate() {
            if let Some(start) = id.and_then(source) {
                let tint = socket_color(id.and_then(|id| report.types.get(&id).copied()));
                draw_wire(&pane, start, layout.input(port), tint);
            }
        }
    }
    if let Some(start) = canvas.source.and_then(source)
        && let Some(end) = ui
            .input(|input| input.pointer.hover_pos())
            .filter(|point| rect.contains(*point))
    {
        draw_wire(&pane, start, end, ACCENT);
    }

    let mut changed = false;
    for ((layout, position), inputs) in layouts
        .iter()
        .zip(
            graph
                .nodes
                .iter_mut()
                .map(|node| (&mut node.position, node.kind.inputs_mut())),
        )
        .chain(std::iter::once((
            &output,
            (
                &mut graph.output.position,
                graph.output.inputs.as_mut_slice(),
            ),
        )))
        .map(|(layout, (position, inputs))| ((layout, position), inputs))
    {
        if !layout.rect.expand(12.0).intersects(rect) {
            continue;
        }
        let card_id = id.with(layout.card);
        let stroke = if report
            .issues
            .iter()
            .any(|issue| card_for_issue(issue) == layout.card && issue.blocking)
        {
            ERROR
        } else if layout.card == canvas.selected {
            ACCENT
        } else {
            Color32::from_rgb(53, 78, 99)
        };
        pane.rect(
            layout.rect,
            6.0,
            Color32::from_rgb(23, 36, 51),
            Stroke::new(1.0_f32, stroke),
            egui::StrokeKind::Inside,
        );
        let title = Rect::from_min_size(layout.rect.min, egui::vec2(WIDTH, HEADER));
        pane.rect_filled(title.shrink(1.0), 5.0, Color32::from_rgb(33, 52, 69));
        pane.text(
            title.left_center() + egui::vec2(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            layout.title,
            FontId::proportional(13.5),
            TEXT,
        );
        let title = ui.interact(title.intersect(rect), card_id, Sense::click_and_drag());
        if title.clicked() || title.dragged() {
            canvas.selected = layout.card;
        }
        if title.dragged() {
            let delta = title.drag_delta();
            *position = bounds([position[0] + delta.x, position[1] + delta.y]);
            changed |= delta != Vec2::ZERO;
        }
        for (port, (label, connection)) in layout.inputs.iter().zip(inputs).enumerate() {
            let center = layout.input(port);
            let tint = socket_color(connection.and_then(|id| report.types.get(&id).copied()));
            pane.circle_filled(center, 5.0, tint);
            pane.text(
                center + egui::vec2(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                *label,
                FontId::proportional(12.0),
                MUTED,
            );
            let socket = ui
                .interact(
                    Rect::from_center_size(center, egui::Vec2::splat(20.0)).intersect(rect),
                    card_id.with(("in", port)),
                    Sense::click(),
                )
                .on_hover_text("Щелчок соединяет вход. Правая кнопка отключает.");
            if socket.clicked() {
                canvas.selected = layout.card;
                if let Some(source) = canvas.source.take()
                    && layout.card != Card::Node(source)
                {
                    changed |= *connection != Some(source);
                    *connection = Some(source);
                }
            }
            if socket.secondary_clicked() {
                changed |= connection.take().is_some();
                canvas.selected = layout.card;
            }
        }
        if let Card::Node(node) = layout.card {
            let center = layout.source();
            let value_type = report.types.get(&node).copied();
            let tint = socket_color(value_type);
            pane.circle_filled(
                center,
                6.0,
                if canvas.source == Some(node) {
                    ACCENT
                } else {
                    tint
                },
            );
            if layout.inputs.is_empty() {
                pane.text(
                    center + egui::vec2(-12.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    value_type.map_or("выход", ValueType::label),
                    FontId::proportional(12.0),
                    MUTED,
                );
            }
            let socket = ui
                .interact(
                    Rect::from_center_size(center, egui::Vec2::splat(20.0)).intersect(rect),
                    card_id.with("out"),
                    Sense::click(),
                )
                .on_hover_text("Начать соединение");
            if socket.clicked() {
                canvas.selected = layout.card;
                canvas.source = (canvas.source != Some(node)).then_some(node);
            }
        }
    }
    changed
}

fn socket_color(kind: Option<ValueType>) -> Color32 {
    match kind {
        Some(ValueType::Color) => Color32::from_rgb(246, 201, 104),
        Some(ValueType::Float) => Color32::from_rgb(112, 191, 248),
        Some(ValueType::Uv) => Color32::from_rgb(199, 154, 237),
        None => Color32::from_rgb(107, 136, 156),
    }
}

fn draw_wire(painter: &egui::Painter, start: Pos2, end: Pos2, color: Color32) {
    let pull = ((start.x - end.x).abs() * 0.48).clamp(42.0, 128.0);
    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
        [
            start,
            start + egui::vec2(pull, 0.0),
            end - egui::vec2(pull, 0.0),
            end,
        ],
        false,
        Color32::TRANSPARENT,
        Stroke::new(2.0_f32, color),
    ));
}

fn properties(ui: &mut egui::Ui, id: Id, graph: &mut Graph, canvas: &mut Canvas) -> bool {
    let choices: Vec<_> = graph
        .nodes
        .iter()
        .map(|node| (node.id, node.kind.label()))
        .collect();
    match canvas.selected {
        Card::Output => {
            let mut changed = false;
            ui.label("Выход материала");
            ui.horizontal_wrapped(|ui| {
                for (port, (label, input)) in OUTPUT_INPUTS
                    .iter()
                    .zip(&mut graph.output.inputs)
                    .enumerate()
                {
                    changed |= input_menu(
                        ui,
                        id.with(("material input", port)),
                        label,
                        input,
                        &choices,
                        None,
                    );
                }
            });
            changed
        }
        Card::Node(node_id) => {
            let Some(node) = graph.nodes.iter_mut().find(|node| node.id == node_id) else {
                return false;
            };
            let mut changed = false;
            let mut remove = false;
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{} · {}", node.kind.label(), node_id));
                match &mut node.kind {
                    Kind::Color(rgb) => {
                        let before = *rgb;
                        let from_linear = |x: f32| {
                            if x <= 0.0031308 {
                                x * 12.92
                            } else {
                                1.055 * x.powf(1.0 / 2.4) - 0.055
                            }
                        };
                        let to_linear = |x: f32| {
                            if x <= 0.04045 {
                                x / 12.92
                            } else {
                                ((x + 0.055) / 1.055).powf(2.4)
                            }
                        };
                        let mut linear = before.map(to_linear);
                        if ui.color_edit_button_rgb(&mut linear).changed() {
                            *rgb = linear.map(|channel| from_linear(channel).clamp(0.0, 1.0));
                            changed |= *rgb != before;
                        }
                    }
                    Kind::Float(value) => {
                        changed |= ui
                            .add(
                                egui::DragValue::new(value)
                                    .prefix("Значение ")
                                    .speed(0.01)
                                    .range(-64.0..=64.0),
                            )
                            .changed();
                    }
                    kind => {
                        let labels = kind.input_labels();
                        for (port, (label, input)) in
                            labels.iter().zip(kind.inputs_mut()).enumerate()
                        {
                            changed |= input_menu(
                                ui,
                                id.with(("node input", node_id, port)),
                                label,
                                input,
                                &choices,
                                Some(node_id),
                            );
                        }
                    }
                }
                remove = ui.small_button("Удалить узел").clicked();
            });
            if remove {
                graph.remove(node_id);
                canvas.selected = Card::Output;
                canvas.source = canvas.source.filter(|source| *source != node_id);
                changed = true;
            }
            changed
        }
    }
}

fn input_menu(
    ui: &mut egui::Ui,
    id: Id,
    label: &str,
    input: &mut Option<NodeId>,
    choices: &[(NodeId, &str)],
    current: Option<NodeId>,
) -> bool {
    let before = *input;
    let selected = input
        .and_then(|id| choices.iter().find(|(choice, _)| *choice == id))
        .map_or_else(
            || format!("{label} · пусто"),
            |(id, kind)| format!("{label} · {kind} {id}"),
        );
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(130.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(input, None, "Отключить");
            for &(choice, kind) in choices.iter().filter(|(id, _)| Some(*id) != current) {
                ui.selectable_value(input, Some(choice), format!("{kind} {choice}"));
            }
        });
    *input != before
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, RawInput};

    const SIZE: [f32; 2] = [820.0, 630.0];

    fn frame(context: &egui::Context, graph: &mut Graph, events: Vec<Event>) -> egui::FullOutput {
        context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::from(SIZE))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor(ui, graph);
                });
            },
        )
    }

    fn click(position: Pos2, button: PointerButton) -> Vec<Event> {
        vec![
            Event::PointerMoved(position),
            Event::PointerButton {
                pos: position,
                button,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            Event::PointerButton {
                pos: position,
                button,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    fn sockets(output: &egui::FullOutput, radius: f32) -> Vec<Pos2> {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle) if circle.radius == radius => Some(circle.center),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn actual_egui_ports_wire_replace_and_disconnect() {
        let context = egui::Context::default();
        let mut graph = Graph::default();
        let color_id = graph
            .add(Kind::Color([0.6, 0.3, 0.7]), [32.0, 32.0])
            .unwrap();
        let float_id = graph.add(Kind::Float(0.8), [32.0, 135.0]).unwrap();
        let output = frame(&context, &mut graph, Vec::new());
        let sources = sockets(&output, 6.0);
        let targets = sockets(&output, 5.0);
        assert_eq!(sources.len(), 2);
        assert_eq!(targets.len(), OUTPUT_INPUTS.len());

        frame(
            &context,
            &mut graph,
            click(sources[0], PointerButton::Primary),
        );
        frame(
            &context,
            &mut graph,
            click(targets[0], PointerButton::Primary),
        );
        assert_eq!(graph.output.inputs[0], Some(color_id));
        assert!(graph.compile().unwrap().base_color.is_some());

        frame(
            &context,
            &mut graph,
            click(sources[1], PointerButton::Primary),
        );
        let error = frame(
            &context,
            &mut graph,
            click(targets[0], PointerButton::Primary),
        );
        assert_eq!(graph.output.inputs[0], Some(float_id));
        assert!(graph.compile().is_none());
        assert!(
            graph
                .report()
                .issues
                .iter()
                .any(|issue| issue.blocking && issue.port == Some(0))
        );
        drop(error);

        frame(
            &context,
            &mut graph,
            click(targets[0], PointerButton::Secondary),
        );
        assert_eq!(graph.output.inputs[0], None);
        assert!(graph.compile().unwrap().base_color.is_none());
    }
}
