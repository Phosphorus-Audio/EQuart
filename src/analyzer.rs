use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex64};
use std::sync::{Arc, OnceLock};

pub(crate) mod capture;
#[cfg(feature = "analyzer-profile")]
pub(crate) mod profile;

pub const MIN_HZ: f32 = 10.0;
const MIN_ORDER: usize = 8;
const MAX_ORDER: usize = 16;
pub const MAX_SPECTRUM_BINS: usize = (1 << (MAX_ORDER - 1)) + 1;
const PLAN_COUNT: usize = MAX_ORDER - MIN_ORDER + 1;

pub const RESOLUTIONS: [u32; 4] = [1_024, 2_048, 4_096, 8_192];
pub const RESOLUTION_LABELS: [&str; 4] = ["Low", "Medium", "High", "Maximum"];
pub const DEFAULT_RESOLUTION: u32 = 2;

// Preserve the persisted indices for the default octave and ERB modes.
pub const SPECTRUM_SMOOTHINGS: [&str; 7] = [
    "1/12 Octave",
    "1/2 ERB",
    "1/24 Octave",
    "1/6 Octave",
    "1/3 Octave",
    "1 Octave",
    "1 ERB",
];
pub const DEFAULT_SPECTRUM_SMOOTHING: u32 = 1;
/// Release rate multipliers for the Speed setting, one half-life each time the
/// label steps down: Very Fast is 2, Very Slow an eighth. The names say what
/// the plot does -- how long a peak stays up after the sound has stopped --
/// and the factor is what `configure_release` and the peak hold both use.
///
/// The order is also the persistence order. Adding a step at the bottom keeps
/// every saved session on the rate it was saved with: the first three entries
/// kept their factors when the labels moved down one, so an index written by an
/// older build still means the same speed.
pub const SPECTRUM_SPEEDS: [f32; 5] = [2.0, 1.0, 0.5, 0.25, 0.125];
pub const SPECTRUM_SPEED_LABELS: [&str; 5] = ["Very Fast", "Fast", "Normal", "Slow", "Very Slow"];
/// The default index: the middle setting, `Normal`. A fresh session, or one
/// with no saved value, starts here. A session that wrote an index keeps it,
/// so moving the default only changes new sessions and never rewrites a saved
/// speed.
pub const DEFAULT_SPECTRUM_SPEED: u32 = 2;

/// How much the drawn spectrum rises across the octaves, in dB per octave.
///
/// A spectrum plotted in dBFS leans the wrong way: almost every signal loses
/// six or so dB per octave just by being music, so the top of the plot is a
/// quiet smear while the bottom carries all the shape. Tilting the display
/// lifts each octave by this amount, so the trace reads closer to level and the
/// quiet octaves are as easy to judge as the loud ones.
///
/// It is a display setting only -- the numbers are added after the measurement,
/// so the analyser, the audio and the right-hand scale are untouched. 4.5 is
/// the default and is what an "analyser tilt" usually means; zero leaves the
/// raw dBFS.
pub const TILTS: [f32; 5] = [0.0, 1.5, 3.0, 4.5, 6.0];
pub const TILT_LABELS: [&str; 5] = ["Off", "1.5 dB/oct", "3 dB/oct", "4.5 dB/oct", "6 dB/oct"];
pub const DEFAULT_TILT: u32 = 3;
/// Every octave is measured against this frequency, so the tilt pivots here:
/// the trace crosses its untilted position at 1 kHz and nowhere else.
pub const TILT_PIVOT_HZ: f64 = 1_000.0;

/// What the peak hold does on screen.
///
/// The hold is a second envelope: each bin keeps the loudest it has been and
/// gives that back slowly, so transients stay readable after the trace has
/// moved on. It can be drawn as a line over the trace, used as the trace
/// itself, or turned off.
///
/// Drawn as the trace, it is a much calmer picture -- the frame-by-frame
/// levels flicker, the held ones do not -- and the two layers become one
/// silhouette each with nothing white on top. That is closer to what a
/// mastering-grade analyser looks like.
pub const HOLD_MODES: [&str; 3] = ["Off", "Line", "Smooth"];
/// New sessions show the held envelope as the trace itself: calmer than a line
/// over the flickering measurement, and what a mastering-grade analyser looks
/// like. Saved sessions keep whatever they had stored.
pub const DEFAULT_HOLD: u32 = 2;

