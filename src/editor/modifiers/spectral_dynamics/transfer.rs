use super::*;
use crate::editor::spectrum::gradient_fill;
use crate::region::dynamics::{CEILING, MAX_POINTS, Meter, Point, Segment};

impl EquartEditor {
    pub(super) fn transfer_curve(
        &self,
        ui: &mut egui::Ui,
        plot: Rect,
        index: usize,
        control: Control,
        meter: Option<Meter>,
        scale: LevelScale,
    ) {
        let id = ui.make_persistent_id(("transfer-plot", index));
        let graph = ui.interact(plot, id, Sense::click());
        let painter = ui.painter_at(plot);
        let accent =
            self.theme.region[index].gamma_multiply(if control.bypass { 0.48 } else { 1.0 });
        let text = self.theme.text.muted;
        let mut curve = control.curve;
        let custom = curve.is_custom();
        let mut samples = Vec::with_capacity((MAX_POINTS - 1) * 25 + 17);

        if custom {
            let first = curve.points()[0].input.max(scale.low());
            for step in 0..8 {
                let input = egui::lerp(scale.low()..=first, step as f32 / 8.0);
                samples.push((input, input));
            }
            for pair in curve.points().windows(2) {
                let [left, right] = [pair[0], pair[1]];
                if right.input < scale.low() {
                    continue;
                }
                for step in 0..24 {
                    let input = egui::lerp(
                        left.input.max(scale.low())..=right.input,
                        step as f32 / 24.0,
                    );
                    samples.push((input, input));
                }
                if left.segment == Segment::Hold {
                    samples.push((right.input, right.input - 0.0001));
                }
            }
            let last = curve.points()[curve.points().len() - 1]
                .input
                .max(scale.low());
            for step in 0..=8 {
                let input = egui::lerp(last..=CEILING, step as f32 / 8.0);
                samples.push((input, input));
            }
        } else {
            for step in 0..=128 {
                let input = egui::lerp(scale.low()..=CEILING, step as f32 / 128.0);
                samples.push((input, input));
            }
        }

        let path = samples
            .iter()
            .map(|&(x, input)| {
                scale.xy(
                    plot,
                    x,
                    curve
                        .output(input)
                        .unwrap_or_else(|| control.transfer(input)),
                )
            })
            .collect::<Vec<_>>();
        let live_x = meter
            .filter(|meter| meter.input >= scale.low())
            .map(|meter| scale.xy(plot, meter.input, 0.0).x);

        painter.rect_filled(plot, 0.0, self.theme.panel.menu);
        let fill: Vec<_> = path
            .iter()
            .map(|point| Pos2::new(point.x, point.y.min(plot.bottom())))
            .collect();
        gradient_fill(
            &painter,
            &fill,
            |_, y| plot.bottom() - y,
            &[0.0, 0.7, 1.0],
            |i, stop| {
                let edge = live_x.map_or(0.0, |x| (1.0 - (path[i].x - x).abs() / 23.0).max(0.0));
                accent.gamma_multiply([0.30 + edge * 0.26, 0.08 + edge * 0.07, 0.025][stop])
            },
        );
        scale.grid(&painter, plot, text);

        for db in scale.ticks() {
            let x = scale.xy(plot, db, 0.0).x;
            let alpha = if db == 0.0 { 0.46 } else { 0.22 };
            painter.vline(
                x,
                plot.y_range(),
                Stroke::new(1.0, text.gamma_multiply(alpha)),
            );
            let (label_x, alignment) = if db == CEILING {
                (x - 2.0, egui::Align2::RIGHT_TOP)
            } else {
                (x + 2.0, egui::Align2::LEFT_TOP)
            };
            painter.text(
                Pos2::new(label_x, plot.top() + 2.0),
                alignment,
                LevelScale::tick_text(db),
                FontId::monospace(8.0),
                text,
            );
        }

        painter.line_segment(
            [plot.left_bottom(), plot.right_top()],
            Stroke::new(1.0, text.gamma_multiply(0.48)),
        );
        let mut has_applied_overlay = false;
        if custom {
            let applied = samples
                .iter()
                .map(|&(x, input)| scale.xy(plot, x, control.transfer(input)))
                .collect::<Vec<_>>();
            if path
                .iter()
                .zip(&applied)
                .any(|(edit, dsp)| (edit.y - dsp.y).abs() > 0.75)
            {
                has_applied_overlay = true;
                painter.extend(egui::Shape::dashed_line(
                    &applied,
                    Stroke::new(1.05, text),
                    4.0,
                    3.0,
                ));
            }
        }

        painter.add(egui::Shape::line(path, Stroke::new(1.7, accent)));
        if let Some(x) = live_x {
            painter.vline(
                x,
                plot.y_range(),
                Stroke::new(1.0, accent.gamma_multiply(0.48)),
            );
        }

        if let Some(meter) = meter {
            let applied =
                meter.input + meter.reduction + if control.bypass { 0.0 } else { control.makeup };
            painter.circle_filled(
                scale.xy(plot, meter.input, applied),
                3.0,
                self.theme.text.primary,
            );
        }

        if graph.secondary_clicked()
            && let Some(position) = graph.interact_pointer_pos()
        {
            if !custom {
                curve = Curve::from_standard(control);
            }
            if curve.insert(scale.point(plot, position)) {
                self.params.spectral_transfer_curves.edit(index, curve);
            }
        }

        if custom {
            self.curve_points(ui, &painter, plot, index, control, accent, scale);
        }

        if let Some(position) = graph.hover_pos() {
            let input = scale.point(plot, position).input;
            let hint = if has_applied_overlay {
                "\nDashed line shows the applied curve with Range and Makeup."
            } else {
                ""
            };
            graph.on_hover_text(format!(
                "Input {input:.1} dB\nApplied output {:.1} dB\nScroll to zoom. Right-click to add a point.{hint}",
                control.transfer(input),
            ));
        }
    }

