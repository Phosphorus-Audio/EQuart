use super::*;
use crate::oversample::Oversampling;
use crate::processing_mode::{LinearQuality, ProcessingMode};

#[test]
fn linear_phase_impulse_is_symmetric_and_dry_is_aligned() {
    let mut bands = flat_bands();
    bands[0] = band(1_000.0, 12.0, 2.0, FilterKind::Peak);
    bands[0].channel = ChannelMode::Left;

    let mut config = controls(bands);
    config.linear_phase = Some(LinearQuality::Standard);
    let mut engine = Engine::new(RATE);

    engine.set_processing(config.linear_phase, Oversampling::Off);

    let delay = engine.latency() as usize;
    let frames = 2 * delay + 1;
    let mut left = vec![0.0_f64; frames];
    let mut right = left.clone();
    left[0] = 1.0;
    right[0] = 1.0;

    nice_assert_no_alloc::assert_no_alloc(|| {
        for (left, right) in left.chunks_mut(117).zip(right.chunks_mut(117)) {
            engine.process_block_with_sidechain(left, right, None, &config);
        }
    });

    assert_eq!(engine.latency() as usize, delay);
    assert!(left[..delay].iter().any(|x| x.abs() > 1e-3));

    for offset in 1..=delay {
        assert!((left[delay - offset] - left[delay + offset]).abs() < 1e-10);
    }

    for (i, &sample) in right.iter().enumerate() {
        assert!((sample - f64::from(i == delay)).abs() < 1e-10);
    }

    let magnitude: f64 = left
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let angle =
                std::f64::consts::TAU * 1_000.0 * (i as f64 - delay as f64) / f64::from(RATE);
            x * angle.cos()
        })
        .sum();
    assert!((20.0 * magnitude.log10() - 12.0).abs() < 0.15);

    engine.reset();
    config.bypass = true;
    left.fill(0.0);
    right.fill(0.0);
    left[0] = 1.0 + 1e-10;
    engine.process_block_with_sidechain(&mut left, &mut right, None, &config);
    assert_eq!(left[delay], 1.0 + 1e-10);
    assert_eq!(left.iter().filter(|&&x| x != 0.0).count(), 1);

    for (quality, latency) in [(LinearQuality::Short, 768), (LinearQuality::High, 4352)] {
        config.linear_phase = Some(quality);
        left.fill(0.0);
        right.fill(0.0);
        left[0] = 1.0;

        nice_assert_no_alloc::assert_no_alloc(|| {
            engine.process_block_with_sidechain(&mut left, &mut right, None, &config);
        });

        assert_eq!(engine.latency(), latency as u32);
        assert_eq!(left[latency], 1.0);
        assert_eq!(left.iter().filter(|&&x| x != 0.0).count(), 1);
    }
}

#[test]
fn zero_and_natural_phase_use_different_design_rates_and_latency() {
    let mut bands = flat_bands();
    bands[0] = band(15_000.0, 12.0, 1.0, FilterKind::Peak);
    let mut config = controls(bands);
    let mut engine = Engine::new(44_100.0);


    config.oversampling = ProcessingMode::ZeroLatency.oversampling(Oversampling::X4);
    let zero = engine.process_frame([1.0; 2], &config);
    assert_eq!(engine.latency(), 0);
    assert!(zero[0] > 0.0);

    config.oversampling = ProcessingMode::NaturalPhase.oversampling(Oversampling::Off);
    let natural = engine.process_frame([1.0; 2], &config);
    assert_eq!(engine.latency(), 48);
    assert!(natural[0].abs() < 0.01);
}

