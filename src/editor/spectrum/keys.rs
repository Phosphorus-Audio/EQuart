use super::*;

/// A4 = MIDI 69 = 440 Hz
pub(super) fn note_hz(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

pub(super) fn note_at(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

pub(super) fn is_black(note: i32) -> bool {
    matches!(note.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

const PITCH_CLASSES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub(super) fn note_name(note: i32) -> String {
    format!(
        "{}{}",
        PITCH_CLASSES[note.rem_euclid(12) as usize],
        note.div_euclid(12) - 1
    )
}

pub(super) fn white_key_edges(note: i32) -> (f32, f32) {
    let mut previous = note - 1;
    while is_black(previous) {
        previous -= 1;
    }
    let mut next = note + 1;
    while is_black(next) {
        next += 1;
    }
    ((previous + note) as f32 * 0.5, (note + next) as f32 * 0.5)
}

pub(super) fn visible_notes(plot: &Plot, strip: Rect) -> std::ops::RangeInclusive<i32> {
    let low = note_at(plot.hz_extended(strip.left())).floor() as i32 - 1;
    let high = note_at(plot.hz_extended(strip.right())).ceil() as i32 + 1;
    low..=high
}

pub(crate) fn keys_strip(outer: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(outer.left(), outer.bottom() - KEYS_HEIGHT),
        outer.right_bottom(),
    )
}

pub(super) fn draw_keys(
    p: &egui::Painter,
    plot: &Plot,
    outer: Rect,
    pointer: Option<Pos2>,
    tint: Color32,
    bottom_fade: bool,
) {
    let rect = keys_strip(outer);
    let palette = theme::current(p.ctx());
    let keys = palette.piano;
    let painter = p.with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, keys.well);

    let hovered = pointer
        .filter(|pos| {
            (rect.left()..=rect.right()).contains(&pos.x)
                && (plot.rect.top()..=rect.bottom()).contains(&pos.y)
        })
        .map(|pos| {
            let hz = plot.hz_extended(pos.x);
            (pos.x, hz, note_at(hz).round() as i32)
        });

    let x = |note: f32| plot.x_unchecked(note_hz(note));
    let range = visible_notes(plot, rect);
    let semitone = x(1.0) - x(0.0);
    let white_top = rect.top() + 1.0;
    let white_bottom = rect.bottom();
    let highlight = tint.gamma_multiply(0.36);

    for note in range.clone() {
        if is_black(note) {
            continue;
        }

        let (left, right) = white_key_edges(note);
        let key = Rect::from_min_max(
            Pos2::new(x(left), white_top),
            Pos2::new(x(right), white_bottom),
        );
        if !key.intersects(rect) {
            continue;
        }

        gradient_rect(
            &painter,
            key,
            keys.white_top,
            keys.white_bottom,
        );
        if hovered.is_some_and(|(_, _, h)| h == note) {
            painter.rect_filled(key, 0.0, highlight);
        }
        painter.line_segment(
            [key.left_top(), key.left_bottom()],
            Stroke::new(
                0.7,
                if note.rem_euclid(12) == 0 {
                    Color32::from_gray(48)
                } else {
                    Color32::from_gray(78)
                },
            ),
        );
        painter.line_segment(
            [key.left_bottom(), key.right_bottom()],
            Stroke::new(0.6, Color32::from_white_alpha(52)),
        );
    }

    for note in range {
        if !is_black(note) {
            continue;
        }

        let centre = x(note as f32);
        let half = (semitone * 0.38).max(0.95);
        let key = Rect::from_min_max(
            Pos2::new(centre - half, rect.top()),
            Pos2::new(centre + half, rect.top() + KEYS_HEIGHT * 0.58),
        );
        if !key.intersects(rect) {
            continue;
        }

        painter.rect_filled(key, 1.0, keys.black);
        if hovered.is_some_and(|(_, _, h)| h == note) {
            painter.rect_filled(key, 1.0, tint.gamma_multiply(0.62));
        }
    }

    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(1.0, Color32::from_gray(9)),
    );

    let backdrop = keys.well;
    let wash = |alpha: u8| {
        Color32::from_rgba_unmultiplied(backdrop.r(), backdrop.g(), backdrop.b(), alpha)
    };
    for (edge_x, inner_x) in [
        (rect.left(), rect.left() + KEYS_FADE),
        (rect.right(), rect.right() - KEYS_FADE),
    ] {
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(Pos2::new(edge_x, rect.top()), wash(255));
        mesh.colored_vertex(Pos2::new(edge_x, rect.bottom()), wash(255));
        mesh.colored_vertex(Pos2::new(inner_x, rect.top()), wash(0));
        mesh.colored_vertex(Pos2::new(inner_x, rect.bottom()), wash(0));
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        painter.add(egui::Shape::mesh(mesh));
    }

    if bottom_fade {
        let bottom = Rect::from_min_max(
            Pos2::new(rect.left(), rect.bottom() - 8.0),
            rect.right_bottom(),
        );
        gradient_rect(&painter, bottom, wash(0), wash(128));
    }

    if let Some((pointer_x, hz, note)) = hovered {
        let text = format!("{}  {}", note_name(note), format_hz(hz));
        let font = FontId::new(10.0, egui::FontFamily::Name(GLOBAL_FONT.into()));
        let size = p.layout_no_wrap(text.clone(), font.clone(), palette.text.primary).size();
        let width = size.x + 12.0;
        let height = size.y + 6.0;
        let right = (rect.right() - width).max(rect.left());
        let left = (pointer_x - width * 0.5).clamp(rect.left(), right);
        let label = Rect::from_min_size(
            Pos2::new(left, rect.top() - height - 3.0),
            Vec2::new(width, height),
        );
        let plate = Color32::from_rgb(22, 26, 24).lerp_to_gamma(tint, 0.22);
        p.rect_filled(label, 3.0, plate.gamma_multiply(0.94));
        p.text(
            label.center(),
            egui::Align2::CENTER_CENTER,
            text,
            font,
            palette.text.primary,
        );
    }
}