    fn curve_points(
        &self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        plot: Rect,
        index: usize,
        control: Control,
        accent: Color32,
        scale: LevelScale,
    ) {
        let curve = control.curve;
        let bank = &self.params.spectral_transfer_curves;
        let clip = ui.clip_rect().intersect(plot);
        let drag_id = ui.make_persistent_id(("transfer-node-drag", index));

        for (slot, points) in curve.points().windows(2).enumerate() {
            let point = points[0];
            if point.segment == Segment::Hold {
                continue;
            }

            let input = (point.input + points[1].input) * 0.5;
            let pos = scale.xy(
                plot,
                input,
                curve.output(input).expect("active custom curve"),
            );
            if !clip.contains(pos) {
                continue;
            }
            let hit = Rect::from_center_size(pos, Vec2::splat(10.0)).intersect(clip);
            let handle = ui
                .interact(
                    hit,
                    ui.make_persistent_id(("transfer-bend", index, slot)),
                    Sense::drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeVertical);
            painter.circle_stroke(pos, 2.5 + hover_amount(&handle), Stroke::new(1.0, accent));

            if handle.dragged() && handle.drag_motion().y != 0.0 {
                let mut changed = curve;
                let mut point = point;
                if point.segment == Segment::Linear {
                    point.segment = Segment::Bend;
                }
                let direction = if points[1].output >= point.output {
                    1.0
                } else {
                    -1.0
                };
                point.bend -= direction * handle.drag_motion().y * (8.0 / plot.height());
                changed.set(slot, point);
                bank.edit(index, changed);
            }
            handle.on_hover_text("Drag to bend this segment");
        }

        for (slot, &point) in curve.points().iter().enumerate() {
            let pos = scale.xy(plot, point.input, point.output);
            let hit = Rect::from_center_size(pos, Vec2::splat(12.0)).intersect(clip);
            if !hit.is_positive() {
                continue;
            }
            let response = ui
                .interact(
                    hit,
                    ui.make_persistent_id(("transfer-node", index, slot)),
                    Sense::click_and_drag(),
                )
                .on_hover_cursor(egui::CursorIcon::Grab);
            painter.circle_filled(pos, 3.8 + hover_amount(&response), self.theme.panel.menu);
            painter.circle_stroke(pos, 4.5 + hover_amount(&response), Stroke::new(1.3, accent));

            if response.drag_started() {
                let origin = ui
                    .input(|input| input.pointer.press_origin())
                    .unwrap_or(pos);
                ui.data_mut(|data| data.insert_temp(drag_id, (curve, slot, pos - origin)));
            }
            if (response.dragged() || response.drag_stopped())
                && let Some((mut original, source, offset)) =
                    ui.data(|data| data.get_temp::<(Curve, usize, Vec2)>(drag_id))
                && source == slot
            {
                if let Some(position) = response.interact_pointer_pos() {
                    let value = scale.point(plot, position + offset);
                    let original_point = original.points()[source];
                    original.set(
                        source,
                        Point {
                            input: value.input,
                            output: value.output,
                            ..original_point
                        },
                    );
                    bank.edit(index, original);
                }
                if response.drag_stopped() {
                    ui.data_mut(|data| data.remove::<(Curve, usize, Vec2)>(drag_id));
                }
            }

            egui::Popup::context_menu(&response)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .frame(menu::card(ui.ctx()))
                .width(185.0)
                .show(|ui| self.point_menu(ui, index, slot, curve, point));
            response.on_hover_text(format!(
                "Input {:.1} dB\nPoint {:.1} dB\nApplied {:.1} dB",
                point.input,
                point.output,
                control.transfer(point.input),
            ));
        }
    }

