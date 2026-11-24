use crate::band::BAND_COUNT;
use crate::engine::Engine;

mod fir;
mod modulation;
pub(crate) mod region_dynamics;
use crate::region::{
    REGION_COUNT,
    resonance::{REDUCTIONS, Reduction},
};
pub(crate) use fir::FirResponse;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Meter readings and frequency scale used to draw an editor frame.
pub(crate) struct MeterReadings {
    pub(crate) input_peak: f32,
    pub(crate) output_peak: f32,
    pub(crate) output_max: f32,
    pub(crate) sample_rate: f32,
    pub(crate) latency_samples: u32,
}

pub(crate) struct Telemetry {
    pub(crate) fir: fir::FirTelemetry,
    input_peak: AtomicU32,
    output_peak: AtomicU32,
    output_max: AtomicU32,
    sample_rate: AtomicU32,
    latency_samples: AtomicU32,
    /// live dynamic gain offset per band for UI
    dynamic: [AtomicU32; BAND_COUNT],
    dynamic_levels: [AtomicU32; BAND_COUNT],
    detected_pitch: [AtomicU32; BAND_COUNT],
    tracked_frequency: [AtomicU32; BAND_COUNT],
    modulation: [modulation::Published; BAND_COUNT],
    harmonic_hz: [AtomicU32; REGION_COUNT],
    region_response: [[AtomicU64; crate::region::response::SLOTS]; REGION_COUNT],
    region_dynamics_target: AtomicU32,
    pub(crate) region_dynamics: [region_dynamics::History; REGION_COUNT],
    pub(crate) region_dynamics_response: region_dynamics::Response,
    /// Momentary editor request. Zero means normal output.
    delta_region: AtomicU32,
    audition: AtomicU32,
    resonance: [[AtomicU32; REDUCTIONS]; REGION_COUNT],
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            fir: Default::default(),
            input_peak: AtomicU32::new(0),
            output_peak: AtomicU32::new(0),
            output_max: AtomicU32::new(0),
            sample_rate: AtomicU32::new(0),
            latency_samples: AtomicU32::new(0),
            dynamic: std::array::from_fn(|_| AtomicU32::new(0)),
            dynamic_levels: std::array::from_fn(|_| AtomicU32::new(f32::NEG_INFINITY.to_bits())),
            detected_pitch: std::array::from_fn(|_| AtomicU32::new(0)),
            tracked_frequency: std::array::from_fn(|_| AtomicU32::new(0)),
            modulation: Default::default(),
            harmonic_hz: Default::default(),
            region_response: Default::default(),
            region_dynamics_target: AtomicU32::new(0),
            region_dynamics: Default::default(),
            region_dynamics_response: Default::default(),
            delta_region: AtomicU32::new(0),
            audition: AtomicU32::new(0),
            resonance: Default::default(),
        }
    }
}

impl Telemetry {
    pub(crate) fn record_output_max(&self, peak: f32) {
        // positive float bits have the same order as their values
        let bits = peak.to_bits();

        if peak.is_finite() && peak > 0.0 && bits > self.output_max.load(Ordering::Relaxed) {
            self.output_max.fetch_max(bits, Ordering::Relaxed);
        }
    }

    pub(crate) fn clear_output_max(&self) {
        self.output_max.store(0, Ordering::Relaxed);
    }

    pub(crate) fn set_region_dynamics_monitor(&self, target: Option<(usize, bool)>) {
        let value = target.map_or(0, |(index, record)| {
            (index as u32 + 1) | (u32::from(record) << 16)
        });
        self.region_dynamics_target.store(value, Ordering::Relaxed);
    }

    pub(crate) fn region_dynamics_monitor(&self) -> Option<(usize, bool)> {
        let value = self.region_dynamics_target.load(Ordering::Relaxed);
        ((value & 0xffff) as usize)
            .checked_sub(1)
            .map(|index| (index, value >> 16 != 0))
    }

    pub(crate) fn set_delta_region(&self, region: Option<usize>) {
        self.delta_region
            .store(region.map_or(0, |i| i as u32 + 1), Ordering::Relaxed);
    }

    pub(crate) fn delta_region(&self) -> Option<usize> {
        (self.delta_region.load(Ordering::Relaxed) as usize).checked_sub(1)
    }

    pub(crate) fn set_audition(&self, target: Option<crate::engine::audition::Target>) {
        self.audition.store(
            target.map_or(0, crate::engine::audition::Target::encode),
            Ordering::Relaxed,
        );
    }

    pub(crate) fn audition(&self) -> Option<crate::engine::audition::Target> {
        crate::engine::audition::Target::decode(self.audition.load(Ordering::Relaxed))
    }

    pub(crate) fn publish(&self, engine: &Engine) {
        self.input_peak
            .store(engine.input_peak.to_bits(), Ordering::Relaxed);
        self.output_peak
            .store(engine.output_peak.to_bits(), Ordering::Relaxed);
        self.sample_rate
            .store(engine.rate.to_bits(), Ordering::Relaxed);
        self.latency_samples
            .store(engine.latency(), Ordering::Relaxed);

        self.fir.publish(engine);
        self.region_dynamics_response
            .write(engine.region_dynamics.response());

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
        for (index, band) in self.modulation.iter().enumerate() {
            band.write(engine.modulation(index));
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
        for (index, row) in self.region_response.iter().enumerate() {
            for (slot, response) in row.iter().zip(engine.region_response(index)) {
                let bits = if response.enabled {
                    (u64::from(response.reference.to_bits()) << 32)
                        | u64::from(response.strength.to_bits())
                } else {
                    0
                };
                slot.store(bits, Ordering::Relaxed);
            }
        }

        if let Some((index, record)) = engine.region_dynamics_observed {
            #[cfg(feature = "analyzer-profile")]
            let _timer = record.then(|| {
                crate::analyzer::profile::Timer::new(crate::analyzer::profile::REGION_HISTORY)
            });
            let (frame, meter) = engine.region_dynamics.meter(index);
            self.region_dynamics[index].write(frame, engine.rate, meter, record);
        }
    }

    pub(crate) fn meters(&self) -> MeterReadings {
        MeterReadings {
            input_peak: f32::from_bits(self.input_peak.load(Ordering::Relaxed)),
            output_peak: f32::from_bits(self.output_peak.load(Ordering::Relaxed)),
            output_max: f32::from_bits(self.output_max.load(Ordering::Relaxed)),
            sample_rate: self.sample_rate(),
            latency_samples: self.latency_samples.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }

    pub(crate) fn modulation(&self, index: usize) -> Option<crate::modulation::Live> {
        self.modulation[index].read()
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

    pub(crate) fn response_region(
        &self,
        index: usize,
        mut region: crate::region::RegionControl,
    ) -> crate::region::RegionControl {
        for (slot, response) in self.region_response[index]
            .iter()
            .zip(&mut region.responses)
        {
            let bits = slot.load(Ordering::Relaxed);
            if response.enabled && bits != 0 {
                response.strength = f32::from_bits(bits as u32);
                response.reference = f32::from_bits((bits >> 32) as u32);
                response.bypass = false;
            }
        }

        region
    }
}
