//! 2x/4x linear-phase FIR oversampling for the EQ stage (stack-only, no alloc)

use crate::band::COEFF_FRAMES;
use crate::precision::{Sample, convert};
use nice_plug::prelude::Enum;

pub const TAPS: usize = 65;
const DELAY: usize = (TAPS - 1) / 2;
const MAX_LATENCY: usize = DELAY * 3 / 2;
const ODD: usize = TAPS / 2;
const EVEN_DELAY: usize = DELAY / 2;
const UP_HISTORY: usize = DELAY;
const DOWN_HISTORY: usize = TAPS - 1;

pub const CHUNK: usize = COEFF_FRAMES;
pub const HIGH: usize = CHUNK * 4;

const UP_PAD: usize = UP_HISTORY + 2 * CHUNK;
const DOWN_PAD: usize = DOWN_HISTORY + 4 * CHUNK;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Oversampling {
    #[default]
    #[id = "off"]
    #[name = "Off"]
    Off,
    #[id = "2x"]
    #[name = "2x"]
    X2,
    #[id = "4x"]
    #[name = "4x"]
    X4,
}

impl Oversampling {
    pub const ALL: [Self; 3] = [Self::Off, Self::X2, Self::X4];

    pub fn factor(self) -> usize {
        match self {
            Self::Off => 1,
            Self::X2 => 2,
            Self::X4 => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "1×",
            Self::X2 => "2×",
            Self::X4 => "4×",
        }
    }
}

/// Kaiser-windowed half-band sinc (center=0.5, odd taps sum to 0.5)
fn design<T: Sample>() -> [T; TAPS] {
    let beta = T::value(8.6);

    let mut taps = [T::zero(); TAPS];
    let mut odd_sum = T::zero();

    for (n, tap) in taps.iter_mut().enumerate() {
        let t = T::value(n as f64) - T::value(DELAY as f64);

        if t == T::zero() {
            *tap = T::value(0.5);
            continue;
        }

        // Everything else on the even grid is a zero of the ideal response.
        if n % 2 == 0 {
            continue;
        }

        let x = t / T::value(DELAY as f64);
        let window = bessel_i0(beta * (T::one() - x * x).max(T::zero()).sqrt()) / bessel_i0(beta);
        let angle = T::FRAC_PI_2() * t;
        let value = T::value(0.5) * (angle.sin() / angle) * window;

        *tap = value;
        odd_sum += value;
    }

    // Half the DC gain sits on the centre tap, half on the odd taps.
    let scale = T::value(0.5) / odd_sum;

    for n in (1..TAPS).step_by(2) {
        taps[n] *= scale;
    }

    taps
}

fn bessel_i0<T: Sample>(x: T) -> T {
    let half = x * T::value(0.5);
    let mut term = T::one();
    let mut sum = T::one();

    for k in 1..32 {
        let ratio = half / T::value(k as f64);
        term *= ratio * ratio;
        sum += term;
    }

    sum
}

/// taps laid out for forward dot product (even phase is just delay)
struct Design<T: Sample> {
    center: T,
    odd: [T; ODD],
    down: [T; TAPS],
}

impl<T: Sample> Design<T> {
    fn new() -> Self {
        let taps = design();

        let mut odd = [T::zero(); ODD];
        for (i, tap) in odd.iter_mut().enumerate() {
            *tap = taps[2 * (ODD - 1 - i) + 1];
        }

        let mut down = [T::zero(); TAPS];
        for (i, tap) in down.iter_mut().enumerate() {
            *tap = taps[TAPS - 1 - i];
        }

        Self {
            center: T::value(2.0) * taps[DELAY],
            odd,
            down,
        }
    }
}

/// Four accumulators, because a 65 tap sum would otherwise serialise on its own
/// latency.
#[inline]
fn dot<T: Sample>(taps: &[T], window: &[T]) -> T {
    debug_assert_eq!(taps.len(), window.len());

    let mut acc = [T::zero(); 4];
    for (t, w) in taps.chunks_exact(4).zip(window.chunks_exact(4)) {
        acc[0] += t[0] * w[0];
        acc[1] += t[1] * w[1];
        acc[2] += t[2] * w[2];
        acc[3] += t[3] * w[3];
    }

    let mut tail = T::zero();
    for k in (taps.len() / 4) * 4..taps.len() {
        tail += taps[k] * window[k];
    }

    (acc[0] + acc[1]) + (acc[2] + acc[3]) + tail
}