const FPS: f32 = 60.0;
const INITIAL_DB: f32 = -240.0;

/// How fast the peak-hold line falls at Normal, in decibels per second. Slow
/// enough that a transient stays readable after the trace has moved on, quick
/// enough that the line is empty again within a bar or two. The Speed setting
/// scales it by the same factor it scales the trace's release, so the two
/// envelopes keep their distance at every speed: at twice the rate the line
/// would otherwise be stranded above a trace that has already fallen past it,
/// and at half the rate the trace would catch it up and the line would vanish
/// into the fill.
const HOLD_DB_PER_SECOND: f32 = 24.0;

/// The fall speeds up with the distance: every this many decibels the line sits
/// above the trace doubles its rate. A peak that is far from the audio left the
/// plot quickly instead of hovering over it, while a small residue still eases
/// down instead of snapping onto the trace.
const HOLD_DISTANCE_DB: f32 = 30.0;

/// Upper bound on the ERB half-width, in bins, so the smoothing cannot turn a
/// high tone into a mound wider than the plot can resolve.
const SMOOTH_ERB_MAX_HALF: f64 = 4.0;

/// Stable indices used by the stored analyzer choices and their menus.
#[derive(Clone, Copy)]
pub(crate) enum AnalyzerOption {
    Resolution,
    Speed,
    Smoothing,
    Tilt,
    Hold,
}

impl AnalyzerOption {
    pub(crate) fn index(self, stored: u32) -> u32 {
        let (count, default) = match self {
            Self::Resolution => (RESOLUTIONS.len(), DEFAULT_RESOLUTION),
            Self::Speed => (SPECTRUM_SPEEDS.len(), DEFAULT_SPECTRUM_SPEED),
            Self::Smoothing => (SPECTRUM_SMOOTHINGS.len(), DEFAULT_SPECTRUM_SMOOTHING),
            Self::Tilt => (TILTS.len(), DEFAULT_TILT),
            Self::Hold => (HOLD_MODES.len(), DEFAULT_HOLD),
        };
        if (stored as usize) < count {
            stored
        } else {
            default
        }
    }
}

pub(crate) fn spectrum_range(stored: u32) -> u32 {
    if crate::params::SPECTRUM_RANGES.contains(&stored) {
        stored
    } else {
        crate::params::DEFAULT_SPECTRUM_RANGE
    }
}

/// settings for the display analyzer
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AnalyzerSettings {
    resolution: u32,
    speed: u32,
    smoothing: u32,
    tilt: u32,
    frozen: bool,
    range: u32,
}

impl AnalyzerSettings {
    pub(crate) fn read(params: &crate::params::EquartParams) -> Self {
        use std::sync::atomic::Ordering::Relaxed;
        Self {
            resolution: params.analyzer_index(AnalyzerOption::Resolution),
            speed: params.analyzer_index(AnalyzerOption::Speed),
            smoothing: params.analyzer_index(AnalyzerOption::Smoothing),
            tilt: params.analyzer_index(AnalyzerOption::Tilt),
            frozen: params.analyzer_frozen.load(Relaxed),
            range: spectrum_range(params.spectrum_range_db.load(Relaxed)),
        }
    }
}

#[derive(Default)]
pub(crate) struct SpectrumFrame<'a> {
    pub bin_hz: f32,
    pub levels: &'a [f32],
    pub output_levels: &'a [f32],
    /// The peak each bin has reached and not yet released. Drawn as a line
    /// above the trace, or as the trace itself -- see [`HOLD_MODES`].
    pub hold_levels: &'a [f32],
    pub output_hold_levels: &'a [f32],
}

