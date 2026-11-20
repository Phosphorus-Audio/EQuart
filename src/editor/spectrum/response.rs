use super::*;

/// get complex response of one band at a freq
pub(super) fn band_response(
    sections: &[Coefficients; SECTIONS],
    count: usize,
    rate: f32,
    hz: f32,
) -> Complex32 {
    let mut response = Complex32::new(1.0, 0.0);
    for section in sections.iter().take(count) {
        response *= section.response(rate, hz);
    }
    response
}

type ResponseSections = ([Coefficients; SECTIONS], usize);

struct BandResponse {
    base: ResponseSections,
    live: Option<ResponseSections>,
    range: Option<[ResponseSections; 2]>,
}

impl BandResponse {
    fn new(control: BandControl, offset: f32, rate: f32) -> Self {
        let base = control.sections(rate);
        let with_offset = |delta| {
            BandControl {
                gain: control.gain + delta,
                ..control
            }
            .sections(rate)
        };
        let mut live = None;
        let mut range = None;
        if control.enabled && control.kind.has_gain() {
            live = (offset != 0.0).then(|| with_offset(offset));
            if control.dynamics.enabled {
                // include 0 so same-sign ranges still start from base
                let (low, high) = [control.dynamics.above, control.dynamics.below]
                    .into_iter()
                    .filter(|rule| rule.enabled)
                    .fold((0.0_f32, 0.0_f32), |(low, high), rule| {
                        (low.min(rule.range), high.max(rule.range))
                    });
                if low != high {
                    range = Some([low, high].map(|delta| {
                        if delta == 0.0 {
                            base
                        } else {
                            with_offset(delta)
                        }
                    }));
                }
            }
        }
        Self { base, live, range }
    }
}

fn response_db(response: Complex32) -> f32 {
    20.0 * response.norm().max(1e-9).log10()
}

pub(super) struct ResponseInput<'a> {
    pub(super) controls: &'a [BandControl; BAND_COUNT],
    pub(super) offsets: &'a [f32; BAND_COUNT],
    pub(super) regions: &'a [RegionControl],
    pub(super) harmonic_hz: Option<f32>,
    pub(super) telemetry: &'a crate::Telemetry,
    pub(super) rate: f32,
    pub(super) selected: Option<usize>,
}

