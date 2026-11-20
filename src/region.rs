mod harmonic;
pub(crate) mod resonance;

pub use harmonic::{HarmonicControl, HarmonicMode, MAX_HARMONIC};

use crate::band::{Band, BandControl, SECTIONS};
use crate::filter::{CutSlope, FilterKind, bell_bandwidth};
use crate::params::{FREQUENCY_MAX_HZ, Q_MAX, Q_MIN};
use nice_plug::prelude::*;
use resonance::{REDUCTIONS, Reduction};
use std::f32::consts::{PI, SQRT_2};

pub const REGION_COUNT: usize = 6;
pub const DEFAULT_SOFTNESS: f32 = 35.0;
pub const DEFAULT_GAIN: f32 = -6.0;
const BASE_FILTERS: usize = 2 + MAX_HARMONIC;
const FILTERS: usize = BASE_FILTERS + REDUCTIONS;
const _: () = assert!(FILTERS <= u32::BITS as usize);

#[derive(Clone, Copy, PartialEq)]
pub struct RegionControl {
    pub enabled: bool,
    pub bypass: bool,
    pub low: f32,
    pub high: f32,
    pub softness: f32,
    pub gain: f32,
    pub harmonic: HarmonicControl,
    pub resonance: resonance::ResonanceControl,
}

impl Default for RegionControl {
    fn default() -> Self {
        Self {
            enabled: false,
            bypass: false,
            low: 500.0,
            high: 2_000.0,
            softness: DEFAULT_SOFTNESS,
            gain: DEFAULT_GAIN,
            resonance: Default::default(),
            harmonic: HarmonicControl::default(),
        }
    }
}

impl RegionControl {
    pub fn bounds(self) -> [f32; 2] {
        [self.low.min(self.high), self.low.max(self.high)]
    }

    pub fn from_band(band: BandControl, rate: f32) -> Self {
        let center = (PI * band.frequency.min(rate * 0.49) / rate).tan();
        let width = if band.kind == FilterKind::Peak && band.slope.biquad_count() > 1 {
            bell_bandwidth(band.q)
        } else {
            band.q.recip()
        };
        let half_span = (0.5 * width).asinh();
        let [low, high] = [-half_span, half_span].map(|span| {
            ((center * span.exp()).atan() * rate / PI)
                .clamp(crate::analyzer::MIN_HZ, FREQUENCY_MAX_HZ)
        });
        Self {
            enabled: true,
            bypass: !band.enabled,
            low,
            high,
            softness: slope_softness(band.slope),
            resonance: Default::default(),
            gain: if band.kind.has_gain() {
                band.gain
            } else {
                DEFAULT_GAIN
            },
            harmonic: HarmonicControl::default(),
        }
    }

    pub fn to_band(self, rate: f32) -> BandControl {
        let (center, width) = self.warped_geometry(rate);
        let slope = [
            CutSlope::Db24,
            CutSlope::Db36,
            CutSlope::Db48,
            CutSlope::Db72,
            CutSlope::Db96,
        ]
        .into_iter()
        .min_by(|&a, &b| {
            (self.softness - slope_softness(a))
                .abs()
                .total_cmp(&(self.softness - slope_softness(b)).abs())
        })
        .expect("a region converts to one of the bell slopes");
        BandControl {
            enabled: !self.bypass,
            frequency: center.atan() * rate / PI,
            gain: self.gain,
            q: (SQRT_2 * center / width).clamp(Q_MIN, Q_MAX),
            slope,
            ..BandControl::default()
        }
    }

    fn warped_geometry(self, rate: f32) -> (f32, f32) {
        let [low, high] = self
            .bounds()
            .map(|hz| (PI * hz.min(rate * 0.49) / rate).tan());
        ((low * high).sqrt(), high - low)
    }

    /// prewarped edges set the flat-top bell's half-gain points
    pub fn bands(self, rate: f32) -> [BandControl; 2] {
        let mut bands = [BandControl {
            enabled: false,
            ..BandControl::default()
        }; 2];
        if !self.enabled {
            return bands;
        }

        let (center, width) = self.warped_geometry(rate);
        if width == 0.0 {
            return bands;
        }
        let frequency = center.atan() * rate / PI;
        let q = SQRT_2 * center / width;
        let soft = self.softness * 0.01;
        for (band, (slope, amount)) in bands
            .iter_mut()
            .zip([(CutSlope::Db24, soft), (CutSlope::Db96, 1.0 - soft)])
        {
            *band = BandControl {
                enabled: !self.bypass && self.gain != 0.0 && amount > 0.0,
                frequency,
                gain: self.gain * amount,
                q,
                slope,
                ..BandControl::default()
            };
        }
        bands
    }

    pub fn filter_plan(self, rate: f32, pitch: Option<f32>) -> impl Iterator<Item = BandControl> {
        let (base, masks) = harmonic::filters(self, rate, pitch);
        base.into_iter().chain(masks)
    }
}

fn slope_softness(slope: CutSlope) -> f32 {
    (8 - slope.biquad_count().max(2)) as f32 * (100.0 / 6.0)
}

pub struct Region {
    filters: Box<[Band]>,
    targets: Box<[BandControl]>,
    last: Option<(RegionControl, f32, Option<f32>)>,
    active: u32,
}

