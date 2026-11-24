use super::*;
use crate::dynamics::DetectorInput;
use crate::oversample::Oversampling;
use crate::processing_mode::LinearQuality;
use crate::region::dynamics::{Control, Curve, Detector, Mode, Point, Segment};
use crate::region::{FundamentalSource, RegionControl};

const BOUNDS: [f32; 7] = [10.0, 100.0, 250.0, 800.0, 2800.0, 7400.0, 30_000.0];

fn region(low: f32, high: f32) -> RegionControl {
    RegionControl {
        enabled: true,
        low,
        high,
        gain: 0.0,
        dynamics: Control {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn combined(rate: f32, frames: usize, tones: &[(f32, f32)]) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            tones
                .iter()
                .map(|&(hz, amp)| {
                    amp * (std::f64::consts::TAU * f64::from(hz / rate) * i as f64).sin() as f32
                })
                .sum()
        })
        .collect()
}

fn amplitude(signal: &[f32], hz: f32, rate: f32) -> f32 {
    let remainder = &signal[signal.len() / 2..];
    let [re, im] = remainder
        .iter()
        .enumerate()
        .fold([0.0; 2], |[re, im], (i, &sample)| {
            let angle = std::f64::consts::TAU * f64::from(hz / rate) * i as f64;
            [
                re + f64::from(sample) * angle.cos(),
                im + f64::from(sample) * angle.sin(),
            ]
        });
    (2.0 * re.hypot(im) / remainder.len() as f64) as f32
}

fn process(
    engine: &mut Engine,
    config: &crate::engine::Controls,
    sizes: &[usize],
    left: &mut [f32],
    right: &mut [f32],
) {
    nice_assert_no_alloc::assert_no_alloc(|| {
        let mut offset = 0;
        for size in sizes.iter().cycle() {
            let end = (offset + size).min(left.len());
            engine.process_block(&mut left[offset..end], &mut right[offset..end], config);
            offset = end;
            if offset == left.len() {
                break;
            }
        }
    });
}

#[test]
fn spectral_transfer() {
    let mut control = Control {
        threshold: -20.0,
        knee: 0.0,
        ..Default::default()
    };
    assert_eq!(control.transfer(-20.0), -20.0);
    assert_eq!(control.transfer(-30.0), -30.0);
    for ratio in [1.0, 2.0, 4.0, 10.0, 40.0] {
        control.ratio = ratio;
        assert!((control.transfer(-8.0) - (-20.0 + 12.0 / ratio)).abs() < 1e-5);
    }

    control.ratio = 4.0;
    control.knee = 6.0;
    let slope =
        |db: f32, offset: f32| (control.transfer(db + offset) - control.transfer(db)) / offset;
    for edge in [-23.0, -17.0] {
        assert!((slope(edge, 0.01) - slope(edge, -0.01)).abs() < 0.002);
    }

    control.mode = Mode::Expander;
    control.makeup = -3.0;
    control.range = 5.0;
    assert_eq!(control.transfer(-50.0), -58.0);
    assert_eq!(control.transfer(-17.0), -20.0);
    assert!(control.transfer(f32::NAN).is_finite());
    assert!(control.transfer(f32::INFINITY).is_finite());
}

