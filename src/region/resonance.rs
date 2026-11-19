use super::{REGION_COUNT, RegionControl};
use crate::band::BandControl;
use crate::filter::CutSlope;
use nice_plug::prelude::*;
use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex32};
use std::{f32::consts::TAU, sync::Arc};

const WINDOW: usize = 2048;
const HOP: usize = WINDOW / 4;
pub(crate) const PEAKS: usize = 4;

#[derive(Clone, Copy, PartialEq)]
pub struct ResonanceControl {
    pub enabled: bool,
    pub depth: f32,
    pub threshold: f32,
    pub q: f32,
}

impl Default for ResonanceControl {
    fn default() -> Self {
        Self {
            enabled: false,
            depth: 6.0,
            threshold: 8.0,
            q: 10.0,
        }
    }
}

#[derive(Params)]
pub struct ResonanceParams {
    #[id = "res_enabled"]
    pub enabled: BoolParam,
    #[id = "res_bypassed"]
    pub bypassed: BoolParam,
    #[id = "res_depth"]
    pub depth: FloatParam,
    #[id = "res_threshold"]
    pub threshold: FloatParam,
    #[id = "res_q"]
    pub q: FloatParam,
}

impl ResonanceParams {
    pub(super) fn new(prefix: &str) -> Self {
        let defaults = ResonanceControl::default();
        let name = |label| format!("{prefix} Resonance {label}");
        let db = |label, value| {
            FloatParam::new(
                name(label),
                value,
                FloatRange::Linear {
                    min: 0.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
        };
        Self {
            enabled: BoolParam::new(name("Enabled"), false),
            bypassed: BoolParam::new(name("Bypassed"), false),
            depth: db("Depth", defaults.depth),
            threshold: db("Prominence", defaults.threshold),
            q: FloatParam::new(
                name("Q"),
                defaults.q,
                FloatRange::Logarithmic {
                    min: 2.0,
                    max: 32.0,
                },
            ),
        }
    }

    pub(super) fn control(&self) -> ResonanceControl {
        ResonanceControl {
            enabled: self.enabled.value() && !self.bypassed.value(),
            threshold: self.threshold.value(),
            depth: self.depth.value(),
            q: self.q.value(),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Peak {
    pub hz: f32,
    pub excess: f32,
}

impl Peak {
    pub(crate) fn band(self, region: RegionControl) -> BandControl {
        let res = region.resonance;
        let [low, high] = region.bounds();
        let cut = (self.excess - res.threshold).clamp(0.0, res.depth);
        BandControl {
            enabled: region.enabled
                && !region.bypass
                && res.enabled
                && cut > 0.0
                && (low..=high).contains(&self.hz),
            frequency: self.hz,
            gain: -cut,
            q: res.q,
            slope: CutSlope::Db12,
            ..BandControl::default()
        }
    }
}

pub(crate) struct Detector {
    fft: Arc<dyn RealToComplex<f32>>,
    ring: Box<[[f32; 2]]>,
    window: Box<[f32]>,
    input: Vec<f32>,
    output: Vec<Complex32>,
    scratch: Vec<Complex32>,
    frame_power: Vec<f32>,
    power: Vec<f32>,
    sum: Vec<f64>,
    cursor: usize,
    remaining: usize,
    pub peaks: [[Peak; PEAKS]; REGION_COUNT],
}

impl Detector {
    pub(crate) fn new() -> Self {
        let fft = RealFftPlanner::new().plan_fft_forward(WINDOW);
        Self {
            ring: vec![[0.0; 2]; WINDOW].into_boxed_slice(),
            window: (0..WINDOW)
                .map(|i| 0.5 - 0.5 * (TAU * i as f32 / WINDOW as f32).cos())
                .collect(),
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            frame_power: vec![0.0; WINDOW / 2 + 1],
            power: vec![0.0; WINDOW / 2 + 1],
            sum: vec![0.0; WINDOW / 2 + 2],
            cursor: 0,
            remaining: WINDOW,
            peaks: Default::default(),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.cursor = 0;
        self.remaining = WINDOW;
        self.power.fill(0.0);
        self.peaks = Default::default();
    }

    pub(crate) fn push(
        &mut self,
        left: &[f32],
        right: &[f32],
        rate: f32,
        regions: &[RegionControl; REGION_COUNT],
    ) {
        for (&l, &r) in left.iter().zip(right) {
            self.ring[self.cursor] = [l, r];
            self.cursor = (self.cursor + 1) % WINDOW;
            self.remaining -= 1;
            if self.remaining == 0 {
                self.analyze(rate, regions);
                self.remaining = HOP;
            }
        }
    }

    fn analyze(&mut self, rate: f32, regions: &[RegionControl; REGION_COUNT]) {
        self.frame_power.fill(0.0);
        for channel in 0..2 {
            for (i, sample) in self.input.iter_mut().enumerate() {
                *sample = self.ring[(self.cursor + i) % WINDOW][channel] * self.window[i];
            }
            self.fft
                .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
                .expect("resonance FFT buffers match the plan");
            for (power, bin) in self.frame_power.iter_mut().zip(&self.output) {
                *power += bin.norm_sqr();
            }
        }
        let attack = 1.0 - (-(HOP as f32) / (0.012 * rate)).exp();
        let release = 1.0 - (-(HOP as f32) / (0.055 * rate)).exp();
        for (i, (power, &frame)) in self.power.iter_mut().zip(&self.frame_power).enumerate() {
            let alpha = if frame > *power { attack } else { release };
            *power += alpha * (frame - *power);
            self.sum[i + 1] = self.sum[i] + f64::from(*power);
        }
        let previous = self.peaks;
        self.peaks = Default::default();
        let bin_hz = rate / WINDOW as f32;
        // scan union of regions once even if they overlap
        let mut intervals = [(0, 0); REGION_COUNT];
        let mut count = 0;
        for region in regions {
            let [low, high] = region.bounds();
            if region.enabled && !region.bypass && region.resonance.enabled
                && region.resonance.depth > 0.0 && low < high {
                let first = ((low / bin_hz - 0.5).ceil() as usize).max(1);
                let end = ((high / bin_hz + 0.5).floor() as usize + 1).min(self.power.len() - 1);
                if first < end {
                    intervals[count] = (first, end);
                    count += 1;
                }
            }
        }
        let ranges = &mut intervals[..count];
        ranges.sort_unstable();
        let mut scanned = 0;
        let bins = ranges.iter().flat_map(|&(first, end)| {
            let start = first.max(scanned);
            scanned = scanned.max(end);
            start..end
        });
        for i in bins {
            let [left, center, right] = [self.power[i - 1], self.power[i], self.power[i + 1]];
            if center <= left || center < right {
                continue;
            }
            // skip Hann main lobe, compare vs sideband mean (floor -90 dBFS)
            let span = (i / 5).max(6);
            let lo = i.saturating_sub(span);
            let hi = (i + span + 1).min(self.power.len());
            let a = i.saturating_sub(2);
            let b = (i + 3).min(hi);
            let background = ((self.sum[a] - self.sum[lo]) + (self.sum[hi] - self.sum[b]))
                / (a - lo + hi - b) as f64;
            let floor = (WINDOW as f32 * 0.25).powi(2) * 1e-9;
            let contrast = center / (background as f32).max(floor);
            let excess = 10.0 * contrast.min(self.frame_power[i] / floor).log10();
            let [l, c, r] = [left, center, right].map(|p| p.max(f32::MIN_POSITIVE).ln());
            let offset = if 2.0 * c > l + r {
                0.5 * (r - l) / (2.0 * c - l - r)
            } else {
                0.0
            };
            let hz = (i as f32 + offset) * bin_hz;
            for (slots, region) in self.peaks.iter_mut().zip(regions) {
                let [low, high] = region.bounds();
                if !region.enabled
                    || region.bypass
                    || !region.resonance.enabled
                    || region.resonance.depth == 0.0
                    || excess <= region.resonance.threshold
                    || hz < low
                    || hz > high
                    || low == high
                {
                    continue;
                }
                if let Some(at) = slots.iter().position(|peak| excess > peak.excess) {
                    slots[at..].rotate_right(1);
                    slots[at] = Peak { hz, excess };
                }
            }
        }
        // Keep nearby peaks on the same biquad when their strength order swaps.
        for (slots, old) in self.peaks.iter_mut().zip(previous) {
            let ranked = *slots;
            *slots = Default::default();
            let mut matched = [false; PEAKS];
            for (i, peak) in ranked.iter().enumerate().filter(|(_, p)| p.hz > 0.0) {
                let nearest = (0..PEAKS)
                    .filter(|&j| slots[j].hz == 0.0 && old[j].hz > 0.0)
                    .map(|j| (j, (peak.hz - old[j].hz).abs()))
                    .filter(|&(_, distance)| distance <= (peak.hz * 0.1).max(bin_hz))
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((j, _)) = nearest {
                    slots[j] = *peak;
                    matched[i] = true;
                }
            }
            let mut free = slots.iter_mut().filter(|p| p.hz == 0.0);
            for (peak, _) in ranked
                .into_iter()
                .zip(matched)
                .filter(|(p, used)| p.hz > 0.0 && !used)
            {
                *free.next().expect("one filter slot per selected peak") = peak;
            }
        }
    }
}