impl<'a> SpectrumFrame<'a> {
    pub(crate) fn from_analyzers([input, output]: [&'a Analyzer; 2]) -> Self {
        let (bin_hz, levels) = input.spectrum();
        Self {
            bin_hz,
            levels,
            output_levels: output.spectrum().1,
            hold_levels: input.hold(),
            output_hold_levels: output.hold(),
        }
    }
}

struct Transform {
    fft: Arc<dyn RealToComplex<f64>>,
    window: Vec<f32>,
}

fn transforms() -> &'static [Transform; PLAN_COUNT] {
    static TRANSFORMS: OnceLock<[Transform; PLAN_COUNT]> = OnceLock::new();
    TRANSFORMS.get_or_init(|| {
        let mut planner = RealFftPlanner::new();
        std::array::from_fn(|index| {
            let size = 1 << (MIN_ORDER + index);
            let window = (0..size)
                .map(|i| {
                    let phase = std::f64::consts::TAU * i as f64 / size as f64;
                    ((1.0 - phase.cos()) / size as f64) as f32
                })
                .collect();
            Transform {
                fft: planner.plan_fft_forward(size),
                window,
            }
        })
    })
}

fn resolution_samples(rate: f32, resolution: usize) -> usize {
    let base = match rate {
        ..=12_500.0 => 1_024,
        ..=25_000.0 => 2_048,
        ..=50_000.0 => 4_096,
        ..=100_000.0 => 8_192,
        ..=200_000.0 => 16_384,
        _ => 32_768,
    };
    base * RESOLUTIONS[resolution] as usize / 4_096
}

pub fn max_frequency(sample_rate: f32) -> f32 {
    20_000.0_f32.min(sample_rate * 0.49)
}

pub struct Analyzer {
    transforms: &'static [Transform; PLAN_COUNT],
    resolution: usize,
    smoothing: usize,
    speed: usize,
    range: f32,
    frozen: bool,
    rate: f32,
    size: usize,
    hop: usize,
    hop_size: usize,
    rings: [Vec<f32>; 2],
    position: usize,
    dc_input: [f32; 2],
    dc_output: [f32; 2],
    input: Vec<f64>,
    fft: Vec<Complex64>,
    scratch: Vec<Complex64>,
    power: Vec<f32>,
    cumulative: Vec<f64>,
    bounds: Vec<(usize, usize)>,
    /// The tilt table: dB added to each bin before it is displayed.
    tilt: Vec<f32>,
    /// How fast that table rises, in dB per octave. See [`TILTS`].
    tilt_slope: f32,
    display: Vec<f32>,
    hold: Vec<f32>,
}

impl Analyzer {
    pub fn new(rate: f32) -> Self {
        let transforms = transforms();
        let max_size = 1 << MAX_ORDER;
        let scratch_size = transforms
            .iter()
            .map(|plan| plan.fft.get_scratch_len())
            .max()
            .expect("the analyzer has at least one FFT plan");
        let mut analyzer = Self {
            transforms,
            resolution: DEFAULT_RESOLUTION as usize,
            smoothing: DEFAULT_SPECTRUM_SMOOTHING as usize,
            speed: DEFAULT_SPECTRUM_SPEED as usize,
            range: crate::params::DEFAULT_SPECTRUM_RANGE as f32,
            frozen: false,
            rate,
            size: 0,
            hop: 0,
            hop_size: 0,
            rings: std::array::from_fn(|_| vec![0.0; max_size]),
            position: 0,
            dc_input: [0.0; 2],
            dc_output: [0.0; 2],
            input: vec![0.0; max_size],
            fft: vec![Complex64::default(); MAX_SPECTRUM_BINS],
            scratch: vec![Complex64::default(); scratch_size],
            power: vec![0.0; MAX_SPECTRUM_BINS],
            cumulative: vec![0.0; MAX_SPECTRUM_BINS + 1],
            bounds: vec![(0, 0); MAX_SPECTRUM_BINS],
            tilt: vec![0.0; MAX_SPECTRUM_BINS],
            tilt_slope: TILTS[DEFAULT_TILT as usize],
            display: vec![INITIAL_DB; MAX_SPECTRUM_BINS],
            hold: vec![INITIAL_DB; MAX_SPECTRUM_BINS],
        };
        analyzer.configure_transform();
        analyzer
    }

