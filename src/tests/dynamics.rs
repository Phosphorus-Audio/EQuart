use super::*;

// dynamic bands tests

fn dynamic_band(enabled: bool) -> Controls {
    let mut bands = flat_bands();
    bands[0] = BandControl {
        enabled: true,
        frequency: 1_000.0,
        gain: 0.0,
        q: 3.0,
        kind: FilterKind::Peak,
        dynamics: DynamicControl {
            enabled,
            ..Default::default()
        },
        ..BandControl::default()
    };
    controls(bands)
}

fn sine(i: usize) -> f32 {
    0.5 * (std::f32::consts::TAU * 1_000.0 * i as f32 / RATE).sin()
}

#[test]
fn dynamic_tone_separates_modulation_from_static_residual() {
    use crate::band::COEFF_FRAMES;
    use crate::dynamics::DetectorMode;
    use crate::precision::Precision;
    use realfft::RealFftPlanner;

    let frames = RATE as usize;
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(frames);
    let measure = |samples: &[f32], tone: usize| {
        let mut input: Vec<_> = samples.iter().copied().map(f64::from).collect();
        let mut spectrum = fft.make_output_vec();
        fft.process(&mut input, &mut spectrum).unwrap();
        let power = spectrum[tone].norm_sqr();
        let residual: f64 = spectrum
            .iter()
            .enumerate()
            .filter(|(bin, _)| *bin != tone)
            .map(|(bin, value)| {
                (if bin == 0 || bin == frames / 2 {
                    0.5
                } else {
                    1.0
                }) * value.norm_sqr()
            })
            .sum();

        let harmonic: f64 = spectrum
            .iter()
            .skip(2 * tone)
            .step_by(tone)
            .map(|value| value.norm_sqr())
            .sum();

        let clock = frames / COEFF_FRAMES;
        let sideband = tone.abs_diff((2 * tone).abs_diff(clock));
        let strongest_spur = spectrum
            .iter()
            .enumerate()
            .filter(|(bin, _)| *bin != tone)
            .max_by(|(_, a), (_, b)| a.norm_sqr().total_cmp(&b.norm_sqr()))
            .unwrap()
            .0;

        let db = [
            residual,
            harmonic,
            spectrum[3 * tone].norm_sqr(),
            spectrum[sideband].norm_sqr(),
        ]
        .map(|energy| 10.0 * (energy / power).log10());
        (db, strongest_spur)
    };

    for tone in [110, 999] {
        let amplitude = 10.0_f64.powf(-6.34 / 20.0);
        let source: Vec<_> = (0..frames)
            .map(|i| {
                (amplitude
                    * (std::f64::consts::TAU * ((i * tone) % frames) as f64 / f64::from(RATE))
                        .sin()) as f32
            })
            .collect();

        let mut config = dynamic_band(true);

        config.bands[0].frequency = tone as f32;
        config.bands[0].q = 1.0;

        let dynamic = &mut config.bands[0].dynamics;

        dynamic.threshold = -14.0;
        dynamic.above.range = -12.0;
        dynamic.above.ratio = 1.0;
        dynamic.detect.mode = DetectorMode::Broadband;

        let render = |config: &Controls| {
            let mut engine = Engine::new(RATE);


            let mut gain_sum = 0.0;
            let mut left = source.clone();
            let mut right = source.clone();

            engine.process_block(&mut left, &mut right, config);
            left.copy_from_slice(&source);
            right.copy_from_slice(&source);

            for (l, r) in left
                .chunks_mut(COEFF_FRAMES)
                .zip(right.chunks_mut(COEFF_FRAMES))
            {
                engine.process_block(l, r, config);
                gain_sum += f64::from(engine.dynamic_gains()[0]);
            }
            (
                measure(&left, tone),
                (gain_sum / (frames / COEFF_FRAMES) as f64) as f32,
            )
        };

        println!(
            "{tone} Hz input [residual, THD, H3, clock sideband] dB: {:?}",
            measure(&source, tone)
        );

        for precision in [Precision::Single, Precision::Double] {
            config.precision = precision;
            config.bands[0].dynamics.enabled = true;
            config.bands[0].dynamics.attack_ms = 2.0;
            config.bands[0].dynamics.release_ms = 2.0;

            let (fast, gain) = render(&config);

            assert!(
                (-5.0..-4.0).contains(&gain),
                "the dynamic band must operate away from its limits: {gain}"
            );

            config.bands[0].dynamics.attack_ms = 40.0;
            config.bands[0].dynamics.release_ms = 40.0;

            let (slow, _) = render(&config);

            config.bands[0].dynamics.enabled = false;
            config.bands[0].gain = gain;

            let (frozen, _) = render(&config);

            config.bands[0].gain = 0.0;

            println!(
                "{tone} Hz {precision:?}: fast {fast:?}, slow {slow:?}, frozen {frozen:?}, gain {gain:.2} dB"
            );

            assert!(
                frozen.0[0] < -135.0,
                "{precision:?}, {tone} Hz static reference"
            );

            if tone == 110 {
                assert!(fast.0[1] < -64.0, "{precision:?}: 110 Hz THD {fast:?}");
                assert!(
                    fast.0[1] - slow.0[1] > 15.0,
                    "{precision:?}: 110 Hz smoothing"
                );
            } else {
                assert!(
                    fast.0[0] < -122.0 && fast.0[3] < -126.0,
                    "{precision:?}: 999 Hz modulation {fast:?}"
                );
            }
        }
    }
}

