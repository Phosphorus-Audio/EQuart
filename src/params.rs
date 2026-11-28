use crate::band::{BAND_COUNT, BandControl, ChannelMode};
use crate::dynamics::DynamicParams;
use crate::filter::{CutSlope, FilterKind};
use crate::oversample::Oversampling;
use crate::pitch::PitchParams;
use crate::precision::Precision;
use nice_plug::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const UI_SCALES: [u32; 8] = [75, 90, 100, 110, 125, 150, 175, 200];
pub const Q_MIN: f32 = 0.025;
pub const Q_MAX: f32 = 40.0;

/// EQ y scale (+/-range dB)
pub const AXIS_RANGES: [u32; 6] = [4, 8, 12, 18, 24, 28];
pub const DEFAULT_AXIS_RANGE: u32 = 12;

/// spectrum y range (0..-range dB)
pub const SPECTRUM_RANGES: [u32; 5] = [60, 72, 90, 96, 120];
pub const DEFAULT_SPECTRUM_RANGE: u32 = 90;

/// max dial freq (DSP clamps to 0.49 * rate)
pub const FREQUENCY_MAX_HZ: f32 = 30_000.0;

#[derive(Params)]
pub struct EquartParams {
    #[nested(array, group = "Band")]
    pub bands: Box<[BandParams; BAND_COUNT]>,

    #[nested(array, group = "Spectral Region")]
    pub regions: [crate::region::RegionParams; crate::region::REGION_COUNT],

    #[persist = "spectral_transfer_curves"]
    pub(crate) spectral_transfer_curves: crate::region::dynamics::CurveBank,

    /// bitmask of user-placed slots (persist-only, not automated)
    #[persist = "bands_placed"]
    pub bands_placed: AtomicU32,

    #[nested(group = "Global")]
    pub global: GlobalParams,

    #[persist = "ui_scale_percent"]
    pub ui_scale_percent: AtomicU32,

    #[persist = "axis_range_db"]
    pub axis_range_db: AtomicU32,

    #[persist = "spectrum_range_db"]
    pub spectrum_range_db: AtomicU32,

    #[persist = "analyzer_frozen"]
    pub analyzer_frozen: AtomicBool,

    #[persist = "analyzer"]
    pub analyzer: AtomicBool,
}

impl Default for EquartParams {
    fn default() -> Self {
        Self {
            bands: (0..BAND_COUNT)
                .map(BandParams::at)
                .collect::<Box<[_]>>()
                .try_into()
                .ok()
                .expect("one parameter group per band"),
            regions: std::array::from_fn(crate::region::RegionParams::new),
            spectral_transfer_curves: Default::default(),
            bands_placed: AtomicU32::new(0),
            global: GlobalParams::default(),
            ui_scale_percent: AtomicU32::new(100),
            axis_range_db: AtomicU32::new(DEFAULT_AXIS_RANGE),
            spectrum_range_db: AtomicU32::new(DEFAULT_SPECTRUM_RANGE),
            analyzer_frozen: AtomicBool::new(false),
            analyzer: AtomicBool::new(true),
        }
    }
}

#[derive(Params)]
pub struct GlobalParams {
    #[id = "output"]
    pub output: FloatParam,

    #[id = "pan"]
    pub pan: FloatParam,

    #[id = "mix"]
    pub mix: FloatParam,

    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "oversampling"]
    pub oversampling: EnumParam<Oversampling>,

    #[id = "processing_mode"]
    pub(crate) processing_mode: EnumParam<crate::processing_mode::ProcessingMode>,
    #[id = "linear_quality"]
    pub(crate) linear_quality: EnumParam<crate::processing_mode::LinearQuality>,
    #[id = "dsp_precision"]
    pub(crate) precision: EnumParam<Precision>,
}

impl Default for GlobalParams {
    fn default() -> Self {
        let parse_pan = formatters::s2v_f32_panning();

        Self {
            output: FloatParam::new(
                "Output Gain",
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 12.0,
                },
            )
            .display_as(" dB", 2),
            mix: FloatParam::new(
                "Mix",
                100.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .display_as("%", 1),
            pan: FloatParam::new(
                "Pan",
                0.0,
                FloatRange::Linear {
                    min: -1.0,
                    max: 1.0,
                },
            )
            .with_value_to_string(formatters::v2s_f32_panning())
            .with_string_to_value(std::sync::Arc::new(move |text| {
                parse_pan(text).filter(|value| value.is_finite())
            })),
            bypass: BoolParam::new("Bypass", false).make_bypass(),
            oversampling: EnumParam::new("Oversampling", Oversampling::Off).non_automatable(),
            processing_mode: EnumParam::new("Processing Mode", Default::default())
                .non_automatable(),
            linear_quality: EnumParam::new("Linear Phase Quality", Default::default())
                .non_automatable(),
            precision: EnumParam::new("DSP Precision", Precision::default()).non_automatable(),
        }
    }
}

