use super::{DetectionMode, DetectionRange, PitchTracking, Target, TargetMode, bounded_frequency};
use crate::dsp::{self, State};
use crate::filter::{Coefficients, FilterKind};
use pitch_estimate::{McLeodDetector, PitchDetector};
use std::f32::consts::PI;

const WINDOW: usize = 1024;
// Half the window covers a 25 Hz period even at decimated rates near 24 kHz.
const EXTENDED_WINDOW: usize = 2048;
// At ~4 kHz this covers ~256 ms while retaining harmonics above the 300 Hz pitch ceiling.
const BASS_WINDOW: usize = 1024;
const HOP: usize = 128;

struct DetectorCore<
    const WINDOW: usize,
    const BINS: usize,
    const PERIODIC_BINS: usize,
    const STATES: usize,
> {
    minimum: f32,
    maximum: f32,
    hop_size: usize,
    mode: DetectionMode,
    tracking: PitchTracking<BINS, PERIODIC_BINS, STATES>,
    rate: f32,
    sample_rate: f32,
    decimation: usize,
    phase: usize,
    filters: [[State; 2]; 2],
    lowpass: [Coefficients; 2],
    wide_filters: [[State; 4]; 2],
    wide_lowpass: [Coefficients; 4],
    rings: [Vec<f32>; 2],
    position: usize,
    filled: usize,
    hop: usize,
    mpm: McLeodDetector<f32>,
    frame: Vec<f32>,
}

impl<const WINDOW: usize, const BINS: usize, const PERIODIC_BINS: usize, const STATES: usize>
    DetectorCore<WINDOW, BINS, PERIODIC_BINS, STATES>
{
    fn new(sample_rate: f32, range: DetectionRange) -> Self {
        let minimum = range.minimum();
        let maximum = range.maximum();
        let (rate, decimation, hop_size) = sampling(sample_rate, range);

        // reserve lags for any host rate before audio thread starts
        let mut mpm = McLeodDetector::new(WINDOW, lag_limit(2.0 * target_rate(range), minimum))
            .expect("Invalid MPM window configuration")
            .with_k(0.93)
            .with_clarity_threshold(0.0);
        mpm.set_tau_max(lag_limit(rate, minimum));

        Self {
            minimum,
            maximum,
            hop_size,
            mode: DetectionMode::default(),
            tracking: PitchTracking::new(rate, WINDOW, hop_size, minimum, maximum),
            rate,
            sample_rate,
            decimation,
            phase: 0,
            filters: [[State::default(); 2]; 2],
            lowpass: lowpass(sample_rate, 1000.0, [0.5411961, 1.306563]),
            wide_filters: [[State::default(); 4]; 2],
            wide_lowpass: wide_lowpass(sample_rate, rate),
            rings: std::array::from_fn(|_| vec![0.0; WINDOW]),
            position: 0,
            filled: 0,
            hop: 0,
            mpm,
            frame: vec![0.0; WINDOW],
        }
    }

    fn reset(&mut self) {
        self.tracking.reset();
        self.filters = [[State::default(); 2]; 2];
        self.wide_filters = [[State::default(); 4]; 2];
        self.position = 0;
        self.filled = 0;

        self.phase = 0;

        self.hop = 0;
    }

    fn prepare(
        &mut self,
        sample_rate: f32,
        range: DetectionRange,
        mode: DetectionMode,
        switched: bool,
    ) -> bool {
        let retuned = self.sample_rate != sample_rate;
        if retuned {
            let (rate, decimation, hop) = sampling(sample_rate, range);
            self.sample_rate = sample_rate;
            self.rate = rate;
            self.decimation = decimation;
            self.mpm.set_tau_max(lag_limit(rate, self.minimum));
            self.tracking.set_hop(hop as f32 / rate);
            self.lowpass = lowpass(sample_rate, 1000.0, [0.5411961, 1.306563]);
            self.wide_lowpass = wide_lowpass(sample_rate, rate);
        }
        let changed = retuned || switched || self.mode != mode;
        if changed {
            self.mode = mode;
            self.tracking.set_mode(mode);
            self.reset();
        }
        changed
    }

    fn push(&mut self, samples: [f32; 2]) -> bool {
        // bass uses steep anti-alias filter; others keep 1kHz path
        let mut filtered = [0.0; 2];

        for ch in 0..2 {
            if self.mode == DetectionMode::SpectralMelody
                || self.maximum == DetectionRange::Bass.maximum()
            {
                let mut value = samples[ch];

                for (filter, coefficients) in
                    self.wide_filters[ch].iter_mut().zip(self.wide_lowpass)
                {
                    value = dsp::step(filter, &coefficients, value);
                }

                filtered[ch] = value;
            } else {
                let first = dsp::step(&mut self.filters[ch][0], &self.lowpass[0], samples[ch]);
                filtered[ch] = dsp::step(&mut self.filters[ch][1], &self.lowpass[1], first);
            }
        }

        self.phase += 1;

        if self.phase != self.decimation {
            return false;
        }

        self.phase = 0;

        for (ch, value) in filtered.into_iter().enumerate() {
            self.rings[ch][self.position] = value;
        }

        self.position = (self.position + 1) % WINDOW;
        self.filled = (self.filled + 1).min(WINDOW);
        self.hop += 1;

        if self.hop < self.hop_size {
            return false;
        }

        self.hop = 0;
        self.filled == WINDOW
    }

    fn analyze(&mut self, gate: f32, threshold: f32, target: Target) -> f32 {
        let means: [f32; 2] =
            std::array::from_fn(|ch| self.rings[ch].iter().sum::<f32>() / WINDOW as f32);
        let powers: [f32; 2] =
            std::array::from_fn(|ch| self.rings[ch].iter().map(|v| (v - means[ch]).powi(2)).sum());
        let channel = usize::from(powers[1] > powers[0]);
        let mean = means[channel];

        for i in 0..WINDOW {
            self.frame[i] = self.rings[channel][(self.position + i) % WINDOW] - mean;
        }

        let rms = (self.frame.iter().map(|v| v * v).sum::<f32>() / WINDOW as f32).sqrt();

        if rms < gate {
            self.tracking.reset();
            return 0.0;
        }

        if self.mode != DetectionMode::Mpm || target.mode != TargetMode::Auto {
            return self
                .tracking
                .analyze(&self.frame, self.rate, &mut self.mpm, threshold, target)
                .0;
        }

        self.mpm
            .detect(&self.frame, self.rate.round() as u32)
            .filter(|pitch| pitch.clarity >= threshold)
            .and_then(|pitch| {
                let hz = pitch.frequency * self.rate / self.rate.round();
                bounded_frequency(hz, self.minimum, self.maximum.min(1000.0))
            })
            .unwrap_or(0.0)
    }
}

