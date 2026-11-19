use nice_plug::prelude::*;
use realfft::num_complex::Complex32;
use std::f32::consts::PI;

pub const FLAT_TILT_SECTIONS: usize = 8;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    #[default]
    #[id = "bell"]
    #[name = "Bell"]
    Peak,
    #[id = "low_shelf"]
    #[name = "Low Shelf"]
    LowShelf,
    #[id = "high_shelf"]
    #[name = "High Shelf"]
    HighShelf,
    #[id = "low_cut"]
    #[name = "Low Cut"]
    Highpass,
    #[id = "high_cut"]
    #[name = "High Cut"]
    Lowpass,
    #[id = "notch"]
    #[name = "Notch"]
    Notch,
    #[id = "tilt_shelf"]
    #[name = "Tilt Shelf"]
    TiltShelf,
    #[id = "flat_tilt"]
    #[name = "Flat Tilt"]
    FlatTilt,
    #[id = "band_pass"]
    #[name = "Band Pass"]
    BandPass,
}

impl FilterKind {
    pub const ALL: [Self; 9] = [
        Self::Peak,
        Self::LowShelf,
        Self::HighShelf,
        Self::Highpass,
        Self::Lowpass,
        Self::Notch,
        Self::TiltShelf,
        Self::FlatTilt,
        Self::BandPass,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Peak => "Bell",
            Self::LowShelf => "Low Shelf",
            Self::HighShelf => "High Shelf",
            Self::Highpass => "Low Cut",
            Self::Lowpass => "High Cut",
            Self::Notch => "Notch",
            Self::TiltShelf => "Tilt Shelf",
            Self::FlatTilt => "Flat Tilt",
            Self::BandPass => "Band Pass",
        }
    }

    /// Short label used on the type chips.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Peak => "BELL",
            Self::LowShelf => "LO SH",
            Self::HighShelf => "HI SH",
            Self::Highpass => "LO CUT",
            Self::Lowpass => "HI CUT",
            Self::Notch => "NOTCH",
            Self::TiltShelf => "TILT SH",
            Self::FlatTilt => "FLAT TILT",
            Self::BandPass => "BAND PASS",
        }
    }

    pub fn has_gain(self) -> bool {
        self == Self::Peak || self.is_shelf() || self == Self::FlatTilt
    }

    pub fn is_shelf(self) -> bool {
        matches!(self, Self::LowShelf | Self::HighShelf | Self::TiltShelf)
    }

    pub fn is_cut(self) -> bool {
        matches!(self, Self::Highpass | Self::Lowpass)
    }

    pub fn slopes(self) -> &'static [CutSlope] {
        match self {
            Self::Peak | Self::Notch | Self::BandPass => &CutSlope::EVEN_ORDER,
            Self::FlatTilt => &[],
            _ => &CutSlope::ALL,
        }
    }

    pub fn effective_slope(self, slope: CutSlope) -> CutSlope {
        match self {
            Self::Peak | Self::Notch | Self::BandPass => slope.as_even_order(),
            _ => slope,
        }
    }

    pub fn uses_q(self, slope: CutSlope) -> bool {
        self != Self::FlatTilt && (slope != CutSlope::Db6 || !(self.is_cut() || self.is_shelf()))
    }
}

/// filter slope in dB/oct (6 dB/oct per pole for cuts/shelves, 12 for bells/notches)
#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CutSlope {
    #[id = "6"]
    #[name = "6 dB/oct"]
    Db6,
    #[default]
    #[id = "12"]
    #[name = "12 dB/oct"]
    Db12,
    #[id = "18"]
    #[name = "18 dB/oct"]
    Db18,
    #[id = "24"]
    #[name = "24 dB/oct"]
    Db24,
    #[id = "30"]
    #[name = "30 dB/oct"]
    Db30,
    #[id = "36"]
    #[name = "36 dB/oct"]
    Db36,
    #[id = "48"]
    #[name = "48 dB/oct"]
    Db48,
    #[id = "72"]
    #[name = "72 dB/oct"]
    Db72,
    #[id = "96"]
    #[name = "96 dB/oct"]
    Db96,
}

