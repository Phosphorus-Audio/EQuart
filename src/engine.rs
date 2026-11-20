use crate::analyzer::Analyzer;
use crate::audio_path::{AudioPath, OutputMix};
use crate::band::{BAND_COUNT, BandControl, COEFF_FRAMES, CoefficientTiming};
use crate::dsp;
use crate::dynamics::{Dynamics, Sidechain};
use crate::oversample::Oversampling;
use crate::pitch::PitchRack;
use crate::precision::Precision;
use crate::region::{REGION_COUNT, RegionControl};

mod host;
pub(crate) use host::HostSample;

pub struct Controls {
    pub bands: [BandControl; BAND_COUNT],
    pub regions: [RegionControl; REGION_COUNT],
    pub output_db: f32,
    pub mix: f32,
    pub bypass: bool,
    pub oversampling: Oversampling,
    pub precision: Precision,
}

pub struct Engine {
    /// Sample rate the host runs at.
    pub rate: f32,
    single: Box<AudioPath<f32>>,
    double: Box<AudioPath<f64>>,
    precision: Precision,
    /// Two detector envelopes per band, and the dB offset they last produced.
    /// The detectors watch the dry input; the offsets are folded into the
    /// coefficient targets once per COEFF_FRAMES.
    dynamics: [Dynamics; BAND_COUNT],
    dynamic_gains: [f32; BAND_COUNT],
    detectors_live: bool,
    detector_frames: usize,
    pitch: PitchRack,
    pub(crate) resonance: crate::region::resonance::Detector,
    resonance_live: bool,
    pub(crate) delta_request: Option<usize>,
    delta_region: Option<usize>,
    delta_mix: f32,
    mix: f32,
    initialize_mix: bool,
    /// Frames since the last coefficient update.
    frame: usize,
    alpha: f32,
    step: f32,
    decay: f32,
    block_input: f32,
    block_output: f32,
    pub input_peak: f32,
    pub output_peak: f32,
    pub analyze: bool,
    pub analyzer: Analyzer,
    pub output_analyzer: Analyzer,
    /// Set when a non-finite sample is seen, so the host can be told.
    pub fault: Option<&'static str>,
    /// Debug builds only: synthesise input so the analyser can be inspected
    /// without a host. Enabled by touching `target/test-signal`. Tests feed
    /// their own signals, so they never take this path.
    #[cfg(all(debug_assertions, not(test)))]
    test_signal: bool,
}

