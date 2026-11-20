use crate::analyzer::{MAX_SPECTRUM_BINS, SpectrumFrame};
use crate::band::BAND_COUNT;
use crate::engine::Engine;
use crate::region::{
    REGION_COUNT,
    resonance::{REDUCTIONS, Reduction},
};
use std::sync::atomic::{AtomicU32, Ordering, fence};

/// Meter readings and frequency scale used to draw an editor frame.
pub(crate) struct MeterReadings {
    pub(crate) input_peak: f32,
    pub(crate) output_peak: f32,
    pub(crate) sample_rate: f32,
}

/// Reusable buffers and the publication currently stored in them.
#[derive(Default)]
pub(crate) struct SpectrumSnapshot {
    frame: SpectrumFrame,
    epoch: Option<u32>,
}

impl SpectrumSnapshot {
    pub(crate) fn frame(&self) -> &SpectrumFrame {
        &self.frame
    }

    #[cfg(debug_assertions)]
    pub(crate) fn update_preview(&mut self, analyzer: &crate::analyzer::Analyzer) {
        self.epoch = None;
        let (bin_hz, levels) = analyzer.spectrum();
        self.frame.bin_hz = bin_hz;
        for (destination, source) in [
            (&mut self.frame.levels, levels),
            (&mut self.frame.output_levels, levels),
            (&mut self.frame.hold_levels, analyzer.hold()),
            (&mut self.frame.output_hold_levels, analyzer.hold()),
        ] {
            destination.clear();
            destination.extend_from_slice(source);
        }
    }
}

pub(crate) struct Telemetry {
    input_peak: AtomicU32,
    output_peak: AtomicU32,
    sample_rate: AtomicU32,
    spectrum: Box<[AtomicU32]>,
    output_spectrum: Box<[AtomicU32]>,
    /// Peak each bin reached and has not released yet, drawn as the hold line.
    hold_spectrum: Box<[AtomicU32]>,
    output_hold_spectrum: Box<[AtomicU32]>,
    spectrum_epoch: AtomicU32,
    spectrum_count: AtomicU32,
    spectrum_bin_hz: AtomicU32,
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
            input_peak: AtomicU32::new(0),
            output_peak: AtomicU32::new(0),
            sample_rate: AtomicU32::new(0),
            spectrum: (0..MAX_SPECTRUM_BINS).map(|_| AtomicU32::new(0)).collect(),
            output_spectrum: (0..MAX_SPECTRUM_BINS).map(|_| AtomicU32::new(0)).collect(),
            hold_spectrum: (0..MAX_SPECTRUM_BINS).map(|_| AtomicU32::new(0)).collect(),
            output_hold_spectrum: (0..MAX_SPECTRUM_BINS).map(|_| AtomicU32::new(0)).collect(),
            spectrum_epoch: AtomicU32::new(0),
            spectrum_count: AtomicU32::new(0),
            spectrum_bin_hz: AtomicU32::new(0),
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

    pub(crate) fn publish(&self, engine: &Engine, spectrum_changed: bool) {
        self.input_peak
            .store(engine.input_peak.to_bits(), Ordering::Relaxed);
        self.output_peak
            .store(engine.output_peak.to_bits(), Ordering::Relaxed);
        self.sample_rate
            .store(engine.rate.to_bits(), Ordering::Relaxed);

        if spectrum_changed {
            let (bin_hz, spectrum) = engine.analyzer.spectrum();
            self.spectrum_epoch.fetch_add(1, Ordering::Relaxed);
            // release fence so reader sees odd epoch
            fence(Ordering::Release);
            for (atomic, value) in self.spectrum.iter().zip(spectrum) {
                atomic.store(value.to_bits(), Ordering::Relaxed);
            }
            for (atomic, value) in self
                .output_spectrum
                .iter()
                .zip(engine.output_analyzer.spectrum().1)
            {
                atomic.store(value.to_bits(), Ordering::Relaxed);
            }
            for (atomic, value) in self.hold_spectrum.iter().zip(engine.analyzer.hold()) {
                atomic.store(value.to_bits(), Ordering::Relaxed);
            }
            for (atomic, value) in self
                .output_hold_spectrum
                .iter()
                .zip(engine.output_analyzer.hold())
            {
                atomic.store(value.to_bits(), Ordering::Relaxed);
            }
            self.spectrum_count
                .store(spectrum.len() as u32, Ordering::Relaxed);
            self.spectrum_bin_hz
                .store(bin_hz.to_bits(), Ordering::Relaxed);
            self.spectrum_epoch.fetch_add(1, Ordering::Release);
        }

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

    /// Refresh the editor-owned snapshot only after a complete publication.
    pub(crate) fn read_spectrum(&self, snapshot: &mut SpectrumSnapshot) -> bool {
        loop {
            let epoch = self.spectrum_epoch.load(Ordering::Acquire);
            if epoch & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            if snapshot.epoch == Some(epoch) {
                return false;
            }

            let count = self.spectrum_count.load(Ordering::Relaxed) as usize;
            let bin_hz = f32::from_bits(self.spectrum_bin_hz.load(Ordering::Relaxed));
            snapshot.epoch = None;
            let frame = &mut snapshot.frame;
            for (source, destination) in [
                (&self.spectrum, &mut frame.levels),
                (&self.output_spectrum, &mut frame.output_levels),
                (&self.hold_spectrum, &mut frame.hold_levels),
                (&self.output_hold_spectrum, &mut frame.output_hold_levels),
            ] {
                destination.clear();
                destination.extend(
                    source[..count]
                        .iter()
                        .map(|level| f32::from_bits(level.load(Ordering::Relaxed))),
                );
            }

            // reject if writer updated mid-read
            fence(Ordering::Acquire);
            if self.spectrum_epoch.load(Ordering::Relaxed) == epoch {
                frame.bin_hz = bin_hz;
                snapshot.epoch = Some(epoch);
                return true;
            }
        }
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
