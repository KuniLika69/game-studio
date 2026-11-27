use eframe::egui::{self, Color32, CornerRadius, Stroke};

pub(crate) const ACCENT: Color32 = Color32::from_rgb(199, 164, 255);
pub(crate) const MUTED: Color32 = Color32::from_rgb(189, 180, 207);
pub(crate) const BACKDROP: Color32 = Color32::from_rgb(15, 12, 22);
pub(crate) const PANEL: Color32 = Color32::from_rgb(26, 21, 38);
pub(crate) const TEXT: Color32 = Color32::from_rgb(248, 244, 255);
pub(crate) const SURFACE: Color32 = Color32::from_rgb(34, 27, 49);
pub(crate) const EDGE: Color32 = Color32::from_rgb(66, 53, 91);
pub(crate) const SELECTED: Color32 = Color32::from_rgb(65, 43, 100);
pub(crate) const HOVER: Color32 = Color32::from_rgb(50, 36, 73);
pub(crate) const BRIGHT: Color32 = Color32::from_rgb(226, 203, 255);

pub(crate) fn studio_mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same((size * 0.30).round() as u8),
        SELECTED,
        Stroke::new(1.0_f32, ACCENT),
        egui::StrokeKind::Inside,
    );
    let face = egui::Rect::from_center_size(
        rect.center() + egui::vec2(0.0, size * 0.05),
        egui::vec2(size * 0.58, size * 0.40),
    );
    painter.rect_filled(face, (size * 0.14).round(), BRIGHT);
    let tip = rect.center() + egui::vec2(0.0, -size * 0.29);
    painter.line_segment([tip, face.center_top()], Stroke::new(1.4_f32, BRIGHT));
    painter.circle_filled(tip, size * 0.06, BRIGHT);
    for side in [-1.0, 1.0] {
        let eye = face.center() + egui::vec2(side * size * 0.12, 0.0);
        painter.circle_filled(eye, size * 0.045, SELECTED);
    }
}

pub(crate) fn configure(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(MUTED);
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BACKDROP;
    visuals.faint_bg_color = SURFACE;
    visuals.code_bg_color = BACKDROP;
    visuals.window_stroke = Stroke::new(1.0_f32, EDGE);
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.selection.bg_fill = SELECTED;
    visuals.selection.stroke = Stroke::new(1.0_f32, BRIGHT);
    visuals.hyperlink_color = ACCENT;

    let widgets = &mut visuals.widgets;
    for widget in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(7);
        widget.fg_stroke.color = TEXT;
    }
    widgets.noninteractive.bg_fill = PANEL;
    widgets.noninteractive.weak_bg_fill = PANEL;
    widgets.noninteractive.bg_stroke.color = EDGE;
    widgets.inactive.bg_fill = SURFACE;
    widgets.inactive.weak_bg_fill = SURFACE;
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, EDGE);
    widgets.hovered.bg_fill = HOVER;
    widgets.hovered.weak_bg_fill = HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    widgets.active.bg_fill = SELECTED;
    widgets.active.weak_bg_fill = SELECTED;
    widgets.active.bg_stroke = Stroke::new(1.0_f32, BRIGHT);
    widgets.open.bg_fill = SELECTED;
    widgets.open.weak_bg_fill = SELECTED;
    widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT);

    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 25.0;
    });
}
