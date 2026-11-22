use super::*;

const HALF_TURN: f32 = 180.0;
const TURN: f32 = 2.0 * HALF_TURN;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Wrapped,
    Unwrapped,
}

impl Mode {
    pub(super) const LABELS: [&str; 2] = ["Wrapped Phase", "Unwrapped Phase"];

    fn label(self) -> &'static str {
        match self {
            Self::Wrapped => Self::LABELS[0],
            Self::Unwrapped => Self::LABELS[1],
        }
    }
}

struct Section {
    warp: f64,
    damping: f64,
    constant: f64,
    linear: f64,
    quadratic: f64,
}

impl From<&Coefficients> for Section {
    fn from(coeff: &Coefficients) -> Self {
        let [direct, band, low] = coeff.mix.map(f64::from);
        let damping = f64::from(coeff.k);
        Self {
            warp: f64::from(coeff.g),
            damping,
            constant: direct + low,
            linear: direct * damping + band,
            quadratic: direct,
        }
    }
}

impl Section {
    fn zero_ratio(&self) -> Option<f64> {
        if self.constant > 0.0 && self.quadratic > 0.0 {
            Some((self.constant / self.quadratic).sqrt())
        } else {
            None
        }
    }

    fn angle(&self, warped: f64) -> f64 {
        let ratio = warped / self.warp;
        let r2 = ratio * ratio;
        // Each polynomial stays in one complex half-plane for positive frequency.
        // Subtracting their angles preserves the winding count of the section.
        let numerator = (self.linear * ratio).atan2(self.constant - self.quadratic * r2);
        let denominator = (self.damping * ratio).atan2(1.0 - r2);
        numerator - denominator
    }
}

pub(super) struct Response {
    sections: Vec<Section>,
    scale: f64,
    nulls: Vec<f32>,
}

impl Response {
    pub(super) fn new<'a>(
        plot: &Plot,
        rate: f32,
        sections: impl IntoIterator<Item = &'a Coefficients>,
    ) -> Self {
        let sections: Vec<_> = sections.into_iter().map(Section::from).collect();
        let scale = std::f64::consts::PI / f64::from(rate);
        let mut nulls: Vec<_> = sections
            .iter()
            .filter(|section| section.linear == 0.0)
            .filter_map(|section| {
                let ratio = section.zero_ratio()?;
                let hz = (section.warp * ratio).atan() / scale;
                let x = plot.x_unchecked(hz as f32);
                plot.rect.x_range().contains(x).then_some(x)
            })
            .collect();
        nulls.sort_unstable_by(f32::total_cmp);
        nulls.dedup();
        Self {
            sections,
            scale,
            nulls,
        }
    }

    pub(super) fn nulls(&self) -> &[f32] {
        &self.nulls
    }

    pub(super) fn features(&self) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.sections.iter().flat_map(|section| {
            let pole = (section.warp as f32, (section.damping * 0.5) as f32);
            let zero = section.zero_ratio().map(|ratio| {
                let width = (section.linear / (section.quadratic * ratio)).abs() * 0.5;
                ((section.warp * ratio) as f32, width as f32)
            });
            std::iter::once(pole).chain(zero)
        })
    }

    fn degrees_at(&self, hz: f32) -> f32 {
        let warped = (f64::from(hz) * self.scale).tan();
        self.sections
            .iter()
            .map(|section| section.angle(warped))
            .sum::<f64>()
            .to_degrees() as f32
    }

    pub(super) fn sample(&self, plot: &Plot, xs: &[f32]) -> Vec<(f32, Option<f32>)> {
        xs.iter()
            .copied()
            .filter(|&x| plot.rect.x_range().contains(x))
            .map(|x| {
                let phase = self
                    .nulls
                    .binary_search_by(|null| null.total_cmp(&x))
                    .is_err()
                    .then(|| self.degrees_at(plot.hz(x)));
                (x, phase)
            })
            .collect()
    }
}

struct Axis {
    min: f32,
    max: f32,
    step: f32,
}

impl Axis {
    fn new(mode: Mode, samples: &[(f32, Option<f32>)]) -> Self {
        if mode == Mode::Wrapped {
            return Self {
                min: -HALF_TURN,
                max: HALF_TURN,
                step: 90.0,
            };
        }
        let (mut min, mut max) = samples
            .iter()
            .filter_map(|(_, phase)| *phase)
            .fold((0.0_f32, 0.0_f32), |(low, high), phase| {
                (low.min(phase), high.max(phase))
            });
        if max - min < HALF_TURN {
            min = min.min(-90.0);
            max = max.max(90.0);
        }
        let mut step = 45.0_f32;
        while (max / step).ceil() - (min / step).floor() > 5.0 {
            step *= 2.0;
        }
        Self {
            min: (min / step).floor() * step,
            max: (max / step).ceil() * step,
            step,
        }
    }

    fn y(&self, rect: Rect, degrees: f32) -> f32 {
        egui::lerp(
            rect.bottom()..=rect.top(),
            (degrees - self.min) / (self.max - self.min),
        )
    }
}