#[test]
fn custom_transfer() {
    let mut plugin = crate::Equart::default();
    let params = plugin.params.clone();
    let region_params = &params.regions[0];
    let set = |param: &nice_plug::prelude::BoolParam| {
        use nice_plug::prelude::Param;

        // SAFETY: the params arc keeps the parameter alive
        unsafe { param.as_ptr()._internal_set_normalized_value(1.0) };
    };
    set(&region_params.enabled);
    set(&region_params.dynamics.enabled);

    let mut curve = Curve::from_standard(Control::default());
    assert!(curve.insert(Point {
        input: -20.0,
        output: -32.0,
        ..Default::default()
    }));
    let point = curve.points()[1];
    curve.set(
        1,
        Point {
            bend: 0.8,
            segment: Segment::Smooth,
            ..point
        },
    );
    let shaped = curve.output(-4.0).unwrap();
    curve.set(
        1,
        Point {
            segment: Segment::Linear,
            ..point
        },
    );
    assert!((shaped - curve.output(-4.0).unwrap()).abs() > 3.0);

    params.spectral_transfer_curves.edit(0, curve);
    let state = crate::presets::state_of(&params);
    let preset = crate::presets::Preset::capture(state, "Custom Transfer");
    let restored = crate::params::EquartParams::default();
    let state = preset.into_state(&restored);
    crate::presets::restore(&restored, &state);
    let restored = restored.spectral_transfer_curves.read(0).unwrap();
    assert_eq!(restored, curve);

    let mut config = plugin.controls();
    assert_eq!(config.regions[0].dynamics.curve, restored);
    config.regions[0].low = 10.0;
    config.regions[0].high = 30_000.0;
    config.regions[0].gain = 0.0;
    let mut signal = vec![0.1; RATE as usize];
    let mut right = signal.clone();
    process(
        &mut Engine::new(RATE),
        &config,
        &[128, 31],
        &mut signal,
        &mut right,
    );
    let expected = 10.0_f32.powf(-32.0 / 20.0);
    assert!((signal[signal.len() - 1] - expected).abs() < 1e-4);

    params
        .spectral_transfer_curves
        .edit(0, curve.with_enabled(false));
    let standard = plugin.controls().regions[0].dynamics;
    assert!(!standard.curve.is_custom());
    assert_eq!(standard.curve.points(), curve.points());
    assert_eq!(standard.transfer(-20.0), -20.0);
}

#[test]
fn spectral_multiband() {
    let tones = [(45.0, 0.24), (750.0, 0.17), (9500.0, 0.2)];
    let input = combined(RATE, RATE as usize * 2, &tones);
    let mut config = controls(flat_bands());

    for edges in [&[10.0, 150.0, 2500.0, 30_000.0][..], &BOUNDS[..]] {
        config.regions = Default::default();
        for (slot, edge) in edges.windows(2).enumerate() {
            config.regions[slot] = region(edge[0], edge[1]);
        }

        let mut neutral = input.clone();
        process(
            &mut Engine::new(RATE),
            &config,
            &[128],
            &mut neutral,
            &mut input.clone(),
        );
        assert_eq!(neutral, input);

        for (slot, target, unaffected) in [(0, 45.0, 9500.0), (edges.len() - 2, 9500.0, 45.0)] {
            config.regions[slot].dynamics.threshold = -32.0;
            config.regions[slot].dynamics.ratio = 4.0;
            let mut left = input.clone();
            let mut engine = Engine::new(RATE);
            process(
                &mut engine,
                &config,
                &[128, 31, 1024],
                &mut left,
                &mut input.clone(),
            );

            let target_ratio = amplitude(&left, target, RATE) / amplitude(&input, target, RATE);
            let leakage = amplitude(&left, unaffected, RATE) / amplitude(&input, unaffected, RATE);
            assert!(
                target_ratio < 0.75 && leakage > 0.95,
                "target {target_ratio}, leakage {leakage}"
            );
            assert!(engine.region_dynamics.meter(slot).1.reduction < -9.0);
            assert_eq!(engine.region_dynamics.meter(1).1.reduction, 0.0);
            config.regions[slot].dynamics.ratio = 1.0;
        }
    }
}

