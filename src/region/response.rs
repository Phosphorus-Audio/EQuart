use super::{MAX_HARMONIC, RegionControl};
use crate::modulation::relation::{Curve, Scale};
use crate::params::FREQUENCY_MAX_HZ;
use nice_plug::prelude::Enum;

mod params;
pub(crate) use params::{ResponseParams, migrate_state};

pub(crate) const SLOTS: usize = 2;
const MIN_HZ: f32 = crate::analyzer::MIN_HZ;

pub(crate) fn order(reverse: bool) -> [usize; SLOTS] {
    if reverse { [1, 0] } else { [0, 1] }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Enum)]
pub(crate) enum Scope {
    #[default]
    #[name = "Whole Region"]
    Whole,
    #[name = "Per Harmonic"]
    Harmonic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Enum)]
pub(crate) enum Value {
    #[default]
    #[name = "Region Gain"]
    Gain,
    #[name = "Region Center"]
    Center,
    #[name = "Region Width"]
    Width,
    #[name = "Edge Softness"]
    Softness,
    Fundamental,
    #[name = "Resonance Q"]
    ResonanceQ,
    #[name = "Resonance Depth"]
    ResonanceDepth,
    #[name = "Prominence"]
    Prominence,
    #[name = "Harmonic Frequency"]
    HarmonicFrequency,
    #[name = "Harmonic Index"]
    HarmonicIndex,
    #[name = "Harmonic Width"]
    HarmonicWidth,
    #[name = "Harmonic Depth"]
    HarmonicDepth,
}

impl Value {
    pub(crate) const WHOLE_SOURCES: [Self; 4] =
        [Self::Gain, Self::Center, Self::Width, Self::Fundamental];
    pub(crate) const WHOLE_TARGETS: [Self; 8] = [
        Self::Gain,
        Self::Center,
        Self::Width,
        Self::Softness,
        Self::ResonanceQ,
        Self::ResonanceDepth,
        Self::Prominence,
        Self::HarmonicWidth,
    ];
    pub(crate) const HARMONIC_SOURCES: [Self; 5] = [
        Self::HarmonicFrequency,
        Self::HarmonicIndex,
        Self::Fundamental,
        Self::HarmonicWidth,
        Self::HarmonicDepth,
    ];
    pub(crate) const HARMONIC_TARGETS: [Self; 2] = [Self::HarmonicWidth, Self::HarmonicDepth];