    fn point_menu(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        slot: usize,
        mut curve: Curve,
        point: Point,
    ) {
        let bank = &self.params.spectral_transfer_curves;
        let end = slot + 1 == curve.points().len();
        ui.strong(format!("Point {} / {}", slot + 1, curve.points().len()));
        if ui
            .add_enabled(curve.points().len() > 2, egui::Button::new("Delete Point"))
            .clicked()
        {
            curve.remove(slot);
            bank.edit(index, curve);
            ui.close();
            return;
        }

        let mut edit = point;
        ui.horizontal(|ui| {
            ui.label("Input");
            ui.add(
                egui::DragValue::new(&mut edit.input)
                    .speed(0.1)
                    .suffix(" dB"),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Output");
            ui.add(
                egui::DragValue::new(&mut edit.output)
                    .speed(0.1)
                    .suffix(" dB"),
            );
        });

        ui.add_enabled_ui(!end, |ui| {
            ui.separator();
            ui.strong("Next Segment");
            for (segment, text) in [
                (Segment::Linear, "Linear"),
                (Segment::Bend, "Curve"),
                (Segment::Smooth, "Smooth"),
                (Segment::SCurve, "S Curve"),
                (Segment::Hold, "Hold"),
            ] {
                if ui.selectable_label(edit.segment == segment, text).clicked() {
                    edit.segment = segment;
                }
            }
            ui.add_enabled(
                !matches!(edit.segment, Segment::Linear | Segment::Hold),
                egui::DragValue::new(&mut edit.bend)
                    .speed(0.01)
                    .range(-1.0..=1.0)
                    .prefix("Bend "),
            );
        });

        if edit != point {
            curve.set(slot, edit);
            bank.edit(index, curve);
        }

        ui.separator();
        let clipboard = egui::Id::unique("transfer-point-value");
        if ui.button("Copy Value").clicked() {
            ui.data_mut(|data| data.insert_temp(clipboard, point));
        }
        let copied = ui.data(|data| data.get_temp::<Point>(clipboard));
        if ui
            .add_enabled(copied.is_some(), egui::Button::new("Paste Value"))
            .clicked()
            && let Some(copied) = copied
        {
            curve.set(
                slot,
                Point {
                    input: point.input,
                    ..copied
                },
            );
            bank.edit(index, curve);
        }
    }
}
