use crate::band::{BAND_COUNT, BandControl};
use crate::params::{FREQUENCY_MAX_HZ, Q_MAX, Q_MIN};
use nice_plug::prelude::{Enum, Transport};

mod params;
pub(crate) use params::{LfoParams, MappingParams, migrate_state};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Enum)]
pub(crate) enum Target {
    #[default]
    Frequency,
    Gain,
    Q,
}

impl Target {
    pub(crate) const ALL: [Self; 3] = [Self::Frequency, Self::Gain, Self::Q];

    pub(crate) fn label(self) -> &'static str {
        Self::variants()[self.to_index()]
    }

    pub(crate) fn limits(self, rate: f32) -> (f32, f32) {
        match self {
            Self::Frequency => (crate::analyzer::MIN_HZ, FREQUENCY_MAX_HZ.min(rate * 0.49)),
            Self::Gain => (-24.0, 24.0),
            Self::Q => (Q_MIN, Q_MAX),
        }
    }

    pub(crate) fn value(self, band: BandControl) -> f32 {
        match self {
            Self::Frequency => band.frequency,
            Self::Gain => band.gain,
            Self::Q => band.q,
        }
    }

    pub(crate) fn supported(self, band: BandControl) -> bool {
        match self {
            Self::Frequency => true,
            Self::Gain => band.kind.has_gain(),
            Self::Q => band.kind.uses_q(band.slope),
        }
    }

    fn set(self, band: &mut BandControl, value: f32, rate: f32) {
        let (min, max) = self.limits(rate);
        let value = value.clamp(min, max);

        match self {
            Self::Frequency => band.frequency = value,
            Self::Gain => band.gain = value,
            Self::Q => band.q = value,
        }
    }

    pub(crate) fn offset(self, base: f32, units: f32, rate: f32) -> f32 {
        let (min, max) = self.limits(rate);
        let value = match self {
            Self::Frequency => base * (units / 12.0).exp2(),
            Self::Gain => base + units,
            Self::Q => base * units.exp2(),
        };
        value.clamp(min, max)
    }

    pub(crate) fn display(self, value: f32) -> String {
        match self {
            Self::Frequency => format!("{value:.1} Hz"),
            Self::Gain => format!("{value:+.2} dB"),
            Self::Q => format!("{value:.3}"),
        }
    }

    pub(crate) fn offset_text(self, value: f32) -> String {
        match self {
            Self::Frequency => format!("{value:+.2} st"),
            Self::Gain => self.display(value),
            Self::Q => format!("×{:.3}", value.exp2()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mapping {
    pub enabled: bool,
    pub bypass: bool,
    pub source: Target,
    pub target: Target,
    pub strength: f32,
    pub reference: f32,
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            enabled: false,
            bypass: false,
            source: Target::Frequency,
            target: Target::Q,
            strength: 0.0,
            reference: 200.0,
        }
    }
}

impl Mapping {
    pub(crate) fn output(self, source: f32, base: f32, rate: f32) -> f32 {
        if self.source == self.target || self.strength == 0.0 || !self.strength.is_finite() {
            return base;
        }

        let distance = match self.source {
            Target::Gain => (source - self.reference) / 6.0,
            _ => (source / self.reference).log2(),
        };
        let units = match self.target {
            Target::Frequency => 12.0,
            Target::Gain => 6.0,
            Target::Q => 1.0,
        };
        if distance.is_finite() {
            self.target.offset(
                base,
                distance * self.strength.clamp(-1.0, 1.0) * units,
                rate,
            )
        } else {
            base
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Enum)]
pub(crate) enum Wave {
    #[default]
    Sine,
    Triangle,
    #[name = "Saw up"]
    SawUp,
    #[name = "Saw down"]
    SawDown,
    Square,
    #[name = "Sample & hold"]
    Random,
}

impl Wave {
    pub(crate) const ALL: [Self; 6] = [
        Self::Sine,
        Self::Triangle,
        Self::SawUp,
        Self::SawDown,
        Self::Square,
        Self::Random,
    ];

    pub(crate) fn sample(self, phase: f32, random: f32) -> f32 {
        match self {
            Self::Sine => (phase * std::f32::consts::TAU).sin(),
            Self::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            Self::SawUp => phase.mul_add(2.0, -1.0),
            Self::SawDown => phase.mul_add(-2.0, 1.0),
            Self::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Self::Random => random,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Enum)]
pub(crate) enum Division {
    #[name = "1/1"]
    Whole,
    #[name = "1/2"]
    Half,
    #[default]
    #[name = "1/4"]
    Quarter,
    #[name = "1/8"]
    Eighth,
    #[name = "1/16"]
    Sixteenth,
    #[name = "1/32"]
    ThirtySecond,
}

impl Division {
    pub(crate) const ALL: [Self; 6] = [
        Self::Whole,
        Self::Half,
        Self::Quarter,
        Self::Eighth,
        Self::Sixteenth,
        Self::ThirtySecond,
    ];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Enum)]
pub(crate) enum Timing {
    #[default]
    Straight,
    Dotted,
    Triplet,
}

impl Timing {
    pub(crate) const ALL: [Self; 3] = [Self::Straight, Self::Dotted, Self::Triplet];
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Lfo {
    pub enabled: bool,
    pub bypass: bool,
    pub target: Target,
    pub wave: Wave,
    pub sync: bool,
    pub division: Division,
    pub timing: Timing,
    pub hz: f32,
    pub depth: f32,
    pub phase: f32,
    pub unipolar: bool,
    pub transport: bool,
}

impl Default for Lfo {
    fn default() -> Self {
        Self {
            enabled: false,
            bypass: false,
            target: Target::Gain,
            wave: Wave::Sine,
            sync: true,
            division: Division::Quarter,
            timing: Timing::Straight,
            hz: 1.0,
            depth: 0.0,
            phase: 0.0,
            unipolar: false,
            transport: false,
        }
    }
}

impl Lfo {
    pub(crate) const LINEAR_MAX_HZ: f64 = 1.0;

    fn rate(self, tempo: f64) -> f64 {
        if self.sync {
            let multiplier = match self.timing {
                Timing::Straight => 1.0,
                Timing::Dotted => 1.5,
                Timing::Triplet => 2.0 / 3.0,
            };
            let beats = [4.0, 2.0, 1.0, 0.5, 0.25, 0.125][self.division.to_index()];
            tempo / (60.0 * beats * multiplier)
        } else {
            f64::from(self.hz)
        }
    }

    pub(crate) fn polar(self, sample: f32) -> f32 {
        if self.unipolar {
            (sample + 1.0) * 0.5
        } else {
            sample
        }
    }
}

mod runtime;
pub(crate) use runtime::{Live, Rack};