impl CutSlope {
    pub const ALL: [Self; 9] = [
        Self::Db6,
        Self::Db12,
        Self::Db18,
        Self::Db24,
        Self::Db30,
        Self::Db36,
        Self::Db48,
        Self::Db72,
        Self::Db96,
    ];

    /// Slopes built from whole biquads for bells, notches and band-pass filters.
    pub const EVEN_ORDER: [Self; 6] = [
        Self::Db12,
        Self::Db24,
        Self::Db36,
        Self::Db48,
        Self::Db72,
        Self::Db96,
    ];

    /// One biquad per 12 dB/oct, with at least one for a bell or notch.
    pub fn biquad_count(self) -> usize {
        (self.order() / 2).max(1)
    }

    /// Map the odd-order steps to the whole-biquad slope list.
    pub fn as_even_order(self) -> Self {
        match self.biquad_count() {
            1 => Self::Db12,
            2 => Self::Db24,
            3 => Self::Db36,
            4 => Self::Db48,
            6 => Self::Db72,
            _ => Self::Db96,
        }
    }

    pub fn order(self) -> usize {
        match self {
            Self::Db6 => 1,
            Self::Db12 => 2,
            Self::Db18 => 3,
            Self::Db24 => 4,
            Self::Db30 => 5,
            Self::Db36 => 6,
            Self::Db48 => 8,
            Self::Db72 => 12,
            Self::Db96 => 16,
        }
    }

    pub fn label(self) -> String {
        format!("{} dB/oct", self.order() * 6)
    }

    /// step slope by wheel notches, clamped at ends
    pub fn step(self, steps: i32) -> Self {
        let at = Self::ALL
            .iter()
            .position(|&slope| slope == self)
            .expect("every cut slope is in the ordered list") as i32;
        let index = at
            .saturating_add(steps)
            .clamp(0, Self::ALL.len() as i32 - 1) as usize;
        Self::ALL[index]
    }

    /// Number of second-order sections, and whether one real pole is left over.
    pub fn sections(self) -> (usize, bool) {
        (self.order() / 2, self.order() % 2 == 1)
    }