#[test]
fn linear_phase_tracks_short_attack_after_latency_compensation() {
    const ONSET: usize = 24_100;
    let mut bands = flat_bands();
    bands[0] = band(1_000.0, 0.0, 2.0, FilterKind::Peak);
    let dynamics = &mut bands[0].dynamics;
    dynamics.enabled = true;
    dynamics.threshold = -18.0;
    dynamics.above.range = -12.0;
    dynamics.above.ratio = 1.0;
    dynamics.attack_ms = 2.0;

    let mut config = controls(bands);
    config.linear_phase = Some(LinearQuality::Standard);
    let mut engine = Engine::new(RATE);


    let input: Vec<_> = (0..48_000)
        .map(|i| {
            let wave = (std::f64::consts::TAU * (i % 48) as f64 / 48.0).sin() as f32;
            wave * if i >= ONSET { 0.7 } else { 0.05 }
        })
        .collect();
    let mut left = input.clone();
    let mut right = input.clone();

    let tone: Vec<_> = (0..=LinearQuality::Standard.center())
        .map(|n| (std::f64::consts::TAU * n as f64 / 48.0).cos())
        .collect();
    let mut threshold = None;
    let mut target_90 = None;
    let mut gain_90 = None;
    let mut kernel_90 = None;
    let mut active_revision = 0;

    nice_assert_no_alloc::assert_no_alloc(|| {
        for (block, (left, right)) in left.chunks_mut(32).zip(right.chunks_mut(32)).enumerate() {
            engine.process_block(left, right, &config);
            let time = (block + 1) * 32;
            let level = engine.dynamic_levels().next().flatten().unwrap();

            if level >= -18.0 {
                threshold.get_or_insert(time);
            }
            if level >= -7.2 {
                target_90.get_or_insert(time);
            }
            if engine.dynamic_gains()[0] <= -10.8 {
                gain_90.get_or_insert(time);
            }

            let revision = engine.linear_response_revision();
            if revision != active_revision {
                active_revision = revision;
                let taps = engine.linear_response().unwrap()[0].unwrap();
                let magnitude = taps[0]
                    + 2.0
                        * taps[1..]
                            .iter()
                            .zip(&tone[1..])
                            .map(|(tap, tone)| tap * tone)
                            .sum::<f64>();
                if 20.0 * magnitude.abs().log10() <= -10.8 {
                    kernel_90.get_or_insert(time);
                }
            }
        }
    });

    let delay = engine.latency() as usize;
    let timings = [threshold, target_90, gain_90, kernel_90].map(|time| time.unwrap() - ONSET);
    eprintln!(
        "dynamic timing, samples from input onset: threshold/target/gain/kernel = {timings:?}, PDC {delay}"
    );
    assert!(timings[0] <= timings[1] && timings[1] <= timings[2] && timings[2] <= timings[3]);

    let gain_at = |sample| {
        let rms = |period: &[f32]| period.iter().map(|x| x * x).sum::<f32>().sqrt();
        db(
            rms(&left[sample + delay..sample + delay + 48]),
            rms(&input[sample..sample + 48]),
        )
    };
    let early_gain = gain_at(ONSET + 720);
    assert!(
        early_gain < -10.5,
        "dynamic gain after 15 ms was {early_gain} dB"
    );
    assert!((gain_at(40_000) + 12.0).abs() < 0.2);
}

#[test]
fn extended_linear_phase_resolves_a_narrow_32_hz_bell() {
    let mut bands = flat_bands();
    bands[0] = band(32.0, 12.0, 12.0, FilterKind::Peak);
    let mut config = controls(bands);
    config.linear_phase = Some(LinearQuality::Extended);
    let mut engine = Engine::new(RATE);


    let mut left = vec![0.0_f64; 34_000];
    let mut right = left.clone();
    left[0] = 1.0;

    nice_assert_no_alloc::assert_no_alloc(|| {
        for (left, right) in left.chunks_mut(63).zip(right.chunks_mut(63)) {
            engine.process_block_with_sidechain(left, right, None, &config);
        }
    });

    let delay = engine.latency() as usize;
    assert_eq!(delay, 16_640);

    let magnitude: f64 = left
        .iter()
        .enumerate()
        .map(|(n, &tap)| {
            let angle = std::f64::consts::TAU * 32.0 * (n as f64 - delay as f64) / f64::from(RATE);
            tap * angle.cos()
        })
        .sum();
    let gain = 20.0 * magnitude.log10();
    assert!((10.0..=12.0).contains(&gain), "32 Hz gain was {gain} dB");

    for offset in 1..=delay {
        assert!((left[delay - offset] - left[delay + offset]).abs() < 1e-12);
    }
}

