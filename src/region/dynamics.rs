use crate::dynamics::DetectorInput;
use nice_plug::prelude::Enum;

mod curve;
mod curve_bank;
mod mask;
mod params;
mod runtime;

pub(crate) use curve::{CEILING, Curve, MAX_POINTS, Point, Segment};
pub(crate) use curve_bank::{CurveBank, FIELD as CURVE_FIELD};
pub(crate) use mask::MaskResponse;
pub(crate) use params::{DynamicsParams, migrate_state};
pub(crate) use runtime::{Meter, Rack};

pub(crate) const FLOOR: f32 = -96.0;

#[derive(Clone, Copy, Debug, Default, Enum, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Compressor,
    Expander,
}

#[derive(Clone, Copy, Debug, Default, Enum, PartialEq, Eq)]
pub(crate) enum Detector {
    Peak,
    #[default]
    #[name = "RMS"]
    Rms,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Control {
    pub enabled: bool,
    pub bypass: bool,
    pub mode: Mode,
    pub detector: Detector,
    pub source: DetectorInput,
    pub threshold: f32,
    pub ratio: f32,
    pub attack: f32,
    pub release: f32,
    pub knee: f32,
    pub makeup: f32,
    pub range: f32,
    pub link: f32,
    pub curve: Curve,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            enabled: false,
            bypass: false,
            mode: Mode::Compressor,
            detector: Detector::Rms,
            source: DetectorInput::Main,
            threshold: -18.0,
            ratio: 1.0,
            attack: 10.0,
            release: 100.0,
            knee: 6.0,
            makeup: 0.0,
            range: 24.0,
            link: 100.0,
            curve: Curve::default(),
        }
    }
}

impl Control {
    pub(crate) fn reduction(&self, input: f32) -> f32 {
        if !input.is_finite() || !self.range.is_finite() {
            return 0.0;
        }
        if let Some(output) = self.curve.output(input) {
            let range = self.range.max(0.0);
            return (output - input).clamp(-range, range);
        }

        if !self.threshold.is_finite() || !self.ratio.is_finite() || !self.knee.is_finite() {
            return 0.0;
        }

        let ratio = self.ratio.max(1.0);
        let (distance, slope) = match self.mode {
            Mode::Compressor => (input - self.threshold, 1.0 - ratio.recip()),
            Mode::Expander => (self.threshold - input, ratio - 1.0),
        };
        let half = self.knee.max(0.0) * 0.5;
        let active = if distance <= -half {
            0.0
        } else if distance >= half {
            distance
        } else {
            (distance + half).powi(2) / (4.0 * half)
        };
        -(slope * active).min(self.range.max(0.0))
    }

    pub(crate) fn transfer(&self, input: f32) -> f32 {
        let input = if input.is_finite() { input } else { FLOOR };
        let makeup = if self.makeup.is_finite() {
            self.makeup
        } else {
            0.0
        };
        input + self.reduction(input) + makeup
    }
}
