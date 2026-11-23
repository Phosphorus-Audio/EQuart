use crate::dsp::{self, State};
use crate::dynamics::DynamicControl;
use crate::filter::{Coefficients, CutSlope, FLAT_TILT_SECTIONS, FilterKind};
use crate::precision::Sample;
use nice_plug::prelude::Enum;

pub const BAND_COUNT: usize = 32;
// pub const BAND_COUNT: usize = 48;
pub const SECTIONS: usize = 8;
// pub const SECTIONS: usize = 8;
pub const COEFF_FRAMES: usize = 32;

/// snap threshold to switch back to block kernel
const SNAP: f32 = 2e-5;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelMode {
    #[default]
    #[id = "stereo"]
    #[name = "Stereo"]
    Stereo,
    #[id = "left"]
    #[name = "Left"]
    Left,
    #[id = "right"]
    #[name = "Right"]
    Right,
    #[id = "mid"]
    #[name = "Mid"]
    Mid,
    #[id = "side"]
    #[name = "Side"]
    Side,
}

impl ChannelMode {
    pub const ALL: [Self; 5] = [Self::Stereo, Self::Left, Self::Right, Self::Mid, Self::Side];

    pub fn tag(self) -> &'static str {
        match self {
            Self::Stereo => "ST",
            Self::Left => "L",
            Self::Right => "R",
            Self::Mid => "M",
            Self::Side => "S",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Stereo => "Stereo",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Mid => "Mid",
            Self::Side => "Side",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandControl {
    pub enabled: bool,
    pub frequency: f32,
    pub gain: f32,
    pub q: f32,
    pub kind: FilterKind,
    pub slope: CutSlope,
    pub channel: ChannelMode,
    pub dynamics: DynamicControl,
    pub pitch: crate::pitch::PitchControl,
    pub(crate) mapping: crate::modulation::Mapping,
    pub(crate) lfo: crate::modulation::Lfo,
}

impl Default for BandControl {
    fn default() -> Self {
        Self {
            enabled: true,
            frequency: 1_000.0,
            gain: 0.0,
            q: 0.7,
            kind: FilterKind::Peak,
            slope: CutSlope::default(),
            channel: ChannelMode::Stereo,
            dynamics: DynamicControl::default(),
            pitch: crate::pitch::PitchControl::default(),
            mapping: Default::default(),
            lfo: Default::default(),
        }
    }
}

impl BandControl {
    /// Number of second-order sections this band needs.
    pub fn section_count(&self) -> usize {
        if self.kind.is_cut() || self.kind.is_shelf() {
            self.slope.order().div_ceil(2)
        } else if matches!(
            self.kind,
            FilterKind::Peak | FilterKind::Notch | FilterKind::BandPass
        ) {
            self.slope.biquad_count()
        } else if self.kind == FilterKind::FlatTilt {
            FLAT_TILT_SECTIONS
        } else {
            1
        }
    }

    /// compute biquad coeffs and section count (cuts use Butterworth, steep bells use transformed low shelf)
    pub fn sections(&self, rate: f32) -> ([Coefficients; SECTIONS], usize) {
        self.sections_for(rate)
    }

    pub(crate) fn sections_for<T: Sample>(
        &self,
        rate: f32,
    ) -> ([Coefficients<T>; SECTIONS], usize) {
        // let rate = Coefficients::value(f64::from(rate));
        // let q = Coefficients::value(f64::from(self.q));
        // let gain = Coefficients::value(f64::from(self.gain));
        let rate = T::value(f64::from(rate));
        let q = T::value(f64::from(self.q));
        let gain = T::value(f64::from(self.gain));
        let frequency =
            T::value(f64::from(self.frequency)).clamp(T::value(10.0), rate * T::value(0.49));
        let count = self.section_count();
        let mut sections = [Coefficients::identity(); SECTIONS];

        if self.kind.is_cut() {
            let (_, first_order) = self.slope.sections();
            let base = self.slope.section_q::<T>();
            let resonance = (T::SQRT_2() * q).clamp(T::value(0.05), T::value(40.0));
            let g = (T::PI() * frequency / rate).tan();

            for i in 0..count {
                let is_first_order = first_order && i == count - 1;
                let q = if is_first_order {
                    T::zero()
                } else if i == 0 {
                    (base[0] * resonance).clamp(T::value(0.05), T::value(40.0))
                } else {
                    base[i]
                };
                sections[i] = Coefficients::cut_section(g, q, self.kind, is_first_order);
            }
        } else if self.kind == FilterKind::Peak && count > 1 {
            let g = (T::PI() * frequency / rate).tan();
            Coefficients::bell_sections(g, q, gain, &mut sections[..count]);
        } else if self.kind.is_shelf() && self.slope != CutSlope::Db12 {
            let (pairs, first_order) = self.slope.sections();
            let base = self.slope.section_q::<T>();
            let db_per_order = gain / T::value(self.slope.order() as f64);
            let resonance = q.powf(T::value(0.46));
            for (i, section) in sections.iter_mut().take(pairs).enumerate() {
                let q = if i == 0 { base[i] * resonance } else { base[i] };
                *section = Coefficients::for_kind(
                    rate,
                    frequency,
                    q,
                    T::value(2.0) * db_per_order,
                    self.kind,
                );
            }

            if first_order {
                let g = (T::PI() * frequency / rate).tan();
                sections[pairs] = Coefficients::first_order_shelf(g, db_per_order, self.kind);
            }
        } else if matches!(self.kind, FilterKind::Notch | FilterKind::BandPass) {
            let g = (T::PI() * frequency / rate).tan();
            Coefficients::band_filter_sections(g, q, self.kind, &mut sections[..count]);
        } else if self.kind == FilterKind::FlatTilt {
            Coefficients::flat_tilt_sections(rate, frequency, gain, &mut sections[..count]);
        } else {
            let q = if self.kind == FilterKind::TiltShelf {
                q * T::FRAC_1_SQRT_2()
            } else {
                q
            };
            sections[0] = Coefficients::for_kind(rate, frequency, q, gain, self.kind);
        }

        (sections, count)
    }
}

struct Slot<T: Sample> {
    coeff: Coefficients<T>,
    previous: Coefficients<T>,
    kernel: T::BlockKernel,
    state: [State<T>; 2],
    block: bool,
}

impl<T: Sample> Slot<T> {
    fn new() -> Self {
        Self {
            coeff: Coefficients::identity(),
            previous: Coefficients::identity(),
            kernel: T::kernel(&Coefficients::identity()),
            state: [State::zero(); 2],
            block: true,
        }
    }

    #[inline]
    fn run(&mut self, channel: usize, buf: &mut [T], position: usize, span: usize) {
        if self.block {
            T::run_block(&self.kernel, &self.coeff, &mut self.state[channel], buf);
        } else {
            let state = &mut self.state[channel];
            let inverse_span = T::value((span as f64).recip());

            for (i, sample) in buf.iter_mut().enumerate() {
                let frame = position + i + 1;
                let coeff = if frame < span {
                    self.previous
                        .interpolate(self.coeff, T::value(frame as f64) * inverse_span)
                } else {
                    self.coeff
                };
                *sample = dsp::step(state, &coeff, *sample);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CoefficientTiming {
    pub alpha: f32,
    pub fade_step: f32,
    pub frames: usize,
}

pub struct Band<T: Sample = f32> {
    /// heap-allocated so 32 bands won't blow up host stack on init
    slots: Box<[Slot<T>]>,
    /// fade-in for sections 1..
    fade: [f32; SECTIONS - 1],
    sections: usize,
    log_frequency: f32,
    gain: f32,
    q: f32,
    wet: f32,
    previous_wet: f32,
    started: bool,
    ramp_on_start: bool,
    /// true while smoothing, so steady state can skip update
    moving: bool,
    ramp_frame: usize,
    ramp_frames: usize,
    last: Option<BandControl>,
    pub frequency: f32,
}

impl<T: Sample> Band<T> {
    pub fn new() -> Self {
        Self::with_rack(SECTIONS, false)
    }

    pub(crate) fn with_rack(capacity: usize, ramp_on_start: bool) -> Self {
        Self {
            slots: (0..capacity).map(|_| Slot::new()).collect(),
            fade: [0.0; SECTIONS - 1],
            sections: 1,
            log_frequency: 1_000.0_f32.ln(),
            gain: 0.0,
            q: 0.7,
            wet: 0.0,
            previous_wet: 0.0,
            started: false,
            ramp_on_start,
            moving: true,
            ramp_frame: 0,
            ramp_frames: COEFF_FRAMES,
            last: None,
            frequency: 1_000.0,
        }
    }

    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.state = [State::zero(); 2];
            slot.previous = slot.coeff;
            slot.kernel = T::kernel(&slot.coeff);
            slot.block = true;
        }
        self.fade.fill(0.0);
        self.wet = 0.0;
        self.previous_wet = 0.0;
        self.sections = 1;
        self.started = false;
        self.moving = true;
        self.ramp_frame = 0;
        self.last = None;
    }

    pub(crate) fn transfer_from<S: Sample>(&mut self, source: &Band<S>) {
        self.fade = source.fade;
        self.sections = source.sections;
        self.log_frequency = source.log_frequency;
        self.gain = source.gain;
        self.q = source.q;
        self.wet = source.wet;
        self.previous_wet = source.previous_wet;
        self.started = source.started;
        self.frequency = source.frequency;
        self.ramp_frame = source.ramp_frame;
        self.ramp_frames = source.ramp_frames;
        self.invalidate();

        for (slot, previous) in self.slots.iter_mut().zip(&source.slots) {
            slot.coeff = Coefficients::from_coefficients(previous.coeff);
            slot.previous = Coefficients::from_coefficients(previous.previous);
            slot.state = previous.state.map(State::from_state);
            slot.block = false;
        }
    }

    /// force coeff rebuild on next update
    pub fn invalidate(&mut self) {
        self.last = None;
        self.moving = true;
    }

    /// smooth coeffs and enable ramp once per COEFF_FRAMES
    pub fn update(&mut self, control: BandControl, rate: f32, timing: CoefficientTiming) {
        let changed = self.last != Some(control);
        if changed {
            self.last = Some(control);
        }

        if !changed && !self.moving {
            return;
        }

        let CoefficientTiming {
            alpha,
            fade_step: step,
            frames,
        } = timing;
        self.ramp_frame = 0;
        self.ramp_frames = frames;

        self.previous_wet = self.wet;

        if !self.started && !control.enabled {
            self.moving = false;
            return;
        }

        let target_wet = f32::from(control.enabled);
        let frequency = control.frequency.clamp(10.0, rate * 0.49);
        let (targets, count) = control.sections_for::<T>(rate);
        assert!(count <= self.slots.len(), "filter exceeds its section rack");

        if !self.started {
            self.started = true;
            self.log_frequency = frequency.ln();
            self.gain = control.gain;
            self.q = control.q;
            self.frequency = frequency;

            for (i, slot) in self.slots.iter_mut().enumerate() {
                slot.coeff = targets[i];
                slot.previous = targets[i];
                slot.kernel = T::kernel(&targets[i]);
                slot.block = true;
                slot.state = [State::zero(); 2];
            }

            self.fade.fill(0.0);
            for fade in self.fade.iter_mut().take(count.saturating_sub(1)) {
                *fade = 1.0;
            }
            self.sections = count;
            self.wet = if self.ramp_on_start {
                step.min(target_wet)
            } else {
                self.previous_wet = target_wet;
                target_wet
            };
            self.moving = self.wet != target_wet;
            return;
        }

        self.log_frequency += alpha * (frequency.ln() - self.log_frequency);
        self.gain += alpha * (control.gain - self.gain);
        self.q += alpha * (control.q - self.q);
        self.frequency = self.log_frequency.exp();

        // smooth coeffs, snap when close
        let mut settled = true;
        for (slot, &target) in self.slots.iter_mut().zip(&targets).take(count) {
            let current = slot.coeff;
            slot.previous = current;
            // A moving detector target must stay on the smoothing path even inside the snap range.
            slot.coeff = if !changed && close(current, target) {
                target
            } else {
                current.approach(target, T::value(f64::from(alpha)))
            };
            if current == slot.coeff {
                if !slot.block {
                    slot.kernel = T::kernel(&slot.coeff);
                    slot.block = true;
                }
            } else {
                slot.block = false;
                settled = false;
            }
        }
        for slot in self.slots.iter_mut().skip(count) {
            // Switched to a shape that needs fewer sections: fade the rest out.
            slot.previous = slot.coeff;
            slot.block = false;
        }

        // Section fade-in/out.
        let mut sections = 1;
        for i in 1..self.slots.len() {
            let target = if i < count { 1.0 } else { 0.0 };
            let fade = &mut self.fade[i - 1];
            *fade += (target - *fade).clamp(-step, step);

            if *fade != target {
                settled = false;
            }
            if *fade == 0.0 && i >= count {
                self.slots[i].state = [State::zero(); 2];
            }
            if *fade > 0.0 {
                sections = i + 1;
            }
        }
        self.sections = sections;

        self.wet += (target_wet - self.wet).clamp(-step, step);

        if self.wet == 0.0 && self.previous_wet == 0.0 && target_wet == 0.0 {
            // Fully closed: drop stale state so re-enabling starts clean.
            for slot in &mut self.slots {
                slot.state = [State::zero(); 2];
                slot.block = false;
            }
            self.fade.fill(0.0);
            self.sections = 1;
            self.started = false;
            self.moving = false;
            return;
        }

        self.moving = !settled || self.wet != target_wet || self.previous_wet != self.wet;
    }

    /// filter stereo channels (handles Mid/Side here)
    pub fn process(&mut self, left: &mut [T], right: &mut [T], route: ChannelMode) {
        if !self.is_processing() {
            return;
        }

        match route {
            ChannelMode::Stereo => {
                self.run(left, 0);
                self.run(right, 1);
            }
            ChannelMode::Left => self.run(left, 0),
            ChannelMode::Right => self.run(right, 1),
            ChannelMode::Mid => {
                // M in left, S in right (in-place)
                to_mid_side(left, right);
                self.run(left, 0);
                from_mid_side(left, right);
            }
            ChannelMode::Side => {
                to_mid_side(left, right);
                self.run(right, 1);
                from_mid_side(left, right);
            }
        }
        self.ramp_frame = (self.ramp_frame + left.len()).min(self.ramp_frames);
    }

    pub(crate) fn is_processing(&self) -> bool {
        self.wet != 0.0 || self.previous_wet != 0.0
    }

    fn run(&mut self, buf: &mut [T], channel: usize) {
        const TILE: usize = 64;

        let wet = self.wet;
        let fading = self.previous_wet != wet;
        let mut dry = [T::zero(); TILE];
        let mut spare = [T::zero(); TILE];

        for (i, tile) in buf.chunks_mut(TILE).enumerate() {
            let len = tile.len();

            if fading || wet < 1.0 {
                dry[..len].copy_from_slice(tile);
            }

            self.cascade(tile, channel, &mut spare[..len], self.ramp_frame + i * TILE);

            if fading {
                let start = self.ramp_frame + i * TILE;
                for (frame, (sample, &dry)) in tile.iter_mut().zip(&dry[..len]).enumerate() {
                    let t =
                        (start + frame + 1).min(self.ramp_frames) as f32 / self.ramp_frames as f32;
                    let wet = self.previous_wet + t * (wet - self.previous_wet);
                    *sample = dry + T::value(f64::from(wet)) * (*sample - dry);
                }
            } else if wet < 1.0 {
                dsp::blend(&dry[..len], tile, wet);
            }
        }
    }

    /// run active sections (crossfading ones use spare buffer)
    fn cascade(&mut self, buf: &mut [T], channel: usize, spare: &mut [T], position: usize) {
        let sections = self.sections;
        self.slots[0].run(channel, buf, position, self.ramp_frames);

        for i in 1..sections {
            let fade = self.fade[i - 1];
            if fade <= 0.0 {
                continue;
            }

            if fade >= 1.0 {
                self.slots[i].run(channel, buf, position, self.ramp_frames);
            } else {
                spare.copy_from_slice(buf);
                self.slots[i].run(channel, spare, position, self.ramp_frames);
                for (sample, next) in buf.iter_mut().zip(spare.iter()) {
                    *sample += T::value(f64::from(fade)) * (*next - *sample);
                }
            }
        }
    }
}

impl<T: Sample> Default for Band<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Split both channels into mid (left) and side (right).
fn to_mid_side<T: Sample>(left: &mut [T], right: &mut [T]) {
    for (l, r) in left.iter_mut().zip(right.iter_mut()) {
        let (lv, rv) = (*l, *r);
        *l = (lv + rv) * T::value(0.5);
        *r = (lv - rv) * T::value(0.5);
    }
}

/// Put mid (left) and side (right) back together.
fn from_mid_side<T: Sample>(left: &mut [T], right: &mut [T]) {
    for (l, r) in left.iter_mut().zip(right.iter_mut()) {
        let (m, s) = (*l, *r);
        *l = m + s;
        *r = m - s;
    }
}

/// Two coefficients are the same up to float rounding.
fn close<T: Sample>(a: Coefficients<T>, b: Coefficients<T>) -> bool {
    let scale = |reference: T| reference.abs().max(T::one()) * T::value(f64::from(SNAP));

    (a.g - b.g).abs() <= scale(b.g)
        && (a.k - b.k).abs() <= scale(b.k)
        && (0..3).all(|i| (a.mix[i] - b.mix[i]).abs() <= scale(b.mix[i]))
}
