use crate::analyzer::AnalyzerOption;
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
    pub bands: [BandParams; BAND_COUNT],

    #[nested(array, group = "Spectral Region")]
    pub regions: [crate::region::RegionParams; crate::region::REGION_COUNT],

    /// bitmask of user-placed slots (persist-only, not automated)
    #[persist = "bands_placed"]
    pub bands_placed: AtomicU32,

    #[id = "output"]
    pub output: FloatParam,

    #[id = "mix"]
    pub mix: FloatParam,

    #[id = "bypass"]
    pub bypass: BoolParam,

    /// 2x/4x oversampling to reduce high-freq bilinear warping
    #[id = "oversampling"]
    pub oversampling: EnumParam<Oversampling>,
    #[id = "dsp_precision"]
    pub(crate) precision: EnumParam<Precision>,

    #[persist = "ui_scale_percent"]
    pub ui_scale_percent: AtomicU32,

    #[persist = "axis_range_db"]
    pub axis_range_db: AtomicU32,

    #[persist = "spectrum_range_db"]
    pub spectrum_range_db: AtomicU32,

    #[persist = "analyzer_resolution"]
    pub analyzer_resolution: AtomicU32,

    #[persist = "analyzer_speed"]
    pub analyzer_speed: AtomicU32,

    #[persist = "analyzer_smoothing"]
    pub analyzer_smoothing: AtomicU32,

    #[persist = "analyzer_hold"]
    pub analyzer_hold: AtomicU32,

    #[persist = "analyzer_tilt"]
    pub analyzer_tilt: AtomicU32,

    #[persist = "analyzer_frozen"]
    pub analyzer_frozen: AtomicBool,

    #[persist = "analyzer"]
    pub analyzer: AtomicBool,

    #[persist = "phase_curve"]
    pub phase_curve: AtomicBool,

    #[persist = "phase_unwrapped"]
    pub phase_unwrapped: AtomicBool,

    #[persist = "meter_fade"]
    pub meter_fade: AtomicBool,

    #[persist = "meter_visible"]
    pub meter_visible: AtomicBool,
}