fn paths(mode: Mode, axis: &Axis, rect: Rect, samples: &[(f32, Option<f32>)]) -> Vec<Vec<Pos2>> {
    let mut paths = Vec::new();
    let mut points = Vec::new();
    let mut previous: Option<(f32, f32, i32)> = None;
    for &(x, phase) in samples {
        let Some(phase) = phase else {
            if points.len() > 1 {
                paths.push(std::mem::take(&mut points));
            }
            points.clear();
            previous = None;
            continue;
        };
        let turns = if mode == Mode::Wrapped {
            ((phase + HALF_TURN) / TURN).floor() as i32
        } else {
            0
        };
        if let Some((last_x, last_phase, mut last_turn)) = previous {
            let direction = (turns - last_turn).signum();
            let edge = HALF_TURN * direction as f32;
            while last_turn != turns {
                let seam = TURN * last_turn as f32 + edge;
                let t = (seam - last_phase) / (phase - last_phase);
                let crossing = egui::lerp(last_x..=x, t);
                points.push(Pos2::new(crossing, axis.y(rect, edge)));
                paths.push(std::mem::take(&mut points));
                points.push(Pos2::new(crossing, axis.y(rect, -edge)));
                last_turn += direction;
            }
        }
        points.push(Pos2::new(x, axis.y(rect, phase - TURN * turns as f32)));
        previous = Some((x, phase, turns));
    }
    if points.len() > 1 {
        paths.push(points);
    }
    paths
}

pub(super) fn draw(p: &egui::Painter, plot: &Plot, mode: Mode, samples: &[(f32, Option<f32>)]) {
    let phase_color = theme::current(p.ctx()).curves.phase;
    let axis = Axis::new(mode, samples);
    let curve = p.with_clip_rect(plot.rect);
    for path in paths(mode, &axis, plot.rect, samples) {
        curve.extend(egui::Shape::dashed_line(
            &path,
            Stroke::new(1.35, phase_color),
            6.0,
            3.5,
        ));
    }

    let x = plot.rect.right();
    let count = ((axis.max - axis.min) / axis.step).round() as usize;
    for i in 0..=count {
        let degrees = axis.min + i as f32 * axis.step;
        let label = match degrees {
            0.0 => "0°".into(),
            angle if angle < 0.0 => format!("−{:.0}°", -angle),
            angle => format!("+{angle:.0}°"),
        };
        let y = axis.y(plot.rect, degrees);
        let ink = phase_color.gamma_multiply(0.85);
        p.line_segment(
            [Pos2::new(x - 5.0, y), Pos2::new(x, y)],
            Stroke::new(1.0, ink),
        );
        p.text(
            Pos2::new(x - 8.0, y),
            egui::Align2::RIGHT_CENTER,
            label,
            FontId::monospace(9.5),
            ink,
        );
    }
}

pub(super) fn draw_label(p: &egui::Painter, plot: &Plot, outer: Rect, mode: Mode) {
    p.text(
        Pos2::new(plot.rect.center().x, outer.top() + Layout::HEADER + 8.0),
        egui::Align2::CENTER_CENTER,
        mode.label(),
        FontId::proportional(10.0),
        theme::current(p.ctx()).curves.phase,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_preserves_winding_without_relying_on_sample_density() {
        let outer = Rect::from_min_size(Pos2::ZERO, Vec2::new(720.0, 400.0));
        let plot = Plot::new(outer, 48_000.0);
        let lowpass = Coefficients::for_kind(48_000.0, 1_000.0, 16.0, 0.0, FilterKind::Lowpass);
        let response = Response::new(&plot, 48_000.0, [&lowpass; 32]);
        assert!((response.degrees_at(1_000.0) + 2_880.0).abs() < 0.1);

        for kind in [
            FilterKind::Highpass,
            FilterKind::Peak,
            FilterKind::LowShelf,
            FilterKind::Notch,
        ] {
            let section = Coefficients::for_kind(48_000.0, 1_000.0, 2.0, -12.0, kind);
            let response = Response::new(&plot, 48_000.0, [&section; 3]);
            for hz in [250.0, 950.0, 1_050.0, 10_000.0] {
                let expected = section.response(48_000.0, hz).powu(3).arg().to_degrees();
                let error =
                    (response.degrees_at(hz) - expected + HALF_TURN).rem_euclid(TURN) - HALF_TURN;
                assert!(error.abs() < 0.01, "{kind:?} at {hz} Hz: {error}°");
            }
        }

        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0));
        for (before, after, edge) in [(170.0, 190.0, rect.top()), (-170.0, -190.0, rect.bottom())] {
            let samples = [(20.0, Some(before)), (80.0, Some(after))];
            let axis = Axis::new(Mode::Wrapped, &samples);
            let segments = paths(Mode::Wrapped, &axis, rect, &samples);
            assert_eq!(segments.len(), 2);
            assert_eq!(segments[0][1], Pos2::new(50.0, edge));
            assert_eq!(segments[1][0], Pos2::new(50.0, rect.bottom() - edge));
        }

        let samples = [(0.0, Some(0.0)), (100.0, Some(-1_080.0))];
        let wrapped = Axis::new(Mode::Wrapped, &samples);
        assert_eq!(paths(Mode::Wrapped, &wrapped, rect, &samples).len(), 4);
        let unwrapped = Axis::new(Mode::Unwrapped, &samples);
        assert!(unwrapped.min <= -1_080.0);
        assert_eq!(paths(Mode::Unwrapped, &unwrapped, rect, &samples).len(), 1);

        let notch = Coefficients::for_kind(48_000.0, 1_000.0, 1.0, 0.0, FilterKind::Notch);
        let response = Response::new(&plot, 48_000.0, [&notch]);
        let samples = response.sample(&plot, response.nulls());
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].1, None);
    }
}
