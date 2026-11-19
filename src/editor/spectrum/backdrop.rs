use super::*;

// slop, generate by something u know
// idk how does it works

/// Feather the four sides of the plate.
///
/// The axis is inset 48/22/30/60 px from the plate to make room for the dB
/// scales and the keyboard, and the drawing runs out to the plate itself: the
/// grid, the analyser and the curves all carry on into those margins. The ramps
/// are anchored to the plate's edge, so the drawing dissolves on its way to it
/// rather than stopping at a bare ring of plate.
///
/// The two sides do not reach in by the same amount, which is why they are not
/// one loop with one width. Both used to be a flat 120 px; on the right, where
/// the margin is 30, that covered ninety-six pixels of the plot -- the
/// analyser's top octave and the 20 kHz line with it -- and the quietest part of
/// every trace read as if the analyser had given up. That band now stops at the
/// axis. The left one keeps its 120 and so reaches 72 px past the axis into the
/// plot: the low end of the curve is steep and dense, and taking it down into
/// the scale column reads as the response running out of the frame, where the
/// same treatment at the right would just hide a quiet trace.
///
/// Every edge is opaque at the border, so nothing reads as cut off. The top
/// band starts below the header, so the glass itself is never washed out.
/// Two bands overlap in each corner. It runs after the curves and before the
/// labels, keys, nodes and readout, so all of those stay crisp even near a
/// border.
pub(super) const FADE_LEFT: f32 = 120.0;
pub(super) const FADE_TOP: f32 = 56.0;
pub(super) const FADE_BOTTOM: f32 = 72.0;
pub(super) const FADE_SIDE_ALPHA: f32 = 1.0;
/// How much darker the side mask is than the plate. A wash that matches the
/// plate exactly leaves the already faint grid looking just as bright at the
/// border as in the middle, so the fade reads as a flat edge.
const FADE_SIDE_SHADE: u8 = 9;
const FADE_STEPS: usize = 16;

/// `plate` is the rect the backdrop gradient is anchored to -- the whole
/// canvas, not just this region, so the mask colour matches what is actually
/// behind the drawing (and behind the translucent bars) at every height.
/// One frame of a scale column's glow. An exponential approach with separate
/// time constants for rising and falling, which is what makes the patch ease
/// in under the pointer and linger a moment after it leaves. Once it is within
/// a rounding step of the target it snaps, so the value settles and the caller
/// can stop asking for frames.
pub(super) fn ease_glow(current: f32, target: f32, dt: f32) -> f32 {
    let tau = if target > current {
        AXIS_FADE_IN
    } else {
        AXIS_FADE_OUT
    };
    let next = current + (target - current) * (1.0 - (-dt / tau).exp());
    if (next - target).abs() < 0.0005 {
        target
    } else {
        next
    }
}