    pub fn spectrum(&self) -> (f32, &[f32]) {
        (
            self.rate / self.size as f32,
            &self.display[..=self.size / 2],
        )
    }

    /// The peak each bin has reached and not yet released, one entry per bin
    /// that [`Self::spectrum`] returns.
    pub fn hold(&self) -> &[f32] {
        &self.hold[..=self.size / 2]
    }

    pub(crate) fn apply_settings(&mut self, settings: AnalyzerSettings) {
        self.set_resolution(settings.resolution);
        self.set_speed(settings.speed);
        self.set_smoothing(settings.smoothing);
        self.set_tilt(settings.tilt);
        self.set_frozen(settings.frozen);
        self.set_range(settings.range);
    }

    pub fn set_resolution(&mut self, resolution: u32) {
        let resolution = AnalyzerOption::Resolution.index(resolution) as usize;
        if self.resolution != resolution {
            self.resolution = resolution;
            self.configure_transform();
        }
    }

    pub fn set_smoothing(&mut self, smoothing: u32) {
        let smoothing = AnalyzerOption::Smoothing.index(smoothing) as usize;
        if self.smoothing != smoothing {
            self.smoothing = smoothing;
            self.configure_smoothing();
        }
    }

    pub fn set_speed(&mut self, speed: u32) {
        let speed = AnalyzerOption::Speed.index(speed) as usize;
        if self.speed != speed {
            self.speed = speed;
        }
    }

    pub fn set_range(&mut self, range: u32) {
        let range = spectrum_range(range) as f32;
        if self.range != range {
            // range only changes display axis, decay stays fixed
            self.range = range;
        }
    }

    /// display tilt index (visual only)
    pub fn set_tilt(&mut self, tilt: u32) {
        let slope = TILTS[AnalyzerOption::Tilt.index(tilt) as usize];
        if self.tilt_slope != slope {
            self.tilt_slope = slope;
            self.configure_tilt();
        }
    }