impl Region {
    pub fn new() -> Self {
        Self {
            filters: (0..2)
                .map(|_| Band::with_rack(SECTIONS, true))
                .chain((0..MAX_HARMONIC).map(|_| Band::with_rack(harmonic::MASK_SECTIONS, true)))
                .chain((0..REDUCTIONS).map(|_| Band::with_rack(1, true)))
                .collect(),
            targets: vec![harmonic::inactive(); FILTERS].into_boxed_slice(),
            last: None,
            active: 0,
        }
    }

    pub fn reset(&mut self) {
        for filter in &mut self.filters {
            filter.reset();
        }
        self.last = None;
        self.active = 0;
    }

    pub fn update(
        &mut self,
        control: RegionControl,
        rate: f32,
        pitch: Option<f32>,
        alpha: f32,
        step: f32,
        reductions: &[Reduction; REDUCTIONS],
    ) {
        let pitch = pitch.filter(|_| control.harmonic.enabled);
        let mut pending = self.active;
        if self.last != Some((control, rate, pitch)) {
            if self.last.is_some_and(|(_, old_rate, _)| old_rate != rate) {
                for filter in &mut self.filters {
                    filter.invalidate();
                }
            }
            for (index, (target, next)) in self
                .targets
                .iter_mut()
                .zip(control.filter_plan(rate, pitch))
                .enumerate()
            {
                if next.enabled {
                    *target = next;
                    pending |= 1 << index;
                } else {
                    target.enabled = false;
                }
            }
            self.last = Some((control, rate, pitch));
        }
        for (slot, cut) in reductions.iter().enumerate() {
            let index = BASE_FILTERS + slot;
            let target = &mut self.targets[index];
            if cut.hz > 0.0 {
                let next = cut.band(control);
                let same_cut = target.frequency == next.frequency && target.channel == next.channel;
                if next.enabled && (same_cut || !self.filters[index].is_processing()) {
                    *target = next;
                    pending |= 1 << index;
                    continue;
                }
            }
            target.enabled = false;
        }
        self.active = 0;
        while pending != 0 {
            let index = pending.trailing_zeros() as usize;
            pending &= pending - 1;
            let filter = &mut self.filters[index];
            filter.update(self.targets[index], rate, alpha, step);
            if filter.is_processing() {
                self.active |= 1 << index;
            }
        }
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_mask(self.active, left, right);
    }

    /// Capture only what the resonance filters remove, after the region and harmonic EQ.
    pub(crate) fn process_with_delta(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        delta_l: &mut [f32],
        delta_r: &mut [f32],
    ) {
        const BASE_MASK: u32 = (1 << BASE_FILTERS) - 1;
        self.process_mask(self.active & BASE_MASK, left, right);
        delta_l.copy_from_slice(left);
        delta_r.copy_from_slice(right);
        self.process_mask(self.active & !BASE_MASK, left, right);
        for (delta, processed) in delta_l.iter_mut().zip(left).chain(delta_r.iter_mut().zip(right)) {
            *delta -= *processed;
        }
    }

    fn process_mask(&mut self, mut active: u32, left: &mut [f32], right: &mut [f32]) {
        while active != 0 {
            let index = active.trailing_zeros() as usize;
            active &= active - 1;
            self.filters[index].process(left, right, self.targets[index].channel);
        }
    }
}

#[derive(Params)]
pub struct RegionParams {
    #[id = "region_enabled"]
    pub enabled: BoolParam,
    #[id = "region_bypass"]
    pub bypass: BoolParam,
    #[id = "region_low"]
    pub low: FloatParam,
    #[id = "region_high"]
    pub high: FloatParam,
    #[id = "region_softness"]
    pub softness: FloatParam,
    #[id = "region_gain"]
    pub gain: FloatParam,
    #[nested(group = "Resonance")]
    pub resonance: Box<resonance::ResonanceParams>,
    #[nested(group = "Harmonic")]
    pub harmonic: Box<harmonic::HarmonicParams>,
}

impl RegionParams {
    pub fn new(index: usize) -> Self {
        let prefix = format!("Spectral Region {}", index + 1);
        let defaults = RegionControl::default();
        let frequency = |label, default| {
            FloatParam::new(
                format!("{prefix} {label}"),
                default,
                FloatRange::Logarithmic {
                    min: crate::analyzer::MIN_HZ,
                    max: FREQUENCY_MAX_HZ,
                },
            )
            .with_unit(" Hz")
            .with_step_size(0.01)
        };
        Self {
            enabled: BoolParam::new(format!("{prefix} Enabled"), defaults.enabled),
            bypass: BoolParam::new(format!("{prefix} Bypass"), defaults.bypass),
            low: frequency("Range Start", defaults.low),
            high: frequency("Range End", defaults.high),
            softness: FloatParam::new(
                format!("{prefix} Edge Softness"),
                defaults.softness,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_unit("%"),
            gain: FloatParam::new(
                format!("{prefix} Gain"),
                defaults.gain,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            resonance: Box::new(resonance::ResonanceParams::new(&prefix)),
            harmonic: Box::new(harmonic::HarmonicParams::new(&prefix)),
        }
    }

    pub fn control(&self) -> RegionControl {
        RegionControl {
            enabled: self.enabled.value(),
            bypass: self.bypass.value(),
            low: self.low.value(),
            high: self.high.value(),
            softness: self.softness.value(),
            gain: self.gain.value(),
            harmonic: self.harmonic.control(),
            resonance: self.resonance.control(),
        }
    }
}
