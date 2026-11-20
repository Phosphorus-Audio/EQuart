use super::RegionControl;
use crate::band::BandControl;
use crate::filter::CutSlope;
use crate::pitch::{BASS_MIN_HZ, DetectionRange, MAX_HZ, PitchControl, PitchDetectionParams};
use nice_plug::prelude::*;
use realfft::num_complex::Complex32;
use std::f32::consts::{PI, SQRT_2};

pub const MAX_HARMONIC: usize = 16;
pub(super) const MASK_SECTIONS: usize = 2;
pub(super) fn inactive() -> BandControl {
    BandControl {
        enabled: false,
        slope: CutSlope::Db24,
        ..BandControl::default()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Enum)]
pub enum HarmonicMode {
    #[default]
    #[id = "only"]
    Only,
    #[id = "protect"]
    Protect,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Enum)]
pub enum FundamentalSource {
    #[default]
    #[id = "tracked"]
    Tracked,
    #[id = "fixed"]
    Fixed,
}

impl FundamentalSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tracked => "Tracked",
            Self::Fixed => "Fixed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HarmonicControl {
    pub enabled: bool,
    pub mode: HarmonicMode,
    pub first: i32,
    pub last: i32,
    pub width_percent: f32,
    pub source: FundamentalSource,
    pub fixed_hz: f32,
    pub detection: PitchControl,
}

impl Default for HarmonicControl {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: HarmonicMode::Only,
            first: 2,
            last: 8,
            width_percent: 20.0,
            source: FundamentalSource::Tracked,
            fixed_hz: 110.0,
            detection: PitchControl {
                enabled: true,
                profile: DetectionRange::Low,
                ..PitchControl::default()
            },
        }
    }
}

impl HarmonicControl {
    pub fn fundamental(self, tracked: Option<f32>) -> Option<f32> {
        if !self.enabled {
            return None;
        }
        match self.source {
            FundamentalSource::Tracked => tracked,
            FundamentalSource::Fixed => Some(self.fixed_hz),
        }
    }

    pub(crate) fn migrate_state(state: &mut PluginState) {
        use nice_plug::plugin::ParamValue::{F32, I32};

        let defaults = Self::default();
        let detection = defaults.detection;
        let parameters = [
            ("harmonic_width", F32(defaults.width_percent)),
            ("harmonic_source", I32(defaults.source.to_index() as i32)),
            ("harmonic_fixed_hz", F32(defaults.fixed_hz)),
            (
                "harmonic_pitch_algorithm",
                I32(detection.algorithm.to_index() as i32),
            ),
            (
                "harmonic_pitch_profile",
                I32(detection.profile.to_index() as i32),
            ),
            ("harmonic_pitch_tracking", F32(detection.tracking_ms)),
            ("harmonic_pitch_gate", F32(detection.gate_db)),
            ("harmonic_pitch_clarity", F32(detection.clarity * 100.0)),
            (
                "harmonic_pitch_target",
                I32(detection.target.mode.to_index() as i32),
            ),
            ("harmonic_pitch_minimum", F32(detection.target.minimum)),
            ("harmonic_pitch_maximum", F32(detection.target.maximum)),
            ("harmonic_pitch_reference", F32(detection.target.reference)),
            ("harmonic_pitch_tolerance", F32(detection.target.tolerance)),
        ];

        for index in 1..=super::REGION_COUNT {
            for (id, value) in &parameters {
                state
                    .params
                    .entry(format!("{id}_{index}"))
                    .or_insert_with(|| value.clone());
            }
        }
    }
}