    /// freeze spectrum (keeps max peaks, stops decay)
    pub fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
    }

    pub fn retune(&mut self, rate: f32) {
        if rate.is_finite() && rate >= 8_000.0 && self.rate != rate {
            self.rate = rate;
            self.reset();
            self.configure_transform();
        }
    }

    pub fn reset(&mut self) {
        self.rings.iter_mut().for_each(|ring| ring.fill(0.0));
        self.position = 0;
        self.hop = 0;
        self.dc_input = [0.0; 2];
        self.dc_output = [0.0; 2];
        self.display.fill(INITIAL_DB);
        self.hold.fill(INITIAL_DB);
    }

    fn configure_transform(&mut self) {
        self.size = resolution_samples(self.rate, self.resolution);
        self.hop_size = (self.rate / FPS).round() as usize;
        self.display.fill(INITIAL_DB);
        self.hold.fill(INITIAL_DB);
        self.configure_tilt();
        self.configure_smoothing();
    }

    /// rebuild tilt table without resizing FFT
    fn configure_tilt(&mut self) {
        let bin_hz = f64::from(self.rate) / self.size as f64;
        for (i, db) in self.tilt[..=self.size / 2].iter_mut().enumerate() {
            *db = (f64::from(self.tilt_slope) * (i.max(1) as f64 * bin_hz / TILT_PIVOT_HZ).log2())
                as f32;
        }
    }

    fn configure_smoothing(&mut self) {
        let last = self.size / 2;
        let bin_hz = f64::from(self.rate) / self.size as f64;
        let bandwidth = match self.smoothing {
            2 => 1.0 / 24.0,
            3 => 1.0 / 6.0,
            4 => 1.0 / 3.0,
            5 => 1.0,
            _ => 1.0 / 12.0,
        };
        let ratio = 2.0_f64.powf(bandwidth * 0.5);
        for (index, bounds) in self.bounds[..=last].iter_mut().enumerate() {
            let k = index as f64;
            let (low, high) = match self.smoothing {
                1 | 6 => {
                    let fraction = if self.smoothing == 1 { 0.25 } else { 0.5 };
                    // cap ERB half-width in bins so high-freq tones stay sharp
                    let half = (fraction * (24.7 / bin_hz + 0.107939 * k)).min(SMOOTH_ERB_MAX_HALF);
                    ((k - half).max(0.0), k + half)
                }
                _ => (k / ratio, k * ratio),
            };
            *bounds = (low.round() as usize, (high.round() as usize).min(last) + 1);
        }
    }

    pub(crate) fn ingest(&mut self, input: [f32; 2]) {
        let mask = self.rings[0].len() - 1;
        for (channel, sample) in input.into_iter().enumerate() {
            let filtered = sample - self.dc_input[channel] + 0.9999 * self.dc_output[channel];
            self.dc_input[channel] = sample;
            self.dc_output[channel] = filtered;
            self.rings[channel][self.position] = filtered;
        }
        self.position = (self.position + 1) & mask;
        self.hop += 1;
    }

    pub(crate) fn refresh(&mut self) {
        if self.hop >= self.hop_size {
            let elapsed = std::mem::take(&mut self.hop) as f32 / self.rate;
            self.analyze(elapsed);
        }
    }

    #[cfg(any(test, debug_assertions))]
    pub fn push(&mut self, input: [f32; 2]) {
        self.ingest(input);
        self.refresh();
    }

    fn analyze(&mut self, elapsed: f32) {
        #[cfg(feature = "analyzer-profile")]
        let fft_timer = profile::Timer::new(profile::FFT);
        let transform = &self.transforms[self.size.ilog2() as usize - MIN_ORDER];
        let count = self.size / 2 + 1;
        let mask = self.rings[0].len() - 1;
        let start = (self.position + self.rings[0].len() - self.size) & mask;
        let power = &mut self.power[..count];
        power.fill(0.0);
        let identical = (0..self.size).all(|i| {
            let at = (start + i) & mask;
            self.rings[0][at] == self.rings[1][at]
        });
        let channels = if identical { 1 } else { 2 };
        for ring in &self.rings[..channels] {
            for (i, &weight) in transform.window.iter().enumerate() {
                self.input[i] = f64::from(ring[(start + i) & mask] * weight);
            }
            transform
                .fft
                .process_with_scratch(
                    &mut self.input[..self.size],
                    &mut self.fft[..count],
                    &mut self.scratch,
                )
                .expect("spectrum FFT buffers match the selected transform");
            for (sum, value) in power.iter_mut().zip(&self.fft) {
                *sum += value.norm_sqr() as f32;
            }
        }

        if identical {
            for bin in power.iter_mut() {
                *bin *= 2.0;
            }
        }

        #[cfg(feature = "analyzer-profile")]
        drop(fft_timer);
        #[cfg(feature = "analyzer-profile")]
        let _timer = profile::Timer::new(profile::SMOOTH);
        for _ in 0..2 {
            self.cumulative[0] = 0.0;
            for (i, &value) in power.iter().enumerate() {
                self.cumulative[i + 1] = self.cumulative[i] + f64::from(value);
            }
            for (value, &(start, end)) in power.iter_mut().zip(&self.bounds) {
                *value =
                    ((self.cumulative[end] - self.cumulative[start]) / (end - start) as f64) as f32;
            }
        }

        let speed = SPECTRUM_SPEEDS[self.speed];
        let half_life = 0.15 / (1.1 * speed);
        let release = 1.0 - 0.5_f32.powf(elapsed / half_life);
        let hold_step = HOLD_DB_PER_SECOND * speed * elapsed;
        let floor = -self.range;
        // freeze stops fall but still tracks higher peaks
        for (i, &power) in power.iter().enumerate() {
            let target = 10.0 * power.max(1e-24).log10() + self.tilt[i];
            let previous = self.display[i];
            if self.frozen {
                self.display[i] = target.max(previous);
                self.hold[i] = self.display[i].max(self.hold[i]);
                continue;
            }
            // instant attack, smooth release
            self.display[i] = target.max(previous + release * (target - previous));
            // hold drops faster the further it is above visible trace
            let gap = (self.hold[i] - target.max(floor)).max(0.0);
            self.hold[i] = target.max(self.hold[i] - hold_step * (1.0 + gap / HOLD_DISTANCE_DB));
        }
    }
}
