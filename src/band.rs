use crate::dsp::{self, Kernel, State};
use crate::dynamics::DynamicControl;
use crate::filter::{Coefficients, CutSlope, FLAT_TILT_SECTIONS, FilterKind};
use nice_plug::prelude::Enum;
use std::f32::consts::PI;

pub const BAND_COUNT: usize = 32;
pub const SECTIONS: usize = 8;
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
        let frequency = self.frequency.clamp(10.0, rate * 0.49);
        let count = self.section_count();
        let mut sections = [Coefficients::identity(); SECTIONS];

        if self.kind.is_cut() {
            let (_, first_order) = self.slope.sections();
            let base = self.slope.section_q();
            let resonance = (std::f32::consts::SQRT_2 * self.q).clamp(0.05, 40.0);
            let g = (PI * frequency / rate).tan();

            for i in 0..count {
                let is_first_order = first_order && i == count - 1;
                let q = if is_first_order {
                    0.0
                } else if i == 0 {
                    (base[0] * resonance).clamp(0.05, 40.0)
                } else {
                    base[i]
                };
                sections[i] = Coefficients::cut_section(g, q, self.kind, is_first_order);
            }
        } else if self.kind == FilterKind::Peak && count > 1 {
            let g = (PI * frequency / rate).tan();
            Coefficients::bell_sections(g, self.q, self.gain, &mut sections[..count]);
        } else if self.kind.is_shelf() && self.slope != CutSlope::Db12 {
            let (pairs, first_order) = self.slope.sections();
            let base = self.slope.section_q();
            // split gain across order; calibrate Q in log space
            let db_per_order = self.gain / self.slope.order() as f32;
            let resonance = self.q.powf(0.46);
            for (i, section) in sections.iter_mut().take(pairs).enumerate() {
                let q = if i == 0 { base[i] * resonance } else { base[i] };
                *section =
                    Coefficients::for_kind(rate, frequency, q, 2.0 * db_per_order, self.kind);
            }
            if first_order {
                let g = (PI * frequency / rate).tan();
                sections[pairs] = Coefficients::first_order_shelf(g, db_per_order, self.kind);
            }
        } else if matches!(self.kind, FilterKind::Notch | FilterKind::BandPass) {
            let g = (PI * frequency / rate).tan();
            Coefficients::band_filter_sections(g, self.q, self.kind, &mut sections[..count]);
        } else if self.kind == FilterKind::FlatTilt {
            Coefficients::flat_tilt_sections(rate, frequency, self.gain, &mut sections[..count]);
        } else {
            let q = if self.kind == FilterKind::TiltShelf {
                self.q * std::f32::consts::FRAC_1_SQRT_2
            } else {
                self.q
            };
            sections[0] = Coefficients::for_kind(rate, frequency, q, self.gain, self.kind);
        }

        (sections, count)
    }
}

struct Slot {
    coeff: Coefficients,
    kernel: Kernel,
    state: [State; 2],
    block: bool,
}

impl Slot {
    fn new() -> Self {
        Self {
            coeff: Coefficients::identity(),
            kernel: Kernel::new(&Coefficients::identity()),
            state: [State::default(); 2],
            block: true,
        }
    }

    #[inline]
    fn run(&mut self, channel: usize, buf: &mut [f32]) {
        if self.block {
            self.kernel.apply_slice(&mut self.state[channel], buf);
        } else {
            let coeff = self.coeff;
            let state = &mut self.state[channel];
            for sample in buf.iter_mut() {
                *sample = dsp::step(state, &coeff, *sample);
            }
        }
    }
}

pub struct Band {
    /// heap-allocated so 32 bands won't blow up host stack on init
    slots: Box<[Slot]>,
    /// fade-in for sections 1..
    fade: [f32; SECTIONS - 1],
    sections: usize,
    log_frequency: f32,
    gain: f32,
    q: f32,
    wet: f32,
    started: bool,
    ramp_on_start: bool,
    /// true while smoothing, so steady state can skip update
    moving: bool,
    last: Option<BandControl>,
    pub frequency: f32,
}