    pub(crate) fn label(self) -> &'static str {
        Self::variants()[self.to_index()]
    }

    pub(crate) fn scale(self) -> Scale {
        match self {
            Self::Center
            | Self::Fundamental
            | Self::HarmonicFrequency
            | Self::HarmonicWidth
            | Self::ResonanceQ => Scale::Ratio,
            Self::Gain | Self::ResonanceDepth | Self::Prominence | Self::HarmonicDepth => {
                Scale::Linear(6.0)
            }
            Self::Softness => Scale::Linear(25.0),
            Self::Width | Self::HarmonicIndex => Scale::Linear(1.0),
        }
    }

    pub(crate) fn limits(self, rate: f32) -> (f32, f32) {
        match self {
            Self::Center | Self::Fundamental | Self::HarmonicFrequency => {
                (MIN_HZ, FREQUENCY_MAX_HZ.min(rate * 0.49))
            }
            Self::Gain => (-24.0, 24.0),
            Self::Width => (0.0, (FREQUENCY_MAX_HZ.min(rate * 0.49) / MIN_HZ).log2()),
            Self::Softness => (0.0, 100.0),
            Self::ResonanceQ => (2.0, 32.0),
            Self::HarmonicIndex => (1.0, MAX_HARMONIC as f32),
            Self::HarmonicWidth => (5.0, 100.0),
            Self::ResonanceDepth | Self::Prominence | Self::HarmonicDepth => (0.0, 24.0),
        }
    }

    pub(crate) fn unit(self) -> &'static str {
        match self {
            Self::Center | Self::Fundamental | Self::HarmonicFrequency => " Hz",
            Self::Gain | Self::ResonanceDepth | Self::Prominence | Self::HarmonicDepth => " dB",
            Self::Softness | Self::HarmonicWidth => "%",
            Self::Width => " oct",
            Self::HarmonicIndex | Self::ResonanceQ => "",
        }
    }

    pub(crate) fn display(self, value: f32) -> String {
        let unit = self.unit();
        if self == Self::HarmonicIndex {
            format!("{value:.0}")
        } else {
            format!("{value:.2}{unit}")
        }
    }

    pub(crate) fn available(self, region: RegionControl) -> bool {
        match self {
            Self::ResonanceQ | Self::ResonanceDepth | Self::Prominence => region.resonance.enabled,
            Self::Fundamental
            | Self::HarmonicFrequency
            | Self::HarmonicIndex
            | Self::HarmonicWidth
            | Self::HarmonicDepth => region.harmonic.enabled,
            _ => true,
        }
    }

    pub(crate) fn shared(self, region: RegionControl, fundamental: Option<f32>) -> Option<f32> {
        let [low, high] = region.bounds();
        match self {
            Self::Gain => Some(region.gain),
            Self::Center => Some((low * high).sqrt()),
            Self::Width => Some((high / low).log2()),
            Self::Softness => Some(region.softness),
            Self::Fundamental => fundamental,
            Self::ResonanceQ => Some(region.resonance.q),
            Self::ResonanceDepth => Some(region.resonance.depth),
            Self::Prominence => Some(region.resonance.threshold),
            Self::HarmonicWidth => Some(region.harmonic.width_percent),
            _ => None,
        }
    }

    fn write(self, region: &mut RegionControl, value: f32, rate: f32) {
        match self {
            Self::Gain => region.gain = value,
            Self::Softness => region.softness = value,
            Self::ResonanceQ => region.resonance.q = value,
            Self::ResonanceDepth => region.resonance.depth = value,
            Self::Prominence => region.resonance.threshold = value,
            Self::HarmonicWidth => region.harmonic.width_percent = value,
            Self::Center | Self::Width => {
                let (min, max) = Self::Center.limits(rate);
                let [low, high] = region.bounds().map(|hz| hz.clamp(min, max));
                let center = if self == Self::Center {
                    value
                } else {
                    (low * high).sqrt()
                };
                let width = if self == Self::Width {
                    value
                } else {
                    (high / low).log2()
                };
                let ratio = (0.5 * width).exp2();
                let center = center.clamp(min * ratio, (max / ratio).max(min * ratio));
                region.low = (center / ratio).clamp(min, max);
                region.high = (center * ratio).clamp(min, max);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Response {
    pub enabled: bool,
    pub bypass: bool,
    pub scope: Scope,
    pub source: Value,
    pub target: Value,
    pub curve: Curve,
    pub strength: f32,
    pub reference: f32,
}

impl Default for Response {
    fn default() -> Self {
        Self {
            enabled: false,
            bypass: false,
            scope: Scope::Whole,
            source: Value::Gain,
            target: Value::ResonanceQ,
            curve: Curve::Power,
            strength: 0.0,
            reference: -6.0,
        }
    }
}

impl Response {
    pub(crate) fn valid(self, region: RegionControl) -> bool {
        let (sources, targets) = match self.scope {
            Scope::Whole => (
                Value::WHOLE_SOURCES.as_slice(),
                Value::WHOLE_TARGETS.as_slice(),
            ),
            Scope::Harmonic => (
                Value::HARMONIC_SOURCES.as_slice(),
                Value::HARMONIC_TARGETS.as_slice(),
            ),
        };
        self.source != self.target
            && sources.contains(&self.source)
            && targets.contains(&self.target)
            && self.source.available(region)
            && self.target.available(region)
            && (self.scope != Scope::Harmonic || region.harmonic.enabled)
    }

    pub(crate) fn output(self, input: f32, base: f32, rate: f32) -> f32 {
        self.curve.output(
            (self.source.scale(), input, self.reference),
            (self.target.scale(), base, self.target.limits(rate)),
            self.strength,
        )
    }
}

pub(crate) fn whole(mut region: RegionControl, pitch: Option<f32>, rate: f32) -> RegionControl {
    if !region.enabled || region.bypass || !region.responses.iter().any(|response| response.enabled)
    {
        return region;
    }

    let base = region;
    let fundamental = base.harmonic.fundamental(pitch);

    for response in &mut region.responses {
        if response.scope == Scope::Whole {
            *response = Response::default();
        }
    }

    for slot in base.response_order() {
        let response = base.responses[slot];
        if !response.enabled
            || response.bypass
            || response.strength == 0.0
            || response.scope != Scope::Whole
            || !response.valid(base)
        {
            continue;
        }

        if let Some(input) = response.source.shared(base, fundamental)
            && let Some(previous) = response.target.shared(region, fundamental)
        {
            let effective = response.output(input, previous, rate);
            if effective != previous {
                response.target.write(&mut region, effective, rate);
            }
        }
    }

    region
}

#[derive(Default)]
pub(crate) struct Slew {
    previous: [Response; SLOTS],
}

impl Slew {
    pub(crate) fn update(&mut self, mut region: RegionControl, alpha: f32) -> RegionControl {
        for (response, previous) in region.responses.iter_mut().zip(&mut self.previous) {
            if !region.enabled || region.bypass || !response.enabled {
                *previous = Response::default();
                continue;
            }

            let same_route = previous.enabled
                && previous.scope == response.scope
                && previous.source == response.source
                && previous.target == response.target
                && previous.curve == response.curve;
            let strength = if response.bypass || !response.strength.is_finite() {
                0.0
            } else {
                response.strength
            };
            let old = if same_route { previous.strength } else { 0.0 };
            response.strength = if (strength - old).abs() < 1e-5 {
                strength
            } else {
                old + alpha * (strength - old)
            };

            if same_route {
                let scale = response.source.scale();
                let old = scale.encode(previous.reference);
                let target = scale.encode(response.reference);
                if old.is_finite() && target.is_finite() && (target - old).abs() > 1e-5 {
                    response.reference = scale.decode(old + alpha * (target - old));
                }
            }

            response.bypass = false;
            *previous = *response;
        }

        region
    }

    pub(crate) fn controls(&self) -> [Response; SLOTS] {
        self.previous
    }
}
