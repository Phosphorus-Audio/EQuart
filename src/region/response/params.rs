use super::*;
use crate::params::FloatDisplay;
use nice_plug::{plugin::ParamValue, prelude::*};

const REFERENCE_RATE: f32 = 96_000.0;

impl Value {
    pub(crate) fn reference_position(self, value: f32) -> f32 {
        let (min, max) = self.limits(REFERENCE_RATE);
        let scale = self.scale();
        let min = scale.encode(min);
        let max = scale.encode(max);
        ((scale.encode(value) - min) / (max - min)).clamp(0.0, 1.0)
    }

    fn reference_value(self, position: f32) -> f32 {
        let (min, max) = self.limits(REFERENCE_RATE);
        let scale = self.scale();
        let value =
            scale.decode(scale.encode(min) + position * (scale.encode(max) - scale.encode(min)));
        if self == Self::HarmonicIndex {
            value.round()
        } else {
            value
        }
    }
}

#[derive(Params)]
pub(crate) struct ResponseParams {
    #[id = "response_enabled"]
    pub enabled: BoolParam,
    #[id = "response_bypass"]
    pub bypass: BoolParam,
    #[id = "response_scope"]
    pub scope: EnumParam<Scope>,
    #[id = "response_source"]
    pub source: EnumParam<Value>,
    #[id = "response_target"]
    pub target: EnumParam<Value>,
    #[id = "response_curve"]
    pub curve: EnumParam<Curve>,
    #[id = "response_strength"]
    pub strength: FloatParam,
    #[id = "response_reference"]
    pub reference: FloatParam,
}

impl ResponseParams {
    pub(crate) fn new(prefix: &str, index: usize) -> Self {
        let name = |label| format!("{prefix} / Response {} / {label}", index + 1);
        let default = Response::default();
        Self {
            enabled: BoolParam::new(name("Enabled"), false).non_automatable(),
            bypass: BoolParam::new(name("Bypass"), false),
            scope: EnumParam::new(name("Scope"), default.scope).non_automatable(),
            source: EnumParam::new(name("Source"), default.source).non_automatable(),
            target: EnumParam::new(name("Target"), default.target).non_automatable(),
            curve: EnumParam::new(name("Curve"), default.curve).non_automatable(),
            strength: FloatParam::new(
                name("Strength"),
                default.strength,
                FloatRange::Linear {
                    min: -1.0,
                    max: 1.0,
                },
            )
            .display_as("", 2),
            reference: FloatParam::new(
                name("Reference Position"),
                default.source.reference_position(default.reference) * 100.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .display_as("%", 2),
        }
    }

    pub(crate) fn control(&self) -> Response {
        let source = self.source.value();
        Response {
            enabled: self.enabled.value(),
            bypass: self.bypass.value(),
            scope: self.scope.value(),
            source,
            target: self.target.value(),
            curve: self.curve.value(),
            strength: self.strength.value(),
            reference: source.reference_value(self.reference.value() * 0.01),
        }
    }
}

pub(crate) fn migrate_state(state: &mut PluginState) {
    let default = Response::default();
    let fields = [
        ("response_enabled", ParamValue::Bool(false)),
        ("response_bypass", ParamValue::Bool(false)),
        (
            "response_scope",
            ParamValue::I32(default.scope.to_index() as i32),
        ),
        (
            "response_source",
            ParamValue::I32(default.source.to_index() as i32),
        ),
        (
            "response_target",
            ParamValue::I32(default.target.to_index() as i32),
        ),
        (
            "response_curve",
            ParamValue::I32(default.curve.to_index() as i32),
        ),
        ("response_strength", ParamValue::F32(default.strength)),
        (
            "response_reference",
            ParamValue::F32(default.source.reference_position(default.reference) * 100.0),
        ),
    ];

    for region in 1..=super::super::REGION_COUNT {
        state
            .params
            .entry(format!("region_reverse_responses_{region}"))
            .or_insert(ParamValue::Bool(false));

        for slot in 1..=SLOTS {
            for (field, value) in &fields {
                state
                    .params
                    .entry(format!("{field}_{slot}_{region}"))
                    .or_insert_with(|| value.clone());
            }
        }
    }
}
