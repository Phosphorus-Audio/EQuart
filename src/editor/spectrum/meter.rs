use super::*;

pub(in crate::editor) fn meter_column(canvas: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(
            canvas.right() - Layout::METER_W,
            canvas.top() + Layout::HEADER,
        ),
        Pos2::new(canvas.right(), canvas.bottom() - Layout::FOOTER),
    )
}

pub(super) fn meter_rect(canvas: Rect, plot: &Plot) -> Rect {
    Rect::from_min_max(
        Pos2::new(canvas.right() - Layout::METER_W, plot.spectrum_y(0.0)),
        meter_column(canvas).max,
    )
}

pub(super) fn meter_color(db: f32) -> Color32 {
    if db >= -1.0 {
        RED
    } else if db >= -6.0 {
        Color32::from_rgb(232, 142, 52)
    } else if db >= -18.0 {
        Color32::from_rgb(224, 198, 72)
    } else {
        Color32::from_rgb(104, 192, 108)
    }
}

const METER_SEGMENT_ROUNDING: u8 = 1;

pub(super) const METER_READOUT_SIZE: f32 = 8.0;

pub(super) fn meter_segments(track: Rect, pixels_per_point: f32) -> Vec<Rect> {
    let pp = pixels_per_point.max(1.0);
    let snap = |y: f32| (y * pp).round() / pp;
    let pitch = (5.0 * pp).round() / pp;
    let lit = (4.0 * pp).max(1.0).round() / pp;

    let mut segments = Vec::new();
    let mut top = snap(track.top());
    while top < track.bottom() {
        segments.push(Rect::from_min_max(
            Pos2::new(track.left(), top),
            Pos2::new(track.right(), (top + lit).min(track.bottom())),
        ));
        top = snap(top + pitch);
    }
    segments
}

impl EquartEditor {
    pub(in crate::editor) fn level_meter(
        &self,
        ui: &mut egui::Ui,
        p: &egui::Painter,
        canvas: Rect,
        values: &MeterReadings,
    ) {
        let visible = self.meter_visible();
        let id = ui.make_persistent_id("level-meter-gain");
        let mut pressing = false;

        if let Some(gui) = &self.gui {
            let gain = &self.params.global.output;
            let setter = gui.param_setter();

            if visible {
                let response = ui
                    .interact(meter_column(canvas), id, Sense::drag())
                    .on_hover_and_drag_cursor(egui::CursorIcon::ResizeVertical);
                pressing = response.dragged_by(egui::PointerButton::Primary);

                if response.drag_started_by(egui::PointerButton::Primary) {
                    setter.begin_set_parameter(gain);
                    ui.data_mut(|data| data.insert_temp(id, gain.unmodulated_normalized_value()));
                }

                if pressing
                    && let Some(delta) = response.total_drag_delta()
                    && let Some(position) = response.interact_pointer_pos()
                    && let Some(start) = ui.data(|data| data.get_temp::<f32>(id))
                {
                    let value = (start - delta.y * 0.004).clamp(0.0, 1.0);

                    if value != gain.unmodulated_normalized_value() {
                        setter.set_parameter(gain, gain.preview_plain(value));
                    }

                    let painter = ui.ctx().layer_painter(egui::LayerId::new(
                        egui::Order::Tooltip,
                        id.with("readout"),
                    ));
                    draw_gain_readout(
                        &painter,
                        canvas,
                        position,
                        gain.normalized_value_to_string(value, true),
                    );
                }
            }

            if !pressing && ui.data_mut(|data| data.remove_temp::<f32>(id)).is_some() {
                setter.end_set_parameter(gain);
            }
        }

        if visible {
            draw_level_meter(
                p,
                canvas,
                values,
                self.spectrum_range(),
                self.meter_fade(),
                self.piano_keys_visible(),
                pressing,
            );
        }
    }
}

fn draw_gain_readout(p: &egui::Painter, canvas: Rect, pointer: Pos2, text: String) {
    let galley = p.layout_no_wrap(text, FontId::monospace(12.0), TEXT);
    let padding = Vec2::new(8.0, 4.0);
    let size = galley.size() + padding * 2.0;
    let min = Pos2::new(
        (pointer.x - size.x - 12.0).max(canvas.left() + 4.0),
        (pointer.y - size.y * 0.5).clamp(canvas.top() + 4.0, canvas.bottom() - size.y - 4.0),
    );

    p.rect(
        Rect::from_min_size(min, size),
        6.0,
        Color32::from_gray(34),
        Stroke::new(1.0, Color32::from_gray(68)),
        egui::StrokeKind::Inside,
    );
    p.galley(min + padding, galley, TEXT);
}