impl Engine {
    pub fn new(rate: f32) -> Self {
        Self {
            rate,
            single: Box::new(AudioPath::new()),
            double: Box::new(AudioPath::new()),
            precision: Precision::Single,
            dynamics: std::array::from_fn(|_| Dynamics::new(rate)),
            dynamic_gains: [0.0; BAND_COUNT],
            detectors_live: false,
            detector_frames: 0,
            pitch: PitchRack::new(rate),
            resonance: crate::region::resonance::Detector::new(),
            resonance_live: false,
            delta_request: None,
            delta_region: None,
            delta_mix: 0.0,
            mix: 1.0,
            initialize_mix: true,
            frame: 0,
            alpha: 1.0 - (-(COEFF_FRAMES as f32) / (0.012 * rate)).exp(),
            step: COEFF_FRAMES as f32 / (0.008 * rate),
            decay: (-(COEFF_FRAMES as f32) / (0.35 * rate)).exp(),
            block_input: 0.0,
            block_output: 0.0,
            input_peak: 0.0,
            output_peak: 0.0,
            analyze: true,
            analyzer: Analyzer::new(rate),
            output_analyzer: Analyzer::new(rate),
            fault: None,
            #[cfg(all(debug_assertions, not(test)))]
            test_signal: std::path::Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/target/test-signal"
            ))
            .exists(),
        }
    }

    pub fn reset(&mut self) {
        self.single.reset();
        self.double.reset();
        for dynamic in &mut self.dynamics {
            dynamic.reset();
        }
        self.pitch.reset();
        self.resonance.reset();
        self.resonance_live = false;
        self.delta_request = None;
        self.delta_region = None;
        self.delta_mix = 0.0;
        self.dynamic_gains.fill(0.0);
        self.detectors_live = false;
        self.detector_frames = 0;
        self.analyzer.reset();
        self.output_analyzer.reset();
        self.frame = 0;
        self.initialize_mix = true;
        self.fault = None;
        self.input_peak = 0.0;
        self.output_peak = 0.0;
        self.block_input = 0.0;
        self.block_output = 0.0;
    }

    /// Follow a sample rate the host never announced in `activate`.
    ///
    /// The smoother coefficients, the band cascades and the analyser's
    /// frequency mapping all depend on the rate. A host that starts processing
    /// at a different rate than it activated at would otherwise leave the
    /// analyser mapping FFT bins as if they were at the old rate, which reads
    /// silence for everything above a fraction of the spectrum.
    pub fn retune(&mut self, rate: f32) {
        if !rate.is_finite() || rate < 8_000.0 || (rate - self.rate).abs() < 0.5 {
            return;
        }

        self.rate = rate;
        self.resonance.reset();
        self.detector_frames = 0;
        self.alpha = 1.0 - (-(COEFF_FRAMES as f32) / (0.012 * rate)).exp();
        self.step = COEFF_FRAMES as f32 / (0.008 * rate);
        self.decay = (-(COEFF_FRAMES as f32) / (0.35 * rate)).exp();

        self.single.invalidate_bands();
        self.double.invalidate_bands();
        for dynamic in &mut self.dynamics {
            dynamic.retune(rate);
        }

        self.analyzer.retune(rate);
        self.output_analyzer.retune(rate);
    }

    /// Latency of the oversampled path, in frames of the host's rate.
    pub fn latency(&self) -> u32 {
        match self.precision {
            Precision::Single => self.single.resampler.latency(),
            Precision::Double => self.double.resampler.latency(),
        }
    }

    pub(crate) fn set_oversampling(&mut self, mode: Oversampling) {
        let changed = match self.precision {
            Precision::Single => self.single.set_oversampling(mode),
            Precision::Double => self.double.set_oversampling(mode),
        };

        if changed {
            self.frame = 0;
        }
    }

    fn set_precision(&mut self, precision: Precision) {
        if self.precision == precision {
            return;
        }

        match precision {
            Precision::Single => self.single.transfer_from(&self.double),
            Precision::Double => self.double.transfer_from(&self.single),
        }

        self.precision = precision;
    }

    pub fn harmonic_hz(&self, index: usize) -> Option<f32> {
        self.pitch.harmonic_hz(index)
    }

    /// live dB offset per band for UI
    pub fn dynamic_gains(&self) -> &[f32; BAND_COUNT] {
        &self.dynamic_gains
    }

    pub fn dynamic_levels(&self) -> impl Iterator<Item = Option<f32>> + '_ {
        self.dynamics.iter().map(|dynamic| dynamic.level)
    }

    pub fn pitch_levels(&self) -> impl Iterator<Item = (Option<f32>, Option<f32>)> + '_ {
        (0..BAND_COUNT).map(|i| {
            let frequency = match self.precision {
                Precision::Single => self.single.bands[i].frequency,
                Precision::Double => self.double.bands[i].frequency,
            };
            (
                self.pitch.detected_band(i),
                self.pitch.band_target(i).map(|_| frequency),
            )
        })
    }

    /// integrate detector power on dry audio, update gains at coeff clock
    fn detect(
        &mut self,
        left: &[f32],
        right: &[f32],
        sidechain: Option<Sidechain<'_>>,
        controls: &Controls,
    ) {
        let rate = self.rate;
        let boundary = self.frame + left.len() == COEFF_FRAMES;
        let elapsed = self.detector_frames + left.len();
        self.detector_frames = if boundary { 0 } else { elapsed };
        self.detectors_live = false;
        for (i, (dynamic, band)) in self.dynamics.iter_mut().zip(&controls.bands).enumerate() {
            let mut control = band.dynamics;
            control.enabled &= band.enabled && !controls.bypass;
            let active = dynamic.prepare(
                control,
                rate,
                self.pitch.band_target(i).unwrap_or(band.frequency),
                band.q,
                band.kind,
                sidechain.is_some(),
            );
            if active {
                for (frame, (&l, &r)) in left.iter().zip(right).enumerate() {
                    let reference = sidechain.map_or([0.0; 2], |[sl, sr]| [sl[frame], sr[frame]]);
                    dynamic.push([l, r], reference, control.onsets_amount > 0.0);
                }
            }
            if boundary {
                if active || dynamic.gain.abs() > 1e-4 {
                    dynamic.advance(control, rate, elapsed);
                } else {
                    dynamic.gain = 0.0;
                }
                self.dynamic_gains[i] = dynamic.gain;
            }
            self.detectors_live |= active || dynamic.gain != 0.0;
        }
        if !self.detectors_live {
            self.detector_frames = 0;
        }
    }

    /// process stereo planar audio in-place
    pub fn process_block_with_sidechain<T: HostSample>(
        &mut self,
        left: &mut [T],
        right: &mut [T],
        sidechain: Option<Sidechain<'_, T>>,
        controls: &Controls,
    ) {
        #[cfg(all(debug_assertions, not(test)))]
        if self.test_signal {
            test_signal(left, right);
        }

        let frames = left.len().min(right.len());
        let sidechain = sidechain.filter(|[l, r]| l.len() >= frames && r.len() >= frames);
        let detects = !controls.bypass
            && controls
                .bands
                .iter()
                .any(|band| band.enabled && band.kind.has_gain() && band.dynamics.enabled);

        let resonance_live = !controls.bypass
            && controls
                .regions
                .iter()
                .any(|r| r.enabled && !r.bypass && r.resonance.enabled && r.resonance.depth > 0.0);

        if self.resonance_live && !resonance_live {
            self.resonance.reset();
        }
        self.resonance_live = resonance_live;
        self.pitch.configure(
            self.rate,
            &controls.bands,
            controls.bypass,
            controls.regions.map(RegionControl::harmonic_detection),
        );
        let analyze = self.analyze;

        let precision = if T::NATIVE_DOUBLE {
            Precision::Double
        } else {
            controls.precision
        };
        self.set_precision(precision);
        self.set_oversampling(controls.oversampling);
        let requested = self.delta_request.filter(|&index| {
            !controls.bypass
                && controls
                    .regions
                    .get(index)
                    .is_some_and(|r| r.enabled && !r.bypass && r.resonance.enabled)
        });

        let fade_step = 1.0 / (0.005 * self.rate);

        if controls.bypass {
            self.delta_region = None;
            self.delta_mix = 0.0;
        }

        let mut analysis_l = [0.0; COEFF_FRAMES];
        let mut analysis_r = [0.0; COEFF_FRAMES];
        let mut side_l = [0.0; COEFF_FRAMES];
        let mut side_r = [0.0; COEFF_FRAMES];
        let mut offset = 0;
        while offset < frames {
            // split at coeff-update boundaries
            let end = (offset + (COEFF_FRAMES - self.frame)).min(frames);
            let left = &mut left[offset..end];
            let right = &mut right[offset..end];
            if self.frame == 0 {
                self.refresh(controls);
            }
            let left_analysis = T::analysis(left, &mut analysis_l);
            let right_analysis = T::analysis(right, &mut analysis_r);
            self.pitch.process(left_analysis, right_analysis);
            if resonance_live {
                self.resonance
                    .push(left_analysis, right_analysis, self.rate, &controls.regions);
            }

            if detects || self.detectors_live {
                let sidechain = sidechain.map(|[l, r]| {
                    [
                        T::analysis(&l[offset..end], &mut side_l),
                        T::analysis(&r[offset..end], &mut side_r),
                    ]
                });
                self.detect(left_analysis, right_analysis, sidechain, controls);
            }

            if analyze {
                for (l, r) in left_analysis.iter().zip(right_analysis) {
                    self.analyzer.push([*l, *r]);
                }
            }

            let input_peak = dsp::peak(left, right);

            if self.delta_mix == 0.0 && self.delta_region != requested {
                self.delta_region = requested;
                if requested.is_some() {
                    match self.precision {
                        Precision::Single => self.single.resampler.reset_delta(),
                        Precision::Double => self.double.resampler.reset_delta(),
                    }
                }
            }
            let output = OutputMix {
                wet: if controls.bypass { 0.0 } else { self.mix },
                delta: self.delta_region.map(|index| {
                    let step = if requested == self.delta_region {
                        fade_step
                    } else {
                        -fade_step
                    };
                    (index, step)
                }),
            };

            T::render(self, left, right, controls, output);

            if analyze {
                let left = T::analysis(left, &mut analysis_l);
                let right = T::analysis(right, &mut analysis_r);
                for (l, r) in left.iter().zip(right) {
                    self.output_analyzer.push([*l, *r]);
                }
            }

            let output_peak = dsp::peak(left, right);
            if !input_peak.is_finite() {
                self.fault = Some("equart: non-finite audio input");
            } else if !output_peak.is_finite() {
                self.fault = Some("equart: non-finite DSP output");
            }

            self.block_input = self.block_input.max(input_peak.double() as f32);
            self.block_output = self.block_output.max(output_peak.double() as f32);

            self.frame += end - offset;
            if self.frame >= COEFF_FRAMES {
                self.frame = 0;
                self.input_peak = (self.input_peak * self.decay).max(self.block_input);
                self.output_peak = (self.output_peak * self.decay).max(self.block_output);
                self.block_input = 0.0;
                self.block_output = 0.0;
            }

            offset = end;
        }
    }

    /// Duplicate a mono buffer into both channels before filtering.
    pub fn process_mono<T: HostSample>(
        &mut self,
        buf: &mut [T],
        sidechain: Option<Sidechain<'_, T>>,
        controls: &Controls,
    ) {
        const TILE: usize = 512;

        let mut right = [T::zero(); TILE];
        let sidechain = sidechain.filter(|[l, r]| l.len() >= buf.len() && r.len() >= buf.len());
        for (index, tile) in buf.chunks_mut(TILE).enumerate() {
            let right = &mut right[..tile.len()];
            right.copy_from_slice(tile);
            let start = index * TILE;
            let end = start + tile.len();
            let sidechain = sidechain.map(|[l, r]| [&l[start..end], &r[start..end]]);
            self.process_block_with_sidechain(tile, right, sidechain, controls);
        }
    }

    #[cfg(test)]
    pub fn process_block(&mut self, left: &mut [f32], right: &mut [f32], controls: &Controls) {
        self.process_block_with_sidechain(left, right, None, controls);
    }

    /// Convenience wrapper for one frame; used by the tests.
    #[cfg(test)]
    pub fn process_frame(&mut self, input: [f32; 2], controls: &Controls) -> [f32; 2] {
        let mut left = [input[0]];
        let mut right = [input[1]];
        self.process_block(&mut left, &mut right, controls);
        [left[0], right[0]]
    }

    fn refresh(&mut self, controls: &Controls) {
        let mode = match self.precision {
            Precision::Single => self.single.resampler.mode(),
            Precision::Double => self.double.resampler.mode(),
        };

        let rate = self.rate * mode.factor() as f32;
        let timing = CoefficientTiming {
            alpha: self.alpha,
            fade_step: self.step,
            frames: COEFF_FRAMES * mode.factor(),
        };

        for (i, &control) in controls.bands.iter().enumerate() {
            let mut control = control;
            if let Some(hz) = self
                .pitch
                .refresh_band(i, &control, controls.bypass, self.rate)
            {
                control.frequency = hz;
            }
            if control.kind.has_gain() {
                control.gain += self.dynamic_gains[i];
            }
            match self.precision {
                Precision::Single => self.single.bands[i].update(control, rate, timing),
                Precision::Double => self.double.bands[i].update(control, rate, timing),
            }
        }
        for (index, (&control, reductions)) in controls
            .regions
            .iter()
            .zip(self.resonance.reductions())
            .enumerate()
        {
            let harmonic_hz =
                self.pitch
                    .refresh_harmonic(index, control.harmonic_detection(), self.rate);

            match self.precision {
                Precision::Single => self.single.regions[index].update(
                    control,
                    rate,
                    harmonic_hz,
                    timing,
                    reductions,
                ),
                Precision::Double => self.double.regions[index].update(
                    control,
                    rate,
                    harmonic_hz,
                    timing,
                    reductions,
                ),
            }
        }

        match self.precision {
            Precision::Single => self
                .single
                .refresh_output_gain(controls.output_db, self.alpha),
            Precision::Double => self
                .double
                .refresh_output_gain(controls.output_db, self.alpha),
        }
        if self.initialize_mix {
            self.mix = controls.mix;
            self.initialize_mix = false;
        } else {
            self.mix += (controls.mix - self.mix).clamp(-self.step, self.step);
        }
    }
}

