use super::*;

const BASS_CURVE_POINTS: usize = 34;
const BASS_CURVE_SAMPLES: usize = 64;
const SPECTRUM_HUES: [[u8; 3]; 5] = [
    [186, 214, 120],
    [126, 205, 148],
    [104, 190, 190],
    [100, 146, 186],
    [120, 112, 178],
];
pub(super) const SPECTRUM_DEPTH: [f32; 5] = [0.0, 0.08, 0.22, 0.50, 1.0];
const SPECTRUM_ALPHA: [f32; 5] = [0.55, 0.30, 0.11, 0.025, 0.0];

pub(super) fn spectrum_color(t: f32, lift: f32, alpha: u8) -> Color32 {
    let stop = t.clamp(0.0, 1.0) * (SPECTRUM_HUES.len() - 1) as f32;
    let first = (stop as usize).min(SPECTRUM_HUES.len() - 2);
    let weight = stop - first as f32;
    let [r, g, b] = std::array::from_fn(|channel| {
        let a = f32::from(SPECTRUM_HUES[first][channel]);
        let b = f32::from(SPECTRUM_HUES[first + 1][channel]);
        let hue = a + (b - a) * weight;
        (hue + (255.0 - hue) * lift).round() as u8
    });
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

pub(super) fn draw_spectrum(
    p: &egui::Painter,
    plot: &Plot,
    outer: Rect,
    spectrum: &SpectrumFrame,
    hold: Hold,
) {
    let painter = p.with_clip_rect(Rect::from_min_max(
        outer.min,
        Pos2::new(outer.right(), plot.spectrum_bottom),
    ));
    let axis = plot.rect;
    let hue_at = move |x: f32| (x - axis.left()) / axis.width();
    let bottom = plot.spectrum_bottom;
    let fade_height = (bottom - plot.spectrum_top) * (30.0 / plot.spectrum_range);
    let fade = move |y: f32| ((bottom - y) / fade_height).clamp(0.0, 1.0);
    let depth = (axis.height() * 0.40).max(1.0);
    // Smooth mode uses held levels directly as the main trace
    let (input_levels, output_levels) = match hold {
        Hold::Smooth => (&spectrum.hold_levels, &spectrum.output_hold_levels),
        Hold::Off | Hold::Line => (&spectrum.levels, &spectrum.output_levels),
    };
    for (levels, output) in [(input_levels, false), (output_levels, true)] {
        if levels.len() < BASS_CURVE_POINTS || levels.iter().all(|db| *db <= -plot.spectrum_range) {
            continue;
        }
        let points = spectrum_path(plot, spectrum.bin_hz, levels);
        gradient_fill(
            &painter,
            &points,
            |_, _| depth,
            &SPECTRUM_DEPTH,
            |i, stop| {
                let point = points[i];
                spectrum_color(hue_at(point.x), 0.0, 255)
                    .gamma_multiply(SPECTRUM_ALPHA[stop] * fade(point.y))
            },
        );
        if output {
            for ((width, lift, opacity), points) in [(5.0, 0.0, 0.13), (1.6, 0.42, 1.0)]
                .into_iter()
                .zip([points.clone(), points])
            {
                let stroke = egui::epaint::PathStroke::new_uv(width, move |_, point| {
                    spectrum_color(hue_at(point.x), lift, 255)
                        .gamma_multiply(opacity * fade(point.y))
                });
                painter.add(egui::Shape::line(points, stroke));
            }
        }
    }

    // draw white peak-hold line only in Line mode
    if hold == Hold::Line
        && spectrum.hold_levels.len() >= BASS_CURVE_POINTS
        && !spectrum
            .hold_levels
            .iter()
            .all(|db| *db <= -plot.spectrum_range)
    {
        let floor = -plot.spectrum_range;
        let held: Vec<f32> = spectrum
            .hold_levels
            .iter()
            .map(|db| db.max(floor))
            .collect();
        let points = spectrum_path(plot, spectrum.bin_hz, &held);
        // faint glow + main line
        for ((width, opacity), points) in [(3.2, 0.14), (1.1, 0.92)]
            .into_iter()
            .zip([points.clone(), points])
        {
            let stroke = egui::epaint::PathStroke::new_uv(width, move |_, point| {
                let alpha = (255.0 * opacity * fade(point.y)).round() as u8;
                Color32::from_rgba_unmultiplied(238, 243, 245, alpha)
            });
            painter.add(egui::Shape::line(points, stroke));
        }
    }
}

pub(super) fn spectrum_path(plot: &Plot, bin_hz: f32, levels: &[f32]) -> Vec<Pos2> {
    let mut bins = levels.iter().enumerate().map(|(i, &db)| {
        // put DC at half-bin so log(0) won't happen
        let hz = (i as f32).max(0.5) * bin_hz;
        Pos2::new(plot.x_unchecked(hz), plot.spectrum_y(db))
    });

    let bass: [Pos2; BASS_CURVE_POINTS] =
        std::array::from_fn(|_| bins.next().expect("the bass curve has enough bins"));
    let secants: [f32; BASS_CURVE_POINTS - 1] =
        std::array::from_fn(|i| (bass[i + 1].y - bass[i].y) / (bass[i + 1].x - bass[i].x));
    let mut tangents = [0.0; BASS_CURVE_POINTS];
    let slope = |i: isize| match i {
        -1 => 2.0 * secants[0] - secants[1],
        i if i as usize == secants.len() => {
            2.0 * secants[secants.len() - 1] - secants[secants.len() - 2]
        }
        i => secants[i as usize],
    };
    for (i, tangent) in tangents
        .iter_mut()
        .enumerate()
        .take(BASS_CURVE_POINTS - 1)
        .skip(1)
    {
        let i = i as isize;
        let (a, b, c, d) = (slope(i - 2), slope(i - 1), slope(i), slope(i + 1));
        let left_weight = (d - c).abs() + 0.5 * (d + c).abs();
        let right_weight = (b - a).abs() + 0.5 * (b + a).abs();
        let total = left_weight + right_weight;
        *tangent = if total <= f32::EPSILON {
            0.5 * (b + c)
        } else {
            let weight = left_weight / total;
            weight * b + (1.0 - weight) * c
        };
    }

    // use modified Akima + quadratic curve for sparse low-freq FFT bins
    let step = (bass[31].x - bass[0].x) / (BASS_CURVE_SAMPLES - 1) as f32;
    let mut segment = 0;
    let samples: [Pos2; BASS_CURVE_SAMPLES + 1] = std::array::from_fn(|i| {
        if i == BASS_CURVE_SAMPLES {
            return bass[32];
        }
        let x = bass[0].x + i as f32 * step;
        while x > bass[segment + 1].x {
            segment += 1;
        }
        let from = bass[segment];
        let to = bass[segment + 1];
        let dx = to.x - from.x;
        let t = (x - from.x) / dx;
        let u = 1.0 - t;
        let y = (1.0 + 2.0 * t) * u * u * from.y
            + t * u * u * dx * tangents[segment]
            + t * t * (3.0 - 2.0 * t) * to.y
            - t * t * u * dx * tangents[segment + 1];
        Pos2::new(x, y)
    });

    let mut points = Vec::with_capacity(levels.len() + BASS_CURVE_SAMPLES);
    points.push(samples[0]);
    for i in (1..BASS_CURVE_SAMPLES).step_by(2) {
        let from = samples[i - 1];
        let control = samples[i];
        let to = samples[i + 1];
        // subdivide quadratic bezier for egui
        for t in [0.25, 0.5, 0.75, 1.0] {
            points.push(from.lerp(control, t).lerp(control.lerp(to, t), t));
        }
    }

    // simple decimation, drop points within 0.05px tolerance
    let mut anchor = bass[32];
    let mut previous = anchor;
    for next in std::iter::once(bass[33]).chain(bins) {
        let fraction = (previous.x - anchor.x) / (next.x - anchor.x);
        let line_y = (1.0 - fraction) * anchor.y + fraction * next.y;
        if (line_y - previous.y).abs() > 0.05 {
            points.push(previous);
            anchor = previous;
        }
        previous = next;
    }
    points.push(previous);
    points
}