pub(crate) trait FloatDisplay {
    fn display_as(self, unit: &'static str, digits: usize) -> Self;
}

impl FloatDisplay for FloatParam {
    fn display_as(self, unit: &'static str, digits: usize) -> Self {
        self.with_unit(unit)
            .with_value_to_string(formatters::v2s_f32_rounded(digits))
            .with_string_to_value(std::sync::Arc::new(move |text| {
                let unit = unit.trim();
                let (number, scale) = match (unit, text.strip_suffix("kHz")) {
                    ("Hz", Some(number)) => (number, 1000.0),
                    _ => (text.strip_suffix(unit).unwrap_or(text), 1.0),
                };
                let value = number.trim().parse::<f32>().ok()? * scale;
                value.is_finite().then_some(value)
            }))
    }
}

const _: () = assert!(BAND_COUNT <= u32::BITS as usize);

/// The bit a slot owns in the placed mask.
fn placed_bit(index: usize) -> u32 {
    debug_assert!(index < BAND_COUNT, "slot {index} is outside the rack");
    1u32 << (index % u32::BITS as usize)
}

impl EquartParams {
    pub fn band(&self, index: usize) -> &BandParams {
        &self.bands[index]
    }

    /// true if slot holds a placed band
    pub fn is_placed(&self, index: usize) -> bool {
        self.bands_placed.load(Ordering::Relaxed) & placed_bit(index) != 0
    }

    /// Record a point in `index`, and report whether the slot was empty.
    pub fn mark_placed(&self, index: usize) -> bool {
        let bit = placed_bit(index);
        self.bands_placed.fetch_or(bit, Ordering::Relaxed) & bit == 0
    }

    pub fn clear_placed(&self, index: usize) {
        self.bands_placed
            .fetch_and(!placed_bit(index), Ordering::Relaxed);
    }

    pub fn control(&self, index: usize) -> BandControl {
        self.bands[index].control()
    }
}

#[derive(Params)]
pub struct BandParams {
    #[id = "enabled"]
    pub enabled: BoolParam,

    #[id = "type"]
    pub kind: EnumParam<FilterKind>,

    #[id = "channel"]
    pub channel: EnumParam<ChannelMode>,

    #[id = "frequency"]
    pub frequency: FloatParam,

    #[id = "gain"]
    pub gain: FloatParam,

    #[id = "q"]
    pub q: FloatParam,

    #[id = "slope"]
    pub slope: EnumParam<CutSlope>,

    /// Per-band threshold / range / attack / release, folded under the host's
    /// band group so automation sees them as one parameter tree.
    #[nested(group = "Dynamic")]
    pub dynamics: DynamicParams,

    #[nested(group = "F0 Follow")]
    pub pitch: Box<PitchParams>,

    #[nested(group = "Mapping")]
    pub(crate) mapping: Box<crate::modulation::MappingParams>,

    #[nested(group = "LFO")]
    pub(crate) lfo: Box<crate::modulation::LfoParams>,
}

/// Fallback starting shapes for the first eight slots, low to high. Every
/// slot starts disabled -- the default patch is a flat EQ, and points are
/// added by double-clicking empty plot space.
const DEFAULTS: [(FilterKind, f32, f32, f32); 8] = [
    (FilterKind::LowShelf, 90.0, 1.5, 0.7),
    (FilterKind::Peak, 250.0, -1.0, 1.0),
    (FilterKind::Peak, 800.0, 0.0, 1.4),
    (FilterKind::Peak, 2_400.0, 1.2, 1.0),
    (FilterKind::Peak, 5_000.0, -0.8, 2.0),
    (FilterKind::Peak, 9_000.0, 1.0, 1.5),
    (FilterKind::HighShelf, 12_000.0, 1.0, 0.7),
    (FilterKind::Peak, 16_000.0, 0.0, 1.0),
];

/// Log-even frequency from 20 Hz to 20 kHz for a disabled extra slot.
fn default_frequency(index: usize) -> f32 {
    let t = (index - DEFAULTS.len()) as f32 / (BAND_COUNT - DEFAULTS.len()) as f32;
    20.0 * 1_000.0_f32.powf(t)
}

impl BandParams {
    fn frequency_param(band: &str, default: f32) -> FloatParam {
        FloatParam::new(
            format!("{band} / Frequency"),
            default,
            // f(t) = min * (max/min)^t: an octave costs the same drag
            // distance at 100 Hz as it does at 10 kHz. The old skewed range
            // was a power law (10 + 19990*t^2), which made the low end
            // jumpy and the high end sluggish.
            FloatRange::Logarithmic {
                // Same bottom as the axis, so every frequency the plot shows
                // can be a band's frequency.
                min: crate::analyzer::MIN_HZ,
                max: FREQUENCY_MAX_HZ,
            },
        )
        .display_as(" Hz", 2)
        .with_step_size(0.01)
    }

