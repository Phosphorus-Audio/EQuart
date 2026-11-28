// spell 1/sqrt(2) in decimal like a user typing Q
#![allow(clippy::approx_constant)]

use crate::band::{BandControl, ChannelMode};
use crate::dynamics::DynamicControl;
use crate::engine::{Controls, Engine};
use crate::filter::{CutSlope, FilterKind};

const RATE: f32 = 48_000.0;

fn band(frequency: f32, gain: f32, q: f32, kind: FilterKind) -> BandControl {
    BandControl {
        enabled: true,
        frequency,
        gain,
        q,
        kind,
        // cuts are steep, bells use plain biquad
        slope: if kind.is_cut() {
            CutSlope::Db24
        } else {
            CutSlope::Db12
        },
        channel: ChannelMode::Stereo,
        dynamics: DynamicControl::default(),
        ..BandControl::default()
    }
}

fn flat_bands() -> [BandControl; crate::band::BAND_COUNT] {
    std::array::from_fn(|_| BandControl {
        enabled: false,
        ..BandControl::default()
    })
}

fn controls(bands: [BandControl; crate::band::BAND_COUNT]) -> Controls {
    Controls {
        bands,
        regions: Default::default(),
        output_db: 0.0,
        pan: 0.0,
        mix: 1.0,
        bypass: false,
        oversampling: crate::oversample::Oversampling::Off,
        linear_phase: None,
        precision: Default::default(),
    }
}

/// RMS of a sine at `hz` after the filter has settled.
fn measure(controls: &Controls, hz: f32) -> f32 {
    let mut engine = Engine::new(RATE);
    let total = (RATE * 1.0) as usize;
    let settle = (RATE * 0.6) as usize;
    let mut sum = 0.0;
    let mut count = 0;

    for i in 0..total {
        let phase = std::f32::consts::TAU * hz * i as f32 / RATE;
        let sample = [phase.sin() * 0.25, phase.sin() * 0.25];
        let output = engine.process_frame(sample, controls);

        if i >= settle {
            sum += output[0] * output[0];
            count += 1;
        }
    }

    (sum / count as f32).sqrt()
}

fn reference(hz: f32) -> f32 {
    measure(&controls(flat_bands()), hz)
}

fn db(value: f32, reference: f32) -> f32 {
    20.0 * (value / reference).log10()
}

mod bell;
mod brickwall;
mod dynamics;
mod filter_shapes;
mod host;
mod modulation;
mod oversampling;
mod processing;
mod regions;
mod response;
mod signal_path;
mod spectral_dynamics;
mod spectrum;
mod throughput;
