use super::*;
use crate::modulation::relation::Scale;

pub(super) const DEFAULT_HEIGHT: f32 = 54.0;

pub(super) fn graph(
    ui: &mut egui::Ui,
) -> (Rect, Response, crate::display_preferences::theme::Theme) {
    graph_with_height(ui, DEFAULT_HEIGHT)
}

pub(super) fn graph_with_height(
    ui: &mut egui::Ui,
    height: f32,
) -> (Rect, Response, crate::display_preferences::theme::Theme) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let palette = theme::current(ui.ctx());
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, palette.background.top);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, palette.panel.border.gamma_multiply(0.55)),
        egui::StrokeKind::Inside,
    );
    (rect.shrink(5.0), response, palette)
}

pub(super) fn route_title(ui: &mut egui::Ui, source: &str, target: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        ui.strong(source);

        let (icon, _) = ui.allocate_exact_size(Vec2::new(12.0, 13.0), Sense::hover());
        ui.painter().arrow(
            icon.center() - Vec2::new(5.0, 0.0),
            Vec2::new(10.0, 0.0),
            Stroke::new(1.2, ui.visuals().weak_text_color()),
        );

        ui.strong(target);
    });
}

#[derive(Clone, Copy)]
pub(super) struct Axis {
    pub scale: Scale,
    pub bounds: (f32, f32),
}

impl Axis {
    fn coordinate(self, value: f32) -> f32 {
        let min = self.scale.encode(self.bounds.0);
        let max = self.scale.encode(self.bounds.1);
        ((self.scale.encode(value) - min) / (max - min)).clamp(0.0, 1.0)
    }

    fn value(self, position: f32) -> f32 {
        self.scale.decode(egui::lerp(
            self.scale.encode(self.bounds.0)..=self.scale.encode(self.bounds.1),
            position,
        ))
    }
}

pub(super) struct Diagram<'a> {
    pub axes: [Axis; 2],
    pub base: f32,
    pub points: &'a [[f32; 2]],
    pub inactive: u32,
    pub accent: Color32,
    pub height: f32,
}

pub(super) fn relation(
    ui: &mut egui::Ui,
    diagram: Diagram<'_>,
    output: impl Fn(f32) -> f32,
    tip: impl Fn(f32, f32, Option<usize>) -> String,
) {
    let (rect, response, palette) = graph_with_height(ui, diagram.height);
    let [x, y] = diagram.axes;
    let y_at = |value| egui::lerp(rect.bottom()..=rect.top(), y.coordinate(value));
    let position =
        |input, value| Pos2::new(egui::lerp(rect.x_range(), x.coordinate(input)), y_at(value));
    let painter = ui.painter_at(rect.expand(2.0));
    let grid_stroke = Stroke::new(0.7, palette.grid.minor.gamma_multiply(0.8));

    for t in [0.25, 0.5, 0.75] {
        let x = egui::lerp(rect.x_range(), t);
        let y = egui::lerp(rect.y_range(), t);

        painter.vline(x, rect.y_range(), grid_stroke);
        painter.hline(rect.x_range(), y, grid_stroke);
    }

    painter.hline(
        rect.x_range(),
        y_at(diagram.base),
        Stroke::new(1.0, palette.grid.zero),
    );

    if diagram.points.len() == 1 {
        let input = diagram.points[0][0];
        let x = egui::lerp(rect.x_range(), x.coordinate(input));

        painter.vline(
            x,
            rect.y_range(),
            Stroke::new(0.8, diagram.accent.gamma_multiply(0.25)),
        );
    }

    let mut curve = Vec::with_capacity(65);

    curve.extend((0..=64).map(|i| {
        let t = i as f32 / 64.0;

        Pos2::new(egui::lerp(rect.x_range(), t), y_at(output(x.value(t))))
    }));
    painter.add(egui::Shape::line(curve, Stroke::new(1.6, diagram.accent)));

    let pointer = response.hover_pos();
    let mut nearest = None;

    for (index, &[input, value]) in diagram.points.iter().enumerate() {
        let pos = position(input, value);
        if !pos.is_finite() {
            continue;
        }

        let accent = if diagram.inactive & (1 << index) == 0 {
            diagram.accent
        } else {
            diagram.accent.gamma_multiply(0.35)
        };
        painter.circle_filled(pos, 3.0, accent);
        if let Some(pointer) = pointer {
            let distance = pos.distance_sq(pointer);
            if distance <= 64.0 && nearest.is_none_or(|(_, previous)| distance < previous) {
                nearest = Some((index, distance));
            }
        }
    }

    if let Some(pos) = pointer {
        let input = x.value(((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
        response.on_hover_text(tip(input, output(input), nearest.map(|(index, _)| index)));
    }
}