#[test]
fn spectral_overlap_and_gaps() {
    let input = combined(
        RATE,
        RATE as usize * 2,
        &[(110.0, 0.15), (900.0, 0.15), (9000.0, 0.15)],
    );
    let mut config = controls(flat_bands());
    config.regions[0] = region(10.0, 180.0);
    config.regions[1] = region(4500.0, 30_000.0);
    config.regions[0].dynamics.makeup = -18.0;
    config.regions[1].dynamics.makeup = -18.0;

    let mut split = input.clone();
    process(
        &mut Engine::new(RATE),
        &config,
        &[64],
        &mut split,
        &mut input.clone(),
    );
    assert!(amplitude(&split, 900.0, RATE) > 0.13);

    config.regions[1].enabled = false;
    let mut first = input.clone();
    let mut engine = Engine::new(RATE);
    process(&mut engine, &config, &[64], &mut first, &mut input.clone());
    let telemetry = crate::Telemetry::default();
    telemetry.publish(&engine);
    let mut response = crate::region::dynamics::MaskResponse::default();
    telemetry.region_dynamics_response.read_into(&mut response);
    assert!(response.active());
    for hz in [110.0, 900.0, 9000.0] {
        let measured = 20.0 * (amplitude(&first, hz, RATE) / amplitude(&input, hz, RATE)).log10();
        assert!((measured - response.db(hz, RATE)).abs() < 0.02);
    }
    config.regions[1] = config.regions[0];
    let mut overlap = input.clone();
    process(
        &mut Engine::new(RATE),
        &config,
        &[64],
        &mut overlap,
        &mut input.clone(),
    );
    assert!(
        first
            .iter()
            .zip(overlap)
            .all(|(&first, overlap)| first == overlap)
    );

    config.regions[1].enabled = false;
    config.regions[0].softness = 100.0;
    let mut soft = input.clone();
    process(
        &mut Engine::new(RATE),
        &config,
        &[64],
        &mut soft,
        &mut input.clone(),
    );
    assert!((amplitude(&soft, 900.0, RATE) - amplitude(&first, 900.0, RATE)).abs() > 0.0001);

    config.regions[0].softness = RegionControl::default().softness;
    config.regions[1] = config.regions[0];
    config.regions[1].dynamics.makeup = -3.0;
    let mut engine = Engine::new(RATE);
    let mut removed = input.clone();
    let mut right = input.clone();
    let halfway = input.len() / 2;

    process(
        &mut engine,
        &config,
        &[128],
        &mut removed[..halfway],
        &mut right[..halfway],
    );
    config.regions[1].dynamics.enabled = false;
    process(
        &mut engine,
        &config,
        &[128],
        &mut removed[halfway..],
        &mut right[halfway..],
    );

    let expected = amplitude(&first, 110.0, RATE);
    let actual = amplitude(&removed, 110.0, RATE);
    assert!(
        (expected - actual).abs() < 0.001,
        "retained {actual}, reference {expected}"
    );
}

#[test]
fn spectral_stereo_and_sidechain() {
    let mut config = controls(flat_bands());
    config.regions[0] = region(10.0, 30_000.0);
    config.regions[0].dynamics.ratio = 4.0;
    config.regions[0].dynamics.threshold = -30.0;
    config.regions[0].dynamics.source = DetectorInput::Sidechain;

    let input = combined(RATE, RATE as usize, &[(1200.0, 0.12)]);
    let side_left = combined(RATE, RATE as usize, &[(1000.0, 0.62)]);
    let side_right = vec![0.0; input.len()];
    let mut engine = Engine::new(RATE);
    let mut left = input.clone();
    let mut right = input.clone();
    engine.process_block(&mut left, &mut right, &config);
    assert_eq!(left, input);
    assert!(engine.region_dynamics.meter(0).1.external_missing);

    for link in [0.0, 50.0, 100.0] {
        let mut left = input.clone();
        let mut right = input.clone();
        config.regions[0].dynamics.link = link;
        let mut engine = Engine::new(RATE);
        nice_assert_no_alloc::assert_no_alloc(|| {
            engine.process_block_with_sidechain(
                &mut left,
                &mut right,
                Some([&side_left, &side_right]),
                &config,
            )
        });

        let (l, r) = (
            amplitude(&left, 1200.0, RATE),
            amplitude(&right, 1200.0, RATE),
        );
        assert!(l < 0.055);
        match link {
            0.0 => assert!((r - 0.12).abs() < 0.001),
            50.0 => assert!(r > l && r < 0.095),
            _ => assert_eq!(left, right),
        }
    }
}

