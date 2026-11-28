use super::*;
use crate::display_preferences::theme::Curves;

pub(super) const UNITY: Complex32 = Complex32::new(1.0, 0.0);
pub(super) const IDENTITY: [Complex32; 4] = [UNITY, Complex32::ZERO, Complex32::ZERO, UNITY];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Path {
    Mode(ChannelMode),
    SharedStereo(ChannelMode),
    LeftToRight,
    RightToLeft,
}

impl Path {
    fn transfer(self, [ll, lr, rl, rr]: [Complex32; 4]) -> Complex32 {
        match self {
            Self::Mode(ChannelMode::Stereo) => (ll + rr) * 0.5,
            Self::SharedStereo(channel) => Self::Mode(channel).transfer([ll, lr, rl, rr]),
            Self::Mode(ChannelMode::Left) => ll,
            Self::Mode(ChannelMode::Right) => rr,
            Self::Mode(ChannelMode::Mid) => (ll + lr + rl + rr) * 0.5,
            Self::Mode(ChannelMode::Side) => (ll - lr - rl + rr) * 0.5,
            Self::LeftToRight => rl,
            Self::RightToLeft => lr,
        }
    }

    fn label(self, mixed: bool) -> &'static str {
        match self {
            Self::Mode(ChannelMode::Stereo) => "Stereo avg",
            Self::SharedStereo(_) => "Stereo",
            Self::Mode(ChannelMode::Left) if mixed => "L to L",
            Self::Mode(ChannelMode::Right) if mixed => "R to R",
            Self::Mode(mode) => mode.label(),
            Self::LeftToRight => "L to R",
            Self::RightToLeft => "R to L",
        }
    }

    fn color(self, palette: Curves) -> Color32 {
        match self {
            Self::Mode(mode) => palette.channel(mode),
            Self::SharedStereo(_) => palette.response,
            Self::LeftToRight => palette.right,
            Self::RightToLeft => palette.left,
        }
    }

    fn is_crossfeed(self) -> bool {
        matches!(self, Self::LeftToRight | Self::RightToLeft)
    }
}

struct Trace {
    path: Path,
    magnitude: Vec<Pos2>,
    phase: Vec<(f32, Option<f32>)>,
    last_angle: Option<f32>,
}

pub(super) struct Traces {
    paths: Vec<Trace>,
    mixed: bool,
    split: bool,
    draw_phase: bool,
    real_phase: bool,
}

impl Traces {
    pub(super) fn new(modes: u8, capacity: usize, draw_phase: bool, real_phase: bool) -> Self {
        use ChannelMode::*;

        let split = modes & !Stereo.bit() != 0;
        let mixed =
            modes & (Left.bit() | Right.bit()) != 0 && modes & (Mid.bit() | Side.bit()) != 0;
        let unaffected_path = match modes & !Stereo.bit() {
            mask if mask == Left.bit() => Some(Right),
            mask if mask == Right.bit() => Some(Left),
            mask if mask == Mid.bit() => Some(Side),
            mask if mask == Side.bit() => Some(Mid),
            _ => None,
        };

        let paths = ChannelMode::ALL
            .into_iter()
            .filter(|mode| {
                modes & mode.bit() != 0
                    || (!split && *mode == Stereo)
                    || (mixed && matches!(mode, Left | Right))
            })
            .map(|mode| match (mode, unaffected_path) {
                (Stereo, Some(path)) => Path::SharedStereo(path),
                _ => Path::Mode(mode),
            })
            .chain(
                [Path::LeftToRight, Path::RightToLeft]
                    .into_iter()
                    .filter(|_| mixed),
            )
            .map(|path| Trace {
                path,
                magnitude: Vec::with_capacity(capacity),
                phase: Vec::with_capacity(if draw_phase { capacity } else { 0 }),
                last_angle: None,
            })
            .collect();

        Self {
            paths,
            mixed,
            split,
            draw_phase,
            real_phase,
        }
    }

    pub(super) fn push(&mut self, plot: &Plot, x: f32, matrix: [Complex32; 4]) {
        for trace in &mut self.paths {
            let response = trace.path.transfer(matrix);
            trace
                .magnitude
                .push(Pos2::new(x, plot.y_free(response_db(response))));

            if self.draw_phase && plot.rect.x_range().contains(x) {
                let angle = (response.norm_sqr() > 1e-16).then(|| {
                    if self.real_phase {
                        return if response.re < 0.0 { -180.0 } else { 0.0 };
                    }

                    let degrees = response.arg().to_degrees();
                    trace.last_angle.map_or(degrees, |last| {
                        last + (degrees - last + 180.0).rem_euclid(360.0) - 180.0
                    })
                });

                if self.real_phase
                    && trace.last_angle.zip(angle).is_some_and(|(a, b)| a != b)
                    && let Some(&(previous, _)) = trace.phase.last()
                {
                    trace.phase.push(((previous + x) * 0.5, None));
                }

                trace.phase.push((x, angle));
                trace.last_angle = angle;
            }
        }
    }

    pub(super) fn draw(
        self,
        p: &egui::Painter,
        plot: &Plot,
        phase_mode: Option<phase::Mode>,
        analytic: Option<Vec<(f32, Option<f32>)>>,
        hover: Option<Pos2>,
    ) -> Option<Pos2> {
        let palette = theme::current(p.ctx());
        let preview = hover.and_then(|pointer| {
            self.paths
                .iter()
                .filter(|trace| !trace.path.is_crossfeed())
                .filter_map(|trace| preview_point(&trace.magnitude, pointer, plot.rect))
                .min_by(|a, b| a.distance_sq(pointer).total_cmp(&b.distance_sq(pointer)))
        });

        if self.split {
            let mut x = plot.rect.left() + 10.0;
            let y = plot.rect.top() + BAND_READOUT_TOP + 22.0;
            let legend = p.with_clip_rect(plot.rect);
            let heading = if self.mixed {
                "EQ paths · L/R matrix"
            } else {
                "EQ paths"
            };

            x = legend
                .text(
                    Pos2::new(x, y),
                    egui::Align2::LEFT_CENTER,
                    heading,
                    FontId::proportional(10.0),
                    palette.text.muted,
                )
                .right()
                + 10.0;

            for trace in &self.paths {
                let color = trace.path.color(palette.curves);
                x = legend
                    .text(
                        Pos2::new(x, y),
                        egui::Align2::LEFT_CENTER,
                        trace.path.label(self.mixed),
                        FontId::proportional(10.0),
                        color,
                    )
                    .right()
                    + 10.0;
            }
        }

        let phases = self.paths.iter().map(|trace| {
            let color = if self.split {
                trace.path.color(palette.curves)
            } else {
                palette.curves.phase
            };
            (color, analytic.as_deref().unwrap_or(&trace.phase))
        });

        if let Some(mode) = phase_mode {
            phase::draw(p, plot, mode, phases);
        }

        for trace in self.paths {
            let ink = trace.path.color(palette.curves);

            if trace.path.is_crossfeed() {
                p.extend(egui::Shape::dashed_line(
                    &trace.magnitude,
                    Stroke::new(1.35, ink.gamma_multiply(0.7)),
                    3.0,
                    3.0,
                ));
            } else {
                p.add(egui::Shape::line(
                    trace.magnitude.clone(),
                    Stroke::new(5.0, ink.gamma_multiply(0.14)),
                ));
                p.add(egui::Shape::line(
                    trace.magnitude,
                    Stroke::new(1.9, ink.gamma_multiply(0.95)),
                ));
            }
        }

        preview
    }
}