/// Zero-stuff and lowpass: one input sample becomes two.
struct Up2<T: Sample> {
    history: [T; UP_HISTORY],
    padded: [T; UP_PAD],
}

impl<T: Sample> Up2<T> {
    fn new() -> Self {
        Self {
            history: [T::zero(); UP_HISTORY],
            padded: [T::zero(); UP_PAD],
        }
    }

    fn reset(&mut self) {
        self.history = [T::zero(); UP_HISTORY];
    }

    fn process(&mut self, design: &Design<T>, input: &[T], output: &mut [T]) {
        let n = input.len();
        self.padded[..UP_HISTORY].copy_from_slice(&self.history);
        self.padded[UP_HISTORY..UP_HISTORY + n].copy_from_slice(input);

        for (m, pair) in output.chunks_exact_mut(2).take(n).enumerate() {
            let base = UP_HISTORY + m;

            // The even phase is the centre tap alone: one multiply against a
            // delayed input sample, not a 33 tap dot product of mostly zeros.
            pair[0] = design.center * self.padded[base - EVEN_DELAY];
            // Zero stuffing halves the energy; the factor of two restores it.
            pair[1] = T::value(2.0) * dot(&design.odd, &self.padded[base + 1 - ODD..=base]);
        }

        self.remember(input);
    }

    fn remember(&mut self, input: &[T]) {
        let len = input.len();
        if len >= UP_HISTORY {
            self.history.copy_from_slice(&input[len - UP_HISTORY..]);
        } else {
            self.history.copy_within(len.., 0);
            self.history[UP_HISTORY - len..].copy_from_slice(input);
        }
    }
}

/// Lowpass and throw away every other sample.
struct Down2<T: Sample> {
    history: [T; DOWN_HISTORY],
    padded: [T; DOWN_PAD],
}

impl<T: Sample> Down2<T> {
    fn new() -> Self {
        Self {
            history: [T::zero(); DOWN_HISTORY],
            padded: [T::zero(); DOWN_PAD],
        }
    }

    fn reset(&mut self) {
        self.history = [T::zero(); DOWN_HISTORY];
    }

