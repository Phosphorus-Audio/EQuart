mod reduction;

use super::{REGION_COUNT, RegionControl};
use crate::band::ChannelMode;
use nice_plug::prelude::*;
use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex32};
use std::{f32::consts::TAU, sync::Arc};

pub(crate) use reduction::{REDUCTIONS, Reduction};

const WINDOW: usize = 2048;
const HOP: usize = WINDOW / 4;
const PEAKS: usize = 4;

#[derive(Clone, Copy, PartialEq)]
pub struct ResonanceControl {
    pub enabled: bool,
    pub depth: f32,
    pub threshold: f32,
    pub q: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub ratio: f32,
    pub smooth: f32,
    pub stereo_link: f32,
    pub bias: f32,
}

impl Default for ResonanceControl {
    fn default() -> Self {
        Self {
            enabled: false,
            depth: 6.0,
            threshold: 4.5,
            q: 10.0,
            attack_ms: 12.0,
            release_ms: 80.0,
            ratio: 75.0,
            smooth: 35.0,
            stereo_link: 100.0,
            bias: 0.0,
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
    #[id = "res_attack"]
    pub attack: FloatParam,
    #[id = "res_release"]
    pub release: FloatParam,
    #[id = "res_ratio"]
    pub ratio: FloatParam,
    #[id = "res_smooth"]
    pub smooth: FloatParam,
    #[id = "res_link"]
    pub stereo_link: FloatParam,
    #[id = "res_bias"]
    pub bias: FloatParam,
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
        let pct = |label, value, min, max| {
            FloatParam::new(name(label), value, FloatRange::Linear { min, max })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_rounded(0))
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
            attack: FloatParam::new(
                name("Attack"),
                defaults.attack_ms,
                FloatRange::Logarithmic {
                    min: 0.5,
                    max: 100.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            release: FloatParam::new(
                name("Release"),
                defaults.release_ms,
                FloatRange::Logarithmic {
                    min: 5.0,
                    max: 500.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            ratio: pct("Ratio", defaults.ratio, 0.0, 200.0),
            smooth: pct("Smooth", defaults.smooth, 0.0, 100.0),
            stereo_link: pct("Stereo Link", defaults.stereo_link, 0.0, 100.0),
            bias: pct("Bias", defaults.bias, -100.0, 100.0),
        }
    }

    pub(super) fn control(&self) -> ResonanceControl {
        ResonanceControl {
            enabled: self.enabled.value() && !self.bypassed.value(),
            threshold: self.threshold.value(),
            depth: self.depth.value(),
            q: self.q.value(),
            attack_ms: self.attack.value(),
            release_ms: self.release.value(),
            ratio: self.ratio.value(),
            smooth: self.smooth.value(),
            stereo_link: self.stereo_link.value(),
            bias: self.bias.value(),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Candidate {
    hz: f32,
    prominence_db: f32,
    channel: ChannelMode,
}

pub(crate) struct Detector {
    fft: Arc<dyn RealToComplex<f32>>,
    ring: Box<[[f32; 2]]>,
    window: Box<[f32]>,
    input: Vec<f32>,
    output: Vec<Complex32>,
    scratch: Vec<Complex32>,
    frame_power: [Vec<f32>; 2],
    power: [Vec<f32>; 2],
    db: [Vec<f32>; 3],
    sum_db: [Vec<f64>; 3],
    cursor: usize,
    remaining: usize,
    reductions: [[Reduction; REDUCTIONS]; REGION_COUNT],
}

impl Detector {
    pub(crate) fn new() -> Self {
        let fft = RealFftPlanner::new().plan_fft_forward(WINDOW);
        let bins = WINDOW / 2 + 1;
        Self {
            ring: vec![[0.0; 2]; WINDOW].into_boxed_slice(),
            window: (0..WINDOW)
                .map(|i| 0.5 - 0.5 * (TAU * i as f32 / WINDOW as f32).cos())
                .collect(),
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            frame_power: [vec![0.0; bins], vec![0.0; bins]],
            power: [vec![0.0; bins], vec![0.0; bins]],
            db: [vec![0.0; bins], vec![0.0; bins], vec![0.0; bins]],
            sum_db: [vec![0.0; bins + 1], vec![0.0; bins + 1], vec![0.0; bins + 1]],
            cursor: 0,
            remaining: WINDOW,
            reductions: Default::default(),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.cursor = 0;
        self.remaining = WINDOW;
        self.power.iter_mut().for_each(|p| p.fill(0.0));
        self.reductions = Default::default();
    }

    pub(crate) fn reductions(&self) -> &[[Reduction; REDUCTIONS]; REGION_COUNT] {
        &self.reductions
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
        for ch in 0..2 {
            self.frame_power[ch].fill(0.0);
            for (i, sample) in self.input.iter_mut().enumerate() {
                *sample = self.ring[(self.cursor + i) % WINDOW][ch] * self.window[i];
            }
            self.fft
                .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
                .expect("resonance FFT buffers match the plan");
            for (power, bin) in self.frame_power[ch].iter_mut().zip(&self.output) {
                *power = bin.norm_sqr();
            }
        }

        // Fast base envelope to remove single-frame FFT jitter before dB conversion.
        let base_att = 1.0 - (-(HOP as f32) / (0.002 * rate)).exp();
        let base_rel = 1.0 - (-(HOP as f32) / (0.015 * rate)).exp();
        let floor = (WINDOW as f32 * 0.25).powi(2) * 1e-9;
        let floor_db = 10.0 * floor.log10();

        for i in 0..self.power[0].len() {
            for ch in 0..2 {
                let frame = self.frame_power[ch][i];
                let power = &mut self.power[ch][i];
                let alpha = if frame > *power { base_att } else { base_rel };
                *power += alpha * (frame - *power);
                let db = 10.0 * (*power).max(floor).log10();
                self.db[ch][i] = db;
                self.sum_db[ch][i + 1] = self.sum_db[ch][i] + f64::from(db);
            }
            let mid_power = 0.5 * (self.power[0][i] + self.power[1][i]);
            let mid_db = 10.0 * mid_power.max(floor).log10();
            self.db[2][i] = mid_db;
            self.sum_db[2][i + 1] = self.sum_db[2][i] + f64::from(mid_db);
        }

        let mut candidates: [[Candidate; PEAKS]; REGION_COUNT] = Default::default();
        let bin_hz = rate / WINDOW as f32;

        // scan union of active regions once
        let mut intervals = [(0, 0); REGION_COUNT];
        let mut count = 0;
        for region in regions {
            let [low, high] = region.bounds();
            if region.enabled
                && !region.bypass
                && region.resonance.enabled
                && region.resonance.depth > 0.0
                && low < high
            {
                let first = ((low / bin_hz - 0.5).ceil() as usize).max(1);
                let end =
                    ((high / bin_hz + 0.5).floor() as usize + 1).min(self.db[2].len() - 1);
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

        let local_excess = |sum_db: &[f64], db_slice: &[f32], i: usize, span: usize| -> f32 {
            let lo = i.saturating_sub(span);
            let hi = (i + span + 1).min(db_slice.len());
            let a = i.saturating_sub(2);
            let b = (i + 3).min(hi);
            let side_count = a - lo + hi - b;
            if side_count == 0 {
                return 0.0;
            }
            let bg_db = ((sum_db[a] - sum_db[lo]) + (sum_db[hi] - sum_db[b])) as f32
                / side_count as f32;
            (db_slice[i] - bg_db).min(db_slice[i] - floor_db).max(0.0)
        };

        for i in bins {
            // check local maximum on stereo mid or either channel
            let mid_peak = self.db[2][i] > self.db[2][i - 1] && self.db[2][i] >= self.db[2][i + 1];
            let l_peak = self.db[0][i] > self.db[0][i - 1] && self.db[0][i] >= self.db[0][i + 1];
            let r_peak = self.db[1][i] > self.db[1][i - 1] && self.db[1][i] >= self.db[1][i + 1];
            if !mid_peak && !l_peak && !r_peak {
                continue;
            }

            let [l, c, r] = [self.db[2][i - 1], self.db[2][i], self.db[2][i + 1]];
            let offset = if 2.0 * c > l + r {
                0.5 * (r - l) / (2.0 * c - l - r)
            } else {
                0.0
            };
            let hz = (i as f32 + offset) * bin_hz;
            let octaves = (hz / 1_000.0).max(0.01).log2();
            let min_gap = (hz * 0.08).max(bin_hz * 2.5);

            for (slots, region) in candidates.iter_mut().zip(regions) {
                let res = region.resonance;
                let [low, high] = region.bounds();
                if !region.enabled
                    || region.bypass
                    || !res.enabled
                    || res.depth == 0.0
                    || hz < low
                    || hz > high
                    || low == high
                {
                    continue;
                }

                let smooth_scale = 0.5 + (res.smooth * 0.01);
                let span = ((i as f32 * 0.15 * smooth_scale).round() as usize).max(5);

                let link = res.stereo_link * 0.01;
                let ex_m = local_excess(&self.sum_db[2], &self.db[2], i, span);
                let (excess_raw, channel) = if link >= 0.99 {
                    (ex_m, ChannelMode::Stereo)
                } else {
                    let ex_l = local_excess(&self.sum_db[0], &self.db[0], i, span);
                    let ex_r = local_excess(&self.sum_db[1], &self.db[1], i, span);
                    let ex = link * ex_m + (1.0 - link) * ex_l.max(ex_r);
                    let ch = if link < 0.5 && (ex_l - ex_r).abs() > 3.0 * (1.0 - link) {
                        if ex_l > ex_r {
                            ChannelMode::Left
                        } else {
                            ChannelMode::Right
                        }
                    } else {
                        ChannelMode::Stereo
                    };
                    (ex, ch)
                };

                let prominence_db = (excess_raw + octaves * (res.bias * 0.01) * 3.0).max(0.0);
                if prominence_db <= res.threshold {
                    continue;
                }
                let candidate = Candidate {
                    hz,
                    prominence_db,
                    channel,
                };

                if let Some(near) = slots
                    .iter()
                    .position(|p| p.hz > 0.0 && (p.hz - hz).abs() < min_gap)
                {
                    if prominence_db > slots[near].prominence_db {
                        slots[near] = candidate;
                        slots.sort_by(|a, b| b.prominence_db.total_cmp(&a.prominence_db));
                    }
                    continue;
                }

                if let Some(at) = slots
                    .iter()
                    .position(|peak| prominence_db > peak.prominence_db)
                {
                    slots[at..].rotate_right(1);
                    slots[at] = candidate;
                }
            }
        }

        for ((cuts, ranked), region) in self.reductions.iter_mut().zip(candidates).zip(regions) {
            if region.enabled && !region.bypass && region.resonance.enabled {
                reduction::update(cuts, ranked, region.resonance, HOP as f32 / rate);
            } else {
                *cuts = Default::default();
            }
        }
    }
}
