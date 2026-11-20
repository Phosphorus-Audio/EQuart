use super::pool::{HARMONIC_SOURCE, PitchBank, PitchInput, PitchKey, SOURCES};
use super::{DetectionMode, DetectionRange, TargetMode};
use crate::band::{BAND_COUNT, BandControl};
use crate::engine::{Controls, Engine};
use std::f32::consts::TAU;

fn tone(rate: f32, hz: f32, amplitude: f32, length: usize) -> Vec<f32> {
    (0..length)
        .map(|i| amplitude * (TAU * hz * i as f32 / rate).sin())
        .collect()
}

#[test]
fn detectors_lock_and_gate() {
    let rate = 48_000.0;
    let input = tone(rate, 220.0, 0.3, rate as usize / 2);
    let quiet = tone(rate, 220.0, 0.001, rate as usize / 2);
    let mut pitch = PitchInput::new(rate);
    for algorithm in DetectionMode::ALL {
        for target in TargetMode::ALL {
            pitch.reset();
            let mut control = super::PitchControl {
                enabled: true,
                algorithm,
                ..Default::default()
            };
            control.target.mode = target;
            control.target.minimum = 210.0;
            control.target.maximum = 230.0;
            control.target.reference = 220.0;
            pitch.configure(rate, control.into());
            pitch.process(&input, &input);
            let detected = pitch.detected().expect("expected a 220 Hz lock");
            assert!(
                (detected - 220.0).abs() < 2.0,
                "{algorithm:?} {target:?}: {detected}"
            );

            if target == TargetMode::Range {
                control.target.minimum = 235.0;
                control.target.maximum = 236.0;
                pitch.configure(rate, control.into());
                pitch.process(&input, &input);
                assert!(
                    pitch.detected().is_none(),
                    "{algorithm:?} locked outside the range"
                );
            }
            pitch.process(&quiet, &quiet);
            assert!(pitch.detected().is_none(), "{algorithm:?} ignored the gate");
        }
    }
}

#[test]
fn retune_no_alloc() {
    let mut pitch = PitchInput::new(48_000.0);
    let cases = [
        (
            8_000.0,
            DetectionRange::Standard,
            220.0,
            DetectionMode::Pyin,
        ),
        (
            44_100.0,
            DetectionRange::Low,
            35.0,
            DetectionMode::MpmTracking,
        ),
        (
            96_000.0,
            DetectionRange::Bass,
            30.0,
            DetectionMode::SpectralMelody,
        ),
        (
            48_000.0,
            DetectionRange::Standard,
            220.0,
            DetectionMode::Mpm,
        ),
    ];
    for (rate, profile, hz, algorithm) in cases {
        let input = tone(rate, hz, 0.5, rate as usize);
        let control = super::PitchControl {
            enabled: true,
            profile,
            algorithm,
            ..Default::default()
        };
        nice_assert_no_alloc::assert_no_alloc(|| {
            pitch.configure(rate, control.into());
            pitch.process(&input, &input);
        });
        let detected = pitch
            .detected()
            .expect("no lock after changing sample rate/profile");
        assert!(
            (detected - hz).abs() < 2.0,
            "{rate} {profile:?}: {detected}"
        );
    }
}

#[test]
fn pool_shares_and_keeps_lock() {
    let rate = 48_000.0;
    let mut bank = PitchBank::new(rate);
    let mut settings = super::PitchControl {
        profile: DetectionRange::Low,
        algorithm: DetectionMode::Mpm,
        ..Default::default()
    };
    let original = PitchKey::from(settings);
    settings.tracking_ms = 100.0;
    settings.multiplier = 2.0;
    settings.target.reference = 500.0; // Unused in Auto.
    assert_eq!(PitchKey::from(settings), original);
    let mut requests = [None; SOURCES];
    requests[0] = Some(original);
    requests[1] = Some(settings.into());
    requests[HARMONIC_SOURCE] = Some(original);
    let signal = tone(rate, 220.0, 0.1, rate as usize / 2);
    nice_assert_no_alloc::assert_no_alloc(|| {
        bank.configure(rate, requests);
        bank.process(&signal, &signal);
    });
    assert_eq!(bank.active_count(), 1);
    let lock = bank.detected(0).expect("expected a lock");
    assert_eq!(bank.detected(1), Some(lock));
    assert_eq!(bank.detected(HARMONIC_SOURCE), Some(lock));

    settings.gate_db = -20.0;
    requests[0] = Some(settings.into());
    nice_assert_no_alloc::assert_no_alloc(|| bank.configure(rate, requests));
    assert_eq!(bank.active_count(), 2);
    assert_eq!(bank.detected(0), None);
    assert_eq!(bank.detected(1), Some(lock));
    bank.process(&signal, &signal);
    assert_eq!(bank.detected(0), None);
    assert!(bank.detected(1).is_some());
    requests[0] = Some(original);
    requests[1] = None;
    bank.configure(rate, requests);
    assert_eq!(bank.active_count(), 1);
    assert_eq!(bank.detected(0), bank.detected(HARMONIC_SOURCE));
    assert_eq!(bank.detected(1), None);
}

