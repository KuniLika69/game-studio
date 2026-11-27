use eframe::egui::{self, Event, FullOutput, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};

const FRAMES_PER_SECOND: f64 = 60.0;

/// Input for a virtual egui viewport. Cursor locations are logical points.
pub struct Driver {
    rect: Rect,
    frame: u64,
    modifiers: Modifiers,
    scale: f32,
}

impl Driver {
    pub fn new([width, height]: [u32; 2]) -> Self {
        Self::with_scale([width, height], 1.0)
    }

    pub fn with_scale([width, height]: [u32; 2], scale: f32) -> Self {
        Self {
            rect: Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(width.max(1) as f32 / scale, height.max(1) as f32 / scale),
            ),
            frame: 0,
            modifiers: Modifiers::NONE,
            scale,
        }
    }

    /// Allow click detection or UI effects to expire between scripted actions.
    pub fn skip_frames(&mut self, count: u32) {
        self.frame += u64::from(count);
    }

    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    pub fn raw(&mut self, events: Vec<Event>) -> RawInput {
        let mut input = RawInput {
            screen_rect: Some(self.rect),
            time: Some(self.frame as f64 / FRAMES_PER_SECOND),
            predicted_dt: (1.0 / FRAMES_PER_SECOND) as f32,
            modifiers: self.modifiers,
            events,
            ..RawInput::default()
        };
        let viewport = input.viewports.get_mut(&input.viewport_id).unwrap();
        viewport.native_pixels_per_point = Some(self.scale);
        viewport.inner_rect = Some(self.rect);
        viewport.outer_rect = Some(self.rect);
        viewport.monitor_size = Some(self.rect.size());
        viewport.focused = Some(true);
        self.frame += 1;
        input
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
    /// Bounds of the visible glyphs within the painter's clip rectangle.
    pub bounds: Rect,
}

impl Label {
    pub fn center(&self) -> Pos2 {
        self.bounds.center()
    }
}

/// Labels are returned in paint order. Visible duplicate text is kept.
pub fn labels(output: &FullOutput) -> Vec<Label> {
    fn read(shape: &egui::Shape, clip: Rect, labels: &mut Vec<Label>) {
        match shape {
            egui::Shape::Text(shape) if shape.opacity_factor > 0.0 => {
                let text = shape.galley.text().trim();
                let bounds = shape.visual_bounding_rect().intersect(clip);
                if !text.is_empty() && bounds.is_positive() && bounds.is_finite() {
                    labels.push(Label {
                        text: text.to_owned(),
                        bounds,
                    });
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    read(shape, clip, labels);
                }
            }
            _ => {}
        }
    }

    let mut result = Vec::new();
    for shape in &output.shapes {
        read(&shape.shape, shape.clip_rect, &mut result);
    }
    result
}

pub fn find_label(output: &FullOutput, text: &str, occurrence: usize) -> Option<Pos2> {
    find_by(output, occurrence, |label| label == text)
}

pub fn find_label_containing(output: &FullOutput, text: &str, occurrence: usize) -> Option<Pos2> {
    find_by(output, occurrence, |label| label.contains(text))
}

fn find_by(
    output: &FullOutput,
    occurrence: usize,
    predicate: impl Fn(&str) -> bool,
) -> Option<Pos2> {
    labels(output)
        .into_iter()
        .filter(|label| predicate(&label.text))
        .nth(occurrence)
        .map(|label| label.center())
}

pub fn hover(position: Pos2) -> Vec<Event> {
    vec![Event::PointerMoved(position)]
}

pub fn click(position: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(position),
        pointer_button(position, PointerButton::Primary, true),
        pointer_button(position, PointerButton::Primary, false),
    ]
}

#[cfg(test)]
fn double_click(position: Pos2) -> Vec<Event> {
    let mut events = click(position);
    events.extend([
        pointer_button(position, PointerButton::Primary, true),
        pointer_button(position, PointerButton::Primary, false),
    ]);
    events
}

pub fn pointer_button(position: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

pub fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
    vec![
        key_event(key, modifiers, true),
        key_event(key, modifiers, false),
    ]
}

pub fn key_event(key: Key, modifiers: Modifiers, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers,
    }
}

pub fn text(value: &str) -> Vec<Event> {
    vec![Event::Text(value.to_owned())]
}

/// Replaces the currently focused edit field using the Windows Select All shortcut.
pub fn replace_text(value: &str) -> Vec<Event> {
    let mut events = key(
        Key::A,
        Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        },
    );
    if value.is_empty() {
        events.extend(key(Key::Backspace, Modifiers::NONE));
    } else {
        events.extend(text(value));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn painted_labels_drive_clicks_and_text_replacement() {
        let ctx = egui::Context::default();
        let mut driver = Driver::new([600, 400]);
        let mut value = String::from("Предыдущее имя");
        let mut choice = None;
        let mut double_clicked = false;
        let mut show = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for index in 0..2 {
                    let button = ui.button("Выбрать");
                    if button.clicked() {
                        choice = Some(index);
                    }
                    double_clicked |= button.double_clicked();
                }
                ui.text_edit_singleline(&mut value);
                ui.painter_at(Rect::from_min_size(Pos2::ZERO, egui::vec2(1.0, 1.0)))
                    .text(
                        egui::pos2(10.0, 10.0),
                        egui::Align2::LEFT_TOP,
                        "Невидимый текст",
                        egui::FontId::default(),
                        egui::Color32::WHITE,
                    );
            });
        };
        let output = ctx.run(driver.raw(vec![]), &mut show);
        assert!(find_label(&output, "Невидимый текст", 0).is_none());
        assert!(find_label(&output, "Выбрать", 2).is_none());
        let button = find_label(&output, "Выбрать", 1).unwrap();
        let output = ctx.run(driver.raw(double_click(button)), &mut show);
        driver.skip_frames(45);
        let field = find_label_containing(&output, "Предыдущее", 0).unwrap();
        let _ = ctx.run(driver.raw(click(field)), &mut show);
        let _ = ctx.run(driver.raw(replace_text("Ключ от дома")), &mut show);
        assert_eq!(choice, Some(1));
        assert!(double_clicked);
        assert_eq!(value, "Ключ от дома");
    }
}
