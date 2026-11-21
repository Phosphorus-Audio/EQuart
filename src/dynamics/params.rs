use crate::params::{FREQUENCY_MAX_HZ, FloatDisplay, Q_MAX, Q_MIN};
use nice_plug::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Enum)]
pub enum DetectorMode {
    #[default]
    #[id = "band_shape"]
    #[name = "Band shape"]
    BandShape,
    #[id = "follow"]
    #[name = "Follow"]
    Follow,
    #[id = "free"]
    #[name = "Free"]
    Free,
    #[id = "broadband"]
    #[name = "Broadband"]
    Broadband,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Enum)]
pub enum DetectorInput {
    #[default]
    #[id = "main"]
    Main,
    #[id = "sidechain"]
    Sidechain,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetectorControl {
    pub mode: DetectorMode,
    pub input: DetectorInput,
    pub frequency: f32,
    pub q: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainRule {
    pub enabled: bool,
    pub range: f32,
    pub ratio: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicControl {
    pub enabled: bool,
    pub threshold: f32,
    pub above: GainRule,
    pub below: GainRule,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub detect: DetectorControl,
    pub relative: DetectorControl,
    pub relative_amount: f32,
    pub onsets_amount: f32,
}

impl Default for DynamicControl {
    fn default() -> Self {
        let detect = DetectorControl {
            mode: DetectorMode::BandShape,
            input: DetectorInput::Main,
            frequency: 1_000.0,
            q: 1.0,
        };
        Self {
            enabled: false,
            threshold: -24.0,
            above: GainRule {
                enabled: true,
                range: -6.0,
                ratio: 0.75,
            },
            below: GainRule {
                enabled: false,
                range: 6.0,
                ratio: 0.75,
            },
            attack_ms: 10.0,
            release_ms: 100.0,
            detect,
            relative: DetectorControl {
                mode: DetectorMode::Broadband,
                ..detect
            },
            relative_amount: 0.0,
            onsets_amount: 0.0,
        }
    }
}

#[derive(Params)]
pub struct DetectorParams {
    #[id = "mode"]
    pub mode: EnumParam<DetectorMode>,
    #[id = "input"]
    pub input: EnumParam<DetectorInput>,
    #[id = "frequency"]
    pub frequency: FloatParam,
    #[id = "q"]
    pub q: FloatParam,
}

impl DetectorParams {
    fn new(name: &str, defaults: DetectorControl) -> Self {
        Self {
            mode: EnumParam::new(format!("{name} / Mode"), defaults.mode).non_automatable(),
            input: EnumParam::new(format!("{name} / Input"), defaults.input).non_automatable(),
            frequency: FloatParam::new(
                format!("{name} / Frequency"),
                defaults.frequency,
                FloatRange::Logarithmic {
                    min: crate::analyzer::MIN_HZ,
                    max: FREQUENCY_MAX_HZ,
                },
            )
            .display_as(" Hz", 2),
            q: FloatParam::new(
                format!("{name} / Q"),
                defaults.q,
                FloatRange::Logarithmic {
                    min: Q_MIN,
                    max: Q_MAX,
                },
            )
            .display_as("", 3),
        }
    }

    fn control(&self) -> DetectorControl {
        DetectorControl {
            mode: self.mode.value(),
            input: self.input.value(),
            frequency: self.frequency.value(),
            q: self.q.value(),
        }
    }
}

#[derive(Params)]
pub struct DynamicParams {
    #[id = "dynamic_enabled"]
    pub enabled: BoolParam,
    #[id = "dynamic_bypassed"]
    pub bypassed: BoolParam,
    #[id = "dynamic_threshold"]
    pub threshold: FloatParam,
    #[id = "dynamic_range"]
    pub range: FloatParam,
    #[id = "dynamic_attack"]
    pub attack: FloatParam,
    #[id = "dynamic_release"]
    pub release: FloatParam,
    #[id = "above_enabled"]
    pub above_enabled: BoolParam,
    #[id = "above_ratio"]
    pub above_ratio: FloatParam,
    #[id = "below_enabled"]
    pub below_enabled: BoolParam,
    #[id = "below_range"]
    pub below_range: FloatParam,
    #[id = "below_ratio"]
    pub below_ratio: FloatParam,
    #[nested(id_prefix = "detect", group = "Detect")]
    pub detect: DetectorParams,
    #[nested(id_prefix = "relative", group = "Relative")]
    pub relative: DetectorParams,
    #[id = "relative_amount"]
    pub relative_amount: FloatParam,
    #[id = "onsets_amount"]
    pub onsets_amount: FloatParam,
}

impl DynamicParams {
    pub fn new(band: &str) -> Self {
        let defaults = DynamicControl::default();
        let name = |label| format!("{band} / Dynamic / {label}");
        let db = |label, default, min, max| {
            FloatParam::new(name(label), default, FloatRange::Linear { min, max })
                .display_as(" dB", 2)
        };
        let time = |label, default, max| {
            FloatParam::new(
                name(label),
                default,
                FloatRange::Skewed {
                    min: 0.1,
                    max,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .display_as(" ms", 2)
        };
        let percent = |label, default| {
            FloatParam::new(
                name(label),
                default * 100.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .display_as("%", 1)
        };

        Self {
            enabled: BoolParam::new(name("Enabled"), defaults.enabled).non_automatable(),
            bypassed: BoolParam::new(name("Bypass"), false),
            threshold: db("Threshold", defaults.threshold, -80.0, 0.0),
            range: db("Above / Range", defaults.above.range, -24.0, 24.0),
            attack: time("Attack", defaults.attack_ms, 500.0),
            release: time("Release", defaults.release_ms, 2000.0),
            above_enabled: BoolParam::new(name("Above / Enabled"), defaults.above.enabled),
            above_ratio: percent("Above / Ratio", defaults.above.ratio),
            below_enabled: BoolParam::new(name("Below / Enabled"), defaults.below.enabled),
            below_range: db("Below / Range", defaults.below.range, -24.0, 24.0),
            below_ratio: percent("Below / Ratio", defaults.below.ratio),
            detect: DetectorParams::new(&name("Detect"), defaults.detect),
            relative: DetectorParams::new(&name("Relative"), defaults.relative),
            relative_amount: percent("Relative Amount", defaults.relative_amount),
            onsets_amount: percent("Onsets Amount", defaults.onsets_amount),
        }
    }

    pub fn control(&self) -> DynamicControl {
        DynamicControl {
            enabled: self.enabled.value() && !self.bypassed.value(),
            threshold: self.threshold.value(),
            above: GainRule {
                enabled: self.above_enabled.value(),
                range: self.range.value(),
                ratio: self.above_ratio.value() * 0.01,
            },
            below: GainRule {
                enabled: self.below_enabled.value(),
                range: self.below_range.value(),
                ratio: self.below_ratio.value() * 0.01,
            },
            attack_ms: self.attack.value(),
            release_ms: self.release.value(),
            detect: self.detect.control(),
            relative: self.relative.control(),
            relative_amount: self.relative_amount.value() * 0.01,
            onsets_amount: self.onsets_amount.value() * 0.01,
        }
    }
}