#[test]
fn shared_detector_tracking_rates() {
    let rate = 48_000.0;
    let mut engine = Engine::new(rate);
    engine.analyze = false;
    let mut bands = [BandControl {
        enabled: false,
        ..Default::default()
    }; BAND_COUNT];
    for (index, tracking_ms) in [5.0, 300.0].into_iter().enumerate() {
        bands[index] = BandControl {
            enabled: true,
            frequency: 880.0,
            pitch: super::PitchControl {
                enabled: true,
                algorithm: DetectionMode::Mpm,
                tracking_ms,
                ..Default::default()
            },
            ..Default::default()
        };
    }
    let controls = Controls {
        bands,
        regions: Default::default(),
        output_db: 0.0,
        mix: 1.0,
        bypass: false,
        oversampling: Default::default(),
        precision: Default::default(),
    };
    let mut left = tone(rate, 220.0, 0.4, 7_200);
    let mut right = left.clone();
    engine.process_block(&mut left, &mut right, &controls);
    let mut readouts = engine.pitch_levels();
    let (fast_pitch, fast_hz) = readouts.next().unwrap();
    let (slow_pitch, slow_hz) = readouts.next().unwrap();
    assert!(fast_pitch.is_some());
    assert_eq!(fast_pitch, slow_pitch);
    assert!(fast_hz.unwrap() + 200.0 < slow_hz.unwrap());
}

#[test]
fn pitch_mod_holds_frequency_during_dropout() {
    let rate = 48_000.0;
    let mut engine = Engine::new(rate);
    engine.analyze = false;
    let mut bands = [BandControl {
        enabled: false,
        ..Default::default()
    }; BAND_COUNT];
    bands[0] = BandControl {
        frequency: 900.0,
        gain: -12.0,
        q: 4.0,
        ..Default::default()
    };
    bands[0].pitch.enabled = true;
    bands[0].pitch.algorithm = DetectionMode::Mpm;
    let mut controls = Controls {
        bands,
        regions: Default::default(),
        output_db: 0.0,
        mix: 1.0,
        bypass: false,
        oversampling: Default::default(),
        precision: Default::default(),
    };
    let mut input = tone(rate, 220.0, 0.4, rate as usize);
    let mut right = input.clone();
    engine.process_block(&mut input, &mut right, &controls);

    let (detected, applied) = engine.pitch_levels().next().unwrap();
    assert!((detected.unwrap() - 220.0).abs() < 1.0);
    assert!((applied.unwrap() - 220.0).abs() < 1.0);
    let power = input[40_000..].iter().map(|x| x * x).sum::<f32>() / 8_000.0;
    let level = 10.0 * (power / (0.4_f32.powi(2) / 2.0)).log10();
    assert!((level + 12.0).abs() < 0.3, "applied band gain {level} dB");
    assert_eq!(controls.bands[0].frequency, 900.0);
    assert_eq!(engine.latency(), 0);
    assert_eq!(engine.dynamic_gains()[0], 0.0);

    controls.bands[0].pitch.multiplier = 2.0;

    let mut input = tone(rate, 220.0, 0.4, rate as usize);
    for (sample, harmonic) in input.iter_mut().zip(tone(rate, 440.0, 0.06, rate as usize)) {
        *sample += harmonic;
    }

    let mut right = input.clone();
    nice_assert_no_alloc::assert_no_alloc(|| {
        engine.process_block(&mut input, &mut right, &controls);
    });

    let (detected, applied) = engine.pitch_levels().next().unwrap();
    assert!((detected.unwrap() - 220.0).abs() < 1.0);
    assert!((applied.unwrap() - 440.0).abs() < 2.0);

    let tail = &input[36_000..];
    let (sin, cos) = tail
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(sin, cos), (i, sample)| {
            let phase = TAU * 440.0 * i as f32 / rate;
            (sin + sample * phase.sin(), cos + sample * phase.cos())
        });

    let harmonic = 2.0 * sin.hypot(cos) / tail.len() as f32;
    let level = 20.0 * (harmonic / 0.06).log10();
    assert!(
        (level + 12.0).abs() < 0.3,
        "second harmonic gain {level} dB"
    );

    engine.process_block(&mut [0.0; 4_800], &mut [0.0; 4_800], &controls);
    let (detected, held) = engine.pitch_levels().next().unwrap();
    assert_eq!(detected, None);
    let held = held.expect("the band should keep its tracked frequency");
    assert!((held - 440.0).abs() < 2.0, "held frequency was {held} Hz");

    for _ in 0..10 {
        engine.process_block(&mut [0.0; 4800], &mut [0.0; 4800], &controls);
    }

    let (detected, applied) = engine.pitch_levels().next().unwrap();

    assert_eq!(detected, None);
    assert!((applied.unwrap() - held).abs() < 0.1);

    let mut input = tone(rate, 330.0, 0.4, rate as usize / 2);
    let mut right = input.clone();
    engine.process_block(&mut input, &mut right, &controls);
    let (detected, applied) = engine.pitch_levels().next().unwrap();
    assert!((detected.unwrap() - 330.0).abs() < 1.0);
    assert!((applied.unwrap() - 660.0).abs() < 2.0);

    controls.bands[0].pitch.enabled = false;
    // controls.bands[0].pitch.enabled = true;
    engine.process_block(&mut [0.0; 64], &mut [0.0; 64], &controls);

    assert_eq!(engine.pitch_levels().next(), Some((None, None)));
}