impl Band {
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
            started: false,
            ramp_on_start,
            moving: true,
            last: None,
            frequency: 1_000.0,
        }
    }

    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.state = [State::default(); 2];
            slot.kernel = Kernel::new(&slot.coeff);
            slot.block = true;
        }
        self.fade.fill(0.0);
        self.wet = 0.0;
        self.sections = 1;
        self.started = false;
        self.moving = true;
        self.last = None;
    }

    /// force coeff rebuild on next update
    pub fn invalidate(&mut self) {
        self.last = None;
        self.moving = true;
    }

    /// smooth coeffs and enable ramp once per COEFF_FRAMES
    pub fn update(&mut self, control: BandControl, rate: f32, alpha: f32, step: f32) {
        let changed = self.last != Some(control);
        if changed {
            self.last = Some(control);
        }

        if !changed && !self.moving {
            return;
        }

        let target_wet = f32::from(control.enabled);
        let frequency = control.frequency.clamp(10.0, rate * 0.49);
        let (targets, count) = control.sections(rate);
        assert!(count <= self.slots.len(), "filter exceeds its section rack");

        if !self.started {
            if !control.enabled {
                // Never ran: stay cold and cheap until it is switched on.
                self.wet = 0.0;
                self.moving = self.fade.iter().any(|f| *f > 0.0);
                return;
            }

            self.started = true;
            self.log_frequency = frequency.ln();
            self.gain = control.gain;
            self.q = control.q;
            self.frequency = frequency;

            for (i, slot) in self.slots.iter_mut().enumerate() {
                slot.coeff = targets[i];
                slot.kernel = Kernel::new(&targets[i]);
                slot.block = true;
                slot.state = [State::default(); 2];
            }

            self.fade.fill(0.0);
            for fade in self.fade.iter_mut().take(count.saturating_sub(1)) {
                *fade = 1.0;
            }
            self.sections = count;
            self.wet = if self.ramp_on_start {
                step.min(target_wet)
            } else {
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
            if close(current, target) {
                if current != target {
                    slot.coeff = target;
                }
                if !slot.block {
                    slot.kernel = Kernel::new(&target);
                    slot.block = true;
                }
            } else {
                slot.coeff = current.approach(target, alpha);
                slot.block = false;
                settled = false;
            }
        }
        for slot in self.slots.iter_mut().skip(count) {
            // Switched to a shape that needs fewer sections: fade the rest out.
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
                self.slots[i].state = [State::default(); 2];
            }
            if *fade > 0.0 {
                sections = i + 1;
            }
        }
        self.sections = sections;

        self.wet += (target_wet - self.wet).clamp(-step, step);

        if self.wet == 0.0 && target_wet == 0.0 {
            // Fully closed: drop stale state so re-enabling starts clean.
            for slot in &mut self.slots {
                slot.state = [State::default(); 2];
                slot.block = false;
            }
            self.fade.fill(0.0);
            self.sections = 1;
            self.started = false;
            self.moving = false;
            return;
        }

        self.moving = !settled || self.wet != target_wet;
    }

    /// filter stereo channels (handles Mid/Side here)
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], route: ChannelMode) {
        if self.wet == 0.0 {
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
    }

    pub(crate) fn is_processing(&self) -> bool {
        self.wet != 0.0
    }

    fn run(&mut self, buf: &mut [f32], channel: usize) {
        const TILE: usize = 64;

        let wet = self.wet;
        let mut dry = [0.0_f32; TILE];
        let mut spare = [0.0_f32; TILE];

        for tile in buf.chunks_mut(TILE) {
            let len = tile.len();

            if wet < 1.0 {
                dry[..len].copy_from_slice(tile);
            }

            self.cascade(tile, channel, &mut spare[..len]);

            if wet < 1.0 {
                dsp::blend(&dry[..len], tile, wet);
            }
        }
    }

    /// run active sections (crossfading ones use spare buffer)
    fn cascade(&mut self, buf: &mut [f32], channel: usize, spare: &mut [f32]) {
        let sections = self.sections;
        self.slots[0].run(channel, buf);

        for i in 1..sections {
            let fade = self.fade[i - 1];
            if fade <= 0.0 {
                continue;
            }

            if fade >= 1.0 {
                self.slots[i].run(channel, buf);
            } else {
                spare.copy_from_slice(buf);
                self.slots[i].run(channel, spare);
                for (sample, next) in buf.iter_mut().zip(spare.iter()) {
                    *sample += fade * (next - *sample);
                }
            }
        }
    }
}

impl Default for Band {
    fn default() -> Self {
        Self::new()
    }
}

/// Split both channels into mid (left) and side (right).
fn to_mid_side(left: &mut [f32], right: &mut [f32]) {
    for (l, r) in left.iter_mut().zip(right.iter_mut()) {
        let (lv, rv) = (*l, *r);
        *l = (lv + rv) * 0.5;
        *r = (lv - rv) * 0.5;
    }
}

/// Put mid (left) and side (right) back together.
fn from_mid_side(left: &mut [f32], right: &mut [f32]) {
    for (l, r) in left.iter_mut().zip(right.iter_mut()) {
        let (m, s) = (*l, *r);
        *l = m + s;
        *r = m - s;
    }
}

/// Two coefficients are the same up to float rounding.
fn close(a: Coefficients, b: Coefficients) -> bool {
    let scale = |reference: f32| reference.abs().max(1.0) * SNAP;

    (a.g - b.g).abs() <= scale(b.g)
        && (a.k - b.k).abs() <= scale(b.k)
        && (0..3).all(|i| (a.mix[i] - b.mix[i]).abs() <= scale(b.mix[i]))
}
