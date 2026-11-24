use super::super::{REGION_COUNT, RegionControl};
use super::{Control, Detector, FLOOR};
use crate::band::COEFF_FRAMES;
use crate::dsp::{State, step};
use crate::dynamics::DetectorInput;
use crate::filter::Coefficients;
use crate::precision::Sample;
use mask::{Gains, Mask, STEP, Shape};

use super::mask;

const MAX_DELAY: usize = crate::processing_mode::LinearQuality::MAX_CENTER + 256;
const NO_GAIN: Gains = [[0.0; 2]; REGION_COUNT];

#[derive(Clone, Copy, Debug)]
pub(crate) struct Meter {
    pub input: f32,
    pub output: f32,
    pub reduction: f32,
    pub external_missing: bool,
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            input: FLOOR,
            output: FLOOR,
            reduction: 0.0,
            external_missing: false,
        }
    }
}

#[derive(Default)]
struct BandDetector {
    filter: [Option<Coefficients>; 4],
    state: [[State; 4]; 2],
    power: [f32; 2],
    key: (f32, f32, f32),
}

impl BandDetector {
    fn configure(&mut self, shape: Shape, rate: f32) {
        let key = (shape.low, shape.high, rate);
        if self.key == key {
            return;
        }

        self.key = key;
        let select = |hz: f32, high_pass| {
            [1.847_759, 0.765_366_85].map(|k| {
                let g = (std::f32::consts::PI * hz.min(rate * 0.49) / rate).tan();
                let mix = if high_pass {
                    [1.0, -k, -1.0]
                } else {
                    [0.0, 0.0, 1.0]
                };
                Coefficients::new(g, k, mix)
            })
        };
        let high_pass = select(shape.low, true);
        let low_pass = select(shape.high, false);
        self.filter = [
            (shape.low > crate::analyzer::MIN_HZ).then_some(high_pass[0]),
            (shape.low > crate::analyzer::MIN_HZ).then_some(high_pass[1]),
            (shape.high < rate * 0.49).then_some(low_pass[0]),
            (shape.high < rate * 0.49).then_some(low_pass[1]),
        ];
    }

    fn sample(&mut self, input: [f32; 2], detector: Detector, coefficients: Ballistics) {
        for (ch, input) in input.into_iter().enumerate() {
            let mut filtered = if input.is_finite() { input } else { 0.0 };

            for (state, filter) in self.state[ch].iter_mut().zip(&self.filter) {
                if let Some(filter) = filter {
                    filtered = step(state, filter, filtered);
                }
            }

            let next = filtered * filtered;
            let alpha = match detector {
                Detector::Peak if next > self.power[ch] => coefficients.peak,
                _ => coefficients.level,
            };
            self.power[ch] += (next - self.power[ch]) * alpha;
        }
    }

    fn db(&self, link: f32) -> [f32; 2] {
        let high = self.power[0].max(self.power[1]);
        self.power
            .map(|value| 10.0 * (value + link * (high - value)).max(2.511_886_4e-10).log10())
    }
}

fn coefficient(ms: f32, rate: f32, frames: usize) -> f32 {
    -(-(frames as f32) / (0.001 * ms * rate)).exp_m1()
}

#[derive(Clone, Copy, Default)]
struct Ballistics {
    peak: f32,
    level: f32,
    attack: f32,
    release: f32,
    mix: f32,
}

#[derive(Default)]
struct Voice {
    control: Control,
    shape: Option<Shape>,
    detector: BandDetector,
    output: BandDetector,
    coefficients: Ballistics,
    rate: f32,
    reduction: [f32; 2],
    gain: [f32; 2],
    wet: f32,
    makeup: f32,
    meter: Meter,
    quiet: usize,
}

impl Voice {
    fn configure(&mut self, region: RegionControl, rate: f32) {
        let mut control = region.dynamics;
        control.enabled &= region.enabled;
        control.bypass |= region.bypass;
        if !control.enabled && self.shape.is_none() {
            return;
        }

        let [low, high] = region.bounds();

        if control.enabled {
            let shape = Shape {
                low,
                high,
                softness: region.softness,
            };
            self.shape = Some(shape);
            self.detector.configure(shape, rate);
            self.output.configure(shape, rate);
        }

        if self.control.source != control.source || self.control.detector != control.detector {
            self.detector.power = [0.0; 2];
        }

        if self.rate != rate
            || self.control.attack != control.attack
            || self.control.release != control.release
        {
            self.coefficients = Ballistics {
                peak: coefficient(0.4, rate, 1),
                level: coefficient(12.0, rate, 1),
                attack: coefficient(control.attack.max(0.1), rate, STEP),
                release: coefficient(control.release.max(5.0), rate, STEP),
                mix: coefficient(5.0, rate, STEP),
            };
        }

        self.control = control;
        self.rate = rate;
    }

