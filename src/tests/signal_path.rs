use super::*;

#[test]
fn precision_switch_preserves_audio_history() {
    use crate::oversample::Oversampling;
    use crate::precision::Precision;

    let mut bands = flat_bands();
    bands[0] = band(330.0, 0.0, 0.707, FilterKind::Highpass);
    bands[1] = band(1_400.0, 8.0, 3.0, FilterKind::Peak);
    bands[1].channel = ChannelMode::Mid;

    for mode in Oversampling::ALL {
        let mut reference = controls(bands);
        reference.oversampling = mode;
        reference.precision = Precision::Double;
        reference.mix = 0.7;
        reference.output_db = 3.0;
        reference.regions[0].enabled = true;
        reference.regions[0].gain = -6.0;
        reference.regions[0].resonance.enabled = true;

        let mut switching = Controls { ..reference };

        let mut engine = Engine::new(RATE);
        let mut fixed = Engine::new(RATE);

        engine.set_oversampling(mode);
        let latency = engine.latency();
        let mut changed_samples = 0;
        const FRAMES: usize = 127;
        for block in 0..96 {
            switching.precision = if block % 3 == 1 {
                Precision::Single
            } else {
                Precision::Double
            };
            engine.delta_request = (32..64).contains(&block).then_some(0);
            fixed.delta_request = engine.delta_request;
            let phase = |i| (block * FRAMES + i) as f32 * std::f32::consts::TAU / RATE;
            let mut left: [f32; FRAMES] = std::array::from_fn(|i| (phase(i) * 937.5).sin() * 0.3);
            let mut right: [f32; FRAMES] = std::array::from_fn(|i| (phase(i) * 829.0).cos() * 0.2);
            let mut expected_left = left;
            let mut expected_right = right;
            fixed.process_block(&mut expected_left, &mut expected_right, &reference);
            nice_assert_no_alloc::assert_no_alloc(|| {
                engine.process_block(&mut left, &mut right, &switching);
            });
            assert_eq!(engine.latency(), latency);
            for (actual, expected) in left
                .into_iter()
                .chain(right)
                .zip(expected_left.into_iter().chain(expected_right))
            {
                changed_samples += usize::from(actual != expected);
                assert!(
                    (actual - expected).abs() < 1e-4,
                    "{mode:?}, block {block}: {actual} vs {expected}"
                );
            }
        }
        assert!(
            changed_samples > 0,
            "{mode:?}: switching should change the arithmetic"
        );
    }
}

#[test]
fn mix_bypass_latency_aligned() {
    use crate::oversample::Oversampling;

    let mut bands = flat_bands();
    bands[0] = band(1_000.0, 12.0, 1.0, FilterKind::Peak);
    let input = |i: usize| {
        let phase = i as f32 * std::f32::consts::TAU / RATE;
        [(phase * 1000.0).sin() * 0.25, (phase * 830.0).cos() * 0.2]
    };
    for mode in Oversampling::ALL {
        let mut settings = controls(bands);
        settings.oversampling = mode;
        settings.output_db = 6.0;
        let mut engine = Engine::new(RATE);

        let mut wet_engine = Engine::new(RATE);

        let mut reference = controls(bands);
        reference.oversampling = mode;
        reference.output_db = settings.output_db;

        for i in 0..4096 {
            settings.mix = if i < 2048 { 0.5 } else { 0.0 };
            settings.bypass = (1501..1803).contains(&i);
            let wet = wet_engine.process_frame(input(i), &reference);
            let actual = engine.process_frame(input(i), &settings);
            let delay = engine.latency() as usize;
            let dry = i.checked_sub(delay).map_or([0.0; 2], input);
            if settings.bypass || i >= 2600 {
                assert_eq!(actual, dry, "{mode:?} frame {i}");
            } else if i < 2048 {
                for ch in 0..2 {
                    let expected = (dry[ch] + wet[ch]) * 0.5;
                    assert!((actual[ch] - expected).abs() < 1e-6, "{mode:?} frame {i}");
                }
            }
        }
    }
}

