use super::*;
use crate::band::COEFF_FRAMES;
use crate::modulation::{Division, Lfo, Mapping, Rack, Target, Timing, Wave};
use nice_plug::prelude::Transport;

#[test]
fn mapping_power_law() {
    let mut mapping = Mapping::default();

    for (strength, expected) in [
        (-1.0, 1.0),
        (0.0, 2.0),
        (0.5, 2.0_f32.sqrt() * 2.0),
        (1.0, 4.0),
    ] {
        mapping.strength = strength;
        assert!((mapping.output(400.0, 2.0, RATE) - expected).abs() < 1e-5);
    }

    mapping.reference = 400.0;
    assert_eq!(mapping.output(400.0, 2.0, RATE), 2.0);
    assert_eq!(mapping.output(1e12, 2.0, RATE), crate::params::Q_MAX);
    assert_eq!(mapping.output(0.01, 2.0, RATE), crate::params::Q_MIN);
    assert_eq!(mapping.output(f32::NAN, 2.0, RATE), 2.0);

    for source in Target::ALL {
        for target in Target::ALL {
            let reference = if source == Target::Gain { 0.0 } else { 1.0 };
            let input = if source == Target::Gain { 6.0 } else { 2.0 };
            let base = match target {
                Target::Gain => 0.0,
                Target::Frequency => 400.0,
                Target::Q => 2.0,
            };
            let result = Mapping {
                source,
                target,
                reference,
                ..mapping
            }
            .output(input, base, RATE);
            if source == target {
                assert_eq!(result, base);
            } else if target == Target::Gain {
                assert_eq!(result, base + 6.0);
            } else {
                assert_eq!(result, base * 2.0);
            }
        }
    }
}

#[test]
fn lfo_transport_and_waves() {
    let expected = [
        [0.0, 1.0, 0.0, -1.0],
        [-1.0, 0.0, 1.0, 0.0],
        [-1.0, -0.5, 0.0, 0.5],
        [1.0, 0.5, 0.0, -0.5],
        [1.0, 1.0, -1.0, -1.0],
        [0.7; 4],
    ];
    for (wave, points) in Wave::ALL.into_iter().zip(expected) {
        for (phase, want) in [0.0, 0.25, 0.5, 0.75].into_iter().zip(points) {
            assert!((wave.sample(phase, 0.7) - want).abs() < 1e-6);
        }
    }

    let mut rack = Rack::default();
    let mut transport = Transport::new(RATE);
    transport.tempo = Some(120.0);
    transport.pos_beats = Some(3.25);
    transport.playing = true;
    let mut band = band(400.0, 0.0, 2.0, FilterKind::Peak);
    band.lfo = Lfo {
        enabled: true,
        wave: Wave::Random,
        transport: true,
        phase: 90.0,
        ..Lfo::default()
    };

    let tick = |rack: &mut Rack, transport: &Transport, band: BandControl, linear| {
        rack.clock.set(transport);
        rack.update(0, &mut { band }, RATE, linear);
        rack.live(0).unwrap()
    };
    let first = tick(&mut rack, &transport, band, false);
    assert_eq!(first.phase, 0.5);
    rack.clock.advance(480);
    rack.update(0, &mut band, RATE, false);
    let next = rack.live(0).unwrap();
    assert!((next.phase - 0.52).abs() < 1e-5);
    assert_eq!(first.random, next.random);

    transport.tempo = Some(90.0);
    assert_eq!(tick(&mut rack, &transport, band, false).phase, first.phase);
    transport.pos_beats = Some(6.25);
    assert_ne!(
        tick(&mut rack, &transport, band, false).random,
        first.random
    );
    transport.pos_beats = Some(3.25);
    assert_eq!(
        tick(&mut rack, &transport, band, false).random,
        first.random
    );

    transport.playing = false;
    tick(&mut rack, &transport, band, false);
    rack.clock.advance(480);
    rack.update(0, &mut band, RATE, false);
    assert_eq!(rack.live(0).unwrap().phase, first.phase);

    band.lfo.division = Division::Eighth;
    band.lfo.timing = Timing::Dotted;
    let dotted = tick(&mut rack, &transport, band, false);
    assert!((dotted.phase - (3.25_f32 / 0.75 + 0.25).fract()).abs() < 1e-5);
    band.lfo.timing = Timing::Triplet;
    let triplet = tick(&mut rack, &transport, band, false);
    assert!((triplet.phase - (3.25_f32 * 3.0 + 0.25).fract()).abs() < 1e-5);
}

