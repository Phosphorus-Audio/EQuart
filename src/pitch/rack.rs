use super::PitchControl;
use super::pool::{HARMONIC_SOURCE, PitchBank, SOURCES};
use crate::band::{BAND_COUNT, BandControl, COEFF_FRAMES};
use crate::region::REGION_COUNT;

/// shares detector input across matching requests, keeps per-band smoothing
pub(crate) struct PitchRack {
    bank: PitchBank,
    bands: [PitchVoice; BAND_COUNT],
    harmonics: [PitchVoice; REGION_COUNT],
}

impl PitchRack {
    pub(crate) fn new(rate: f32) -> Self {
        Self {
            bank: PitchBank::new(rate),
            bands: [PitchVoice::default(); BAND_COUNT],
            harmonics: [PitchVoice::default(); REGION_COUNT],
        }
    }

    pub(crate) fn reset(&mut self) {
        self.bank.reset();
        self.bands.fill(PitchVoice::default());
        self.harmonics.fill(PitchVoice::default());
    }

    pub(crate) fn configure(
        &mut self,
        rate: f32,
        bands: &[BandControl; BAND_COUNT],
        bypass: bool,
        harmonics: [Option<PitchControl>; REGION_COUNT],
    ) {
        let mut requests = [None; SOURCES];
        if !bypass {
            for (request, band) in requests.iter_mut().zip(bands) {
                *request = (band.enabled && band.pitch.enabled).then(|| band.pitch.into());
            }

            for (request, harmonic) in requests[HARMONIC_SOURCE..].iter_mut().zip(harmonics) {
                *request = harmonic.map(Into::into);
            }
        }
        self.bank.configure(rate, requests);
    }

    pub(crate) fn process(&mut self, left: &[f32], right: &[f32]) {
        self.bank.process(left, right);
    }

    pub(crate) fn refresh_band(
        &mut self,
        index: usize,
        band: &BandControl,
        bypass: bool,
        rate: f32,
    ) -> Option<f32> {
        let voice = &mut self.bands[index];
        if !bypass && band.enabled && band.pitch.enabled {
            voice.advance(
                self.bank.detected(index),
                band.frequency,
                band.pitch.tracking_ms,
                rate,
            );
        } else {
            voice.clear();
        }
        voice.target()
    }

    pub(crate) fn refresh_harmonic(
        &mut self,
        index: usize,
        detection: Option<PitchControl>,
        rate: f32,
    ) -> Option<f32> {
        let voice = &mut self.harmonics[index];
        if let Some(settings) = detection
            && let Some(hz) = self.bank.detected(HARMONIC_SOURCE + index)
        {
            voice.advance(Some(hz), hz, settings.tracking_ms, rate);
        } else {
            voice.clear();
        }
        voice.target()
    }

    pub(crate) fn band_target(&self, index: usize) -> Option<f32> {
        self.bands[index].target()
    }

    pub(crate) fn detected_band(&self, index: usize) -> Option<f32> {
        self.bank.detected(index)
    }

    pub(crate) fn harmonic_hz(&self, index: usize) -> Option<f32> {
        self.harmonics[index].target()
    }
}

#[derive(Clone, Copy, Default)]
struct PitchVoice {
    clock: Option<(f32, f32)>,
    alpha: f64,
    frequency: Option<SmoothedFrequency>,
}

#[derive(Clone, Copy)]
struct SmoothedFrequency {
    log_hz: f64,
    hz: f32,
}

impl PitchVoice {
    fn clear(&mut self) {
        self.frequency = None;
    }

    fn target(&self) -> Option<f32> {
        self.frequency.map(|frequency| frequency.hz)
    }

    /// glide to tracked pitch, or back to saved freq when lost
    fn advance(&mut self, detected: Option<f32>, base_hz: f32, tracking_ms: f32, rate: f32) {
        let Some(from) = self
            .frequency
            .map(|frequency| frequency.log_hz)
            .or_else(|| detected.map(|_| f64::from(base_hz).ln()))
        else {
            return;
        };
        let target = f64::from(detected.unwrap_or(base_hz)).ln();
        if detected.is_none() && (from - target).abs() < 1e-5 {
            self.clear();
            return;
        }
        let clock = (tracking_ms, rate);
        if self.clock != Some(clock) {
            self.alpha = 1.0
                - (-(COEFF_FRAMES as f64) / (0.001 * f64::from(tracking_ms) * f64::from(rate)))
                    .exp();
            self.clock = Some(clock);
        }
        let log_hz = from + self.alpha * (target - from);
        self.frequency = Some(SmoothedFrequency {
            log_hz,
            hz: log_hz.exp() as f32,
        });
    }
}