#[test]
fn reset_mid_chunk() {
    let mut bands = flat_bands();
    bands[0] = band(2_000.0, 0.0, 0.707, FilterKind::Lowpass);
    let settings = controls(bands);
    let mut engine = Engine::new(RATE);
    let impulse = [1.0, -0.5];
    let expected = engine.process_frame(impulse, &settings);
    assert_ne!(expected, impulse);
    engine.reset();
    assert_eq!(engine.process_frame(impulse, &settings), expected);
}

#[test]
fn spectrum_in_out_bins() {
    use crate::analyzer::{AnalyzerSettings, capture};
    let mut engine = Engine::new(RATE);
    let (mut producer, receiver) = capture::channel();
    let mut display = capture::DisplaySpectrum::default();
    let params = crate::params::EquartParams::default();
    let settings = AnalyzerSettings::read(&params);

    let mut controls = controls(flat_bands());
    controls.output_db = -6.0;
    let mut sample_index = 0;
    for block in 0..80 {
        let mut left: [f32; 512] = std::array::from_fn(|_| {
            let phase = std::f64::consts::TAU * 750.0 * sample_index as f64 / f64::from(RATE);
            sample_index += 1;
            (0.5 * phase.sin()) as f32
        });
        let mut right = left;

        nice_assert_no_alloc::assert_no_alloc(|| {
            let mut capture = producer.reserve(RATE, left.len()).unwrap();
            crate::capture_channels(&mut capture, &[&mut left, &mut right], 0);
            engine.process_block(&mut left, &mut right, &controls);
            crate::capture_channels(&mut capture, &[&mut left, &mut right], 1);
            capture.commit_all();
        });

        if block % 4 == 0 {
            display.update(&receiver, RATE, settings, true);
        }
    }

    let frame = display.frame();
    assert_eq!(frame.bin_hz, RATE / 4_096.0);
    assert_eq!(frame.levels.len(), frame.output_levels.len());
    let change = frame.output_levels[64] - frame.levels[64];
    assert!(
        (change + 6.0).abs() < 0.05,
        "output spectrum change: {change} dB"
    );

    params
        .analyzer_resolution
        .store(0, std::sync::atomic::Ordering::Relaxed);
    display.update(&receiver, RATE, AnalyzerSettings::read(&params), true);
    let frame = display.frame();
    assert_eq!(frame.bin_hz, RATE / 1_024.0);
    assert_eq!(frame.levels.len(), 513);
    assert_eq!(frame.output_levels.len(), frame.levels.len());
    assert_eq!(frame.hold_levels.len(), frame.levels.len());
    assert_eq!(frame.output_hold_levels.len(), frame.levels.len());
}

#[test]
fn spectrum_capture_recovers_when_editor_stalls() {
    use crate::analyzer::{AnalyzerSettings, capture};
    let (mut producer, receiver) = capture::channel();
    let mut display = capture::DisplaySpectrum::default();
    let settings = AnalyzerSettings::read(&crate::params::EquartParams::default());

    nice_assert_no_alloc::assert_no_alloc(|| {
        producer.reserve(RATE, usize::MAX).unwrap().commit_all();
        assert!(producer.reserve(RATE, 1).is_none());
    });

    display.update(&receiver, RATE, settings, false);
    assert!(display.frame().levels.is_empty());

    let rate = 96_000.0;
    producer.reserve(rate, 4096).unwrap().commit_all();
    display.update(&receiver, rate, settings, true);

    let mut frames = producer.reserve(rate, 4096).unwrap();
    let (head, tail) = frames.as_mut_slices();

    for (sample, frame) in head.iter_mut().chain(tail).enumerate() {
        let value = (sample as f32 * std::f32::consts::TAU * 3_000.0 / rate).sin();
        *frame = [[value; 2]; 2];
    }

    frames.commit_all();
    display.update(&receiver, rate, settings, true);

    let frame = display.frame();

    assert_eq!(frame.bin_hz, rate / 8192.0);
    assert!(frame.levels[(3000.0 / frame.bin_hz) as usize] > -25.0);
    assert_eq!(frame.levels, frame.output_levels);
}
