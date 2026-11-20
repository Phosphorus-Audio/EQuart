use crate::analyzer::Analyzer;
use crate::band::{BAND_COUNT, Band, BandControl, COEFF_FRAMES};
use crate::dsp;
use crate::dynamics::{Dynamics, Sidechain};
use crate::oversample::{DownsamplePath, HIGH, Oversampling, Resampler};
use crate::pitch::PitchRack;
use crate::region::{REGION_COUNT, Region, RegionControl};

pub struct Controls {
    pub bands: [BandControl; BAND_COUNT],
    pub regions: [RegionControl; REGION_COUNT],
    pub output_db: f32,
    pub mix: f32,
    pub bypass: bool,
    pub oversampling: Oversampling,
}

pub struct Engine {
    /// Sample rate the host runs at.
    pub rate: f32,
    bands: [Band; BAND_COUNT],
    regions: [Region; REGION_COUNT],
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
    /// Smoothed linear output gain, and the value it is heading for.
    output_gain: f32,
    target_gain: f32,
    target_db: f32,
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
    resampler: Resampler,
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
            bands: std::array::from_fn(|_| Band::new()),
            regions: std::array::from_fn(|_| Region::new()),
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
            output_gain: 1.0,
            target_gain: 1.0,
            target_db: 0.0,
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
            resampler: Resampler::default(),
            #[cfg(all(debug_assertions, not(test)))]
            test_signal: std::path::Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/target/test-signal"
            ))
            .exists(),
        }
    }

    pub fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
        for region in &mut self.regions {
            region.reset();
        }
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
        self.resampler.reset();
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

        for band in &mut self.bands {
            band.invalidate();
        }
        for dynamic in &mut self.dynamics {
            dynamic.retune(rate);
        }

        self.analyzer.retune(rate);
        self.output_analyzer.retune(rate);
    }

    /// Latency of the oversampled path, in frames of the host's rate.
    pub fn latency(&self) -> u32 {
        self.resampler.latency()
    }

    pub(crate) fn set_oversampling(&mut self, mode: Oversampling) {
        if self.resampler.set_mode(mode) {
            for band in &mut self.bands {
                band.invalidate();
            }
            self.frame = 0;
        }
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
        self.bands.iter().enumerate().map(|(i, band)| {
            (
                self.pitch.detected_band(i),
                self.pitch.band_target(i).map(|_| band.frequency),
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
    pub fn process_block_with_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        sidechain: Option<Sidechain<'_>>,
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
        let resonance_live = !controls.bypass && controls.regions.iter().any(|r|
            r.enabled && !r.bypass && r.resonance.enabled && r.resonance.depth > 0.0);
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
        let oversampling = controls.oversampling;
        let factor = oversampling.factor();
        self.set_oversampling(oversampling);

        // Wide buffers, written by the interpolator before the biquads see them.
        let mut wide_left = [0.0_f32; HIGH];
        let mut wide_right = [0.0_f32; HIGH];
        let mut dry_left = [0.0; COEFF_FRAMES];
        let mut dry_right = [0.0; COEFF_FRAMES];
        let mut delta_wide_l = [0.0; HIGH];
        let mut delta_wide_r = [0.0; HIGH];
        let mut delta_left = [0.0; COEFF_FRAMES];
        let mut delta_right = [0.0; COEFF_FRAMES];
        let requested = self.delta_request.filter(|&index| {
            !controls.bypass && controls.regions.get(index)
                .is_some_and(|r| r.enabled && !r.bypass && r.resonance.enabled)
        });
        let fade_step = 1.0 / (0.005 * self.rate);
        if controls.bypass {
            self.delta_region = None;
            self.delta_mix = 0.0;
        }

        let mut offset = 0;
        while offset < frames {
            // split at coeff-update boundaries
            let end = (offset + (COEFF_FRAMES - self.frame)).min(frames);
            let left = &mut left[offset..end];
            let right = &mut right[offset..end];
            let len = left.len();

            if self.frame == 0 {
                self.refresh(controls);
            }
            self.pitch.process(left, right);
            if resonance_live {
                self.resonance.push(left, right, self.rate, &controls.regions);
            }
            if detects || self.detectors_live {
                let sidechain = sidechain.map(|[l, r]| [&l[offset..end], &r[offset..end]]);
                self.detect(left, right, sidechain, controls);
            }

            if analyze {
                for (l, r) in left.iter().zip(right.iter()) {
                    self.analyzer.push([*l, *r]);
                }
            }

            let input_peak = dsp::peak(left, right);

            let wet = if controls.bypass { 0.0 } else { self.mix };
            let dry_l = &mut dry_left[..len];
            let dry_r = &mut dry_right[..len];
            if oversampling != Oversampling::Off || wet < 1.0 {
                self.resampler.delayed_dry(left, right, dry_l, dry_r);
            }

            if self.delta_mix == 0.0 && self.delta_region != requested {
                self.delta_region = requested;
                if requested.is_some() {
                    self.resampler.reset_delta();
                }
            }
            let monitor = self.delta_region;
            // Keep filter and FIR history continuous across dry/wet transitions.
            if oversampling == Oversampling::Off {
                let tap = monitor.map(|i| (i, &mut delta_left[..len], &mut delta_right[..len]));
                self.process_filters(left, right, &controls.bands, tap);
            } else {
                let wide = len * factor;
                self.resampler
                    .upsample(left, right, &mut wide_left, &mut wide_right);
                let tap = monitor.map(|i| (i, &mut delta_wide_l[..wide], &mut delta_wide_r[..wide]));
                self.process_filters(
                    &mut wide_left[..wide],
                    &mut wide_right[..wide],
                    &controls.bands,
                    tap,
                );
                self.resampler.downsample(
                    DownsamplePath::Main,
                    &wide_left[..wide],
                    &wide_right[..wide],
                    left,
                    right,
                    len,
                );
                if monitor.is_some() {
                    self.resampler.downsample(
                        DownsamplePath::Delta,
                        &delta_wide_l[..wide],
                        &delta_wide_r[..wide],
                        &mut delta_left[..len],
                        &mut delta_right[..len],
                        len,
                    );
                }
            }

            if wet == 0.0 {
                left.copy_from_slice(dry_l);
                right.copy_from_slice(dry_r);
            } else {
                if self.output_gain != 1.0 {
                    dsp::scale(left, self.output_gain);
                    dsp::scale(right, self.output_gain);
                }
                if wet < 1.0 {
                    dsp::blend(dry_l, left, wet);
                    dsp::blend(dry_r, right, wet);
                }
            }

            if monitor.is_some() {
                let step = if requested == monitor { fade_step } else { -fade_step };
                for i in 0..len {
                    self.delta_mix = (self.delta_mix + step).clamp(0.0, 1.0);
                    if self.delta_mix == 1.0 {
                        left[i] = delta_left[i];
                        right[i] = delta_right[i];
                    } else {
                        left[i] += self.delta_mix * (delta_left[i] - left[i]);
                        right[i] += self.delta_mix * (delta_right[i] - right[i]);
                    }
                }
            }

            if analyze {
                for (l, r) in left.iter().zip(right.iter()) {
                    self.output_analyzer.push([*l, *r]);
                }
            }

            let output_peak = dsp::peak(left, right);
            if !input_peak.is_finite() {
                self.fault = Some("equart: non-finite audio input");
            } else if !output_peak.is_finite() {
                self.fault = Some("equart: non-finite DSP output");
            }

            self.block_input = self.block_input.max(input_peak);
            self.block_output = self.block_output.max(output_peak);

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

    fn process_filters(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        controls: &[BandControl; BAND_COUNT],
        tap: Option<(usize, &mut [f32], &mut [f32])>,
    ) {
        for (band, control) in self.bands.iter_mut().zip(controls) {
            band.process(left, right, control.channel);
        }
        let remaining = if let Some((index, delta_l, delta_r)) = tap {
            let (before, from_target) = self.regions.split_at_mut(index);
            for region in before {
                region.process(left, right);
            }
            from_target[0].process_with_delta(left, right, delta_l, delta_r);
            &mut from_target[1..]
        } else {
            &mut self.regions[..]
        };
        for region in remaining {
            region.process(left, right);
        }
    }

    /// Duplicate a mono buffer into both channels before filtering.
    pub fn process_mono(
        &mut self,
        buf: &mut [f32],
        sidechain: Option<Sidechain<'_>>,
        controls: &Controls,
    ) {
        const TILE: usize = 512;

        let mut right = [0.0_f32; TILE];
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
        let rate = self.rate * self.resampler.mode().factor() as f32;

        for (i, (band, &control)) in self.bands.iter_mut().zip(&controls.bands).enumerate() {
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
            band.update(control, rate, self.alpha, self.step);
        }
        for (index, ((region, &control), reductions)) in self
            .regions
            .iter_mut()
            .zip(&controls.regions)
            .zip(self.resonance.reductions())
            .enumerate()
        {
            let harmonic_hz =
                self.pitch.refresh_harmonic(index, control.harmonic_detection(), self.rate);
            region.update(control, rate, harmonic_hz, self.alpha, self.step, reductions);
        }

        // `powf` only when the target actually moves, not once per block.
        if controls.output_db != self.target_db {
            self.target_db = controls.output_db;
            self.target_gain = 10.0_f32.powf(self.target_db / 20.0);
        }

        self.output_gain += self.alpha * (self.target_gain - self.output_gain);
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
pub(crate) fn test_signal(left: &mut [f32], right: &mut [f32]) {
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

        *l = sample;
        *r = sample;
    }
}