#[test]
fn spectral_ballistics() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut config = controls(flat_bands());
        config.regions[0] = region(10.0, 30_000.0);
        let settings = &mut config.regions[0].dynamics;
        settings.threshold = -21.0;
        settings.ratio = 4.0;
        settings.attack = 10.0;
        settings.release = 100.0;
        settings.knee = 0.0;

        for detector in [Detector::Peak, Detector::Rms] {
            config.regions[0].dynamics.detector = detector;
            let mut engine = Engine::new(rate);
            let frames = rate as usize;
            let mut left = vec![0.045; 2 * frames];
            left[frames / 2..frames].fill(0.45);
            let mut right = left.clone();
            process(&mut engine, &config, &[31, 64, 256], &mut left, &mut right);

            let settled = 20.0 * (left[frames - 1] / 0.45).log10();
            let attack = left[frames / 2..frames]
                .iter()
                .position(|x| 20.0 * (*x / 0.45).log10() <= settled * 0.632)
                .expect("attack crosses 63.2%") as f32
                / rate
                * 1000.0;
            // measure the sustained gain after the frequency mask's step transient
            let skip = (rate * 0.003) as usize;
            let release = left[frames + skip..]
                .iter()
                .position(|x| 20.0 * (*x / 0.045).log10() >= settled * 0.368)
                .map(|sample| sample + skip)
                .expect("release crosses 63.2%") as f32
                / rate
                * 1000.0;
            assert!(
                (7.0..30.0).contains(&attack),
                "{rate:.0} Hz {detector:?} attack {attack:.2} ms"
            );
            assert!(
                (90.0..190.0).contains(&release),
                "{rate:.0} Hz {detector:?} release {release:.2} ms"
            );
        }
    }
}

#[test]
fn spectral_modes_and_automation() {
    let rate = 48_000.0;
    let mut input = combined(rate, 24_576, &[(210.0, 0.19), (2200.0, 0.07)]);
    let mut noise = 1307_u32;
    let mut pink = 0.0;
    for (i, sample) in input.iter_mut().enumerate() {
        noise = noise.wrapping_mul(1664525).wrapping_add(1013904223);
        let white = ((noise >> 8) as f32 / 16_777_216.0 - 0.5) * 0.03;
        pink += 0.08 * (white - pink);
        *sample += white + pink;
        if i == 5000 {
            *sample += 0.28;
        }
        if i < 1024 {
            *sample = 0.0;
        }
    }

    for mode in [
        (Oversampling::Off, None),
        (Oversampling::X2, None),
        (Oversampling::Off, Some(LinearQuality::Short)),
    ] {
        let render = |sizes: &[usize], monitor: bool| {
            let mut settings = controls(flat_bands());
            (settings.oversampling, settings.linear_phase) = mode;
            settings.regions[0] = region(60.0, 1700.0);
            let region = &mut settings.regions[0];
            region.gain = -2.0;
            region.harmonic.enabled = true;
            region.harmonic.source = FundamentalSource::Fixed;
            region.harmonic.fixed_hz = 105.0;
            region.resonance.enabled = true;
            region.resonance.depth = 2.0;
            region.responses[0] = crate::region::response::Response {
                enabled: true,
                target: crate::region::response::Value::ResonanceQ,
                strength: 0.4,
                ..Default::default()
            };
            region.dynamics.attack = 2.0;
            region.dynamics.ratio = 3.0;
            let mut engine = Engine::new(rate);
            engine.prepare_processing(&settings);
            let pdc = engine.latency();
            engine.region_dynamics.monitor = monitor.then_some(0);
            let mut left = input.clone();
            let mut right = input.clone();

            for (i, (left, right)) in left
                .chunks_mut(4096)
                .zip(right.chunks_mut(4096))
                .enumerate()
            {
                let dynamics = &mut settings.regions[0].dynamics;
                dynamics.threshold = if i % 2 == 0 { -34.0 } else { -20.0 };
                dynamics.bypass = i == 3;
                process(&mut engine, &settings, sizes, left, right);
                assert_eq!(engine.latency(), pdc);
            }

            assert!(left.iter().all(|sample| sample.is_finite()));
            let step = left
                .windows(2)
                .enumerate()
                .filter(|(i, _)| !(4900..5300).contains(i))
                .map(|(_, pair)| (pair[1] - pair[0]).abs())
                .fold(0.0, f32::max);
            assert!(step < 0.31, "step {step}");
            left
        };

        assert_eq!(render(&[128], false), render(&[17, 7, 97, 1024], true));
    }
}