    fn gain_param(band: &str, default: f32) -> FloatParam {
        FloatParam::new(
            format!("{band} / Gain"),
            default,
            FloatRange::Linear {
                min: -24.0,
                max: 24.0,
            },
        )
        .display_as(" dB", 2)
        .with_step_size(0.01)
    }

    fn q_param(band: &str, default: f32) -> FloatParam {
        FloatParam::new(
            format!("{band} / Q"),
            default,
            FloatRange::Skewed {
                min: Q_MIN,
                max: Q_MAX,
                factor: FloatRange::skew_factor(-1.0),
            },
        )
        .display_as("", 3)
        .with_step_size(0.001)
    }

    fn at(index: usize) -> Self {
        let (kind, frequency, gain, q) = match DEFAULTS.get(index) {
            Some(&shape) => shape,
            None => (FilterKind::Peak, default_frequency(index), 0.0, 1.0),
        };

        // The host lists every band's parameters in one flat column, so the
        // slot number has to be in the name itself: thirty-two rows of "Gain"
        // tell you nothing about which point you are automating.
        let band = format!("Band {:02}", index + 1);
        let filter = format!("{band} / Filter");

        Self {
            enabled: BoolParam::new(format!("{filter} / Enabled"), false).non_automatable(),
            kind: EnumParam::new(format!("{filter} / Type"), kind).non_automatable(),
            channel: EnumParam::new(format!("{filter} / Channel"), ChannelMode::Stereo)
                .non_automatable(),
            frequency: Self::frequency_param(&filter, frequency),
            gain: Self::gain_param(&filter, gain),
            q: Self::q_param(&filter, q),
            slope: EnumParam::new(format!("{filter} / Slope"), CutSlope::default())
                .non_automatable(),
            dynamics: DynamicParams::new(&band),
            pitch: Box::new(PitchParams::new(&band)),
            mapping: Box::new(crate::modulation::MappingParams::new(&band)),
            lfo: Box::new(crate::modulation::LfoParams::new(&band)),
        }
    }

