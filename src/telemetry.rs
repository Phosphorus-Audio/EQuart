use crate::band::BAND_COUNT;
use crate::engine::Engine;

mod fir;
pub(crate) use fir::FirResponse;
use crate::region::{
    REGION_COUNT,
    resonance::{REDUCTIONS, Reduction},
};
use std::sync::atomic::{AtomicU32, Ordering};

/// Meter readings and frequency scale used to draw an editor frame.
pub(crate) struct MeterReadings {
    pub(crate) input_peak: f32,
    pub(crate) output_peak: f32,
    pub(crate) sample_rate: f32,
}

pub(crate) struct Telemetry {
    pub(crate) fir: fir::FirTelemetry,
    input_peak: AtomicU32,
    output_peak: AtomicU32,
    sample_rate: AtomicU32,
    /// live dynamic gain offset per band for UI
    dynamic: [AtomicU32; BAND_COUNT],
    dynamic_levels: [AtomicU32; BAND_COUNT],
    detected_pitch: [AtomicU32; BAND_COUNT],
    tracked_frequency: [AtomicU32; BAND_COUNT],
    harmonic_hz: [AtomicU32; REGION_COUNT],
    /// Momentary editor request. Zero means normal output.
    delta_region: AtomicU32,
    resonance: [[AtomicU32; REDUCTIONS]; REGION_COUNT],
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            fir: Default::default(),
            input_peak: AtomicU32::new(0),
            output_peak: AtomicU32::new(0),
            sample_rate: AtomicU32::new(0),
            dynamic: std::array::from_fn(|_| AtomicU32::new(0)),
            dynamic_levels: std::array::from_fn(|_| AtomicU32::new(f32::NEG_INFINITY.to_bits())),
            detected_pitch: std::array::from_fn(|_| AtomicU32::new(0)),
            tracked_frequency: std::array::from_fn(|_| AtomicU32::new(0)),
            harmonic_hz: Default::default(),
            delta_region: AtomicU32::new(0),
            resonance: Default::default(),
        }
    }
}

impl Telemetry {
    pub(crate) fn set_delta_region(&self, region: Option<usize>) {
        self.delta_region.store(region.map_or(0, |i| i as u32 + 1), Ordering::Relaxed);
    }

    pub(crate) fn delta_region(&self) -> Option<usize> {
        (self.delta_region.load(Ordering::Relaxed) as usize).checked_sub(1)
    }

    pub(crate) fn publish(&self, engine: &Engine) {
        self.input_peak
            .store(engine.input_peak.to_bits(), Ordering::Relaxed);
        self.output_peak
            .store(engine.output_peak.to_bits(), Ordering::Relaxed);
        self.sample_rate
            .store(engine.rate.to_bits(), Ordering::Relaxed);

        self.fir.publish(engine);

        for (atomic, value) in self.dynamic.iter().zip(engine.dynamic_gains()) {
            atomic.store(value.to_bits(), Ordering::Relaxed);
        }
        for (atomic, level) in self.dynamic_levels.iter().zip(engine.dynamic_levels()) {
            atomic.store(
                level.unwrap_or(f32::NEG_INFINITY).to_bits(),
                Ordering::Relaxed,
            );
        }
        for (i, (detected, tracked)) in engine.pitch_levels().enumerate() {
            self.detected_pitch[i].store(detected.unwrap_or(0.0).to_bits(), Ordering::Relaxed);
            self.tracked_frequency[i].store(tracked.unwrap_or(0.0).to_bits(), Ordering::Relaxed);
        }
        for (row, cuts) in self.resonance.iter().zip(engine.resonance.reductions()) {
            for (atomic, cut) in row.iter().zip(cuts) {
                let hz = cut.hz.round() as u16;
                let db = ((cut.attenuation_db * 128.0).round() as u32).min(0x3FFF);
                let ch = match cut.channel {
                    crate::band::ChannelMode::Left => 1u32,
                    crate::band::ChannelMode::Right => 2u32,
                    _ => 0u32,
                };
                atomic.store(u32::from(hz) | (db << 16) | (ch << 30), Ordering::Relaxed);
            }
        }
        for (index, hz) in self.harmonic_hz.iter().enumerate() {
            hz.store(
                engine.harmonic_hz(index).unwrap_or(0.0).to_bits(),
                Ordering::Relaxed,
            );
        }
    }

    pub(crate) fn meters(&self) -> MeterReadings {
        MeterReadings {
            input_peak: f32::from_bits(self.input_peak.load(Ordering::Relaxed)),
            output_peak: f32::from_bits(self.output_peak.load(Ordering::Relaxed)),
            sample_rate: self.sample_rate(),
        }
    }

    pub(crate) fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }

    pub(crate) fn dynamics(&self) -> [f32; BAND_COUNT] {
        std::array::from_fn(|i| f32::from_bits(self.dynamic[i].load(Ordering::Relaxed)))
    }

    pub(crate) fn dynamic_meter(&self, index: usize) -> (Option<f32>, f32) {
        let level = f32::from_bits(self.dynamic_levels[index].load(Ordering::Relaxed));
        let gain = f32::from_bits(self.dynamic[index].load(Ordering::Relaxed));
        (level.is_finite().then_some(level), gain)
    }

    pub(crate) fn detected_pitch(&self, index: usize) -> Option<f32> {
        let hz = f32::from_bits(self.detected_pitch[index].load(Ordering::Relaxed));
        (hz > 0.0).then_some(hz)
    }

    pub(crate) fn tracked_frequency(&self, index: usize) -> Option<f32> {
        let hz = f32::from_bits(self.tracked_frequency[index].load(Ordering::Relaxed));
        (hz > 0.0).then_some(hz)
    }

    #[cfg(test)]
    pub(crate) fn set_tracked_frequency(&self, index: usize, hz: f32) {
        self.tracked_frequency[index].store(hz.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn resonance_reductions(&self, index: usize) -> [Reduction; REDUCTIONS] {
        std::array::from_fn(|i| {
            let bits = self.resonance[index][i].load(Ordering::Relaxed);
            let channel = match bits >> 30 {
                1 => crate::band::ChannelMode::Left,
                2 => crate::band::ChannelMode::Right,
                _ => crate::band::ChannelMode::Stereo,
            };
            Reduction {
                hz: (bits as u16) as f32,
                attenuation_db: ((bits >> 16) & 0x3FFF) as f32 / 128.0,
                channel,
            }
        })
    }

    pub(crate) fn harmonic_hz(&self, index: usize) -> Option<f32> {
        let hz = f32::from_bits(self.harmonic_hz[index].load(Ordering::Relaxed));
        (hz > 0.0).then_some(hz)
    }
}
