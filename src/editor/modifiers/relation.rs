use super::*;
use crate::modulation::relation::Scale;

pub(super) fn graph(
    ui: &mut egui::Ui,
) -> (Rect, Response, crate::display_preferences::theme::Theme) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::hover());
    let palette = theme::current(ui.ctx());
    ui.painter().rect_filled(rect, 4.0, palette.background.top);
    (rect.shrink(4.0), response, palette)
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
}

pub(super) fn relation(
    ui: &mut egui::Ui,
    diagram: Diagram<'_>,
    output: impl Fn(f32) -> f32,
    tip: impl Fn(f32, f32, Option<usize>) -> String,
) {
    let (rect, response, palette) = graph(ui);
    let [x, y] = diagram.axes;
    let y_at = |value| egui::lerp(rect.bottom()..=rect.top(), y.coordinate(value));
    let position =
        |input, value| Pos2::new(egui::lerp(rect.x_range(), x.coordinate(input)), y_at(value));
    let p = ui.painter_at(rect.expand(2.0));
    p.hline(
        rect.x_range(),
        y_at(diagram.base),
        Stroke::new(1.0, palette.grid.zero),
    );

    let curve = (0..=64)
        .map(|i| {
            let t = i as f32 / 64.0;
            Pos2::new(egui::lerp(rect.x_range(), t), y_at(output(x.value(t))))
        })
        .collect();
    p.add(egui::Shape::line(curve, Stroke::new(1.6, diagram.accent)));

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
        p.circle_filled(pos, 3.0, accent);
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
