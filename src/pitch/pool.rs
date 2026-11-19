use super::{DetectionMode, DetectionRange, Detector, PitchControl, Target, TargetMode};
use crate::band::BAND_COUNT;

pub(super) const HARMONIC_SOURCE: usize = BAND_COUNT;
const SOURCES: usize = BAND_COUNT + 1;
const _: () = assert!(SOURCES <= u64::BITS as usize);

/// only settings that affect pitch detection (tracking/unused target fields don't split groups)
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PitchKey {
    algorithm: DetectionMode,
    profile: DetectionRange,
    gate_db: f32,
    clarity: f32,
    target: Target,
}

impl From<PitchControl> for PitchKey {
    fn from(control: PitchControl) -> Self {
        let target = control.target;
        let target = match target.mode {
            TargetMode::Auto => Target::default(),
            TargetMode::Range => Target {
                mode: target.mode,
                minimum: target.minimum,
                maximum: target.maximum,
                ..Target::default()
            },
            TargetMode::Reference => Target {
                mode: target.mode,
                reference: target.reference,
                tolerance: target.tolerance,
                ..Target::default()
            },
        };
        Self {
            algorithm: control.algorithm,
            profile: control.profile,
            gate_db: control.gate_db,
            clarity: control.clarity,
            target: target.for_minimum(control.profile.minimum()),
        }
    }
}

pub(super) struct PitchInput {
    detector: Detector,
    active: Option<ActiveInput>,
}

struct ActiveInput {
    key: PitchKey,
    gate: f32,
    detected: Option<f32>,
}

impl PitchInput {
    pub fn new(rate: f32) -> Self {
        Self {
            detector: Detector::new(rate),
            active: None,
        }
    }

    pub fn reset(&mut self) {
        self.detector.reset();
        self.active = None;
    }

    pub fn configure(&mut self, rate: f32, key: PitchKey) {
        if self.detector.prepare(rate, key.profile, key.algorithm) || self.key() != Some(key) {
            self.active = Some(ActiveInput {
                key,
                gate: 10.0_f32.powf(key.gate_db / 20.0),
                detected: None,
            });
        }
    }

    fn key(&self) -> Option<PitchKey> {
        self.active.as_ref().map(|input| input.key)
    }

    pub(super) fn detected(&self) -> Option<f32> {
        self.active.as_ref().and_then(|input| input.detected)
    }

    pub fn process(&mut self, left: &[f32], right: &[f32]) {
        let input = self
            .active
            .as_mut()
            .expect("pitch input must be configured before processing");
        for (&left, &right) in left.iter().zip(right) {
            if self.detector.push([left, right]) {
                let hz = self
                    .detector
                    .analyze(input.gate, input.key.clarity, input.key.target);
                input.detected = (hz > 0.0).then_some(hz);
            }
        }
    }
}

pub(super) struct PitchBank {
    inputs: Box<[PitchInput]>,
    requests: [Option<PitchKey>; SOURCES],
    bindings: [Option<usize>; SOURCES],
    active: u64,
    rate: f32,
}

impl PitchBank {
    pub fn new(rate: f32) -> Self {
        Self {
            inputs: (0..SOURCES).map(|_| PitchInput::new(rate)).collect(),
            requests: [None; SOURCES],
            bindings: [None; SOURCES],
            active: 0,
            rate,
        }
    }

    pub fn reset(&mut self) {
        for input in &mut self.inputs {
            input.reset();
        }
        self.requests.fill(None);
        self.bindings.fill(None);
        self.active = 0;
    }

    pub fn configure(&mut self, rate: f32, requests: [Option<PitchKey>; SOURCES]) {
        if self.rate == rate && self.requests == requests {
            return;
        }

        let previous = self.bindings;
        self.bindings.fill(None);
        self.active = 0;
        // reserve unchanged groups first so other bands won't steal their detector
        for (source, &key) in requests.iter().enumerate() {
            if let Some(key) = key
                && let Some(slot) = self
                    .inputs
                    .iter()
                    .position(|input| input.key() == Some(key))
            {
                self.bindings[source] = Some(slot);
                self.active |= 1 << slot;
            }
        }
        // reuse old group's audio window when automating gate/clarity
        for (source, &key) in requests.iter().enumerate() {
            if let Some(key) = key
                && self.bindings[source].is_none()
                && let Some(slot) = previous[source]
                && self.active & (1 << slot) == 0
            {
                self.inputs[slot].configure(rate, key);
                self.bind_group(&requests, key, slot);
            }
        }
        for (source, &key) in requests.iter().enumerate() {
            if let Some(key) = key
                && self.bindings[source].is_none()
            {
                let slot = self.active.trailing_ones() as usize;
                self.inputs[slot].reset();
                self.inputs[slot].configure(rate, key);
                self.bind_group(&requests, key, slot);
            }
        }
        for (slot, input) in self.inputs.iter_mut().enumerate() {
            if self.active & (1 << slot) == 0 {
                if input.active.is_some() {
                    input.reset();
                }
            } else if self.rate != rate {
                input.configure(rate, input.key().expect("active detector has a key"));
            }
        }
        self.requests = requests;
        self.rate = rate;
    }

    fn bind_group(&mut self, requests: &[Option<PitchKey>; SOURCES], key: PitchKey, slot: usize) {
        for (binding, &request) in self.bindings.iter_mut().zip(requests) {
            if request == Some(key) {
                *binding = Some(slot);
            }
        }
        self.active |= 1 << slot;
    }

    pub fn process(&mut self, left: &[f32], right: &[f32]) {
        let mut active = self.active;
        while active != 0 {
            let slot = active.trailing_zeros() as usize;
            active &= active - 1;
            self.inputs[slot].process(left, right);
        }
    }

    pub fn detected(&self, source: usize) -> Option<f32> {
        self.bindings[source].and_then(|slot| self.inputs[slot].detected())
    }

    #[cfg(test)]
    pub(super) fn active_count(&self) -> u32 {
        self.active.count_ones()
    }
}
