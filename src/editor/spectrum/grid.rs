use super::*;


fn frequency_ticks(max_hz: f32) -> Vec<(f32, bool)> {
    let mut ticks = Vec::new();
    let mut decade = 10.0_f32;

    while decade <= max_hz {
        for step in 1..=9 {
            let hz = decade * step as f32;
            if hz > max_hz {
                break;
            }

            ticks.push((hz, matches!(step, 1 | 2 | 5)));
        }

        decade *= 10.0;
    }

    ticks
}

fn axis_ticks(range: f32) -> [f32; 9] {
    let step = range / 4.0;
    std::array::from_fn(|i| (i as f32 - 4.0) * step)
}

fn axis_label(db: f32) -> String {
    if db == 0.0 {
        "0".to_string()
    } else if (db - db.round()).abs() < 1e-4 {
        format!("{db:+.0}")
    } else {
        format!("{db:+.1}")
    }
}

pub(super) fn spectrum_ticks(floor: f32) -> [f32; 4] {
    let step = (floor / 3.0 / 5.0).round() * 5.0;
    [0.0, -step, -step * 2.0, -floor]
}

pub(super) fn draw_grid_lines(p: &egui::Painter, plot: &Plot, outer: Rect) {
    let colors = theme::current(p.ctx()).grid;
    let top = outer.top() + Layout::HEADER;
    for (hz, labelled) in frequency_ticks(plot.max_hz) {
        let x = plot.x(hz);
        if (x - plot.rect.left()).abs() < 0.5 {
            continue;
        }
        p.line_segment(
            [Pos2::new(x, top), Pos2::new(x, outer.bottom())],
            Stroke::new(1.0, if labelled { colors.major } else { colors.minor }),
        );
    }

    for db in axis_ticks(plot.range) {
        let y = plot.y(db);
        if (y - plot.rect.bottom()).abs() < 0.5 {
            continue;
        }
        p.line_segment(
            [Pos2::new(outer.left(), y), Pos2::new(outer.right(), y)],
            Stroke::new(
                if db == 0.0 { 1.2 } else { 1.0 },
                if db == 0.0 {
                    colors.zero
                } else {
                    colors.major
                },
            ),
        );
    }
}

pub(super) fn draw_axis_labels(
    p: &egui::Painter,
    plot: &Plot,
    outer: Rect,
    eq_highlight: f32,
    spectrum_highlight: f32,
) {
    let text = theme::current(p.ctx()).text;

    for (rect, alpha) in [
        (
            Rect::from_min_max(
                Pos2::new(outer.left(), outer.top() + Layout::HEADER),
                Pos2::new(plot.rect.left(), plot.rect.bottom()),
            ),
            eq_highlight,
        ),
        (
            Rect::from_min_max(
                Pos2::new(plot.rect.right(), outer.top() + Layout::HEADER),
                Pos2::new(outer.right(), plot.rect.bottom()),
            ),
            spectrum_highlight,
        ),
    ] {
        soft_wash(p, rect, alpha);
    }

    p.text(
        Pos2::new(plot.rect.left(), outer.top() + Layout::HEADER + 8.0),
        egui::Align2::LEFT_CENTER,
        format!("EQ dB ±{:.0}", plot.range),
        FontId::proportional(10.0),
        text.muted,
    );

    p.text(
        Pos2::new(plot.rect.right(), outer.top() + Layout::HEADER + 8.0),
        egui::Align2::RIGHT_CENTER,
        format!("{:.0} dBFS", plot.spectrum_range),
        FontId::proportional(10.0),
        text.muted,
    );

    for (hz, labelled) in frequency_ticks(plot.max_hz) {
        if !labelled {
            continue;
        }

        let label = if hz >= 1_000.0 {
            format!("{:.0}k", hz / 1_000.0)
        } else {
            format!("{hz:.0}")
        };
        p.text(
            Pos2::new(plot.x(hz), plot.rect.bottom() + 14.0),
            egui::Align2::CENTER_CENTER,
            label,
            FontId::monospace(10.0),
            text.muted,
        );
    }

    let eq_ink = text.muted.lerp_to_gamma(text.primary, (eq_highlight / AXIS_HOVER_ALPHA).clamp(0.0, 1.0));
    for db in axis_ticks(plot.range) {
        p.text(
            Pos2::new(plot.rect.left() - 9.0, plot.y(db)),
            egui::Align2::RIGHT_CENTER,
            axis_label(db),
            FontId::monospace(9.5),
            eq_ink,
        );
    }

    for db in spectrum_ticks(plot.spectrum_floor()) {
        let y = plot.spectrum_y(db);
        p.text(
            Pos2::new(outer.right() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{db:.0}"),
            FontId::monospace(9.5),
            text.spectrum_scale.lerp_to_gamma(
                text.primary,
                (spectrum_highlight / AXIS_HOVER_ALPHA).clamp(0.0, 1.0),
            ),
        );
    }
}