impl Default for EquartParams {
    fn default() -> Self {
        Self {
            bands: std::array::from_fn(BandParams::at),
            regions: std::array::from_fn(crate::region::RegionParams::new),
            bands_placed: AtomicU32::new(0),
            output: FloatParam::new(
                "Output",
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 12.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            mix: FloatParam::new(
                "Mix",
                100.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            bypass: BoolParam::new("Bypass", false).make_bypass(),
            oversampling: EnumParam::new("Oversampling", Oversampling::Off),
            precision: EnumParam::new("DSP Precision", Precision::default()),
            ui_scale_percent: AtomicU32::new(100),
            axis_range_db: AtomicU32::new(DEFAULT_AXIS_RANGE),
            spectrum_range_db: AtomicU32::new(DEFAULT_SPECTRUM_RANGE),
            analyzer_resolution: AtomicU32::new(crate::analyzer::DEFAULT_RESOLUTION),
            analyzer_speed: AtomicU32::new(crate::analyzer::DEFAULT_SPECTRUM_SPEED),
            analyzer_smoothing: AtomicU32::new(crate::analyzer::DEFAULT_SPECTRUM_SMOOTHING),
            analyzer_hold: AtomicU32::new(crate::analyzer::DEFAULT_HOLD),
            analyzer_tilt: AtomicU32::new(crate::analyzer::DEFAULT_TILT),
            analyzer_frozen: AtomicBool::new(false),
            analyzer: AtomicBool::new(true),
            phase_curve: AtomicBool::new(false),
            phase_unwrapped: AtomicBool::new(false),
            meter_fade: AtomicBool::new(true),
            meter_visible: AtomicBool::new(true),
        }
    }
}

const _: () = assert!(BAND_COUNT <= u32::BITS as usize);

/// The bit a slot owns in the placed mask.
fn placed_bit(index: usize) -> u32 {
    debug_assert!(index < BAND_COUNT, "slot {index} is outside the rack");
    1u32 << (index % u32::BITS as usize)
}

impl EquartParams {
    pub(crate) fn analyzer_index(&self, setting: AnalyzerOption) -> u32 {
        let field = match setting {
            AnalyzerOption::Resolution => &self.analyzer_resolution,
            AnalyzerOption::Speed => &self.analyzer_speed,
            AnalyzerOption::Smoothing => &self.analyzer_smoothing,
            AnalyzerOption::Tilt => &self.analyzer_tilt,
            AnalyzerOption::Hold => &self.analyzer_hold,
        };
        setting.index(field.load(Ordering::Relaxed))
    }

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
    #[nested(group = "Dynamics")]
    pub dynamics: DynamicParams,

    #[nested(group = "f0lock")]
    pub pitch: Box<PitchParams>,
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
            format!("{band} Frequency"),
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
        .with_value_to_string(formatters::v2s_f32_hz_then_khz(1))
        .with_step_size(0.01)
    }

    fn gain_param(band: &str, default: f32) -> FloatParam {
        FloatParam::new(
            format!("{band} Gain"),
            default,
            FloatRange::Linear {
                min: -24.0,
                max: 24.0,
            },
        )
        .with_unit(" dB")
        .with_value_to_string(formatters::v2s_f32_rounded(1))
        .with_step_size(0.01)
    }

    fn q_param(band: &str, default: f32) -> FloatParam {
        FloatParam::new(
            format!("{band} Q"),
            default,
            FloatRange::Skewed {
                min: Q_MIN,
                max: Q_MAX,
                factor: FloatRange::skew_factor(-1.0),
            },
        )
        .with_value_to_string(formatters::v2s_f32_rounded(3))
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
        let band = format!("Band {}", index + 1);

        Self {
            enabled: BoolParam::new(format!("{band} Enabled"), false),
            kind: EnumParam::new(format!("{band} Type"), kind),
            channel: EnumParam::new(format!("{band} Channel"), ChannelMode::Stereo),
            frequency: Self::frequency_param(&band, frequency),
            gain: Self::gain_param(&band, gain),
            q: Self::q_param(&band, q),
            slope: EnumParam::new(format!("{band} Slope"), CutSlope::default()),
            dynamics: DynamicParams::new(&band),
            pitch: Box::new(PitchParams::new(&band)),
        }
    }

    pub fn control(&self) -> BandControl {
        let kind = self.kind.value();
        BandControl {
            enabled: self.enabled.value(),
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every parameter the host can automate has to be findable by its name:
    /// the host lists them in one flat column, and "Gain" thirty-two times over
    /// says nothing about which point the row moves.
    #[test]
    fn param_names_include_band() {
        let params = EquartParams::default();
        let mut names: Vec<String> = Vec::new();

        for (index, band) in params.bands.iter().enumerate() {
            let prefix = format!("Band {} ", index + 1);
            for name in [
                band.enabled.name(),
                band.kind.name(),
                band.channel.name(),
                band.frequency.name(),
                band.gain.name(),
                band.q.name(),
                band.slope.name(),
                band.dynamics.enabled.name(),
                band.dynamics.threshold.name(),
                band.dynamics.range.name(),
                band.dynamics.attack.name(),
                band.dynamics.release.name(),
            ] {
                assert!(
                    name.starts_with(&prefix),
                    "{name:?} does not say it belongs to band {}",
                    index + 1
                );
                names.push(name.to_owned());
            }
        }

        for name in [
            params.output.name(),
            params.mix.name(),
            params.bypass.name(),
            params.oversampling.name(),
        ] {
            names.push(name.to_owned());
        }

        let mut unique = std::collections::HashSet::new();
        for name in &names {
            assert!(unique.insert(name.as_str()), "{name:?} is used twice");
        }

        let ids: std::collections::HashSet<_> =
            params.param_map().into_iter().map(|(id, _, _)| id).collect();
        assert!(ids.contains("dsp_precision"));

        for key in [
            "algorithm", "profile", "tracking", "gate", "clarity", "target", "minimum", "maximum",
            "reference", "tolerance",
        ] {
            assert!(ids.contains(&format!("pitch_{key}_1")));
            assert!(ids.contains(&format!("harmonic_pitch_{key}_1")));
        }
    }
}
