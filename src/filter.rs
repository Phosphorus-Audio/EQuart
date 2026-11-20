use crate::precision::Sample;
use nice_plug::prelude::*;
use realfft::num_complex::Complex;

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
    pub fn section_q<T: Sample>(self) -> [T; 8] {
        let order = self.order();
        std::array::from_fn(|i| {
            if i < order / 2 {
                let angle = T::PI() * T::value((2 * i + 1) as f64) / T::value((2 * order) as f64);
                (T::value(2.0) * angle.sin()).recip()
            } else {
                T::zero()
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefficients<T = f32> {
    pub(crate) a1: T,
    pub(crate) a2: T,
    pub(crate) a3: T,
    pub g: T,
    pub k: T,
    pub mix: [T; 3],
}

impl<T: Sample> Coefficients<T> {
    pub fn identity() -> Self {
        Self::new(T::one(), T::one(), [T::one(), T::zero(), T::zero()])
    }

    /// Butterworth sections with highest-Q controlled by Q knob (1/sqrt(2) = flat)
    pub fn cut_section(g: T, q: T, kind: FilterKind, first_order: bool) -> Self {
        if first_order {
            // Exact pole-zero cancellation realizes a first-order TPT section.
            let mix = if kind == FilterKind::Highpass {
                [T::one(), -T::one(), -T::one()]
            } else {
                [T::zero(), T::one(), T::one()]
            };
            return Self::new(g, T::value(2.0), mix);
        }

        let k = T::one() / q;
        let mix = if kind == FilterKind::Highpass {
            [T::one(), -k, -T::one()]
        } else {
            [T::zero(), T::zero(), T::one()]
        };

        Self::new(g, k, mix)
    }

    pub fn first_order_shelf(g: T, db: T, kind: FilterKind) -> Self {
        let db = if kind == FilterKind::TiltShelf {
            T::value(2.0) * db
        } else {
            db
        };

        let a = T::value(10.0).powf(db / T::value(40.0));

        let gain = a * a;
        // The numerator cancels one of the two identical SVF poles.
        match kind {
            FilterKind::LowShelf => Self::new(
                g / a,
                T::value(2.0),
                [T::one(), gain - T::one(), gain - T::one()],
            ),
            FilterKind::TiltShelf => {
                Self::new(g * a, T::value(2.0), [a, a.recip() - a, a.recip() - a])
            }
            _ => Self::new(
                g * a,
                T::value(2.0),
                [gain, T::one() - gain, T::one() - gain],
            ),
        }
    }

    /// transform Butterworth low shelf into a bell via s_lp = (Q/sqrt(2))*(s^2+1)/s
    pub fn bell_sections(g: T, q: T, db: T, sections: &mut [Self]) {
        let order = sections.len();
        let width = bell_bandwidth(q);
        let a = T::value(10.0).powf(db / (T::value(40.0) * T::value(order as f64)));
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
                *section = Self::new(
                    g,
                    k,
                    [T::one(), zero_k * ratio - k, ratio * ratio - T::one()],
                );
            }
        }
        if let [last] = pairs.into_remainder() {
            let k = width / a;
            *last = Self::new(g, k, [T::one(), width * a - k, T::zero()]);
        }
    }

    /// transform Nth-order Butterworth LP into N band-pass/stop biquads
    pub fn band_filter_sections(g: T, q: T, kind: FilterKind, sections: &mut [Self]) {
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
                band_filter_mix(kind, inverse_q, inverse_q, T::one()),
            );
        }
    }

    /// octave-spaced shelves for tilt
    pub fn flat_tilt_sections(rate: T, frequency: T, db: T, sections: &mut [Self]) {
        let db_per_octave = db / T::value(5.0);
        let a = T::value(10.0).powf(db_per_octave / T::value(40.0));
        let pivot = (T::PI() * frequency / rate).tan();
        let mut corner = T::PI() * T::value(2.5) / rate;
        let mut level = T::one();

        for section in sections.iter_mut() {
            // Pair two adjacent real poles in one SVF section.
            let [p1, p2] = [(corner * a).tanh(), (T::value(2.0) * corner * a).tanh()];
            let [z1, z2] = [(corner / a).tanh(), (T::value(2.0) * corner / a).tanh()];
            let g = (p1 * p2).sqrt();
            let k = (p1 + p2) / g;
            let high_gain = (p1 * p2) / (z1 * z2);
            *section = Self::new(
                g,
                k,
                [
                    high_gain,
                    high_gain * ((z1 + z2) / g - k),
                    T::one() - high_gain,
                ],
            );
            level *= high_gain
                * (pivot.hypot(z1) / pivot.hypot(p1))
                * (pivot.hypot(z2) / pivot.hypot(p2));
            corner *= T::value(4.0);
        }
        sections[0].mix = sections[0].mix.map(|tap| tap / level);
    }

    pub fn new(g: T, k: T, mix: [T; 3]) -> Self {
        let a1 = T::one() / (T::one() + g * (g + k));

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
    pub fn for_kind(rate: T, frequency: T, q: T, db: T, kind: FilterKind) -> Self {
        let a = T::value(10.0).powf(db / T::value(40.0));
        let mut g = (T::PI() * frequency / rate).tan();
        let mut k = T::one() / q;

        let mix = match kind {
            FilterKind::Peak => {
                k /= a;
                [T::one(), k * (a * a - T::one()), T::zero()]
            }
            FilterKind::LowShelf => {
                g /= a.sqrt();
                [T::one(), k * (a - T::one()), a * a - T::one()]
            }
            FilterKind::HighShelf => {
                g *= a.sqrt();
                [a * a, k * (T::one() - a) * a, T::one() - a * a]
            }
            FilterKind::TiltShelf => {
                // A high shelf of twice the band gain, centered around 0 dB.
                g *= a;
                let gain = a * a;
                [gain, k * (T::one() - gain), gain.recip() - gain]
            }
            FilterKind::Highpass => [T::one(), -k, -T::one()],
            FilterKind::Lowpass => [T::zero(), T::zero(), T::one()],
            FilterKind::Notch => [T::one(), -k, T::zero()],
            FilterKind::BandPass => [T::zero(), k, T::zero()],
            FilterKind::FlatTilt => unreachable!("flat tilt is designed as a cascade"),
        };

        Self::new(g, k, mix)
    }

    pub fn approach(self, target: Self, alpha: T) -> Self {
        Self::new(
            self.g + alpha * (target.g - self.g),
            self.k + alpha * (target.k - self.k),
            std::array::from_fn(|i| self.mix[i] + alpha * (target.mix[i] - self.mix[i])),
        )
    }

    pub fn response(self, rate: T, frequency: T) -> Complex<T> {
        let r = (T::PI() * frequency / rate).tan() / self.g;
        let denominator = Complex::new(T::one() - r * r, self.k * r);

        Complex::new(self.mix[0], T::zero())
            + (Complex::new(self.mix[2], self.mix[1] * r) / denominator)
    }
}

fn band_half_pole<T: Sample>(order: usize, index: usize, width: T) -> Complex<T> {
    let angle = T::PI() * T::value((2 * index + 1) as f64) / T::value((2 * order) as f64);
    let (sin, cos) = angle.sin_cos();
    Complex::new(-sin, -cos) * (T::value(0.5) * width)
}

/// Half-gain bandwidth in the prewarped domain, relative to the center.
pub(crate) fn bell_bandwidth<T: Sample>(q: T) -> T {
    T::SQRT_2() / q
}

/// solve s^2 - 2*half*s + 1 = 0
fn band_pole<T: Sample>(half: Complex<T>) -> (T, T) {
    let outer = half - (half * half - T::one()).sqrt();
    let radius = outer.norm();

    (radius, -T::value(2.0) * outer.re / radius)
}

fn band_filter_mix<T: Sample>(kind: FilterKind, inverse_q: T, k: T, ratio_squared: T) -> [T; 3] {
    if kind == FilterKind::BandPass {
        [T::zero(), inverse_q, T::zero()]
    } else {
        [T::one(), -k, ratio_squared - T::one()]
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
