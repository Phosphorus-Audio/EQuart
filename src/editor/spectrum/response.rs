use super::*;

mod routes;
#[cfg(test)]
mod tests;

use routes::{IDENTITY, Traces};

/// get complex response of one band at a freq
pub(super) fn band_response(
    sections: &[Coefficients<f64>; SECTIONS],
    count: usize,
    warped: f64,
) -> Complex32 {
    let mut response = realfft::num_complex::Complex64::new(1.0, 0.0);

    for section in sections.iter().take(count) {
        response *= section.response_warped(warped);
    }

    Complex32::new(response.re as f32, response.im as f32)
}

type ResponseSections = ([Coefficients<f64>; SECTIONS], usize);

struct BandResponse {
    base: ResponseSections,
    live: Option<ResponseSections>,
    range: Option<[ResponseSections; 2]>,
}

impl BandResponse {
    fn new(control: BandControl, offset: f32, rate: f32) -> Self {
        let base = control.sections_for(rate);
        let with_offset = |delta| {
            BandControl {
                gain: control.gain + delta,
                ..control
            }
            .sections_for(rate)
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
    pub(super) base: Option<BandControl>,
    pub(super) offsets: &'a [f32; BAND_COUNT],
    pub(super) regions: &'a [RegionControl],
    pub(super) telemetry: &'a crate::Telemetry,
    pub(super) rate: f32,
    pub(super) oversampling: crate::oversample::Oversampling,
    pub(super) selected: Option<usize>,
    pub(super) phase_mode: Option<phase::Mode>,
    pub(super) linear_center: Option<usize>,
    pub(super) fir_response: &'a crate::telemetry::FirResponse,
    pub(super) preview_hover: Option<Pos2>,
}

fn preview_point(curve: &[Pos2], pointer: Pos2, plot: Rect) -> Option<Pos2> {
    const HOVER_RADIUS: f32 = 12.0;

    curve
        .windows(2)
        .filter(|segment| {
            segment[0].x <= pointer.x + HOVER_RADIUS && segment[1].x >= pointer.x - HOVER_RADIUS
        })
        .filter_map(|segment| {
            let start = segment[0];
            let delta = segment[1] - start;
            let t = ((pointer - start).dot(delta) / delta.length_sq()).clamp(0.0, 1.0);
            let point = start + delta * t;
            let distance = point.distance_sq(pointer);

            (distance <= HOVER_RADIUS * HOVER_RADIUS && plot.contains(point))
                .then_some((point, distance))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(point, _)| point)
}

/// draw each band's curve and the total sum line
pub(super) fn draw_response(
    p: &egui::Painter,
    plot: &Plot,
    outer: Rect,
    db: &mut [Vec<f32>; BAND_COUNT],
    input: ResponseInput<'_>,
) -> Option<Pos2> {
    const POINTS: usize = 512;
    let palette = theme::current(p.ctx());
    let ResponseInput {
        controls,
        base,
        offsets,
        regions,
        telemetry,
        rate,
        oversampling,
        selected,
        phase_mode,
        linear_center,
        fir_response,
        preview_hover,
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
            region.filter_plan(rate, telemetry.harmonic_hz(i)).chain(
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
        .map(|band| (band.channel, band.sections_for(rate)))
        .collect();
    let modes = if let Some(center) = linear_center {
        fir_response.channel_modes(rate, center).unwrap_or(0)
    } else {
        controls.iter().chain(&region_bands).filter(|band| band.enabled)
            .fold(0, |mask, band| mask | band.channel.bit())
    };
    let split = modes & !ChannelMode::Stereo.bit() != 0;

    let phase_response = phase_mode.filter(|_| linear_center.is_none()).map(|_| {
        phase::Response::new(
            plot,
            rate,
            responses
                .iter()
                .zip(controls)
                .filter(|(_, control)| control.enabled)
                .map(|(response, _)| response.live.as_ref().unwrap_or(&response.base))
                .chain(region_sections.iter().map(|(_, sections)| sections))
                .flat_map(|(coefficients, count)| &coefficients[..*count]),
        )
    });
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
        if !control.enabled {
            continue;
        }
        if control.brickwall() {
            let center = control.frequency.clamp(10.0, rate * 0.49);
            refine((center * scale).tan(), 0.04);
        }
        if control.gain == 0.0 && band.live.is_none() && band.range.is_none() {
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
                    refine(section.g as f32, section.k as f32 * 0.5);
                    refine(zero as f32, section.k as f32 * 0.5);
                }
            }
        }
    }
    if let Some(response) = &phase_response {
        for (warp, half_width) in response.features() {
            refine(warp, half_width);
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
    if let Some(response) = &phase_response {
        xs.extend([plot.rect.left(), plot.rect.right()]);
        xs.extend(response.nulls());
    }
    xs.sort_unstable_by(f32::total_cmp);
    xs.dedup();

    for (values, control) in db.iter_mut().zip(controls) {
        values.clear();
        if control.enabled {
            values.reserve(xs.len());
        }
    }
    let include_resampler = oversampling.factor() > 1
        && (split || controls.iter().any(|control| control.enabled && control.brickwall()));
    let host_rate = rate / oversampling.factor() as f32;
    let warped_at = |hz| (std::f64::consts::PI * f64::from(hz) / f64::from(rate)).tan();
    let cascade = |mut matrix: [Complex32; 4], channel, response| {
        if split {
            crate::routing::cascade(matrix, channel, response)
        } else {
            matrix[0] *= response;
            matrix[3] = matrix[0];
            matrix
        }
    };
    let response_at = |hz: f32| {
        let mut matrix = IDENTITY;
        let warped = warped_at(hz);

        for (control, response) in controls.iter().zip(&responses) {
            if control.enabled {
                let (coefficients, count) = response.live.as_ref().unwrap_or(&response.base);
                matrix = cascade(matrix, control.channel, band_response(coefficients, *count, warped));
            }
        }
        for (channel, (sections, count)) in &region_sections {
            matrix = cascade(matrix, *channel, band_response(sections, *count, warped));
        }

        matrix
    };
    let mut traces = Traces::new(
        modes,
        xs.len(),
        phase_mode.is_some() && (split || linear_center.is_some()),
        linear_center.is_some() && fir_response.is_symmetric(),
    );
    let mut natural_phase = (include_resampler && !split && phase_mode.is_some())
        .then(|| Vec::with_capacity(xs.len()));

    for &x in &xs {
        let hz = plot.hz_extended(x).min(host_rate * 0.5);
        let warped = warped_at(hz.min(rate * 0.49999));

        let mut matrix = IDENTITY;
        for (band, response) in responses.iter().enumerate() {
            if !controls[band].enabled {
                continue;
            }

            let (coefficients, count) = &response.base;
            let base = band_response(coefficients, *count, warped);
            db[band].push(response_db(base));

            if linear_center.is_none() {
                let live = response.live.as_ref().map_or(base, |(coefficients, count)| {
                    band_response(coefficients, *count, warped)
                });
                matrix = cascade(matrix, controls[band].channel, live);
            }
        }

        if let Some(center) = linear_center {
            if let Some(matrix) = fir_response.matrix(hz, rate, center) {
                traces.push(plot, x, matrix);
            }
            continue;
        }

        for (channel, (sections, count)) in &region_sections {
            matrix = cascade(matrix, *channel, band_response(sections, *count, warped));
        }

        if include_resampler {
            let weights = crate::oversample::response_images(
                oversampling, f64::from(hz), f64::from(host_rate),
            );
            matrix = matrix.map(|value| value * weights[0] as f32);

            for (image, weight) in weights.into_iter().enumerate().skip(1) {
                if weight <= 1e-12 {
                    continue;
                }

                let image_hz = hz + image as f32 * host_rate;
                let signed = if image_hz > rate * 0.5 {
                    image_hz - rate
                } else {
                    image_hz
                };
                let image = response_at(signed.abs());
                for (value, image) in matrix.iter_mut().zip(image) {
                    *value += if signed < 0.0 { image.conj() } else { image } * weight as f32;
                }
            }

            if let Some(phase) = &mut natural_phase {
                phase.push(matrix[0].arg().to_degrees());
            }
        }

        traces.push(plot, x, matrix);
    }

    let painter = p.with_clip_rect(outer);
    let baseline = plot.y(0.0);

    if let (Some(index), Some(base)) = (selected, base)
        && base.enabled && (base.mapping.enabled || base.lfo.enabled)
        && !(linear_center.is_some() && base.brickwall())
    {
        let ink = palette.band[index];
        let draw_curve = |band: BandControl, opacity: f32| {
            let (sections, count) = band.sections_for(rate);
            let points: Vec<_> = xs.iter().map(|&x| {
                let hz = plot.hz_extended(x);
                let db = response_db(band_response(&sections, count, warped_at(hz)));
                Pos2::new(x, plot.y_free(db))
            }).collect();
            painter.extend(egui::Shape::dashed_line(
                &points, Stroke::new(1.0, ink.gamma_multiply(opacity)), 4.0, 3.0,
            ));
        };
        draw_curve(base, 0.55);

        let lfo = base.lfo;
        if linear_center.is_none() && lfo.enabled && !lfo.bypass && lfo.depth > 0.0
            && lfo.target.supported(base)
        {
            use crate::modulation::Target;
            let center = telemetry.modulation(index).map_or_else(|| lfo.target.value(base), |live| live.lfo_base);
            let endpoints = [if lfo.unipolar { 0.0 } else { -lfo.depth }, lfo.depth]
                .map(|depth| lfo.target.offset(center, depth, rate));

            match lfo.target {
                Target::Frequency => {
                    painter.hline(
                        plot.x(endpoints[0])..=plot.x(endpoints[1]),
                        plot.y(controls[index].gain) - 12.0,
                        Stroke::new(2.0, ink.gamma_multiply(0.45)),
                    );
                }
                Target::Gain | Target::Q => {
                    for endpoint in endpoints {
                        let mut band = controls[index];
                        if lfo.target == Target::Gain {
                            band.gain = endpoint;
                        } else {
                            band.q = endpoint;
                        }

                        draw_curve(band, 0.24);
                    }
                }
            }
        }
    }

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
                response_db(band_response(coefficients, *count, warped_at(hz)))
            });
            let base = db[band][i];
            top.push(Pos2::new(x, plot.y_free(a.max(b).max(base))));
            bottom.push(Pos2::new(x, plot.y_free(a.min(b).min(base))));
        }
        let color = palette.band[band];
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
        if linear_center.is_some() && controls[band].brickwall() {
            continue;
        }

        if linear_center.is_some() && selected == Some(band)
            && base.is_some_and(|base| base.mapping.enabled || base.lfo.enabled)
        {
            continue;
        }

        if !controls[band].enabled || db[band].iter().all(|value| value.abs() < FLAT_DB) {
            continue;
        }

        let points: Vec<Pos2> = xs
            .iter()
            .zip(&db[band])
            .map(|(&x, &value)| Pos2::new(x, plot.y_free(value)))
            .collect();
        let selected = selected == Some(band);
        let color = palette.band[band];
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
            || (linear_center.is_some() && control.brickwall())
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
        let at_cutoff = band_response(band_sections, *count, warped_at(control.frequency));
        let foot = plot
            .y_free(20.0 * at_cutoff.norm().max(1e-9).log10())
            .clamp(plot.rect.top(), plot.rect.bottom());
        // start from edge of the 8px dot
        let distance = foot - handle_y;
        if distance > 9.0 || (tilt && distance < -9.0) {
            let start = handle_y + 8.0 * distance.signum();
            painter.add(egui::Shape::line(
                vec![Pos2::new(x, start), Pos2::new(x, foot)],
                Stroke::new(1.2, palette.band[band].gamma_multiply(0.8)),
            ));
        }
    }

    let analytic = phase_response.filter(|_| !split).map(|response| {
        let mut samples = response.sample(plot, &xs);
        if let Some(angles) = natural_phase {
            for (x, phase) in &mut samples {
                if let Some(degrees) = phase
                    && let Ok(index) = xs.binary_search_by(|px| px.total_cmp(x))
                {
                    let offset = (angles[index] - *degrees + 180.0).rem_euclid(360.0) - 180.0;
                    *degrees += offset;
                }
            }
        }
        samples
    });

    traces.draw(&painter, plot, phase_mode, analytic, preview_hover)
}
