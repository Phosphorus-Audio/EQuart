use super::*;
use crate::region::dynamics::{CEILING, Point};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct LevelScale(usize);

impl LevelScale {
    const FLOORS: [f32; 3] = [-48.0, -72.0, -96.0];

    pub(super) fn low(self) -> f32 {
        Self::FLOORS[self.0]
    }

    pub(super) fn xy(self, plot: Rect, input: f32, output: f32) -> Pos2 {
        Pos2::new(
            egui::remap(input, self.low()..=CEILING, plot.x_range()),
            self.y(plot, output),
        )
    }

    pub(super) fn y(self, plot: Rect, db: f32) -> f32 {
        egui::remap(db, self.low()..=CEILING, plot.bottom()..=plot.top())
    }

    pub(super) fn point(self, plot: Rect, position: Pos2) -> Point {
        Point {
            input: egui::remap_clamp(position.x, plot.x_range(), self.low()..=CEILING),
            output: egui::remap_clamp(position.y, plot.y_range(), CEILING..=self.low()),
            ..Point::default()
        }
    }

    pub(super) fn ticks(self) -> impl Iterator<Item = f32> {
        let low = self.low() as i32;
        let step = if self.0 == 0 { 12 } else { 24 };
        (low..=0).step_by(step).map(|db| db as f32).chain([CEILING])
    }

    pub(super) fn tick_text(db: f32) -> String {
        if db > 0.0 {
            format!("{db:+.0}")
        } else {
            format!("{db:.0}")
        }
    }

    pub(super) fn grid(self, painter: &egui::Painter, plot: Rect, ink: Color32) {
        for db in self.ticks() {
            let y = self.y(plot, db);
            let alpha = if db == 0.0 { 0.46 } else { 0.22 };
            painter.hline(
                plot.x_range(),
                y,
                Stroke::new(1.0, ink.gamma_multiply(alpha)),
            );
            let label_y = if db == CEILING { y + 9.0 } else { y };
            painter.text(
                Pos2::new(plot.right() - 3.0, label_y),
                egui::Align2::RIGHT_BOTTOM,
                Self::tick_text(db),
                FontId::monospace(8.0),
                ink,
            );
        }
    }

    pub(super) fn scroll(self, ui: &egui::Ui, chart: Rect, id: egui::Id) -> Self {
        if !ui.rect_contains_pointer(chart)
            || ui.input(|input| input.pointer.any_down())
            || egui::Popup::is_any_open(ui.ctx())
        {
            return self;
        }

        let steps = ui.input(|input| crate::editor::wheel::wheel_steps(&input.events));
        if steps == 0.0 {
            return self;
        }

        ui.input_mut(|input| input.smooth_scroll_delta.y = 0.0);
        let shifted = ui.data_mut(|data| {
            let previous = data.get_temp::<f32>(id).unwrap_or(0.0);
            let (full, remainder) = crate::editor::wheel::take_steps(previous + steps);
            data.insert_temp(id, remainder);
            full
        });
        let next = (self.0 as i32 - shifted).clamp(0, Self::FLOORS.len() as i32 - 1);
        Self(next as usize)
    }

    pub(super) fn settings(self, ui: &mut egui::Ui) -> Self {
        let mut scale = self;
        ui.horizontal(|ui| {
            ui.label("Graph Min");
            for (index, low) in Self::FLOORS.into_iter().enumerate() {
                if ui
                    .add(egui::Button::new(format!("{low:.0} dB")).selected(self.0 == index))
                    .clicked()
                {
                    scale = Self(index);
                }
            }
        });
        scale
    }
}
