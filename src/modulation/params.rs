use super::*;
use crate::params::FloatDisplay;
use nice_plug::plugin::ParamValue;
use nice_plug::prelude::{BoolParam, EnumParam, FloatParam, FloatRange, Params, PluginState};

fn number(name: String, default: f32, range: (f32, f32), unit: &'static str) -> FloatParam {
    let (min, max) = range;
    FloatParam::new(name, default, FloatRange::Linear { min, max }).display_as(unit, 2)
}

pub(crate) fn migrate_state(state: &mut PluginState) {
    let mapping = Mapping::default();
    let lfo = Lfo::default();
    let values = [
        ("mapping_enabled", ParamValue::Bool(mapping.enabled)),
        ("mapping_bypass", ParamValue::Bool(mapping.bypass)),
        (
            "mapping_source",
            ParamValue::I32(mapping.source.to_index() as i32),
        ),
        (
            "mapping_target",
            ParamValue::I32(mapping.target.to_index() as i32),
        ),
        ("mapping_strength", ParamValue::F32(mapping.strength)),
        ("mapping_reference_hz", ParamValue::F32(mapping.reference)),
        ("mapping_reference_gain", ParamValue::F32(0.0)),
        ("mapping_reference_q", ParamValue::F32(2.0)),
        ("lfo_enabled", ParamValue::Bool(lfo.enabled)),
        ("lfo_bypass", ParamValue::Bool(lfo.bypass)),
        ("lfo_target", ParamValue::I32(lfo.target.to_index() as i32)),
        ("lfo_wave", ParamValue::I32(lfo.wave.to_index() as i32)),
        ("lfo_sync", ParamValue::Bool(lfo.sync)),
        (
            "lfo_division",
            ParamValue::I32(lfo.division.to_index() as i32),
        ),
        ("lfo_timing", ParamValue::I32(lfo.timing.to_index() as i32)),
        ("lfo_rate", ParamValue::F32(lfo.hz)),
        ("lfo_depth_hz", ParamValue::F32(lfo.depth)),
        ("lfo_depth_gain", ParamValue::F32(lfo.depth)),
        ("lfo_depth_q", ParamValue::F32(lfo.depth)),
        ("lfo_phase", ParamValue::F32(lfo.phase)),
        ("lfo_unipolar", ParamValue::Bool(lfo.unipolar)),
        ("lfo_transport", ParamValue::Bool(lfo.transport)),
    ];

    for index in 1..=BAND_COUNT {
        for (id, value) in &values {
            state
                .params
                .entry(format!("{id}_{index}"))
                .or_insert_with(|| value.clone());
        }
    }
}

#[derive(Params)]
pub(crate) struct MappingParams {
    #[id = "mapping_enabled"]
    pub enabled: BoolParam,
    #[id = "mapping_bypass"]
    pub bypass: BoolParam,
    #[id = "mapping_source"]
    pub source: EnumParam<Target>,
    #[id = "mapping_target"]
    pub target: EnumParam<Target>,
    #[id = "mapping_strength"]
    pub strength: FloatParam,
    #[id = "mapping_reference_hz"]
    pub reference_hz: FloatParam,
    #[id = "mapping_reference_gain"]
    pub reference_gain: FloatParam,
    #[id = "mapping_reference_q"]
    pub reference_q: FloatParam,
}

impl MappingParams {
    pub(crate) fn new(band: &str) -> Self {
        let name = |label| format!("{band} / Mapping / {label}");
        Self {
            enabled: BoolParam::new(name("Enabled"), false).non_automatable(),
            bypass: BoolParam::new(name("Bypass"), false),
            source: EnumParam::new(name("Source"), Target::Frequency).non_automatable(),
            target: EnumParam::new(name("Target"), Target::Q).non_automatable(),
            strength: number(name("Strength"), 0.0, (-1.0, 1.0), ""),
            reference_hz: FloatParam::new(
                name("Reference Frequency"),
                200.0,
                FloatRange::Logarithmic {
                    min: crate::analyzer::MIN_HZ,
                    max: FREQUENCY_MAX_HZ,
                },
            )
            .display_as(" Hz", 1),
            reference_gain: number(name("Reference Gain"), 0.0, (-24.0, 24.0), " dB"),
            reference_q: FloatParam::new(
                name("Reference Q"),
                2.0,
                FloatRange::Logarithmic {
                    min: Q_MIN,
                    max: Q_MAX,
                },
            )
            .display_as("", 3),
        }
    }