    /// Butterworth pole Qs for each biquad (odd orders have an extra real pole at -1)
    pub fn section_q(self) -> [f32; 8] {
        let order = self.order();
        std::array::from_fn(|i| {
            if i < order / 2 {
                1.0 / (2.0 * (PI * (2 * i + 1) as f32 / (2 * order) as f32).sin())
            } else {
                0.0
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefficients {
    pub(crate) a1: f32,
    pub(crate) a2: f32,
    pub(crate) a3: f32,
    pub g: f32,
    pub k: f32,
    pub mix: [f32; 3],
}

impl Coefficients {
    pub fn identity() -> Self {
        Self::new(1.0, 1.0, [1.0, 0.0, 0.0])
    }

    /// Butterworth sections with highest-Q controlled by Q knob (1/sqrt(2) = flat)
    pub fn cut_section(g: f32, q: f32, kind: FilterKind, first_order: bool) -> Self {
        if first_order {
            // Exact pole-zero cancellation realizes a first-order TPT section.
            let mix = if kind == FilterKind::Highpass {
                [1.0, -1.0, -1.0]
            } else {
                [0.0, 1.0, 1.0]
            };
            return Self::new(g, 2.0, mix);
        }

        let k = 1.0 / q;
        let mix = if kind == FilterKind::Highpass {
            [1.0, -k, -1.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        Self::new(g, k, mix)
    }

    pub fn first_order_shelf(g: f32, db: f32, kind: FilterKind) -> Self {
        let db = if kind == FilterKind::TiltShelf {
            2.0 * db
        } else {
            db
        };
        let a = 10.0_f32.powf(db / 40.0);
        let gain = a * a;
        // The numerator cancels one of the two identical SVF poles.
        match kind {
            FilterKind::LowShelf => Self::new(g / a, 2.0, [1.0, gain - 1.0, gain - 1.0]),
            FilterKind::TiltShelf => Self::new(g * a, 2.0, [a, a.recip() - a, a.recip() - a]),
            _ => Self::new(g * a, 2.0, [gain, 1.0 - gain, 1.0 - gain]),
        }
    }

    /// transform Butterworth low shelf into a bell via s_lp = (Q/sqrt(2))*(s^2+1)/s
    pub fn bell_sections(g: f32, q: f32, db: f32, sections: &mut [Self]) {
        let order = sections.len();
        let width = bell_bandwidth(q);
        let a = 10.0_f32.powf(db / (40.0 * order as f32));
        let mut pairs = sections.chunks_exact_mut(2);
        for (i, pair) in pairs.by_ref().enumerate() {
            let half = band_half_pole(order, i, width);
            let (pole, k) = band_pole(half / a);
            let (zero, zero_k) = band_pole(half * a);
            let ratio = zero / pole;
            for (section, (g, ratio)) in pair
                .iter_mut()
                .zip([(g / pole, ratio.recip()), (g * pole, ratio)])
            {
                *section = Self::new(g, k, [1.0, zero_k * ratio - k, ratio * ratio - 1.0]);
            }
        }
        if let [last] = pairs.into_remainder() {
            let k = width / a;
            *last = Self::new(g, k, [1.0, width * a - k, 0.0]);
        }
    }

    /// transform Nth-order Butterworth LP into N band-pass/stop biquads
    pub fn band_filter_sections(g: f32, q: f32, kind: FilterKind, sections: &mut [Self]) {
        let order = sections.len();
        let inverse_q = q.recip();
        let mut pairs = sections.chunks_exact_mut(2);
        for (i, pair) in pairs.by_ref().enumerate() {
            // pass/stop use p and 1/p (same conjugate pairs on unit circle)
            let (radius, k) = band_pole(band_half_pole(order, i, inverse_q));
            let radius_squared = radius * radius;

            pair[0] = Self::new(
                g / radius,
                k,
                band_filter_mix(kind, inverse_q, k, radius_squared),
            );
            pair[1] = Self::new(
                g * radius,
                k,
                band_filter_mix(kind, inverse_q, k, radius_squared.recip()),
            );
        }
        if let [last] = pairs.into_remainder() {
            // An odd prototype order also has one real pole at -1.
            *last = Self::new(
                g,
                inverse_q,
                band_filter_mix(kind, inverse_q, inverse_q, 1.0),
            );
        }
    }

    /// octave-spaced shelves for tilt
    pub fn flat_tilt_sections(rate: f32, frequency: f32, db: f32, sections: &mut [Self]) {
        let db_per_octave = db / 5.0;
        let a = 10.0_f32.powf(db_per_octave / 40.0);
        let pivot = (PI * frequency / rate).tan();
        let mut corner = PI * 2.5 / rate;
        let mut level = 1.0;
        for section in sections.iter_mut() {
            // Pair two adjacent real poles in one SVF section.
            let [p1, p2] = [(corner * a).tanh(), (2.0 * corner * a).tanh()];
            let [z1, z2] = [(corner / a).tanh(), (2.0 * corner / a).tanh()];
            let g = (p1 * p2).sqrt();
            let k = (p1 + p2) / g;
            let high_gain = (p1 * p2) / (z1 * z2);
            *section = Self::new(
                g,
                k,
                [high_gain, high_gain * ((z1 + z2) / g - k), 1.0 - high_gain],
            );
            level *= high_gain
                * (pivot.hypot(z1) / pivot.hypot(p1))
                * (pivot.hypot(z2) / pivot.hypot(p2));
            corner *= 4.0;
        }
        sections[0].mix = sections[0].mix.map(|tap| tap / level);
    }

    pub fn new(g: f32, k: f32, mix: [f32; 3]) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));

        Self {
            a1,
            a2: g * a1,
            a3: g * g * a1,
            g,
            k,
            mix,
        }
    }

    /// Cytomic TPT SVF output mixes; shelf prewarping keeps the midpoint at fc.
    pub fn for_kind(rate: f32, frequency: f32, q: f32, db: f32, kind: FilterKind) -> Self {
        let a = 10.0_f32.powf(db / 40.0);
        let mut g = (PI * frequency / rate).tan();
        let mut k = 1.0 / q;

        let mix = match kind {
            FilterKind::Peak => {
                k /= a;
                [1.0, k * (a * a - 1.0), 0.0]
            }
            FilterKind::LowShelf => {
                g /= a.sqrt();
                [1.0, k * (a - 1.0), a * a - 1.0]
            }
            FilterKind::HighShelf => {
                g *= a.sqrt();
                [a * a, k * (1.0 - a) * a, 1.0 - a * a]
            }
            FilterKind::TiltShelf => {
                // A high shelf of twice the band gain, centered around 0 dB.
                g *= a;
                let gain = a * a;
                [gain, k * (1.0 - gain), gain.recip() - gain]
            }
            FilterKind::Highpass => [1.0, -k, -1.0],
            FilterKind::Lowpass => [0.0, 0.0, 1.0],
            FilterKind::Notch => [1.0, -k, 0.0],
            FilterKind::BandPass => [0.0, k, 0.0],
            FilterKind::FlatTilt => unreachable!("flat tilt is designed as a cascade"),
        };

        Self::new(g, k, mix)
    }

    pub fn approach(self, target: Self, alpha: f32) -> Self {
        Self::new(
            self.g + alpha * (target.g - self.g),
            self.k + alpha * (target.k - self.k),
            std::array::from_fn(|i| self.mix[i] + alpha * (target.mix[i] - self.mix[i])),
        )
    }

    pub fn response(self, rate: f32, frequency: f32) -> Complex32 {
        let r = (PI * frequency / rate).tan() / self.g;
        let denominator = Complex32::new(1.0 - r * r, self.k * r);

        Complex32::new(self.mix[0], 0.0)
            + (Complex32::new(self.mix[2], self.mix[1] * r) / denominator)
    }
}

fn band_half_pole(order: usize, index: usize, width: f32) -> Complex32 {
    let angle = PI * (2 * index + 1) as f32 / (2 * order) as f32;
    let (sin, cos) = angle.sin_cos();
    Complex32::new(-sin, -cos) * (0.5 * width)
}

/// Half-gain bandwidth in the prewarped domain, relative to the center.
pub(crate) fn bell_bandwidth(q: f32) -> f32 {
    std::f32::consts::SQRT_2 / q
}

/// solve s^2 - 2*half*s + 1 = 0
fn band_pole(half: Complex32) -> (f32, f32) {
    let outer = half - (half * half - 1.0).sqrt();
    let radius = outer.norm();
    (radius, -2.0 * outer.re / radius)
}

fn band_filter_mix(kind: FilterKind, inverse_q: f32, k: f32, ratio_squared: f32) -> [f32; 3] {
    if kind == FilterKind::BandPass {
        [0.0, inverse_q, 0.0]
    } else {
        [1.0, -k, ratio_squared - 1.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slope_step_clamps() {
        assert_eq!(CutSlope::Db12.step(0), CutSlope::Db12);
        assert_eq!(CutSlope::Db12.step(1), CutSlope::Db18);
        assert_eq!(CutSlope::Db12.step(2), CutSlope::Db24);
        assert_eq!(CutSlope::Db12.step(-1), CutSlope::Db6);
        assert_eq!(CutSlope::Db6.step(-4), CutSlope::Db6);
        assert_eq!(CutSlope::Db96.step(4), CutSlope::Db96);
        assert_eq!(CutSlope::Db24.step(100), CutSlope::Db96);
        assert_eq!(CutSlope::Db24.step(-100), CutSlope::Db6);
        assert_eq!(CutSlope::Db24.step(i32::MAX), CutSlope::Db96);
        assert_eq!(CutSlope::Db24.step(i32::MIN), CutSlope::Db6);
    }
}