/// draw each band's curve and the total sum line
pub(super) fn draw_response(
    p: &egui::Painter,
    plot: &Plot,
    outer: Rect,
    db: &mut [Vec<f32>; BAND_COUNT],
    input: ResponseInput<'_>,
) {
    const POINTS: usize = 512;
    let ResponseInput {
        controls,
        offsets,
        regions,
        harmonic_hz,
        telemetry,
        rate,
        selected,
    } = input;

    let responses: Vec<_> = controls
        .iter()
        .zip(offsets)
        .map(|(&control, &offset)| BandResponse::new(control, offset, rate))
        .collect();
    let region_bands: Vec<_> = regions
        .iter()
        .enumerate()
        .flat_map(|(i, &region)| {
            region.filter_plan(rate, harmonic_hz).chain(
                telemetry
                    .resonance_reductions(i)
                    .into_iter()
                    .map(move |cut| cut.band(region)),
            )
        })
        .filter(|band| band.enabled)
        .collect();
    let region_sections: Vec<_> = region_bands
        .iter()
        .map(|band| band.sections(rate))
        .collect();
    // sample along screen pixels so curves always span full width
    let step = outer.width() / (POINTS - 1) as f32;
    let mut xs: Vec<_> = (0..POINTS)
        .map(|i| outer.left() + i as f32 * step)
        .collect();
    let scale = std::f32::consts::PI / rate;
    let mut refine = |warped: f32, half_width: f32| {
        let at = |offset: f32| {
            let ratio = (half_width * offset).asinh().exp();
            plot.x_unchecked((warped * ratio).atan() / scale)
        };
        // add extra points around narrow peaks
        if at(1.0) - at(-1.0) < step * 16.0 {
            xs.extend(
                (-8..=8)
                    .map(|i| at(i as f32 * 0.25))
                    .filter(|&x| (outer.left()..=outer.right()).contains(&x)),
            );
        }
    };
    for control in &region_bands {
        refine(
            (control.frequency * scale).tan(),
            bell_bandwidth(control.q) * 0.5,
        );
    }
    for (control, band) in controls.iter().zip(&responses) {
        if !control.enabled || (control.gain == 0.0 && band.live.is_none() && band.range.is_none())
        {
            continue;
        }
        if control.kind == FilterKind::Peak && control.slope != CutSlope::Db12 {
            let center = control.frequency.clamp(10.0, rate * 0.49);
            refine((center * scale).tan(), bell_bandwidth(control.q) * 0.5);
        } else if control.kind.is_shelf() {
            // refine near shelf's pole and zero
            for (coefficients, count) in std::iter::once(&band.base)
                .chain(band.live.iter())
                .chain(band.range.iter().flatten())
            {
                if let Some(section) = coefficients[..*count]
                    .iter()
                    .min_by(|a, b| a.k.total_cmp(&b.k))
                {
                    let [high, _, low_delta] = section.mix;
                    let zero = section.g * ((high + low_delta) / high).sqrt();
                    refine(section.g, section.k * 0.5);
                    refine(zero, section.k * 0.5);
                }
            }
        }
    }
    xs.extend(
        controls
            .iter()
            .zip(&responses)
            .filter_map(|(control, band)| {
                let x = plot.x(control.frequency.clamp(MIN_HZ, rate * 0.49));
                (band.range.is_some() && (outer.left()..=outer.right()).contains(&x)).then_some(x)
            }),
    );
    for region in regions
        .iter()
        .filter(|region| region.enabled && !region.bypass)
    {
        let [low, high] = region.bounds().map(|hz| hz.min(rate * 0.49));
        let [left, right] = [low, high].map(|hz| plot.x_unchecked(hz));
        xs.extend(
            (-4..=12)
                .map(|i| left + (right - left) * (i as f32 / 8.0))
                .filter(|x| outer.x_range().contains(*x)),
        );
    }
    xs.sort_unstable_by(f32::total_cmp);
    xs.dedup();

    for (values, control) in db.iter_mut().zip(controls) {
        values.clear();
        if control.enabled {
            values.reserve(xs.len());
        }
    }
    let mut sum = Vec::with_capacity(xs.len());
    for &x in &xs {
        let hz = plot.hz_extended(x);

        let mut total = Complex32::new(1.0, 0.0);
        for (band, response) in responses.iter().enumerate() {
            if controls[band].enabled {
                let (coefficients, count) = &response.base;
                let base = band_response(coefficients, *count, rate, hz);
                db[band].push(response_db(base));
                total *= response
                    .live
                    .as_ref()
                    .map_or(base, |(coefficients, count)| {
                        band_response(coefficients, *count, rate, hz)
                    });
            }
        }
        for (sections, count) in &region_sections {
            total *= band_response(sections, *count, rate, hz);
        }

        sum.push(response_db(total));
    }

    let painter = p.with_clip_rect(outer);
    let baseline = plot.y(0.0);

    // skip flat bands so we don't draw useless lines on 0 dB
    const FLAT_DB: f32 = 0.5;

    for (band, response) in responses.iter().enumerate() {
        let Some(bounds) = &response.range else {
            continue;
        };
        let (mut top, mut bottom) = (Vec::with_capacity(xs.len()), Vec::with_capacity(xs.len()));
        for (i, &x) in xs.iter().enumerate() {
            let hz = plot.hz_extended(x);
            let [a, b] = bounds.each_ref().map(|(coefficients, count)| {
                response_db(band_response(coefficients, *count, rate, hz))
            });
            let base = db[band][i];
            top.push(Pos2::new(x, plot.y_free(a.max(b).max(base))));
            bottom.push(Pos2::new(x, plot.y_free(a.min(b).min(base))));
        }
        let color = POINT_COLORS[band];
        let opacity = if selected == Some(band) { 0.23 } else { 0.14 };
        gradient_fill(
            &painter,
            &top,
            |i, y| bottom[i].y - y,
            &[0.0, 1.0],
            |_, _| color.gamma_multiply(opacity),
        );
        for edge in [top, bottom] {
            painter.add(egui::Shape::line(
                edge,
                Stroke::new(1.0, color.gamma_multiply(0.57)),
            ));
        }
    }

    for band in 0..BAND_COUNT {
        if !controls[band].enabled || db[band].iter().all(|value| value.abs() < FLAT_DB) {
            continue;
        }

        let points: Vec<Pos2> = xs
            .iter()
            .zip(&db[band])
            .map(|(&x, &value)| Pos2::new(x, plot.y_free(value)))
            .collect();
        let selected = selected == Some(band);
        let color = POINT_COLORS[band];
        let strength = if selected { 0.22 } else { 0.14 };

        if responses[band].range.is_none() {
            gradient_fill(
                &painter,
                &points,
                |_, y| baseline - y,
                &[0.0, 0.5, 1.0],
                |_, s| color.gamma_multiply([strength, strength * 0.4, 0.0][s]),
            );
        }
        painter.add(egui::Shape::line(
            points,
            Stroke::new(
                if selected { 1.9 } else { 1.2 },
                color.gamma_multiply(if selected { 1.0 } else { 0.85 }),
            ),
        ));
    }

    // draw stem from cut/tilt handle to the curve
    for (band, control) in controls.iter().enumerate() {
        let tilt = matches!(control.kind, FilterKind::TiltShelf | FilterKind::FlatTilt);
        if !control.enabled
            || !(control.kind.is_cut() || tilt)
            || db[band].iter().all(|value| value.abs() < FLAT_DB)
        {
            continue;
        }
        let x = plot.x(control.frequency);
        let handle_y = if tilt {
            plot.y(control.gain)
        } else {
            cut_handle_y(plot, cut_handle_q(control))
        };
        let (band_sections, count) = &responses[band].base;
        let at_cutoff = band_response(band_sections, *count, rate, control.frequency);
        let foot = plot
            .y_free(20.0 * at_cutoff.norm().max(1e-9).log10())
            .clamp(plot.rect.top(), plot.rect.bottom());
        // start from edge of the 8px dot
        let distance = foot - handle_y;
        if distance > 9.0 || (tilt && distance < -9.0) {
            let start = handle_y + 8.0 * distance.signum();
            painter.add(egui::Shape::line(
                vec![Pos2::new(x, start), Pos2::new(x, foot)],
                Stroke::new(1.2, POINT_COLORS[band].gamma_multiply(0.8)),
            ));
        }
    }

    // draw total combined curve
    let combined: Vec<Pos2> = xs
        .iter()
        .zip(sum)
        .map(|(&x, db)| Pos2::new(x, plot.y_free(db)))
        .collect();
    painter.add(egui::Shape::line(
        combined.clone(),
        Stroke::new(5.0, COMBINED.gamma_multiply(0.14)),
    ));
    painter.add(egui::Shape::line(
        combined,
        Stroke::new(1.9, COMBINED.gamma_multiply(0.95)),
    ));
}
