use super::*;
use crate::editor::spectrum::gradient_fill;
use crate::region::dynamics::Meter;
use crate::telemetry::region_dynamics::HISTORY;

impl EquartEditor {
    pub(super) fn dynamics_monitor(
        &self,
        ui: &mut egui::Ui,
        plot: Rect,
        index: usize,
        paused: &mut bool,
        scale: LevelScale,
    ) {
        #[cfg(feature = "analyzer-profile")]
        let _timer = crate::analyzer::profile::Timer::new(crate::analyzer::profile::REGION_MONITOR);
        let id = ui.make_persistent_id(("dynamics-monitor", index));
        let response = ui.interact(plot, id, Sense::click());
        let painter = ui.painter_at(plot);
        let text = self.theme.text.muted;
        let accent = self.theme.region[index];
        let color = [accent, self.theme.text.primary, self.theme.curves.phase];
        let state = id.with("traces");
        let mut shown = ui
            .data(|data| data.get_temp::<[bool; 3]>(state))
            .unwrap_or([true; 3]);

        let zero = scale.y(plot, 0.0);

        painter.rect_filled(plot, 0.0, self.theme.panel.menu);
        painter.vline(
            plot.left(),
            plot.y_range(),
            Stroke::new(1.0, text.gamma_multiply(0.4)),
        );
        scale.grid(&painter, plot, text);

        for seconds in 0..=4 {
            let x = plot.right() - plot.width() * seconds as f32 / 4.0;
            painter.vline(
                x,
                plot.y_range(),
                Stroke::new(1.0, text.gamma_multiply(0.17)),
            );
            let alignment = if seconds == 4 {
                egui::Align2::LEFT_BOTTOM
            } else {
                egui::Align2::RIGHT_BOTTOM
            };
            painter.text(
                Pos2::new(x, plot.bottom() - 2.0),
                alignment,
                format!("{seconds}s"),
                FontId::monospace(8.0),
                text,
            );
        }

        let history = &self.telemetry.region_dynamics[index];
        let samples = history.read().collect::<Vec<_>>();
        for segment in samples
            .chunk_by(|a, b| b.0 - a.0 <= 2)
            .filter(|s| s.len() > 1)
        {
            let trace = |y: &dyn Fn(&Meter) -> f32| {
                segment
                    .iter()
                    .map(|(slot, sample)| {
                        let x = plot.left() + plot.width() * *slot as f32 / (HISTORY - 1) as f32;
                        Pos2::new(x, y(sample))
                    })
                    .collect::<Vec<_>>()
            };

            if shown[0] {
                let input = trace(&|sample| scale.y(plot, sample.input).min(plot.bottom()));
                gradient_fill(
                    &painter,
                    &input,
                    |_, y| plot.bottom() - y,
                    &[0.0, 0.72, 1.0],
                    |_, stop| accent.gamma_multiply([0.70, 0.28, 0.10][stop]),
                );
                painter.add(egui::Shape::line(
                    input,
                    Stroke::new(1.0, accent.gamma_multiply(0.85)),
                ));
            }

            if shown[2] {
                let history = trace(&|sample| {
                    scale
                        .y(plot, sample.reduction)
                        .clamp(plot.top(), plot.bottom())
                });
                gradient_fill(
                    &painter,
                    &history,
                    |_, y| zero - y,
                    &[0.0, 1.0],
                    |_, stop| color[2].gamma_multiply(if stop == 0 { 0.25 } else { 0.05 }),
                );
                painter.add(egui::Shape::line(history, Stroke::new(1.25, color[2])));
            }

            if shown[1] {
                let output = trace(&|sample| scale.y(plot, sample.output));
                painter.add(egui::Shape::line(output, Stroke::new(1.4, color[1])));
            }
        }

        for (trace, (name, x)) in [("IN", 5.0), ("OUT", 25.0), ("GR", 51.0)]
            .into_iter()
            .enumerate()
        {
            let ink = color[trace].gamma_multiply(if shown[trace] { 1.0 } else { 0.4 });
            painter.text(
                Pos2::new(plot.left() + x, plot.top() + 3.0),
                egui::Align2::LEFT_TOP,
                name,
                FontId::monospace(9.0),
                ink,
            );
        }

        egui::Popup::context_menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .frame(menu::card(ui.ctx()))
            .width(164.0)
            .show(|ui| {
                ui.strong("Dynamics Monitor");
                if ui
                    .button(if *paused { "Resume" } else { "Pause" })
                    .clicked()
                {
                    *paused = !*paused;
                }
                ui.separator();
                for (trace, label) in ["Input", "Output", "Gain Reduction"]
                    .into_iter()
                    .enumerate()
                {
                    if ui
                        .selectable_label(
                            shown[trace],
                            egui::RichText::new(label).color(color[trace]),
                        )
                        .clicked()
                    {
                        shown[trace] = !shown[trace];
                    }
                }
            });
        ui.data_mut(|data| data.insert_temp(state, shown));
        response.on_hover_text(if *paused {
            "Paused. Right-click to resume."
        } else {
            "Right-click to pause or show traces."
        });
    }
}
