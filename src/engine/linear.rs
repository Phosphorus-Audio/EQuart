use std::ops::Range;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

static NEXT_KERNEL_REVISION: AtomicU64 = AtomicU64::new(1);

use realfft::{ComplexToReal, RealFftPlanner, num_complex::Complex64};

use super::OutputMix;
use super::convolution::{Convolution, HOP, Kernel, Plans};
use crate::band::{BAND_COUNT, BandControl, ChannelMode, SECTIONS};
use crate::filter::Coefficients;
use crate::precision::Sample;
use crate::processing_mode::LinearQuality;
use crate::region::{FILTERS, REGION_COUNT};

mod worker;
use worker::Worker;

const CONTROL_HISTORY: usize = LinearQuality::MAX_CENTER / HOP + 1;
const QUADRATURE_BINS: usize = 32;
const CAPACITY: usize = BAND_COUNT + REGION_COUNT * FILTERS;

pub(super) const fn latency(center: usize) -> usize {
    center + HOP
}

pub(super) const fn tail(center: usize) -> u32 {
    (2 * center + HOP) as u32
}

struct Design {
    inverse: Arc<dyn ComplexToReal<f64>>,
    warped: Vec<f64>,
    window: Vec<f64>,
    kaiser: Vec<f64>,
    average_correction: Vec<f64>,
    nodes: [[f64; 8]; QUADRATURE_BINS],
}

impl Design {
    fn new(planner: &mut RealFftPlanner<f64>, center: usize) -> Self {
        let fft = 4 * center;
        let warped = (0..=fft / 2)
            .map(|bin| (std::f64::consts::PI * bin as f64 / fft as f64).tan())
            .collect();

        let window = (0..=center)
            .map(|i| {
                // taper the ends while preserving more of a narrow resonance
                let edge = (4.0 * i as f64 / center as f64 - 3.0).max(0.0);
                0.5 + 0.5 * (std::f64::consts::PI * edge).cos()
            })
            .collect();

        let average_correction = (0..=center)
            .map(|i| {
                let angle = std::f64::consts::PI * i as f64 / fft as f64;
                if i == 0 { 1.0 } else { angle / angle.sin() }
            })
            .collect();
        let denominator = bessel_i0(9.62);
        let kaiser = (0..=center)
            .map(|i| bessel_i0(9.62 * (1.0 - (i as f64 / center as f64).powi(2)).sqrt()) / denominator)
            .collect();
        let nodes = std::array::from_fn(|bin| {
            GAUSS_NODES.map(|node| {
                (std::f64::consts::PI * (bin as f64 + 0.5 * node) / fft as f64)
                    .tan()
                    .abs()
            })
        });

        Self {
            average_correction,
            nodes,
            inverse: planner.plan_fft_inverse(fft),
            warped,
            window,
            kaiser,
        }
    }
}

fn bessel_i0(x: f64) -> f64 {
    let mut term = 1.0;
    let mut sum = term;

    for i in 1..=24 {
        term *= (x / (2 * i) as f64).powi(2);
        sum += term;
    }

    sum
}

// nodes and weights for integrating one frequency cell
const GAUSS_NODES: [f64; 8] = [
    -0.9602898564975363,
    -0.7966664774136267,
    -0.525532409916329,
    -0.1834346424956498,
    0.1834346424956498,
    0.525532409916329,
    0.7966664774136267,
    0.9602898564975363,
];
const GAUSS_WEIGHTS: [f64; 8] = [
    0.0506142681451881,
    0.1111905172266872,
    0.1568533229389437,
    0.181341891689181,
    0.181341891689181,
    0.1568533229389437,
    0.1111905172266872,
    0.0506142681451881,
];

struct SharedPlans {
    convolution: Plans,
    designs: [[Design; LinearQuality::ALL.len()]; 2],
}

type Matrix = [f64; 4];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0];