#[test]
fn linear_phase_automation_and_bypass_are_independent_of_host_blocks() {
    const CHANGE: usize = 12_083;
    const BYPASS: usize = 24_311;
    const FRAMES: usize = 36_000;

    let render = |blocks: &[usize]| {
        let mut bands = flat_bands();
        bands[0] = band(1_000.0, 0.0, 1.0, FilterKind::Peak);

        let mut config = controls(bands);
        config.linear_phase = Some(LinearQuality::Standard);
        let mut engine = Engine::new(RATE);


        let input: Vec<_> = (0..FRAMES)
            .map(|i| {
                let angle = std::f64::consts::TAU * (i % 48) as f64 / 48.0;
                [0.2 * angle.sin(), 0.2 * angle.cos()]
            })
            .collect();
        let mut left: Vec<_> = input.iter().map(|x| x[0]).collect();
        let mut right: Vec<_> = input.iter().map(|x| x[1]).collect();

        nice_assert_no_alloc::assert_no_alloc(|| {
            let mut pos = 0;
            let mut callback = blocks.iter().cycle();

            while pos < FRAMES {
                let limit = if pos < CHANGE {
                    CHANGE
                } else if pos < BYPASS {
                    config.bands[0].gain = 12.0;
                    BYPASS
                } else {
                    config.bypass = true;
                    FRAMES
                };
                let end = (pos + callback.next().unwrap()).min(limit);

                engine.process_block_with_sidechain(
                    &mut left[pos..end],
                    &mut right[pos..end],
                    None,
                    &config,
                );
                pos = end;
            }
        });

        let delay = engine.latency() as usize;

        for i in BYPASS + 256..FRAMES {
            assert_eq!([left[i], right[i]], input[i - delay]);
        }

        [left, right]
    };

    let stable = render(&[128]);
    let varied = render(&[17, 7, 97, 128, 251, 64, 31]);
    assert_eq!(stable, varied);

    let amplitude = |i: usize| stable[0][i].hypot(stable[1][i]);
    assert!((amplitude(10_000) - 0.2).abs() < 1e-10);
    assert!((amplitude(20_000) - 0.2 * 10.0_f64.powf(12.0 / 20.0)).abs() < 0.002);

    for i in CHANGE..BYPASS + 256 {
        assert!((amplitude(i) - amplitude(i - 1)).abs() < 0.006);
    }
}

#[test]
fn plot_response_tracks_the_applied_fir_at_each_rate_and_quality() {
    let mut bands = flat_bands();
    bands[0] = band(32.0, 12.0, 12.0, FilterKind::Peak);
    let mut config = controls(bands);
    let telemetry = crate::Telemetry::default();
    let mut response = crate::telemetry::FirResponse::default();

    for (rate, quality) in [
        (48_000.0, LinearQuality::Standard),
        (96_000.0, LinearQuality::Extended),
    ] {
        let mut engine = Engine::new(rate);

        config.linear_phase = Some(quality);
        engine.set_processing(config.linear_phase, Oversampling::Off);

        let center = quality.center_at(rate);
        let delay = engine.latency() as usize;
        let mut left = vec![0.0_f64; 2 * delay + 1];
        let mut right = left.clone();
        left[0] = 1.0;

        response.update(&telemetry.fir);
        nice_assert_no_alloc::assert_no_alloc(|| {
            engine.process_block_with_sidechain(&mut left, &mut right, None, &config);
            telemetry.publish(&engine);
        });
        response.update(&telemetry.fir);

        for hz in [
            19.0, 28.0, 30.0, 31.0, 32.0, 33.0, 34.0, 37.0, 100.0, 1000.0,
        ] {
            let magnitude: f64 = left
                .iter()
                .enumerate()
                .map(|(n, tap)| {
                    let angle =
                        std::f64::consts::TAU * f64::from(hz / rate) * (n as f64 - delay as f64);
                    tap * angle.cos()
                })
                .sum();
            let plotted = response.db(hz, rate, center).unwrap();
            assert!((f64::from(plotted) - 20.0 * magnitude.abs().log10()).abs() < 0.08);
        }

        assert!(response.db(32.0, rate, center / 2).is_none());
    }
}