/// debug-only ~-14 dBFS pink noise
#[cfg(debug_assertions)]
pub(crate) fn test_signal<T: crate::precision::Sample>(left: &mut [T], right: &mut [T]) {
    use std::cell::Cell;

    thread_local! {
        static SEED: Cell<u32> = const { Cell::new(0x1234_5678) };
        static PINK: [Cell<f32>; 3] = const { [const { Cell::new(0.0) }; 3] };
    }

    for (l, r) in left.iter_mut().zip(right.iter_mut()) {
        let white = SEED.with(|seed| {
            let mut value = seed.get();
            value ^= value << 13;
            value ^= value >> 17;
            value ^= value << 5;
            seed.set(value);
            (value as f32 / u32::MAX as f32) * 2.0 - 1.0
        });

        // Paul Kellet's three pole approximation of -3 dB/octave.
        let sample = PINK.with(|state| {
            let b0 = 0.99765 * state[0].get() + white * 0.099_046_0;
            let b1 = 0.963_00 * state[1].get() + white * 0.296_516_4;
            let b2 = 0.570_00 * state[2].get() + white * 1.052_691_3;
            state[0].set(b0);
            state[1].set(b1);
            state[2].set(b2);
            (b0 + b1 + b2 + white * 0.1848) * 0.12
        });

        *l = T::value(f64::from(sample));
        *r = *l;
    }
}