fn cascade(matrix: Matrix, route: ChannelMode, gain: f64) -> Matrix {
    let [ll, lr, rl, rr] = matrix;

    match route {
        ChannelMode::Stereo => matrix.map(|tap| tap * gain),
        ChannelMode::Left => [ll * gain, lr * gain, rl, rr],
        ChannelMode::Right => [ll, lr, rl * gain, rr * gain],
        ChannelMode::Mid | ChannelMode::Side => {
            let same = (gain + 1.0) * 0.5;
            let cross = (gain - 1.0) * if route == ChannelMode::Mid { 0.5 } else { -0.5 };
            [
                same * ll + cross * rl,
                same * lr + cross * rr,
                cross * ll + same * rl,
                cross * lr + same * rr,
            ]
        }
    }
}

struct Stage {
    sections: [Coefficients<f64>; SECTIONS],
    count: usize,
    route: ChannelMode,
    brickwall: Option<(f64, bool)>,
}

impl Stage {
    fn magnitude(&self, warped: f64, bin: f64) -> f64 {
        if let Some((cutoff, highpass)) = self.brickwall {
            let lowpass = (cutoff - bin + 0.5).clamp(0.0, 1.0);
            return if highpass { 1.0 - lowpass } else { lowpass };
        }

        self.sections[..self.count].iter().fold(1.0, |gain, c| {
            let r = warped / c.g;
            let [dry, band, low] = c.mix;
            let real = 1.0 - r * r;
            let imag = c.k * r;
            let numerator = (dry * real + low).powi(2) + (dry * imag + band * r).powi(2);
            let denominator = real * real + imag * imag;
            gain * (numerator / denominator).sqrt()
        })
    }
}

struct Snapshot {
    bands: Vec<BandControl>,
    delta: Option<Range<usize>>,
}

struct Designer {
    plans: &'static SharedPlans,
    rate_index: usize,
    quality: LinearQuality,
    center: usize,
    scratch: Vec<Complex64>,
    spectra: [Vec<Complex64>; 8],
    work: Vec<f64>,
    taps: Vec<f64>,
    stages: Vec<Stage>,
    delta: Option<Range<usize>>,
    design_bin: Option<usize>,
    integrate: bool,
    brickwall: bool,
    stereo: bool,
}

pub(super) struct LinearEq {
    plans: &'static SharedPlans,
    quality: LinearQuality,
    rate_index: usize,
    center: usize,
    convolution: Box<Convolution>,
    designer: Designer,
    configured: Vec<BandControl>,
    rate: f32,
    delta: Option<Range<usize>>,
    worker: Option<Worker>,
    generation: u64,
    async_design: bool,
    ready: bool,
    current: Kernel,
    revision: u64,
    next: Kernel,
    output: Box<[[f64; HOP]; 4]>,
    dry: Vec<[f64; 2]>,
    dry_pos: usize,
    bypass_mix: Option<f64>,
    snapshots: [Snapshot; CONTROL_HISTORY],
    snapshot_head: usize,
    snapshots_seen: usize,
    hop: usize,
    update_hops: usize,
    pos: usize,
}

impl LinearEq {
    pub(super) fn new() -> Self {
        static PLANS: OnceLock<SharedPlans> = OnceLock::new();
        let plans = PLANS.get_or_init(|| {
            let mut planner = RealFftPlanner::new();

            SharedPlans {
                convolution: Plans::new(&mut planner),
                designs: [1, 2].map(|scale| {
                    LinearQuality::ALL
                        .map(|quality| Design::new(&mut planner, scale * quality.center()))
                }),
            }
        });

        Self {
            plans,
            quality: LinearQuality::default(),
            rate_index: 0,
            center: LinearQuality::Standard.center(),
            convolution: Box::new(Convolution::new(&plans.convolution)),
            designer: Designer::new(plans),
            configured: Vec::with_capacity(CAPACITY),
            rate: 0.0,
            delta: None,
            worker: None,
            generation: 0,
            async_design: false,
            ready: false,
            current: Kernel::new(),
            revision: 0,
            next: Kernel::new(),
            output: Box::new([[0.0; HOP]; 4]),
            dry: vec![[0.0; 2]; latency(LinearQuality::MAX_CENTER)],
            dry_pos: 0,
            bypass_mix: None,
            snapshots: std::array::from_fn(|_| Snapshot {
                bands: Vec::with_capacity(CAPACITY),
                delta: None,
            }),
            snapshot_head: 0,
            snapshots_seen: 0,
            hop: 0,
            update_hops: 1,
            pos: 0,
        }
    }

