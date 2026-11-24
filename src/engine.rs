use crate::audio_path::{AudioPath, OutputMix};
use crate::band::{BAND_COUNT, BandControl, COEFF_FRAMES, CoefficientTiming};
use crate::dsp;
use crate::dynamics::{Dynamics, Sidechain};
use crate::oversample::Oversampling;
use crate::pitch::PitchRack;
use crate::precision::Precision;
use crate::processing_mode::LinearQuality;
use crate::region::{REGION_COUNT, RegionControl};

pub(crate) mod audition;
mod convolution;
mod host;
mod linear;
pub(crate) use host::HostSample;

pub struct Controls {
    pub bands: [BandControl; BAND_COUNT],
    pub regions: [RegionControl; REGION_COUNT],
    pub output_db: f32,
    pub pan: f32,
    pub mix: f32,
    pub bypass: bool,
    pub oversampling: Oversampling,
    pub linear_phase: Option<LinearQuality>,
    pub precision: Precision,
}

pub struct Engine {
    /// Sample rate the host runs at.
    pub rate: f32,
    single: Box<AudioPath<f32>>,
    double: Box<AudioPath<f64>>,
    precision: Precision,
    linear: linear::LinearEq,
    linear_phase: Option<LinearQuality>,
    /// Two detector envelopes per band, and the dB offset they last produced.
    /// The detectors watch the dry input; the offsets are folded into the
    /// coefficient targets once per COEFF_FRAMES.
    dynamics: [Dynamics; BAND_COUNT],
    dynamic_gains: [f32; BAND_COUNT],
    detectors_live: bool,
    detector_frames: usize,
    pitch: PitchRack,
    modulation: crate::modulation::Rack,
    pub(crate) resonance: crate::region::resonance::Detector,
    region_responses: [crate::region::response::Slew; REGION_COUNT],
    pub(crate) region_dynamics: crate::region::dynamics::Rack,
    pub(crate) region_dynamics_observed: Option<(usize, bool)>,
    effective_regions: [RegionControl; REGION_COUNT],
    resonance_live: bool,
    pub(crate) delta_request: Option<usize>,
    pub(crate) audition_request: Option<audition::Target>,
    audition: audition::Audition,
    delta_region: Option<usize>,
    delta_mix: f32,
    mix: f32,
    initialize_mix: bool,
    pan: Option<f64>,
    /// Frames since the last coefficient update.
    frame: usize,
    alpha: f32,
    step: f32,
    decay: f32,
    block_input: f32,
    block_output: f32,
    pub input_peak: f32,
    pub output_peak: f32,
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
            linear: linear::LinearEq::new(),
            linear_phase: None,
            dynamics: std::array::from_fn(|_| Dynamics::new(rate)),
            dynamic_gains: [0.0; BAND_COUNT],
            detectors_live: false,
            detector_frames: 0,
            pitch: PitchRack::new(rate),
            modulation: Default::default(),
            resonance: crate::region::resonance::Detector::new(),
            region_responses: Default::default(),
            region_dynamics: crate::region::dynamics::Rack::new(),
            region_dynamics_observed: None,
            effective_regions: Default::default(),
            resonance_live: false,
            delta_request: None,
            audition_request: None,
            audition: Default::default(),
            delta_region: None,
            delta_mix: 0.0,
            mix: 1.0,
            initialize_mix: true,
            pan: None,
            frame: 0,
            alpha: 1.0 - (-(COEFF_FRAMES as f32) / (0.012 * rate)).exp(),
            step: COEFF_FRAMES as f32 / (0.008 * rate),
            decay: (-(COEFF_FRAMES as f32) / (0.35 * rate)).exp(),
            block_input: 0.0,
            block_output: 0.0,
            input_peak: 0.0,
            output_peak: 0.0,
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
        self.linear.reset();
        for dynamic in &mut self.dynamics {
            dynamic.reset();
        }
        self.pitch.reset();
        self.modulation = Default::default();
        self.resonance.reset();
        self.region_responses = Default::default();
        self.region_dynamics.reset();
        self.effective_regions = Default::default();
        self.resonance_live = false;
        self.delta_request = None;
        self.audition_request = None;
        self.audition = Default::default();
        self.delta_region = None;
        self.delta_mix = 0.0;
        self.dynamic_gains.fill(0.0);
        self.detectors_live = false;
        self.detector_frames = 0;
        self.frame = 0;
        self.initialize_mix = true;
        self.pan = None;
        self.fault = None;
        self.input_peak = 0.0;
        self.output_peak = 0.0;
        self.block_input = 0.0;
        self.block_output = 0.0;
    }

    /// Follow a sample rate the host never announced in `activate`.
    ///
    /// update the filter and detector clocks to the rate used for processing
    pub fn retune(&mut self, rate: f32) {
        if !rate.is_finite() || rate < 8_000.0 || (rate - self.rate).abs() < 0.5 {
            return;
        }

        self.rate = rate;
        self.linear.set_rate(rate);
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
    }

    /// processing latency in host samples
    pub fn latency(&self) -> u32 {
        if let Some(quality) = self.linear_phase {
            return linear::latency(quality.center_at(self.rate)) as u32;
        }

        match self.precision {
            Precision::Single => self.single.resampler.latency(),
            Precision::Double => self.double.resampler.latency(),
        }
    }

    pub(crate) fn linear_response_revision(&self) -> u64 {
        self.linear_phase
            .map_or(0, |_| self.linear.response_revision())
    }

    pub(crate) fn linear_response(&self) -> Option<[Option<&[f64]>; 4]> {
        self.linear_phase.and_then(|_| self.linear.response())
    }

    pub(crate) fn tail(&self) -> u32 {
        self.linear_phase
            .map_or(0, |quality| linear::tail(quality.center_at(self.rate)))
    }

    pub(crate) fn set_processing(
        &mut self,
        linear_phase: Option<LinearQuality>,
        oversampling: Oversampling,
    ) {
        if self.linear_phase != linear_phase {
            self.linear_phase = linear_phase;
            self.frame = 0;
            self.delta_mix = 0.0;
            self.delta_region = None;

            if let Some(quality) = linear_phase {
                self.linear.set_quality(quality, self.rate);
            } else {
                self.single.reset();
                self.double.reset();
            }
        }

        self.set_oversampling(oversampling);
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

    pub(crate) fn set_transport(&mut self, transport: &nice_plug::prelude::Transport) {
        self.modulation.clock.set(transport);
    }

    pub(crate) fn modulation(&self, index: usize) -> Option<crate::modulation::Live> {
        self.modulation.live(index)
    }

    pub(crate) fn region_response(
        &self,
        index: usize,
    ) -> [crate::region::response::Response; crate::region::response::SLOTS] {
        self.region_responses[index].controls()
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
                self.pitch.band_target(i).map(|tracked| {
                    if self.modulation.live(i).is_some() {
                        tracked
                    } else {
                        frequency
                    }
                }),
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
            control.enabled &= band.enabled;
            let live = self.modulation.live(i);
            let frequency = live.map_or_else(
                || self.pitch.band_target(i).unwrap_or(band.frequency),
                |live| live.frequency,
            );
            let q = live.map_or(band.q, |live| live.q);
            let active =
                dynamic.prepare(control, rate, frequency, q, band.kind, sidechain.is_some());
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
        self.process_channels(left, right, sidechain, controls, true);
    }

    fn process_channels<T: HostSample>(
        &mut self,
        left: &mut [T],
        right: &mut [T],
        sidechain: Option<Sidechain<'_, T>>,
        controls: &Controls,
        stereo: bool,
    ) {
        #[cfg(all(debug_assertions, not(test)))]
        if self.test_signal {
            test_signal(left, right);
        }

        let frames = left.len().min(right.len());
        let target_pan = if controls.bypass {
            0.0
        } else {
            f64::from(controls.pan)
        };
        let sidechain = sidechain.filter(|[l, r]| l.len() >= frames && r.len() >= frames);
        let detects = controls
            .bands
            .iter()
            .any(|band| band.enabled && band.kind.has_gain() && band.dynamics.enabled);

        let resonance_live =
            controls
                .regions
                .iter()
                .zip(&self.effective_regions)
                .any(|(base, effective)| {
                    base.enabled
                        && !base.bypass
                        && base.resonance.enabled
                        && (base.resonance.depth > 0.0
                            || effective.resonance.depth > 0.0
                            || base.responses.iter().any(|effect| {
                                effect.enabled
                                    && effect.scope == crate::region::response::Scope::Whole
                                    && effect.target
                                        == crate::region::response::Value::ResonanceDepth
                                    && !effect.bypass
                                    && effect.strength != 0.0
                                    && effect.valid(*base)
                            }))
                });

        if self.resonance_live && !resonance_live {
            self.resonance.reset();
        }
        self.resonance_live = resonance_live;
        self.pitch.configure(
            self.rate,
            &controls.bands,
            controls.regions.map(RegionControl::harmonic_detection),
        );

        let precision = if T::NATIVE_DOUBLE {
            Precision::Double
        } else {
            controls.precision
        };
        self.set_precision(precision);
        self.set_processing(controls.linear_phase, controls.oversampling);
        let requested = self.delta_request.filter(|&index| {
            !controls.bypass
                && controls
                    .regions
                    .get(index)
                    .is_some_and(|r| r.enabled && !r.bypass && r.resonance.enabled)
        });

        let fade_step = 1.0 / (0.005 * self.rate);

        let mut analysis_l = [0.0; COEFF_FRAMES];
        let mut analysis_r = [0.0; COEFF_FRAMES];
        let mut side_l = [0.0; COEFF_FRAMES];
        let mut side_r = [0.0; COEFF_FRAMES];
        let mut offset = 0;
        while offset < frames {
            // split at coeff-update boundaries
            let mut count = COEFF_FRAMES - self.frame;

            if self.linear_phase.is_some() {
                count = count.min(self.linear.frames_until_hop());
            }

            let end = (offset + count).min(frames);
            let left = &mut left[offset..end];
            let right = &mut right[offset..end];
            if self.frame == 0 {
                self.refresh(controls);
                self.region_dynamics.configure(
                    &self.effective_regions,
                    self.rate,
                    self.latency() as usize,
                );
            }
            if self.linear_phase.is_some() && self.linear.at_boundary() {
                self.snapshot_linear(controls);
            }

            let left_analysis = T::analysis(left, &mut analysis_l);
            let right_analysis = T::analysis(right, &mut analysis_r);
            self.pitch.process(left_analysis, right_analysis);
            if resonance_live {
                self.resonance.push(
                    left_analysis,
                    right_analysis,
                    self.rate,
                    &self.effective_regions,
                );
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

            let external = sidechain.map(|[l, r]| [&l[offset..end], &r[offset..end]]);
            self.region_dynamics.detect(left, right, external);

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
                wet: self.mix,
                bypass: controls.bypass,
                bypass_step: 1.0 / (0.005 * f64::from(self.rate)),
                delta: self.delta_region.map(|index| {
                    let step = if requested == self.delta_region {
                        fade_step
                    } else {
                        -fade_step
                    };
                    (index, step)
                }),
            };

            if self.linear_phase.is_some() {
                let gain = match self.precision {
                    Precision::Single => self.single.output_gain(),
                    Precision::Double => self.double.output_gain(),
                };
                self.linear.process(
                    left,
                    right,
                    output,
                    gain,
                    &mut self.delta_mix,
                    &mut self.region_dynamics,
                );
            } else {
                T::render(self, left, right, controls, output);
            }

            let focus = self
                .audition_request
                .filter(|_| !controls.bypass)
                .and_then(|target| {
                    let focus = match target {
                        audition::Target::Band(index) => {
                            let mut band = *controls.bands.get(index)?;
                            band.frequency =
                                self.pitch.band_target(index).unwrap_or(band.frequency);
                            if let Some(live) = self.modulation.live(index) {
                                live.apply(&mut band);
                            }
                            audition::Focus::band(band, band.frequency)
                        }
                        audition::Target::Region(index) => {
                            audition::Focus::region(self.effective_regions.get(index)?.bounds())
                        }
                    };
                    Some((target, focus))
                });
            self.audition.process(left, right, self.rate, focus);

            if stereo {
                self.apply_pan(left, right, target_pan);
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
            self.modulation.clock.advance(end - offset);
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

    fn apply_pan<T: HostSample>(&mut self, left: &mut [T], right: &mut [T], target: f64) {
        let pan = self.pan.get_or_insert(target);

        if *pan == target {
            if target > 0.0 {
                T::scale(left, T::value(1.0 - target));
            } else if target < 0.0 {
                T::scale(right, T::value(1.0 + target));
            }
            return;
        }

        let step = 1.0 / (0.005 * f64::from(self.rate));
        for (left, right) in left.iter_mut().zip(right) {
            *pan += (target - *pan).clamp(-step, step);
            *left = *left * T::value(1.0 - (*pan).max(0.0));
            *right = *right * T::value(1.0 + (*pan).min(0.0));
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
            self.process_channels(tile, right, sidechain, controls, false);
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

    pub(crate) fn prepare_processing(&mut self, controls: &Controls) {
        self.set_processing(controls.linear_phase, controls.oversampling);

        if self.linear_phase.is_some() {
            for (index, region) in controls.regions.iter().copied().enumerate() {
                let smoothed = self.region_responses[index].update(region, self.alpha);
                self.effective_regions[index] =
                    crate::region::response::whole(smoothed, None, self.rate);
            }

            self.snapshot_linear(controls);
        }
    }

    fn snapshot_linear(&mut self, controls: &Controls) {
        let pending = self.linear.begin_snapshot();

        for (index, &band) in controls
            .bands
            .iter()
            .enumerate()
            .filter(|(_, band)| band.enabled)
        {
            let mut control = band;
            control.frequency = self.pitch.band_target(index).unwrap_or(control.frequency);
            if let Some(live) = self.modulation.live(index) {
                live.apply(&mut control);
            }
            control.lfo = Default::default();
            control.mapping = Default::default();

            if control.kind.has_gain() {
                control.gain += self.dynamic_gains[index];
            }

            pending.push(control);
        }

        let mut delta = None;

        for (index, (&region, reductions)) in self
            .effective_regions
            .iter()
            .zip(self.resonance.reductions())
            .enumerate()
        {
            if !region.enabled || region.bypass {
                continue;
            }

            pending.extend(
                region
                    .filter_plan(self.rate, self.pitch.harmonic_hz(index))
                    .filter(|band| band.enabled),
            );
            let start = pending.len();
            pending.extend(
                reductions
                    .iter()
                    .map(|cut| cut.band(region))
                    .filter(|band| band.enabled),
            );

            if self.delta_region == Some(index) && start < pending.len() {
                delta = Some(start..pending.len());
            }
        }

        self.linear.finish_snapshot(self.rate, delta);
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
            if let Some(hz) = self.pitch.refresh_band(i, &control, self.rate) {
                control.frequency = hz;
            }
            self.modulation
                .update(i, &mut control, self.rate, self.linear_phase.is_some());
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
            let smoothed = self.region_responses[index].update(control, self.alpha);
            let control = crate::region::response::whole(smoothed, harmonic_hz, self.rate);
            self.effective_regions[index] = control;

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
