use super::{Control, Detector, Mode};
use crate::dynamics::DetectorInput;
use crate::params::FloatDisplay;
use nice_plug::plugin::{ParamValue, PluginState};
use nice_plug::prelude::*;

#[derive(Params)]
pub(crate) struct DynamicsParams {
    #[id = "region_dyn_enabled"]
    pub enabled: BoolParam,
    #[id = "region_dyn_bypass"]
    pub bypass: BoolParam,
    #[id = "region_dyn_mode"]
    pub mode: EnumParam<Mode>,
    #[id = "region_dyn_detector"]
    pub detector: EnumParam<Detector>,
    #[id = "region_dyn_source"]
    pub source: EnumParam<DetectorInput>,
    #[id = "region_dyn_threshold"]
    pub threshold: FloatParam,
    #[id = "region_dyn_ratio"]
    pub ratio: FloatParam,
    #[id = "region_dyn_attack"]
    pub attack: FloatParam,
    #[id = "region_dyn_release"]
    pub release: FloatParam,
    #[id = "region_dyn_knee"]
    pub knee: FloatParam,
    #[id = "region_dyn_makeup"]
    pub makeup: FloatParam,
    #[id = "region_dyn_range"]
    pub range: FloatParam,
    #[id = "region_dyn_link"]
    pub link: FloatParam,
}

impl DynamicsParams {
    pub(crate) fn new(prefix: &str) -> Self {
        let default = Control::default();
        let name = |label| format!("{prefix} / Dynamics / {label}");
        let linear = |label, default, min, max, unit, digits| {
            FloatParam::new(name(label), default, FloatRange::Linear { min, max })
                .display_as(unit, digits)
        };
        let log = |label, default, min, max, unit, digits| {
            FloatParam::new(name(label), default, FloatRange::Logarithmic { min, max })
                .display_as(unit, digits)
        };

        Self {
            enabled: BoolParam::new(name("Enabled"), false).non_automatable(),
            bypass: BoolParam::new(name("Bypass"), false),
            mode: EnumParam::new(name("Mode"), default.mode).non_automatable(),
            detector: EnumParam::new(name("Detector"), default.detector).non_automatable(),
            source: EnumParam::new(name("Source"), default.source).non_automatable(),
            threshold: linear("Threshold", default.threshold, -72.0, 0.0, " dB", 1),
            ratio: log("Ratio", default.ratio, 1.0, 40.0, ":1", 2),
            attack: log("Attack", default.attack, 0.1, 200.0, " ms", 1),
            release: log("Release", default.release, 5.0, 2000.0, " ms", 1),
            knee: linear("Knee", default.knee, 0.0, 24.0, " dB", 1),
            makeup: linear("Makeup", default.makeup, -24.0, 24.0, " dB", 1),
            range: linear("Range", default.range, 0.0, 72.0, " dB", 1),
            link: linear("Stereo Link", default.link, 0.0, 100.0, "%", 0),
        }
    }

    pub(crate) fn control(&self) -> Control {
        Control {
            enabled: self.enabled.value(),
            bypass: self.bypass.value(),
            mode: self.mode.value(),
            detector: self.detector.value(),
            source: self.source.value(),
            threshold: self.threshold.value(),
            ratio: self.ratio.value(),
            attack: self.attack.value(),
            release: self.release.value(),
            knee: self.knee.value(),
            makeup: self.makeup.value(),
            range: self.range.value(),
            link: self.link.value(),
            curve: Default::default(),
        }
    }
}

pub(crate) fn migrate_state(state: &mut PluginState) {
    use ParamValue::{Bool, F32, I32};

    let default = Control::default();
    state
        .fields
        .entry(super::CURVE_FIELD.to_owned())
        .or_insert_with(|| {
            serde_json::to_string(&[super::Curve::default(); crate::region::REGION_COUNT])
                .expect("default curves serialize")
        });
    let values = [
        ("enabled", Bool(false)),
        ("bypass", Bool(false)),
        ("mode", I32(default.mode.to_index() as i32)),
        ("detector", I32(default.detector.to_index() as i32)),
        ("source", I32(default.source.to_index() as i32)),
        ("threshold", F32(default.threshold)),
        ("ratio", F32(default.ratio)),
        ("attack", F32(default.attack)),
        ("release", F32(default.release)),
        ("knee", F32(default.knee)),
        ("makeup", F32(default.makeup)),
        ("range", F32(default.range)),
        ("link", F32(default.link)),
    ];

    for index in 1..=crate::region::REGION_COUNT {
        for (field, value) in &values {
            state
                .params
                .entry(format!("region_dyn_{field}_{index}"))
                .or_insert_with(|| value.clone());
        }
    }
}