pub(super) fn filters(
    mut region: RegionControl,
    rate: f32,
    pitch: Option<f32>,
) -> ([BandControl; 2], [BandControl; MAX_HARMONIC]) {
    let inactive = inactive();
    let mut masks = [inactive; MAX_HARMONIC];
    if !region.harmonic.enabled {
        return (region.bands(rate), masks);
    }
    let Some(pitch) = region
        .harmonic
        .fundamental(pitch)
        .filter(|_| region.enabled && !region.bypass && region.gain != 0.0)
    else {
        return ([inactive; 2], masks);
    };
    let protect = region.harmonic.mode == HarmonicMode::Protect;
    if protect {
        region.gain = -region.gain.abs();
    }
    let base = region.bands(rate);
    let sections = base.map(|band| band.enabled.then(|| band.sections(rate)));
    let [low, high] = region.bounds().map(|hz| hz.min(rate * 0.49));
    let first = region.harmonic.first as usize;
    let last = region.harmonic.last as usize;

    if low < high {
        let half_width = pitch * (region.harmonic.width_percent / 200.0);
        for number in first.min(last)..=first.max(last) {
            let frequency = pitch * number as f32;
            if !(low..=high).contains(&frequency) {
                continue;
            }
            let response = sections.iter().flatten().fold(
                Complex32::new(1.0, 0.0),
                |value, (coefficients, count)| {
                    coefficients[..*count].iter().fold(value, |value, section| {
                        value * section.response(rate, frequency)
                    })
                },
            );
            let gain = 20.0 * response.norm().log10();
            let warp = |hz: f32| (PI * hz / rate).tan();
            let width =
                warp((frequency + half_width).min(rate * 0.49)) - warp(frequency - half_width);
            masks[number - 1] = BandControl {
                enabled: gain != 0.0,
                frequency,
                gain: if protect { -gain } else { gain },
                q: SQRT_2 * warp(frequency) / width,
                ..inactive
            };
        }
    }

    (if protect { base } else { [inactive; 2] }, masks)
}

#[derive(Params)]
pub struct HarmonicParams {
    #[id = "harmonic_enabled"]
    pub enabled: BoolParam,
    #[id = "harmonic_bypassed"]
    pub bypassed: BoolParam,
    #[id = "harmonic_mode"]
    pub mode: EnumParam<HarmonicMode>,
    #[id = "harmonic_first"]
    pub first: IntParam,
    #[id = "harmonic_last"]
    pub last: IntParam,
    #[id = "harmonic_width"]
    pub width: FloatParam,
    #[id = "harmonic_source"]
    pub source: EnumParam<FundamentalSource>,
    #[id = "harmonic_fixed_hz"]
    pub fixed_hz: FloatParam,
    #[nested(id_prefix = "harmonic", group = "Detection")]
    pub detection: PitchDetectionParams,
}

impl HarmonicParams {
    pub fn new(prefix: &str) -> Self {
        let defaults = HarmonicControl::default();
        let name = |label| format!("{prefix} Harmonic {label}");
        let range = IntRange::Linear {
            min: 1,
            max: MAX_HARMONIC as i32,
        };
        Self {
            enabled: BoolParam::new(name("Enabled"), false),
            bypassed: BoolParam::new(name("Bypassed"), false),
            mode: EnumParam::new(name("Mode"), defaults.mode),
            first: IntParam::new(name("First"), defaults.first, range),
            last: IntParam::new(name("Last"), defaults.last, range),
            width: FloatParam::new(
                name("Width"),
                defaults.width_percent,
                FloatRange::Linear {
                    min: 5.0,
                    max: 100.0,
                },
            )
            .with_unit(" %"),
            source: EnumParam::new(name("Fundamental Source"), defaults.source),
            fixed_hz: FloatParam::new(
                name("Fixed Fundamental"),
                defaults.fixed_hz,
                FloatRange::Logarithmic {
                    min: BASS_MIN_HZ,
                    max: MAX_HZ,
                },
            )
            .with_unit(" Hz"),
            detection: PitchDetectionParams::new(&name("Detection"), defaults.detection),
        }
    }

    pub fn control(&self) -> HarmonicControl {
        HarmonicControl {
            enabled: self.enabled.value() && !self.bypassed.value(),
            mode: self.mode.value(),
            first: self.first.value(),
            last: self.last.value(),
            width_percent: self.width.value(),
            source: self.source.value(),
            fixed_hz: self.fixed_hz.value(),
            detection: self.detection.control(),
        }
    }
}