    pub(crate) fn reference(&self, source: Target) -> &FloatParam {
        match source {
            Target::Frequency => &self.reference_hz,
            Target::Gain => &self.reference_gain,
            Target::Q => &self.reference_q,
        }
    }

    pub(crate) fn control(&self) -> Mapping {
        let source = self.source.value();
        Mapping {
            enabled: self.enabled.value(),
            bypass: self.bypass.value(),
            source,
            target: self.target.value(),
            strength: self.strength.value(),
            reference: self.reference(source).value(),
        }
    }
}

#[derive(Params)]
pub(crate) struct LfoParams {
    #[id = "lfo_enabled"]
    pub enabled: BoolParam,
    #[id = "lfo_bypass"]
    pub bypass: BoolParam,
    #[id = "lfo_target"]
    pub target: EnumParam<Target>,
    #[id = "lfo_wave"]
    pub wave: EnumParam<Wave>,
    #[id = "lfo_sync"]
    pub sync: BoolParam,
    #[id = "lfo_division"]
    pub division: EnumParam<Division>,
    #[id = "lfo_timing"]
    pub timing: EnumParam<Timing>,
    #[id = "lfo_rate"]
    pub rate: FloatParam,
    #[id = "lfo_depth_hz"]
    pub depth_hz: FloatParam,
    #[id = "lfo_depth_gain"]
    pub depth_gain: FloatParam,
    #[id = "lfo_depth_q"]
    pub depth_q: FloatParam,
    #[id = "lfo_phase"]
    pub phase: FloatParam,
    #[id = "lfo_unipolar"]
    pub unipolar: BoolParam,
    #[id = "lfo_transport"]
    pub transport: BoolParam,
}

impl LfoParams {
    pub(crate) fn new(band: &str) -> Self {
        let name = |label| format!("{band} / LFO / {label}");
        Self {
            enabled: BoolParam::new(name("Enabled"), false).non_automatable(),
            bypass: BoolParam::new(name("Bypass"), false),
            target: EnumParam::new(name("Target"), Target::Gain).non_automatable(),
            wave: EnumParam::new(name("Waveform"), Wave::Sine).non_automatable(),
            sync: BoolParam::new(name("Tempo Sync"), true).non_automatable(),
            division: EnumParam::new(name("Division"), Division::Quarter).non_automatable(),
            timing: EnumParam::new(name("Timing"), Timing::Straight).non_automatable(),
            rate: FloatParam::new(
                name("Rate"),
                1.0,
                FloatRange::Logarithmic {
                    min: 0.01,
                    max: 40.0,
                },
            )
            .display_as(" Hz", 2),
            depth_hz: number(name("Frequency Depth"), 0.0, (0.0, 96.0), " st"),
            depth_gain: number(name("Gain Depth"), 0.0, (0.0, 48.0), " dB"),
            depth_q: number(name("Q Depth"), 0.0, (0.0, 3.0), " oct"),
            phase: number(name("Phase"), 0.0, (0.0, 360.0), "°"),
            unipolar: BoolParam::new(name("Unipolar"), false).non_automatable(),
            transport: BoolParam::new(name("Transport Sync"), false).non_automatable(),
        }
    }

    pub(crate) fn depth(&self, target: Target) -> &FloatParam {
        match target {
            Target::Frequency => &self.depth_hz,
            Target::Gain => &self.depth_gain,
            Target::Q => &self.depth_q,
        }
    }

    pub(crate) fn control(&self) -> Lfo {
        let target = self.target.value();
        Lfo {
            enabled: self.enabled.value(),
            bypass: self.bypass.value(),
            target,
            wave: self.wave.value(),
            sync: self.sync.value(),
            division: self.division.value(),
            timing: self.timing.value(),
            hz: self.rate.value(),
            depth: self.depth(target).value(),
            phase: self.phase.value(),
            unipolar: self.unipolar.value(),
            transport: self.transport.value(),
        }
    }
}