    fn process(&mut self, design: &Design<T>, input: &[T], output: &mut [T]) {
        let n = output.len();
        let wide = 2 * n;

        self.padded[..DOWN_HISTORY].copy_from_slice(&self.history);
        self.padded[DOWN_HISTORY..DOWN_HISTORY + wide].copy_from_slice(&input[..wide]);

        for (m, out) in output.iter_mut().enumerate() {
            let base = DOWN_HISTORY + 2 * m;
            *out = dot(&design.down, &self.padded[base + 1 - TAPS..=base]);
        }

        if wide >= DOWN_HISTORY {
            self.history
                .copy_from_slice(&input[wide - DOWN_HISTORY..wide]);
        } else {
            self.history.copy_within(wide.., 0);
            self.history[DOWN_HISTORY - wide..].copy_from_slice(&input[..wide]);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DownsamplePath {
    Main,
    Delta,
}

struct Chain<T: Sample> {
    up: [Up2<T>; 2],
    down: [Down2<T>; 2],
    delta: [Down2<T>; 2],
}

impl<T: Sample> Chain<T> {
    fn new() -> Self {
        Self {
            up: std::array::from_fn(|_| Up2::new()),
            down: std::array::from_fn(|_| Down2::new()),
            delta: std::array::from_fn(|_| Down2::new()),
        }
    }

    fn reset(&mut self) {
        for up in &mut self.up {
            up.reset();
        }
        for down in self.down.iter_mut().chain(&mut self.delta) {
            down.reset();
        }
    }
}

pub struct Resampler<T: Sample = f32> {
    design: Design<T>,
    chains: [Chain<T>; 2],
    /// Scratch between the two stages, per channel.
    scratch: [[T; HIGH]; 2],
    mode: Oversampling,
    dry_history: [[T; 2]; MAX_LATENCY],
    dry_position: usize,
}

impl<T: Sample> Default for Resampler<T> {
    fn default() -> Self {
        Self::new(Oversampling::Off)
    }
}

impl<T: Sample> Resampler<T> {
    pub fn new(mode: Oversampling) -> Self {
        Self {
            design: Design::new(),
            chains: std::array::from_fn(|_| Chain::new()),
            scratch: [[T::zero(); HIGH]; 2],
            dry_history: [[T::zero(); 2]; MAX_LATENCY],
            dry_position: 0,
            mode,
        }
    }

    /// Switch factors. Both directions reset their history: the filters are
    /// linear phase, so a reset is a short fade of silence rather than a click,
    /// and the host realigns anyway because the reported latency changes.
    pub fn set_mode(&mut self, mode: Oversampling) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        self.reset();
        true
    }

    pub fn mode(&self) -> Oversampling {
        self.mode
    }

    pub(crate) fn transfer_from<S: Sample>(&mut self, source: &Resampler<S>) {
        self.mode = source.mode;
        self.dry_history = source.dry_history.map(convert);
        self.dry_position = source.dry_position;

        for (chain, previous) in self.chains.iter_mut().zip(&source.chains) {
            for (stage, old) in chain.up.iter_mut().zip(&previous.up) {
                stage.history = convert(old.history);
            }

            for (stage, old) in chain
                .down
                .iter_mut()
                .chain(&mut chain.delta)
                .zip(previous.down.iter().chain(&previous.delta))
            {
                stage.history = convert(old.history);
            }
        }
    }

    pub fn reset(&mut self) {
        for chain in &mut self.chains {
            chain.reset();
        }
        self.scratch = [[T::zero(); HIGH]; 2];
        self.dry_history.fill([T::zero(); 2]);
        self.dry_position = 0;
    }

    /// Latency in frames of the base rate.
    pub fn latency(&self) -> u32 {
        match self.mode {
            Oversampling::Off => 0,
            Oversampling::X2 => DELAY as u32,
            Oversampling::X4 => MAX_LATENCY as u32,
        }
    }

    /// Produce the dry signal at the same group delay as the wet path.
    pub fn delayed_dry(&mut self, left: &[T], right: &[T], dry_l: &mut [T], dry_r: &mut [T]) {
        let delay = self.latency() as usize;
        if delay == 0 {
            dry_l.copy_from_slice(left);
            dry_r.copy_from_slice(right);
            return;
        }
        for i in 0..left.len() {
            [dry_l[i], dry_r[i]] = self.dry_history[self.dry_position];
            self.dry_history[self.dry_position] = [left[i], right[i]];
            self.dry_position += 1;
            if self.dry_position == delay {
                self.dry_position = 0;
            }
        }
    }

    /// Expand `left`/`right` (`n` frames) into the high-rate buffers.
    pub fn upsample(&mut self, left: &[T], right: &[T], out_left: &mut [T], out_right: &mut [T]) {
        let design = &self.design;
        let n = left.len();

        for (channel, input) in [left, right].into_iter().enumerate() {
            let output = if channel == 0 {
                &mut *out_left
            } else {
                &mut *out_right
            };
            let chain = &mut self.chains[channel];

            match self.mode {
                Oversampling::X2 => chain.up[0].process(design, input, &mut output[..2 * n]),
                Oversampling::X4 => {
                    let middle = &mut self.scratch[channel];
                    chain.up[0].process(design, input, &mut middle[..2 * n]);
                    chain.up[1].process(design, &middle[..2 * n], &mut output[..4 * n]);
                }
                Oversampling::Off => output[..n].copy_from_slice(input),
            }
        }
    }

    pub(crate) fn reset_delta(&mut self) {
        for chain in &mut self.chains {
            for stage in &mut chain.delta {
                stage.reset();
            }
        }
    }

    /// Fold the high-rate buffers back down to `n` frames.
    pub(crate) fn downsample(
        &mut self,
        path: DownsamplePath,
        left: &[T],
        right: &[T],
        out_left: &mut [T],
        out_right: &mut [T],
        n: usize,
    ) {
        let design = &self.design;

        for (channel, input) in [left, right].into_iter().enumerate() {
            let output = if channel == 0 {
                &mut *out_left
            } else {
                &mut *out_right
            };
            let down = match path {
                DownsamplePath::Main => &mut self.chains[channel].down,
                DownsamplePath::Delta => &mut self.chains[channel].delta,
            };

            match self.mode {
                Oversampling::X2 => down[0].process(design, &input[..2 * n], &mut output[..n]),
                Oversampling::X4 => {
                    let middle = &mut self.scratch[channel];
                    down[1].process(design, &input[..4 * n], &mut middle[..2 * n]);
                    down[0].process(design, &middle[..2 * n], &mut output[..n]);
                }
                Oversampling::Off => output[..n].copy_from_slice(&input[..n]),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse(len: usize) -> Vec<f32> {
        let mut signal = vec![0.0; len];
        signal[0] = 1.0;
        signal
    }

    fn round_trip(mode: Oversampling, signal: &[f32]) -> Vec<f32> {
        let mut resampler = Resampler::new(mode);
        let mut out = Vec::with_capacity(signal.len());

        let mut high_left = [0.0; HIGH];
        let mut high_right = [0.0; HIGH];
        let mut left = [0.0; CHUNK];
        let mut right = [0.0; CHUNK];

        for chunk in signal.chunks(CHUNK) {
            let n = chunk.len();
            left[..n].copy_from_slice(chunk);
            right[..n].copy_from_slice(chunk);

            resampler.upsample(&left[..n], &right[..n], &mut high_left, &mut high_right);
            let wide = n * mode.factor();
            resampler.downsample(
                DownsamplePath::Main,
                &high_left[..wide],
                &high_right[..wide],
                &mut left,
                &mut right,
                n,
            );
            out.extend_from_slice(&left[..n]);
        }

        out
    }

    /// The lowpass must pass the audio band and reject the images.
    #[test]
    fn fir_flat_and_steep() {
        let taps = design::<f32>();
        let response = |hz: f32| {
            let rate = 96_000.0_f32;
            let mut real = 0.0;
            let mut imag = 0.0;
            for (n, tap) in taps.iter().enumerate() {
                let phase = -std::f32::consts::TAU * hz * n as f32 / rate;
                real += tap * phase.cos();
                imag += tap * phase.sin();
            }
            (real * real + imag * imag).sqrt()
        };

        // The transition is centred on a quarter of the oversampled rate, so
        // the passband is flat to 19 kHz and the stopband starts at 28 kHz.
        for hz in [20.0, 1_000.0, 10_000.0, 18_000.0, 19_000.0] {
            let gain = response(hz);
            assert!(
                (20.0 * gain.log10()).abs() < 0.02,
                "passband drooped at {hz} Hz: {:.3} dB",
                20.0 * gain.log10()
            );
        }

        for hz in [29_000.0, 36_000.0, 44_000.0] {
            let gain = response(hz);
            assert!(
                20.0 * gain.log10() < -80.0,
                "stopband leaked at {hz} Hz: {:.1} dB",
                20.0 * gain.log10()
            );
        }
    }

    /// A round trip must be the identity, delayed by exactly the reported
    /// latency.
    #[test]
    fn fir_round_trip() {
        for mode in [Oversampling::X2, Oversampling::X4] {
            let factor = mode.factor();
            let input = impulse(256);
            let output = round_trip(mode, &input);
            let latency = Resampler::<f32>::new(mode).latency() as usize;

            let peak = output
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map(|(i, _)| i)
                .unwrap();

            assert!(
                peak.abs_diff(latency) <= 1,
                "{factor}x impulse landed at {peak}, latency says {latency}"
            );
            // Up then down is the filter convolved with itself, so the peak is
            // spread over both filters: check the gain by summing instead.
            let total: f32 = output.iter().sum();
            assert!(
                (total - 1.0).abs() < 0.02,
                "{factor}x round trip lost gain: {total}"
            );
        }
    }
}
