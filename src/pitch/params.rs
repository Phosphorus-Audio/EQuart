use super::{BASS_MIN_HZ, DetectionMode, DetectionRange, MAX_HZ, PitchControl, Target, TargetMode};
use nice_plug::prelude::*;

#[derive(Params)]
pub struct PitchParams {
    #[id = "pitch_enabled"]
    pub enabled: BoolParam,
    #[id = "pitch_bypassed"]
    pub bypassed: BoolParam,
    #[nested]
    pub detection: PitchDetectionParams,
    #[id = "pitch_multiplier"]
    pub multiplier: FloatParam,
}

#[derive(Params)]
pub struct PitchDetectionParams {
    #[id = "pitch_algorithm"]
    pub algorithm: EnumParam<DetectionMode>,
    #[id = "pitch_profile"]
    pub profile: EnumParam<DetectionRange>,
    #[id = "pitch_tracking"]
    pub tracking: FloatParam,
    #[id = "pitch_gate"]
    pub gate: FloatParam,
    #[id = "pitch_clarity"]
    pub clarity: FloatParam,
    #[id = "pitch_target"]
    pub target: EnumParam<TargetMode>,
    #[id = "pitch_minimum"]
    pub minimum: FloatParam,
    #[id = "pitch_maximum"]
    pub maximum: FloatParam,
    #[id = "pitch_reference"]
    pub reference: FloatParam,
    #[id = "pitch_tolerance"]
    pub tolerance: FloatParam,
}

impl PitchParams {
    pub fn new(band: &str) -> Self {
        Self {
            enabled: BoolParam::new(format!("{band} Pitch Enabled"), false),
            bypassed: BoolParam::new(format!("{band} Pitch Bypassed"), false),
            detection: PitchDetectionParams::new(&format!("{band} Pitch"), PitchControl::default()),
            multiplier: FloatParam::new(
                format!("{band} Pitch Multiplier"),
                PitchControl::default().multiplier,
                FloatRange::Logarithmic {
                    min: 0.125,
                    max: 16.0,
                },
            )
            .with_unit("×"),
        }
    }

    pub fn control(&self) -> PitchControl {
        if !self.enabled.value() || self.bypassed.value() {
            return PitchControl::default();
        }
        PitchControl {
            multiplier: self.multiplier.value(),
            ..self.detection.control()
        }
    }
}

impl PitchDetectionParams {
    pub fn new(prefix: &str, defaults: PitchControl) -> Self {
        let name = |label| format!("{prefix} {label}");
        let linear = |label, default, min, max, unit| {
            FloatParam::new(name(label), default, FloatRange::Linear { min, max }).with_unit(unit)
        };
        let frequency = |label, default| {
            FloatParam::new(
                name(label),
                default,
                FloatRange::Logarithmic {
                    min: BASS_MIN_HZ,
                    max: MAX_HZ,
                },
            )
            .with_unit(" Hz")
        };
        Self {
            algorithm: EnumParam::new(name("Algorithm"), defaults.algorithm),
            profile: EnumParam::new(name("Detection Range"), defaults.profile),
            tracking: FloatParam::new(
                name("Tracking"),
                defaults.tracking_ms,
                FloatRange::Skewed {
                    min: 5.0,
                    max: 300.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms"),
            gate: linear("Gate", defaults.gate_db, -80.0, -20.0, " dBFS"),
            clarity: linear("Clarity", defaults.clarity * 100.0, 50.0, 99.0, "%"),
            target: EnumParam::new(name("Target"), defaults.target.mode),
            minimum: frequency("Minimum", defaults.target.minimum),
            maximum: frequency("Maximum", defaults.target.maximum),
            reference: frequency("Reference", defaults.target.reference),
            tolerance: linear("Tolerance", defaults.target.tolerance, 1.0, 24.0, " st"),
        }
    }

    pub fn control(&self) -> PitchControl {
        PitchControl {
            enabled: true,
            algorithm: self.algorithm.value(),
            profile: self.profile.value(),
            tracking_ms: self.tracking.value(),
            gate_db: self.gate.value(),
            clarity: self.clarity.value() * 0.01,
            target: Target {
                mode: self.target.value(),
                minimum: self.minimum.value(),
                maximum: self.maximum.value(),
                reference: self.reference.value(),
                tolerance: self.tolerance.value(),
            },
            ..PitchControl::default()
        }
    }
}