    pub fn control(&self) -> BandControl {
        let kind = self.kind.value();
        let enabled = self.enabled.value();
        BandControl {
            enabled,
            frequency: self.frequency.value(),
            gain: if kind.has_gain() {
                self.gain.value()
            } else {
                0.0
            },
            q: self.q.value(),
            kind,
            slope: kind.effective_slope(self.slope.value()),
            channel: self.channel.value(),
            dynamics: self.dynamics.control(),
            pitch: self.pitch.control(),
            mapping: if enabled && self.mapping.enabled.value() {
                self.mapping.control()
            } else {
                Default::default()
            },
            lfo: if enabled && self.lfo.enabled.value() {
                self.lfo.control()
            } else {
                Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn host_parameter_permissions_names_and_text() {
        let params = EquartParams::default();
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        let mut counts = HashMap::new();

        for (id, param, group) in params.param_map() {
            assert!(ids.insert(id));

            // SAFETY: params owns every pointer and outlives the loop
            unsafe {
                let name = param.name();
                assert!(names.insert(name.to_owned()), "duplicate name {name}");
                assert!(!name.contains("Bypassed"));
                assert!(!name.contains("Detection / Detection"));
                assert!(name.encode_utf16().count() < 128);

                let flags = param.flags();
                assert!(!flags.contains(ParamFlags::HIDDEN), "{name}");
                let automated = !flags.contains(ParamFlags::NON_AUTOMATABLE);
                let (_, subgroup) = group.split_once('/').unwrap_or((&group, ""));
                let category = if group.starts_with("Band ") {
                    subgroup
                        .split('/')
                        .next()
                        .filter(|name| !name.is_empty())
                        .unwrap_or("Filter")
                } else if group.starts_with("Spectral Region ") {
                    subgroup
                        .split('/')
                        .next()
                        .filter(|name| !name.is_empty())
                        .map(|name| {
                            if name.starts_with("Response ") {
                                "Response"
                            } else {
                                name
                            }
                        })
                        .unwrap_or("Region")
                } else {
                    assert_eq!(group, "Global");
                    "Global"
                };
                *counts.entry((category.to_owned(), automated)).or_insert(0) += 1;

                for value in [0.0, param.default_normalized_value(), 1.0] {
                    for include_unit in [true, false] {
                        let text = param.normalized_value_to_string(value, include_unit);
                        assert!(
                            param.string_to_normalized_value(&text).is_some(),
                            "could not parse {name} = {text}",
                        );
                        let unit = param.unit().trim();
                        if !unit.is_empty() && include_unit {
                            assert_eq!(text.matches(unit).count(), 1, "{name} = {text}");
                        }
                    }
                }
            }
        }

        for (group, automated, manual) in [
            ("Global", 4, 4),
            ("Filter", 3 * BAND_COUNT, 4 * BAND_COUNT),
            ("Dynamic", 16 * BAND_COUNT, 5 * BAND_COUNT),
            (
                "Dynamics",
                9 * crate::region::REGION_COUNT,
                4 * crate::region::REGION_COUNT,
            ),
            ("F0 Follow", 7 * BAND_COUNT, 6 * BAND_COUNT),
            ("Mapping", 5 * BAND_COUNT, 3 * BAND_COUNT),
            ("LFO", 6 * BAND_COUNT, 8 * BAND_COUNT),
            (
                "Response",
                3 * crate::region::REGION_COUNT * crate::region::response::SLOTS,
                5 * crate::region::REGION_COUNT * crate::region::response::SLOTS,
            ),
            (
                "Region",
                5 * crate::region::REGION_COUNT,
                2 * crate::region::REGION_COUNT,
            ),
            (
                "Resonance",
                10 * crate::region::REGION_COUNT,
                crate::region::REGION_COUNT,
            ),
            (
                "Harmonic",
                8 * crate::region::REGION_COUNT,
                10 * crate::region::REGION_COUNT,
            ),
        ] {
            assert_eq!(counts[&(group.to_owned(), true)], automated, "{group}");
            assert_eq!(counts[&(group.to_owned(), false)], manual, "{group}");
        }
        assert_eq!(counts.len(), 22);
        assert_eq!(
            ids.len(),
            1530 + 22 * BAND_COUNT
                + crate::region::REGION_COUNT * (14 + 8 * crate::region::response::SLOTS)
        );

        let band = &params.bands[0];
        let pitch = &band.pitch.detection;
        let output = &params.global.output;
        let region = &params.regions[0];

        assert_eq!(band.frequency.name(), "Band 01 / Filter / Frequency");
        assert_eq!(
            region.harmonic.detection.profile.name(),
            "Spectral Region 01 / Harmonic / Detection / Range"
        );
        assert_eq!(
            band.frequency.string_to_normalized_value("1.2345 kHz"),
            band.frequency.string_to_normalized_value("1234.5Hz")
        );
        assert_eq!(band.q.normalized_value_to_string(0.0, true), "0.025");
        assert_eq!(
            pitch
                .tracking
                .normalized_value_to_string(pitch.tracking.default_normalized_value(), true),
            "35.0 ms"
        );
        assert_eq!(
            pitch
                .maximum
                .normalized_value_to_string(pitch.maximum.default_normalized_value(), true),
            "600 Hz"
        );
        assert_eq!(
            output.string_to_normalized_value("-12.345 dB"),
            Some(output.preview_normalized(-12.345))
        );
        assert_eq!(output.string_to_normalized_value("-24 dB"), Some(0.0));
        assert_eq!(output.string_to_normalized_value("12 dB"), Some(1.0));
        assert_eq!(output.string_to_normalized_value("NaN dB"), None);
        assert_eq!(output.string_to_normalized_value("inf"), None);
        assert_eq!(band.frequency.string_to_normalized_value("600 Hz Hz"), None);
    }
}