    pub(super) fn start_worker(&mut self) -> std::io::Result<()> {
        self.worker = Some(Worker::new(self.plans)?);
        self.async_design = self.configured.iter().any(BandControl::brickwall);
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if self.designer.design_bin.take().is_some() || self.async_design {
            self.rate = 0.0;
        }
        self.convolution.reset();
        self.dry.fill([0.0; 2]);
        self.output.fill([0.0; HOP]);
        self.dry_pos = 0;
        self.bypass_mix = None;
        self.snapshot_head = 0;
        self.snapshots_seen = 0;
        self.hop = 0;
        self.pos = 0;
    }

    pub(super) fn response_revision(&self) -> u64 {
        if self.ready { self.revision } else { 0 }
    }

    pub(super) fn response(&self) -> Option<[Option<&[f64]>; 4]> {
        self.ready.then(|| {
            std::array::from_fn(|lane| {
                self.current.routes[lane]
                    .map(|route| &self.current.half_taps[route][..=self.center])
            })
        })
    }

    pub(super) fn set_quality(&mut self, quality: LinearQuality, rate: f32) {
        self.quality = quality;
        self.set_rate(rate);
    }

    pub(super) fn set_rate(&mut self, rate: f32) {
        self.rate = 0.0;
        self.ready = false;
        self.current.identity = true;
        self.center = self.quality.center_at(rate);
        self.rate_index = usize::from(self.center != self.quality.center());
        self.reset();
    }

    pub(super) fn at_boundary(&self) -> bool {
        self.pos == 0
    }

    pub(super) fn frames_until_hop(&self) -> usize {
        HOP - self.pos
    }

    pub(super) fn begin_snapshot(&mut self) -> &mut Vec<BandControl> {
        self.snapshot_head = (self.snapshot_head + 1) % CONTROL_HISTORY;
        self.snapshots_seen = (self.snapshots_seen + 1).min(CONTROL_HISTORY);

        let bands = &mut self.snapshots[self.snapshot_head].bands;
        bands.clear();
        bands
    }