#[test]
fn dynamic_block_independent() {
    use crate::oversample::Oversampling;

    let mut config = dynamic_band(true);
    config.bands[0].dynamics.threshold = -13.0;

    let source: Vec<_> = (0..8_191).map(sine).collect();

    for mode in Oversampling::ALL {
        config.oversampling = mode;
        let render = |chunk| {
            let mut engine = Engine::new(RATE);

            let mut left = source.clone();
            let mut right = source.clone();
            for (left, right) in left.chunks_mut(chunk).zip(right.chunks_mut(chunk)) {
                engine.process_block(left, right, &config);
            }
            left
        };

        let a = render(17);
        let b = render(512);

        assert!(
            a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-5),
            "{mode:?}"
        );
    }
}

#[test]
fn sidechain_relative_detection() {
    use crate::dynamics::{DetectorInput, DetectorMode};

    let mut config = dynamic_band(true);
    let dynamic = &mut config.bands[0].dynamics;
    dynamic.relative.input = DetectorInput::Sidechain;
    dynamic.relative.mode = DetectorMode::Free;
    dynamic.relative.frequency = 2_000.0;
    dynamic.relative.q = 3.0;
    dynamic.relative_amount = 1.0;
    for (reference, expected) in [(Some(0.09), -6.0), (Some(1.0), 0.0), (None, 0.0)] {
        let mut engine = Engine::new(RATE);

        let mut energy = 0.0;
        for i in (0..48_000).step_by(64) {
            let main = std::array::from_fn::<_, 64, _>(|step| sine(i + step) * 0.4);
            let mut left = main;
            let mut right = main;
            let side = std::array::from_fn::<_, 64, _>(|step| {
                reference.unwrap_or(0.0)
                    * (std::f32::consts::TAU * 2_000.0 * (i + step) as f32 / RATE).sin()
            });
            engine.process_block_with_sidechain(
                &mut left,
                &mut right,
                reference.map(|_| [&side[..], &side[..]]),
                &config,
            );
            if i >= 24_000 {
                energy += left.iter().map(|sample| sample * sample).sum::<f32>();
            }
        }
        let telemetry = crate::Telemetry::default();
        telemetry.publish(&engine);
        let (level, gain) = telemetry.dynamic_meter(0);
        assert_eq!(level.is_some(), reference.is_some());
        assert!((gain - expected).abs() < 0.04);
        let measured = db(
            (energy / 24_000.0).sqrt(),
            0.2 * std::f32::consts::FRAC_1_SQRT_2,
        );
        assert!(
            (measured - expected).abs() < 0.05,
            "{reference:?}: {measured:.2} dB"
        );
    }
}

/// dynamic cuts tone when active and restores when off
#[test]
fn dynamics_attenuates_and_restores() {
    let on = dynamic_band(true);
    let off = dynamic_band(false);
    let mut engine = Engine::new(RATE);
    let half = (RATE * 0.5) as usize;

    for (phase, (controls, expected_db)) in [(&on, -6.0), (&off, 0.0)].into_iter().enumerate() {
        let mut input_power = 0.0_f32;
        let mut output_power = 0.0_f32;
        for i in 0..half {
            let x = sine(phase * half + i);
            let y = engine.process_frame([x, x], controls);
            if i >= (RATE * 0.3) as usize {
                input_power += x * x;
                output_power += y[0] * y[0];
            }
        }

        let offset = engine.dynamic_gains()[0];
        assert!(
            (offset - expected_db).abs() < 0.05,
            "offset was {offset:.2} dB"
        );
        let db = 10.0 * (output_power / input_power).log10();
        assert!((db - expected_db).abs() < 0.4, "output gain was {db:.2} dB");
    }
}
