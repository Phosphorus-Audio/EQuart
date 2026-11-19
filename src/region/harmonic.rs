use super::RegionControl;
use crate::band::BandControl;
use crate::filter::CutSlope;
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HarmonicControl {
    pub enabled: bool,
    pub mode: HarmonicMode,
    pub first: i32,
    pub last: i32,
}

impl Default for HarmonicControl {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: HarmonicMode::Only,
            first: 2,
            last: 8,
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
    let Some(pitch) = pitch.filter(|_| region.enabled && !region.bypass && region.gain != 0.0)
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
        // bandwidth is 20% of gap to next harmonic
        let half_width = pitch * 0.1;
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
        }
    }

    pub fn control(&self) -> HarmonicControl {
        HarmonicControl {
            enabled: self.enabled.value() && !self.bypassed.value(),
            mode: self.mode.value(),
            first: self.first.value(),
            last: self.last.value(),
        }
    }
}