    pub(super) fn finish_snapshot(&mut self, rate: f32, delta: Option<Range<usize>>) {
        self.snapshots[self.snapshot_head].delta = delta;

        if self.hop != 0 {
            return;
        }

        let mut sections = self.snapshots[self.snapshot_head]
            .bands
            .iter()
            .map(BandControl::section_count)
            .sum::<usize>();
        sections = sections.max(self.configured.iter().map(BandControl::section_count).sum());
        self.update_hops = self.design_period(sections);

        // align the control snapshot to the source near the middle of the crossfade
        let lookback = (self.center + HOP / 2).saturating_sub(self.update_hops * HOP) / HOP;
        let lookback = lookback.min(self.snapshots_seen - 1);
        let read = (self.snapshot_head + CONTROL_HISTORY - lookback) % CONTROL_HISTORY;
        let snapshot = &self.snapshots[read];

        if self.configured == snapshot.bands && self.rate == rate && self.delta == snapshot.delta {
            return;
        }

        let initial = self.snapshots_seen == 1 || self.rate == 0.0;
        let brickwall = snapshot.bands.iter().any(BandControl::brickwall);
        self.async_design |= brickwall && self.worker.is_some();

        if self.async_design
            && !self.worker.as_mut().unwrap().submit(
                &snapshot.bands,
                snapshot.delta.clone(),
                rate,
                self.quality,
                self.generation,
            )
        {
            return;
        }

        self.configured.clear();
        self.configured.extend_from_slice(&snapshot.bands);
        self.rate = rate;
        self.delta = snapshot.delta.clone();
        if self.async_design {
            self.designer.design_bin = None;
            return;
        }

        self.designer.configure(
            &self.configured, self.delta.clone(), rate, self.quality, &mut self.next,
        );

        if initial {
            self.designer.design(self.design_length());
            self.designer.partition(&mut self.convolution, &mut self.next);
            std::mem::swap(&mut self.current, &mut self.next);
            self.ready = true;
            self.revision = NEXT_KERNEL_REVISION.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn design_period(&self, sections: usize) -> usize {
        // budget more hops for larger spectra and cascades
        let hops = match self.quality {
            LinearQuality::Short | LinearQuality::Standard => 1 + usize::from(sections > 8),
            LinearQuality::High | LinearQuality::Extended => 4,
        };
        hops << self.rate_index
    }

    fn design_length(&self) -> usize {
        2 * self.center + 1
    }
}

impl Designer {
    fn new(plans: &'static SharedPlans) -> Self {
        let max_fft = 4 * LinearQuality::MAX_CENTER;
        let scratch_size = plans.designs.iter().flatten()
            .map(|design| design.inverse.get_scratch_len()).max().unwrap();

        Self {
            plans,
            rate_index: 0,
            quality: LinearQuality::default(),
            center: 0,
            scratch: vec![Complex64::default(); scratch_size],
            spectra: std::array::from_fn(|_| vec![Complex64::default(); max_fft / 2 + 1]),
            work: vec![0.0; max_fft],
            taps: vec![0.0; 2 * LinearQuality::MAX_CENTER + 1],
            stages: Vec::with_capacity(CAPACITY),
            delta: None,
            design_bin: None,
            integrate: false,
            brickwall: false,
            stereo: true,
        }
    }

    fn configure(
        &mut self,
        bands: &[BandControl],
        delta: Option<Range<usize>>,
        rate: f32,
        quality: LinearQuality,
        kernel: &mut Kernel,
    ) {
        self.quality = quality;
        self.center = quality.center_at(rate);
        self.rate_index = usize::from(self.center != quality.center());
        self.delta = delta;
        self.brickwall = bands.iter().any(BandControl::brickwall);
        self.stages.clear();

        for control in bands {
            let brickwall = control.brickwall().then(|| {
                let cutoff = f64::from(control.frequency.clamp(10.0, rate * 0.49))
                    * (4 * self.center) as f64 / f64::from(rate);
                (cutoff, control.kind == crate::filter::FilterKind::Highpass)
            });
            let (sections, count) = if brickwall.is_some() {
                ([Coefficients::identity(); SECTIONS], 0)
            } else {
                control.sections_for(rate)
            };
            self.stages.push(Stage {
                sections,
                count,
                route: control.channel,
                brickwall,
            });
        }

        self.stereo = self
            .stages
            .iter()
            .all(|stage| stage.route == ChannelMode::Stereo);
        let design = &self.plans.designs[self.rate_index][self.quality.index()];
        self.integrate = self.stages.iter().any(|stage| {
            stage.sections[..stage.count].iter().any(|section| {
                section.g < design.warped[QUADRATURE_BINS * 3 / 4]
                    && section.g * section.k < 2.0 * design.warped[1]
                    && section.mix != [1.0, 0.0, 0.0]
            })
        });
        let crossed = self
            .stages
            .iter()
            .any(|stage| matches!(stage.route, ChannelMode::Mid | ChannelMode::Side));
        let mut routes = if self.stereo {
            [Some(0), None, None, Some(0), Some(4), None, None, Some(4)]
        } else {
            [
                Some(0),
                crossed.then_some(1),
                crossed.then_some(2),
                Some(3),
                Some(4),
                crossed.then_some(5),
                crossed.then_some(6),
                Some(7),
            ]
        };

        if self.delta.is_none() {
            routes[4..].fill(None);
        }

        kernel.routes = routes;
        kernel.identity = false;
        kernel.last_tap = self.brickwall;
        self.design_bin = Some(0);
    }

    fn magnitude(&self, warped: f64, bin: f64) -> [f64; 8] {
        if self.stereo && self.delta.is_none() {
            let gain = self
                .stages
                .iter()
                .map(|stage| stage.magnitude(warped, bin))
                .product();
            return [gain, 0.0, 0.0, gain, 0.0, 0.0, 0.0, 0.0];
        }

        let mut matrix = IDENTITY;
        let mut before = IDENTITY;
        let mut removed = [0.0; 4];

        for (i, stage) in self.stages.iter().enumerate() {
            if self.delta.as_ref().is_some_and(|range| range.start == i) {
                before = matrix;
            }

            matrix = cascade(matrix, stage.route, stage.magnitude(warped, bin));

            if self.delta.as_ref().is_some_and(|range| range.end == i + 1) {
                removed = std::array::from_fn(|j| before[j] - matrix[j]);
            }
        }

        let [ll, lr, rl, rr] = matrix;
        let [dl, d_lr, d_rl, dr] = removed;
        [ll, lr, rl, rr, dl, d_lr, d_rl, dr]
    }

    fn design(&mut self, end: usize) {
        let Some(start) = self.design_bin else { return };
        let design = &self.plans.designs[self.rate_index][self.quality.index()];

        for (bin, &warped) in design.warped.iter().enumerate().take(end).skip(start) {
            let mut gain = self.magnitude(warped, bin as f64);
            if self.integrate
                && let Some(nodes) = design.nodes.get(bin)
            {
                let mut average = [0.0; 8];
                for ((&node, weight), offset) in nodes.iter().zip(GAUSS_WEIGHTS).zip(GAUSS_NODES) {
                    let coordinate = (bin as f64 + 0.5 * offset).abs();
                    for (sum, value) in average.iter_mut().zip(self.magnitude(node, coordinate)) {
                        *sum += weight * value;
                    }
                }

                // transition to the point-sampled spectrum once the low bins are covered
                let weight = (((QUADRATURE_BINS - bin) as f64) / 8.0).min(1.0);
                for (value, mean) in gain.iter_mut().zip(average) {
                    *value += weight * (mean - *value);
                }
            }

            if self.stereo && self.delta.is_none() {
                self.spectra[0][bin] = Complex64::new(gain[0], 0.0);
            } else {
                for (lane, gain) in gain.into_iter().enumerate() {
                    self.spectra[lane][bin] = Complex64::new(gain, 0.0);
                }
            }
        }

        self.design_bin = Some(end);
    }

    fn partition(&mut self, convolution: &mut Convolution, kernel: &mut Kernel) {
        #[cfg(feature = "analyzer-profile")]
        let _timer = crate::analyzer::profile::Timer::new(crate::analyzer::profile::FIR_PARTITION);
        let design = &self.plans.designs[self.rate_index][self.quality.index()];
        let fft = design.inverse.len();
        let center = self.center;
        let window = if self.brickwall { &design.kaiser } else { &design.window };

        for (lane, spectrum) in self.spectra.iter_mut().enumerate() {
            if !kernel.routes.contains(&Some(lane)) {
                continue;
            }

            design
                .inverse
                .process_with_scratch(
                    &mut spectrum[..fft / 2 + 1],
                    &mut self.work[..fft],
                    &mut self.scratch,
                )
                .unwrap();
            self.taps[center] = self.work[0] / fft as f64;

            for lag in 1..=center {
                let tap =
                    (self.work[lag] + self.work[fft - lag]) * window[lag] / (2 * fft) as f64;
                let tap = if self.integrate {
                    tap * design.average_correction[lag]
                } else {
                    tap
                };
                self.taps[center - lag] = tap;
                self.taps[center + lag] = tap;
            }

            if lane < 4 {
                kernel.half_taps[lane][..=center]
                    .copy_from_slice(&self.taps[center..=2 * center]);
            }

            let len = 2 * center + usize::from(kernel.last_tap);
            for (part, taps) in self.taps[..len].chunks(HOP).enumerate() {
                convolution.partition(kernel, lane, part, taps);
            }
        }

        self.design_bin = None;
    }
}

impl LinearEq {
    pub(super) fn process<T: Sample>(
        &mut self,
        left: &mut [T],
        right: &mut [T],
        output: OutputMix,
        gain: f64,
        delta_mix: &mut f32,
        dynamics: &mut crate::region::dynamics::Rack,
    ) {
        if self.designer.design_bin.is_some() {
            let design_frames = (self.update_hops - 1).max(1) * HOP;
            let elapsed = (self.hop * HOP + self.pos + left.len()).min(design_frames);
            self.designer.design(elapsed * self.design_length() / design_frames);
        }

        let latency = latency(self.center);
        let target_bypass = f64::from(output.bypass);
        let bypass_mix = self.bypass_mix.get_or_insert(target_bypass);

        #[cfg(feature = "analyzer-profile")]
        let dynamics_timer = dynamics
            .active
            .then(|| crate::analyzer::profile::Timer::new(crate::analyzer::profile::REGION_MASK));

        for (i, (left, right)) in left.iter_mut().zip(right).enumerate() {
            let input = [left.double(), right.double()];
            let dry = std::mem::replace(&mut self.dry[self.dry_pos], input);
            self.dry_pos = (self.dry_pos + 1) % latency;
            self.convolution.input(self.pos, input);

            *bypass_mix +=
                (target_bypass - *bypass_mix).clamp(-output.bypass_step, output.bypass_step);

            if let Some((_, step)) = output.delta {
                *delta_mix = (*delta_mix + step).clamp(0.0, 1.0);
            }

            let delta = f64::from(*delta_mix) * (1.0 - *bypass_mix);
            let wet = f64::from(output.wet) * (1.0 - *bypass_mix);
            let filtered = dynamics.apply(i, [self.output[0][self.pos], self.output[1][self.pos]]);

            for (ch, sample) in [left, right].into_iter().enumerate() {
                let mixed = if wet == 0.0 {
                    dry[ch]
                } else {
                    dry[ch] * (1.0 - wet) + filtered[ch] * gain * wet
                };

                *sample = T::value(mixed * (1.0 - delta) + self.output[ch + 2][self.pos] * delta);
            }

            self.pos += 1;
        }

        #[cfg(feature = "analyzer-profile")]
        drop(dynamics_timer);

        if self.pos == HOP {
            self.finish_hop();
            self.pos = 0;
        }
    }

    fn finish_hop(&mut self) {
        let update_hops = self.update_hops;
        let mut complete = self.designer.design_bin.is_some() && self.hop == update_hops - 1;

        if complete {
            self.designer.partition(&mut self.convolution, &mut self.next);
        } else if let Some(worker) = &mut self.worker {
            complete = worker.receive(self.generation, &mut self.next);
        }

        self.convolution.push();
        self.convolution.render(
            &self.current,
            complete.then_some(&self.next),
            (self.design_length() - 1) / HOP,
            &mut self.output,
        );

        if complete {
            std::mem::swap(&mut self.current, &mut self.next);
            self.ready = true;
            self.revision = NEXT_KERNEL_REVISION.fetch_add(1, Ordering::Relaxed);
        }

        self.hop = (self.hop + 1) % update_hops;
    }
}