fn target_rate(range: DetectionRange) -> f32 {
    if range == DetectionRange::Bass {
        4000.0
    } else {
        12000.0
    }
}

fn sampling(sample_rate: f32, range: DetectionRange) -> (f32, usize, usize) {
    let hop = if range == DetectionRange::Bass {
        64
    } else {
        HOP
    };
    let decimation = ((sample_rate / target_rate(range)) as usize).max(1);
    (sample_rate / decimation as f32, decimation, hop)
}

fn lag_limit(rate: f32, minimum: f32) -> usize {
    (rate / minimum * 1.6).ceil() as usize
}

fn lowpass<const N: usize>(sample_rate: f32, hz: f32, qs: [f32; N]) -> [Coefficients; N] {
    let g = (PI * hz / sample_rate).tan();
    qs.map(|q| Coefficients::cut_section(g, q, FilterKind::Lowpass, false))
}

fn wide_lowpass(sample_rate: f32, rate: f32) -> [Coefficients; 4] {
    lowpass(
        sample_rate,
        (rate * 0.3).min(4000.0),
        [0.5097956, 0.6013449, 0.8999762, 2.5629154],
    )
}

/// All profiles are prepared off the audio thread; only the selected one runs.
pub(super) struct Detector {
    range: DetectionRange,
    standard: Box<DetectorCore<WINDOW, 640, 520, 1280>>,
    extended: Box<DetectorCore<EXTENDED_WINDOW, 760, 640, 1520>>,
    bass: Box<DetectorCore<BASS_WINDOW, 470, 470, 940>>,
}

macro_rules! dispatch_detector {
    ($self:ident, $method:ident($($arg:expr),* $(,)?)) => {
        match $self.range {
            DetectionRange::Standard => $self.standard.$method($($arg),*),
            DetectionRange::Low => $self.extended.$method($($arg),*),
            DetectionRange::Bass => $self.bass.$method($($arg),*),
        }
    };
}

impl Detector {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            range: DetectionRange::default(),
            standard: Box::new(DetectorCore::new(sample_rate, DetectionRange::Standard)),
            extended: Box::new(DetectorCore::new(sample_rate, DetectionRange::Low)),
            bass: Box::new(DetectorCore::new(sample_rate, DetectionRange::Bass)),
        }
    }

    pub fn prepare(
        &mut self,
        sample_rate: f32,
        range: DetectionRange,
        mode: DetectionMode,
    ) -> bool {
        let switched = self.range != range;
        self.range = range;
        dispatch_detector!(self, prepare(sample_rate, range, mode, switched))
    }

    pub fn reset(&mut self) {
        dispatch_detector!(self, reset());
    }

    pub fn push(&mut self, samples: [f32; 2]) -> bool {
        dispatch_detector!(self, push(samples))
    }

    pub fn analyze(&mut self, gate: f32, threshold: f32, target: Target) -> f32 {
        dispatch_detector!(self, analyze(gate, threshold, target))
    }
}
