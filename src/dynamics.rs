//! Parallel detector envelopes and per-band gain control.

mod params;
pub use params::{
    DetectorControl, DetectorInput, DetectorMode, DetectorParams, DynamicControl, DynamicParams,
    GainRule,
};

use crate::{
    dsp::{self, State},
    filter::{Coefficients, FilterKind},
};
use std::f32::consts::{FRAC_1_SQRT_2, PI};

// The equal-density point of Relative mode on the shared Threshold scale.
const RELATIVE_ORIGIN_DB: f32 = -24.0;
const POWER_FLOOR: f32 = 1e-12;

pub type Sidechain<'a, T = f32> = [&'a [T]; 2];

fn alpha(ms: f32, rate: f32, frames: usize) -> f32 {
    -(-(frames as f32) / (ms * 0.001 * rate)).exp_m1()
}

fn power_db(power: f32) -> f32 {
    10.0 * power.max(POWER_FLOOR).log10()
}

impl GainRule {
    fn offset(self, distance: f32) -> f32 {
        if self.enabled {
            (distance * self.ratio)
                .min(self.range.abs())
                .copysign(self.range)
        } else {
            0.0
        }
    }
}

impl DynamicControl {
    fn target(self, level: f32) -> f32 {
        let distance = level - self.threshold;
        if distance >= 0.0 {
            self.above.offset(distance)
        } else {
            self.below.offset(-distance)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Spectrum {
    BandPass,
    LowPass,
    HighPass,
    Broadband,
}

#[derive(Clone, Copy, PartialEq)]
struct DetectorKey {
    spectrum: Spectrum,
    source: DetectorInput,
    frequency: f32,
    q: f32,
}

impl DetectorKey {
    fn new(control: DetectorControl, frequency: f32, q: f32, kind: FilterKind) -> Self {
        let (spectrum, frequency, q) = match control.mode {
            DetectorMode::Free => (Spectrum::BandPass, control.frequency, control.q),
            DetectorMode::Follow => (Spectrum::BandPass, frequency, q),
            DetectorMode::Broadband => (Spectrum::Broadband, 0.0, 1.0),
            // follow shelf region or both halves of tilt
            DetectorMode::BandShape => match kind {
                FilterKind::LowShelf => (Spectrum::LowPass, frequency, FRAC_1_SQRT_2),
                FilterKind::HighShelf => (Spectrum::HighPass, frequency, FRAC_1_SQRT_2),
                FilterKind::TiltShelf | FilterKind::FlatTilt => (Spectrum::Broadband, 0.0, 1.0),
                _ => (Spectrum::BandPass, frequency, q),
            },
        };
        Self {
            spectrum,
            source: control.input,
            frequency,
            q,
        }
    }
}

struct Detector {
    key: Option<DetectorKey>,
    state: [State; 2],
    coefficients: Coefficients,
    /// White-noise power gain, i.e. equivalent noise bandwidth / Nyquist.
    bandwidth: f32,
    first: [f32; 2],
    power: [f32; 2],
    sum: [f32; 2],
}

impl Detector {
    fn new() -> Self {
        Self {
            key: None,
            state: [State::default(); 2],
            coefficients: Coefficients::identity(),
            bandwidth: 1.0,
            first: [0.0; 2],
            power: [0.0; 2],
            sum: [0.0; 2],
        }
    }

    fn reset(&mut self) {
        if self.key.take().is_some() {
            self.state = [State::default(); 2];
            self.first = [0.0; 2];
            self.power = [0.0; 2];
            self.sum = [0.0; 2];
        }
    }

    fn tune(&mut self, key: DetectorKey, rate: f32) {
        if self.key == Some(key) {
            return;
        }
        if self
            .key
            .is_some_and(|old| old.source != key.source || old.spectrum != key.spectrum)
        {
            self.reset();
        }
        self.key = Some(key);
        if key.spectrum == Spectrum::Broadband {
            self.bandwidth = 1.0;
            return;
        }

        let g = (PI * key.frequency.clamp(10.0, rate * 0.49) / rate).tan();
        let k = key.q.recip();
        let lowpass_enbw = |g: f32| g * (1.0 + k * g) / (k * (1.0 + g * (g + k)));
        let (mix, enbw) = match key.spectrum {
            Spectrum::BandPass => ([0.0, k, 0.0], k * g / (1.0 + g * (g + k))),
            Spectrum::LowPass => ([0.0, 0.0, 1.0], lowpass_enbw(g)),
            Spectrum::HighPass => ([1.0, -k, -1.0], lowpass_enbw(g.recip())),
            Spectrum::Broadband => unreachable!("broadband has no filter"),
        };
        self.coefficients = Coefficients::new(g, k, mix);
        self.bandwidth = enbw;
    }

    fn push(&mut self, main: [f32; 2], sidechain: [f32; 2], fast: f32, slow: f32) {
        let Some(key) = self.key else { return };
        let mut input = match key.source {
            DetectorInput::Main => main,
            DetectorInput::Sidechain => sidechain,
        };
        if key.spectrum != Spectrum::Broadband {
            for (sample, state) in input.iter_mut().zip(&mut self.state) {
                *sample = dsp::step(state, &self.coefficients, *sample);
            }
        }
        let power = (input[0] * input[0] + input[1] * input[1]) * 0.5;
        for (i, alpha) in [fast, slow].into_iter().enumerate() {
            self.first[i] += alpha * (power - self.first[i]);
            self.power[i] += alpha * (self.first[i] - self.power[i]);
            self.sum[i] += self.power[i];
        }
    }

    fn finish_interval(&mut self, frames: usize) -> [f32; 2] {
        // Average before decimating to the coefficient clock to attenuate folded ripple.
        std::mem::take(&mut self.sum).map(|sum| sum / frames as f32)
    }
}

/// reads audio without changing it and outputs one dB control signal
pub struct Dynamics {
    detect: Detector,
    relative: Detector,
    power_alphas: [f32; 2],
    power_attack: Option<f32>,
    gain_clock: Option<([f32; 2], usize)>,
    gain_alphas: [f32; 2],
    pub level: Option<f32>,
    pub gain: f32,
}

impl Dynamics {
    pub fn new(rate: f32) -> Self {
        Self {
            detect: Detector::new(),
            relative: Detector::new(),
            // Two cascaded poles retain the nominal low-frequency delay (5 / 45 ms).
            power_alphas: [alpha(2.5, rate, 1), alpha(22.5, rate, 1)],
            power_attack: None,
            gain_clock: None,
            gain_alphas: [0.0; 2],
            level: None,
            gain: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.detect.reset();
        self.relative.reset();
        self.level = None;
        self.gain = 0.0;
    }

    pub fn retune(&mut self, rate: f32) {
        self.power_alphas = [alpha(2.5, rate, 1), alpha(22.5, rate, 1)];
        self.power_attack = None;
        self.gain_clock = None;
        self.detect.reset();
        self.relative.reset();
        self.level = None;
    }

    pub fn prepare(
        &mut self,
        control: DynamicControl,
        rate: f32,
        frequency: f32,
        q: f32,
        kind: FilterKind,
        has_sidechain: bool,
    ) -> bool {
        let use_reference = control.relative_amount > 0.0 && control.onsets_amount < 1.0;
        let needs_sidechain = control.detect.input == DetectorInput::Sidechain
            || (use_reference && control.relative.input == DetectorInput::Sidechain);
        if !control.enabled || !kind.has_gain() || (needs_sidechain && !has_sidechain) {
            self.detect.reset();
            self.relative.reset();
            self.level = None;
            return false;
        }
        let key = DetectorKey::new(control.detect, frequency, q, kind);

        // keep a half-cycle of smoothing on filtered carriers to limit envelope ripple
        // preserve the existing windows for onset and wideband detection
        let pole_ms = if control.onsets_amount == 0.0
            && !use_reference
            && matches!(key.spectrum, Spectrum::BandPass | Spectrum::HighPass)
        {
            (control.attack_ms * 0.25)
                .max(500.0 / key.frequency)
                .min(2.5)
        } else {
            2.5
        };

        if self.power_attack != Some(pole_ms) {
            self.power_attack = Some(pole_ms);
            self.power_alphas[0] = alpha(pole_ms, rate, 1);
        }

        self.detect.tune(key, rate);
        if use_reference {
            self.relative
                .tune(DetectorKey::new(control.relative, frequency, q, kind), rate);
        } else {
            self.relative.reset();
        }
        true
    }

    pub fn push(&mut self, main: [f32; 2], sidechain: [f32; 2], onsets: bool) {
        let [fast, slow] = self.power_alphas;
        self.detect
            .push(main, sidechain, fast, if onsets { slow } else { fast });
        self.relative.push(main, sidechain, fast, fast);
    }

    pub fn advance(&mut self, control: DynamicControl, rate: f32, frames: usize) {
        let clock = ([control.attack_ms, control.release_ms], frames);
        if self.gain_clock != Some(clock) {
            self.gain_clock = Some(clock);
            self.gain_alphas = clock.0.map(|ms| alpha(ms, rate, frames));
        }

        let [fast, slow] = self.detect.finish_interval(frames);
        let [reference, _] = self.relative.finish_interval(frames);
        self.level = self.detect.key.map(|_| {
            let absolute = power_db(fast);
            let mut level = absolute;
            if self.relative.key.is_some() {
                let detect_density = fast / self.detect.bandwidth;
                let reference_density = reference / self.relative.bandwidth;
                let relative =
                    RELATIVE_ORIGIN_DB + power_db(detect_density) - power_db(reference_density);
                level += control.relative_amount * (relative - level);
            }
            if control.onsets_amount > 0.0 {
                let onsets = power_db((fast - slow).max(0.0));
                level += control.onsets_amount * (onsets - level);
            }
            level
        });
        let target = self.level.map_or(0.0, |level| control.target(level));
        let attack = (self.gain == 0.0 || self.gain.signum() == target.signum())
            && target.abs() > self.gain.abs();
        self.gain += self.gain_alphas[usize::from(!attack)] * (target - self.gain);
    }

    #[cfg(test)]
    fn process(
        &mut self,
        source: [f32; 2],
        control: DynamicControl,
        rate: f32,
        frequency: f32,
        q: f32,
        kind: FilterKind,
    ) {
        if self.prepare(control, rate, frequency, q, kind, false) {
            self.push(source, [0.0; 2], control.onsets_amount > 0.0);
        }
        self.advance(control, rate, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn above_below_rules() {
        let control = DynamicControl {
            threshold: -20.0,
            above: GainRule {
                enabled: true,
                range: -4.0,
                ratio: 0.5,
            },
            below: GainRule {
                enabled: true,
                range: 9.0,
                ratio: 0.25,
            },
            ..Default::default()
        };
        for (level, expected) in [
            (-4.0, -4.0),
            (-16.0, -2.0),
            (-20.0, 0.0),
            (-28.0, 2.0),
            (-80.0, 9.0),
        ] {
            assert_eq!(control.target(level), expected);
        }
        let inverse = DynamicControl {
            above: GainRule {
                range: 4.0,
                ..control.above
            },
            below: GainRule {
                range: -9.0,
                ..control.below
            },
            ..control
        };
        assert_eq!(inverse.target(-16.0), 2.0);
        assert_eq!(inverse.target(-28.0), -2.0);
    }

    #[test]
    fn onsets_rise_and_release() {
        let mut control = DynamicControl {
            enabled: true,
            attack_ms: 1.0,
            release_ms: 30.0,
            onsets_amount: 1.0,
            ..Default::default()
        };
        control.detect.mode = DetectorMode::Broadband;
        let mut dynamic = Dynamics::new(48_000.0);
        let mut peak = 0.0_f32;
        for i in 0..24_000 {
            let x = if i < 2_400 { 0.0 } else { 0.25 };
            dynamic.process([x; 2], control, 48_000.0, 1_000.0, 1.0, FilterKind::Peak);
            peak = peak.min(dynamic.gain);
        }
        assert!(peak < -3.0);
        assert!(dynamic.gain.abs() < 0.02);
    }

    /// A detector responds to its configured band and averages stereo power.
    #[test]
    fn stereo_band_detection() {
        for (mode, frequency, amplitude, antiphase, expected) in [
            (DetectorMode::Follow, 1_000.0, 0.5, false, -6.0),
            (DetectorMode::Follow, 1_000.0, 0.5, true, -6.0),
            (DetectorMode::Follow, 1_000.0, 0.001, false, 0.0),
            (DetectorMode::Follow, 4_000.0, 0.5, false, 0.0),
            (DetectorMode::Free, 4_000.0, 0.5, false, -6.0),
        ] {
            let mut control = DynamicControl {
                enabled: true,
                ..Default::default()
            };
            control.detect.mode = mode;
            control.detect.frequency = 4_000.0;
            control.detect.q = 8.0;
            let mut dynamic = Dynamics::new(48000.0);
            for i in 0..48000 {
                let x = amplitude * (TAU * frequency * i as f32 / 48000.0).sin();
                dynamic.process(
                    [x, if antiphase { -x } else { x }],
                    control,
                    48000.0,
                    1000.0,
                    3.0,
                    FilterKind::Peak,
                );
            }
            assert!(
                (dynamic.gain - expected).abs() < 0.02,
                "{mode:?} at {frequency} Hz: {}",
                dynamic.gain
            );
        }
    }
}