#[test]
fn f0_mapping_chain() {
    let mut bands = flat_bands();
    bands[0] = band(200.0, 0.0, 2.0, FilterKind::Peak);
    bands[0].pitch.enabled = true;
    bands[0].pitch.algorithm = crate::pitch::DetectionMode::Mpm;
    bands[0].mapping = Mapping {
        enabled: true,
        strength: 1.0,
        ..Mapping::default()
    };
    let mut controls = controls(bands);
    let mut engine = Engine::new(RATE);
    let tone = (0..48_000)
        .map(|i| (std::f32::consts::TAU * 400.0 * i as f32 / RATE).sin() * 0.4)
        .collect::<Vec<_>>();
    let mut left = tone.clone();
    let mut right = tone.clone();
    nice_assert_no_alloc::assert_no_alloc(|| {
        engine.process_block(&mut left, &mut right, &controls)
    });
    let tracked = engine.modulation(0).unwrap();
    assert!((tracked.frequency - 400.0).abs() < 1.0);
    assert!((tracked.q - 4.0).abs() < 0.03);
    assert_eq!(controls.bands[0].frequency, 200.0);
    assert_eq!(controls.bands[0].q, 2.0);

    controls.bands[0].lfo = Lfo {
        enabled: true,
        target: Target::Frequency,
        depth: 0.5,
        ..Lfo::default()
    };
    controls.bands[0].dynamics.enabled = true;
    left.copy_from_slice(&tone);
    right.copy_from_slice(&tone);
    nice_assert_no_alloc::assert_no_alloc(|| {
        engine.process_block(&mut left, &mut right, &controls)
    });
    let live = engine.modulation(0).unwrap();
    assert!((live.q - 2.0 * live.frequency / 200.0).abs() < 0.03);
    assert!(engine.dynamic_gains()[0].abs() > 0.1);
    assert!(left.iter().all(|value| value.is_finite()));
}

#[test]
fn lfo_audio_block_invariance() {
    let params = crate::modulation::LfoParams::new("Band 1");
    let signal = (0..8192)
        .map(|i| (std::f64::consts::TAU * 1200.0 * i as f64 / f64::from(RATE)).sin() * 0.15)
        .collect::<Vec<_>>();
    let render = |size, mode, target, enabled| {
        let mut bands = flat_bands();
        bands[0] = band(1000.0, 4.0, 6.0, FilterKind::Peak);
        bands[0].lfo = Lfo {
            enabled,
            target,
            depth: params.depth(target).range().unnormalize(1.0),
            hz: 21.0,
            wave: Wave::Square,
            sync: false,
            ..Lfo::default()
        };
        let mut controls = controls(bands);
        match mode {
            1 => controls.oversampling = crate::oversample::Oversampling::X4,
            2 => controls.linear_phase = Some(crate::processing_mode::LinearQuality::Short),
            _ => {}
        }
        let mut engine = Engine::new(RATE);
        let mut l = signal.clone();
        let mut r = signal.clone();
        engine.prepare_processing(&controls);
        let expected_latency = engine.latency();
        nice_assert_no_alloc::assert_no_alloc(|| {
            for (l, r) in l.chunks_mut(size).zip(r.chunks_mut(size)) {
                engine.process_block_with_sidechain(l, r, None, &controls);
            }
        });
        assert_eq!(engine.latency(), expected_latency);
        assert!(l.iter().all(|sample| sample.is_finite()));
        l
    };

    for mode in 0..3 {
        for target in Target::ALL {
            let first = render(32, mode, target, true);
            for size in [64, 128, 1_024] {
                let second = render(size, mode, target, true);
                let residual = first
                    .iter()
                    .zip(second)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f64, f64::max);
                assert!(
                    residual < 1e-8,
                    "mode={mode}, block={size}, residual={residual}"
                );
            }
            assert_ne!(first, render(128, mode, target, false));
        }
    }
}

#[test]
fn discontinuities_are_smoothed() {
    let band = band(700.0, 0.0, 12.0, FilterKind::Peak);
    for wave in [Wave::Square, Wave::SawUp, Wave::SawDown, Wave::Random] {
        let mut rack = Rack::default();
        let mut previous = 0.0_f32;
        for _ in 0..900 {
            let mut current = BandControl {
                lfo: Lfo {
                    enabled: true,
                    wave,
                    depth: 12.0,
                    hz: 40.0,
                    sync: false,
                    ..Lfo::default()
                },
                ..band
            };
            rack.update(0, &mut current, RATE, false);
            rack.clock.advance(COEFF_FRAMES);
            let offset = rack.live(0).unwrap().lfo_offset;
            assert!((offset - previous).abs() < 4.0);
            previous = offset;
        }
    }
}