fn draw_level_meter(
    p: &egui::Painter,
    canvas: Rect,
    values: &MeterReadings,
    spectrum_range: f32,
    fade_foot: bool,
    piano_keys_visible: bool,
    pressing: bool,
) {
    let plot = Plot::with_ranges(
        Layout::plot_outer(canvas, true),
        values.sample_rate,
        DEFAULT_AXIS_RANGE as f32,
        spectrum_range,
        piano_keys_visible,
    );

    let column = meter_column(canvas);
    p.rect_filled(column, 0.0, Color32::from_gray(14));
    p.line_segment(
        [column.left_top(), column.left_bottom()],
        Stroke::new(1.0, Color32::from_gray(44)),
    );

    let rect = meter_rect(canvas, &plot);
    let painter = p.with_clip_rect(rect);

    let track = Rect::from_min_max(
        Pos2::new(rect.left() + 4.0, rect.top()),
        Pos2::new(rect.right() - 4.0, rect.bottom()),
    );
    painter.rect_filled(track, 0.0, Color32::from_gray(22));

    let peak = values.output_peak;
    let peak_db = if peak > 0.0 {
        20.0 * peak.log10()
    } else {
        f32::NEG_INFINITY
    };

    for segment in meter_segments(track, painter.pixels_per_point()) {
        let db = plot.spectrum_db((segment.top() + segment.bottom()) * 0.5);
        let color = meter_color(db);
        let rounding = egui::CornerRadius::same(METER_SEGMENT_ROUNDING);
        painter.rect_filled(
            segment,
            rounding,
            if peak_db >= db {
                color
            } else {
                color.gamma_multiply(0.2)
            },
        );
    }

    if peak_db.is_finite() {
        let y = plot.spectrum_y(peak_db).clamp(track.top(), track.bottom());
        painter.line_segment(
            [Pos2::new(track.left(), y), Pos2::new(track.right(), y)],
            Stroke::new(
                1.5,
                if peak > 1.0 {
                    Color32::from_rgb(255, 214, 214)
                } else {
                    Color32::from_white_alpha(150)
                },
            ),
        );
    }

    if fade_foot {
        fade_meter_foot(p, column, plot.rect.bottom());
    }

    if pressing {
        p.rect_filled(column, 0.0, Color32::from_black_alpha(48));
    }

    draw_meter_readout(p, column, rect.top(), peak_db);
}

fn fade_meter_foot(p: &egui::Painter, column: Rect, from: f32) {
    let top = from.clamp(column.top(), column.bottom());
    let height = column.bottom() - top;
    if height <= 1.0 {
        return;
    }

    let well = Color32::from_gray(14);
    let steps = 10;
    let mut mesh = egui::Mesh::default();
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let y = top + height * t;
        let ink =
            Color32::from_rgba_unmultiplied(well.r(), well.g(), well.b(), meter_fade_alpha(t));
        mesh.colored_vertex(Pos2::new(column.left(), y), ink);
        mesh.colored_vertex(Pos2::new(column.right(), y), ink);
    }
    for step in 0..steps as u32 {
        let first = step * 2;
        mesh.add_triangle(first, first + 1, first + 2);
        mesh.add_triangle(first + 1, first + 3, first + 2);
    }
    p.with_clip_rect(column).add(egui::Shape::mesh(mesh));
}

pub(super) fn meter_fade_alpha(t: f32) -> u8 {
    (t.clamp(0.0, 1.0).powf(1.7) * 255.0).round() as u8
}

fn draw_meter_readout(p: &egui::Painter, column: Rect, ladder_top: f32, peak_db: f32) {
    let head = Rect::from_min_max(column.min, Pos2::new(column.right(), ladder_top));
    if head.height() < 8.0 {
        return;
    }

    let ink = meter_ink(peak_db);
    let galley = p.layout_no_wrap(
        meter_readout_text(peak_db),
        FontId::monospace(METER_READOUT_SIZE),
        ink,
    );

    if galley.size().x <= head.width() && galley.size().y <= head.height() {
        p.galley(head.center() - galley.size() * 0.5, galley, ink);
    }
}

pub(super) fn meter_readout_text(peak_db: f32) -> String {
    if !peak_db.is_finite() {
        "--".to_string()
    } else if peak_db > -9.95 {
        format!("{peak_db:.1}")
    } else {
        format!("{peak_db:.0}")
    }
}

pub(super) fn meter_ink(peak_db: f32) -> Color32 {
    if peak_db.is_finite() {
        meter_color(peak_db)
    } else {
        Color32::from_gray(96)
    }
}