/// A patch of light with no outline: opaque-ish in the middle and fully clear
/// at every edge. Used for the two dB columns, which are click targets -- a
/// filled rectangle with a border read as a panel sitting on the plate, and
/// the frame was the worst of it, so the wash feathers out on all four sides
/// instead. Alpha is the peak in the middle of the patch.
pub(super) fn soft_wash(p: &egui::Painter, rect: Rect, alpha: f32) {
    if alpha <= 0.002 || rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }

    // The columns are narrow and tall: a feather proportional to the width
    // would be a hairline, and one proportional to the height would eat the
    // whole patch, so both are capped.
    let x_feather = (rect.width() * 0.30).min(16.0);
    let y_feather = (rect.height() * 0.08).min(26.0);
    let xs = [
        rect.left(),
        rect.left() + x_feather,
        rect.right() - x_feather,
        rect.right(),
    ];
    let ys = [
        rect.top(),
        rect.top() + y_feather,
        rect.bottom() - y_feather,
        rect.bottom(),
    ];
    // Ramp per column and row: clear on the outside, full inside. Multiply the
    // two so the corners fade in both directions.
    let ramp = [0.0, 1.0, 1.0, 0.0];

    let mut mesh = egui::Mesh::default();
    for (row, y) in ys.iter().enumerate() {
        for (column, x) in xs.iter().enumerate() {
            let a = (alpha * ramp[column] * ramp[row] * 255.0).round() as u8;
            mesh.colored_vertex(
                Pos2::new(*x, *y),
                Color32::from_rgba_unmultiplied(255, 255, 255, a),
            );
        }
    }
    for row in 0..3u32 {
        for column in 0..3u32 {
            let corner = row * 4 + column;
            mesh.add_triangle(corner, corner + 1, corner + 4);
            mesh.add_triangle(corner + 1, corner + 5, corner + 4);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

pub(super) fn edge_fade(p: &egui::Painter, outer: Rect, plate: Rect, axis: Rect) {
    let painter = p.with_clip_rect(outer);

    // The backdrop is a vertical gradient, so the mask has to follow it; a flat
    // colour would show up as a band of the wrong brightness inside the fade.
    let at = |y: f32| ((y - plate.top()) / plate.height().max(1.0)).clamp(0.0, 1.0);
    let plate_color = |y: f32| {
        let t = at(y);
        let lerp = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Color32::from_gray(lerp(PLATE_TOP.r(), PLATE_BOTTOM.r()))
    };
    let tinted = |color: Color32, alpha: f32| {
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (alpha * 255.0) as u8)
    };
    // The sides sit a shade under the plate, so the fade is a visible vignette
    // and not just a wash that happens to match what is behind it.
    let side_color =
        |y: f32| Color32::from_gray(plate_color(y).r().saturating_sub(FADE_SIDE_SHADE));

    // Left and right: the ramp runs across x, and the colour follows the plate
    // down the height, so the band has to be split into rows. The left one is
    // wider than its margin and reaches into the plot; the right one ends at
    // the axis, because the plot is what the fade is there to reveal and not
    // to cover. The header comment says why they differ.
    for (edge_x, inner_x) in [
        (outer.left(), outer.left() + FADE_LEFT),
        (outer.right(), axis.right()),
    ] {
        let mut mesh = egui::Mesh::default();

        for step in 0..=FADE_STEPS {
            let t = step as f32 / FADE_STEPS as f32;
            let y = outer.top() + t * outer.height();
            let color = side_color(y);
            mesh.colored_vertex(Pos2::new(edge_x, y), tinted(color, FADE_SIDE_ALPHA));
            mesh.colored_vertex(Pos2::new(inner_x, y), tinted(color, 0.0));
        }

        for i in 0..FADE_STEPS as u32 {
            let a = i * 2;
            mesh.add_triangle(a, a + 1, a + 2);
            mesh.add_triangle(a + 1, a + 3, a + 2);
        }

        painter.add(egui::Shape::mesh(mesh));
    }

    // Top and bottom: the ramp runs across y. The bands span less than a grey
    // step of the plate gradient, so one colour is enough for the whole strip.
    // The header is glass now: start the top fade below it, or the mask would
    // wash out exactly the grid and analyser we want showing through. In the
    // fade tests `outer` already starts below the header, so this leaves
    // their geometry untouched.
    let fade_top = outer.top().max(plate.top() + Layout::HEADER);
    for (edge_y, inner_y) in [
        (fade_top, fade_top + FADE_TOP),
        (outer.bottom(), outer.bottom() - FADE_BOTTOM),
    ] {
        let color = side_color(edge_y);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(
            Pos2::new(outer.left(), edge_y),
            tinted(color, FADE_SIDE_ALPHA),
        );
        mesh.colored_vertex(
            Pos2::new(outer.right(), edge_y),
            tinted(color, FADE_SIDE_ALPHA),
        );
        mesh.colored_vertex(Pos2::new(outer.left(), inner_y), tinted(color, 0.0));
        mesh.colored_vertex(Pos2::new(outer.right(), inner_y), tinted(color, 0.0));
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        painter.add(egui::Shape::mesh(mesh));
    }
}