    fn detect(&mut self, input: [f32; 2], external: Option<[f32; 2]>, tick: bool) {
        if self.shape.is_none() {
            return;
        }

        let control = self.control;
        let signal = match control.source {
            DetectorInput::Main => Some(input),
            DetectorInput::Sidechain => external,
        };
        self.detector.sample(
            signal.unwrap_or_default(),
            control.detector,
            self.coefficients,
        );

        if tick {
            let level = self.detector.db(control.link * 0.01);
            let live = control.enabled && !control.bypass && signal.is_some();
            self.wet += (f32::from(live) - self.wet) * self.coefficients.mix;
            let makeup = if control.makeup.is_finite() {
                control.makeup
            } else {
                0.0
            };
            self.makeup += (makeup - self.makeup) * self.coefficients.mix;

            for (ch, level) in level.into_iter().enumerate() {
                let next = control.reduction(level);
                let current = &mut self.reduction[ch];
                let alpha = if (next - *current) * next > 0.0 {
                    self.coefficients.attack
                } else {
                    self.coefficients.release
                };
                *current += (next - *current) * alpha;

                if (*current - next).abs() < 1e-5 {
                    *current = next;
                }

                let db = (*current + self.makeup) * self.wet;
                self.gain[ch] = if db.abs() < 1e-5 {
                    0.0
                } else {
                    (db * (std::f32::consts::LN_10 / 20.0)).exp_m1()
                };
            }

            self.quiet = if self.gain == [0.0; 2] {
                self.quiet.saturating_add(STEP)
            } else {
                0
            };

            self.meter.input = level[0].max(level[1]).max(FLOOR);
            self.meter.reduction = self.reduction[0].min(self.reduction[1]) * self.wet;
            self.meter.external_missing = signal.is_none();
        }
    }

    fn monitor(&mut self, output: [f32; 2]) {
        if self.control.enabled {
            self.output
                .sample(output, self.control.detector, self.coefficients);
        }
    }
}

pub(crate) struct Rack {
    voices: [Voice; REGION_COUNT],
    mask: Mask,
    delay: Box<[Gains]>,
    pos: usize,
    latency: usize,
    phase: usize,
    frames: u64,
    gains: [Gains; COEFF_FRAMES],
    delayed: Gains,
    pub(crate) monitor: Option<usize>,
    pub(crate) active: bool,
    tail: usize,
}

impl Rack {
    pub(crate) fn new() -> Self {
        Self {
            voices: Default::default(),
            mask: Default::default(),
            delay: vec![NO_GAIN; MAX_DELAY.div_ceil(STEP)].into_boxed_slice(),
            pos: 0,
            latency: 0,
            phase: 0,
            frames: 0,
            gains: [NO_GAIN; COEFF_FRAMES],
            delayed: NO_GAIN,
            monitor: None,
            active: false,
            tail: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.voices = Default::default();
        self.mask = Default::default();
        self.delay.fill(NO_GAIN);
        self.delayed = NO_GAIN;
        self.pos = 0;
        self.phase = 0;
        self.active = false;
        self.tail = 0;
    }

    pub(crate) fn configure(
        &mut self,
        regions: &[RegionControl; REGION_COUNT],
        rate: f32,
        latency: usize,
    ) {
        if latency != self.latency {
            self.delay.fill(NO_GAIN);
            self.delayed = NO_GAIN;
            self.pos = 0;
            self.latency = latency;
        }

        if !self.active && !regions.iter().any(|r| r.enabled && r.dynamics.enabled) {
            return;
        }

        for (voice, region) in self.voices.iter_mut().zip(regions) {
            voice.configure(*region, rate);

            if voice.shape.is_some()
                && !voice.control.enabled
                && voice.gain == [0.0; 2]
                && voice.quiet >= latency
            {
                *voice = Voice::default();
            }
        }

        self.active = self
            .voices
            .iter()
            .any(|voice| voice.control.enabled || voice.gain != [0.0; 2])
            || self.tail != 0;
        self.mask
            .configure(std::array::from_fn(|i| self.voices[i].shape), rate);

        if !self.active {
            self.voices = Default::default();
            self.mask = Default::default();
        }
    }

    pub(crate) fn detect<T: Sample>(
        &mut self,
        left: &[T],
        right: &[T],
        external: Option<[&[T]; 2]>,
    ) {
        self.frames += left.len() as u64;
        if !self.active {
            return;
        }

        #[cfg(feature = "analyzer-profile")]
        let _timer = crate::analyzer::profile::Timer::new(crate::analyzer::profile::REGION_DETECT);
        for (i, (left, right)) in left.iter().zip(right).enumerate() {
            let input = [left.double() as f32, right.double() as f32];
            let external =
                external.map(|channels| channels.map(|channel| channel[i].double() as f32));
            let tick = self.phase == 0;
            self.phase = (self.phase + 1) % STEP;

            for voice in &mut self.voices {
                voice.detect(input, external, tick);
            }

            if tick {
                let gains = std::array::from_fn(|i| self.voices[i].gain);
                self.delayed = if self.latency == 0 {
                    gains
                } else {
                    let delayed = std::mem::replace(&mut self.delay[self.pos], gains);
                    self.pos = (self.pos + 1) % self.latency.div_ceil(STEP);
                    delayed
                };
                if gains != NO_GAIN {
                    self.tail = self.latency + STEP;
                }
            }

            self.tail = self.tail.saturating_sub(1);
            self.gains[i] = self.delayed;
        }
    }

    pub(crate) fn apply<T: Sample>(&mut self, index: usize, wet: [T; 2]) -> [T; 2] {
        if !self.active {
            return wet;
        }

        let result = self.mask.sample(wet, &self.gains[index]);
        if let Some(index) = self.monitor {
            let output = result.map(|sample| sample.double() as f32);
            self.voices[index].monitor(output);
        }

        result
    }

    pub(crate) fn meter(&self, index: usize) -> (u64, Meter) {
        let voice = &self.voices[index];
        let mut meter = voice.meter;
        let [left, right] = voice.output.db(voice.control.link * 0.01);
        meter.output = left.max(right).max(FLOOR);
        (self.frames, meter)
    }

    pub(crate) fn response(&self) -> super::MaskResponse {
        if self.active {
            self.mask.response()
        } else {
            Default::default()
        }
    }
}
