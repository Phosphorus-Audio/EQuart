use super::*;
use crate::{oversample::Oversampling, precision::Precision, processing_mode::LinearQuality};
use realfft::num_complex::Complex64;
use std::{f64::consts::PI, time::Duration};

fn brick(hz: f32, kind: FilterKind) -> BandControl {
    BandControl {
        slope: CutSlope::Brickwall,
        ..band(hz, 0.0, 12.0, kind)
    }
}

fn impulse(engine: &mut Engine, controls: &Controls, frames: usize) -> Vec<f64> {
    let mut left = vec![0.0; frames];
    let mut right = left.clone();
    left[0] = 1.0;
    right[0] = 1.0;

    nice_assert_no_alloc::assert_no_alloc(|| {
        for (left, right) in left.chunks_mut(128).zip(right.chunks_mut(128)) {
            engine.process_block_with_sidechain(left, right, None, controls);
        }
    });

    left
}

fn transfer(signal: &[f64], rate: f32, hz: f64) -> Complex64 {
    let rotation = Complex64::from_polar(1.0, -2.0 * PI * hz / f64::from(rate));
    let mut advance = Complex64::new(1.0, 0.0);
    let mut result = Complex64::default();

    for sample in signal {
        result += advance * *sample;
        advance *= rotation;
    }

    result
}

fn level(magnitude: f64) -> f64 {
    20.0 * magnitude.log10()
}

#[test]
fn elliptic_response() {
    for rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        for cutoff in [30.0, 100.0, 1_000.0, 10_000.0] {
            let mut engine = Engine::new(rate);

            for kind in [FilterKind::Lowpass, FilterKind::Highpass] {
                let control = brick(cutoff, kind);
                let high = kind == FilterKind::Highpass;
                let mut bands = flat_bands();
                bands[0] = control;
                let mut config = controls(bands);
                config.precision = Precision::Single;
                engine.reset();

                let frames = (f64::from(rate) * 320.0 / f64::from(cutoff)).ceil() as usize;
                let samples = impulse(&mut engine, &config, frames.max(8192));
                assert_eq!(engine.latency(), 0);

                let (coefficients, count) = control.sections_for::<f64>(rate);
                let response = |hz| {
                    coefficients[..count]
                        .iter()
                        .map(|section| section.response(f64::from(rate), hz))
                        .product::<Complex64>()
                };
                let r = f64::from(rate);
                let warp = (PI * f64::from(cutoff) / r).tan();
                let probe =
                    |ratio: f64| (warp * if high { ratio.recip() } else { ratio }).atan() * r / PI;
                let mut pass_min = 0.0_f64;
                let mut pass_max = -1.0_f64;
                let mut stop_max = -200.0_f64;

                for index in 0..=800 {
                    let t = index as f64 / 800.0;
                    let pass = level(response(probe((0.01 + t) * 0.982)).norm());
                    let stop = level(response(probe(1.075 * 300.0_f64.powf(t))).norm());
                    pass_min = pass_min.min(pass);
                    pass_max = pass_max.max(pass);
                    stop_max = stop_max.max(stop);
                }

                assert!(
                    pass_min >= -0.051 && pass_max <= 0.001,
                    "{rate} {cutoff} {kind:?}"
                );
                assert!(stop_max <= -95.99, "{stop_max} at {rate} {cutoff} {kind:?}");
                assert!((level(response(f64::from(cutoff)).norm()) + 3.0103).abs() < 0.0001);

                for ratio in [0.9, 1.0, 1.05, 1.2, 2.0] {
                    let hz = probe(ratio);
                    let actual = transfer(&samples, rate, hz);
                    let error = (actual - response(hz)).norm();
                    assert!(error < 0.00002, "{rate} {cutoff} {kind:?} at {hz}: {error}");
                }
            }
        }
    }
}

#[test]
fn fir_symmetry_and_pdc() {
    let mut bands = flat_bands();
    bands[0] = brick(1_000.0, FilterKind::Lowpass);
    let mut config = controls(bands);
    let mut engine = Engine::new(RATE);

    for quality in LinearQuality::ALL {
        config.linear_phase = Some(quality);
        engine.prepare_processing(&config);
        let delay = engine.latency() as usize;
        let center = quality.center_at(RATE);
        assert_eq!(delay, center + 256);
        let samples = impulse(&mut engine, &config, delay + center + 1);
        let taps = engine.linear_response().unwrap()[0].unwrap();
        let cutoff = level(transfer(&samples, RATE, 1_000.0).norm());

        assert!((cutoff + 6.0206).abs() < 0.05, "{quality:?}: {cutoff}");
        assert!((level(transfer(&samples, RATE, 500.0).norm())).abs() < 0.04);
        assert!(level(transfer(&samples, RATE, 2_000.0).norm()) < -90.0);

        for offset in 0..=center {
            assert!((samples[delay - offset] - samples[delay + offset]).abs() < 1e-12);
            assert!((samples[delay + offset] - taps[offset]).abs() < 1e-12);
        }
    }
}

#[test]
fn brickwall_automation() {
    let mut bands = flat_bands();
    bands[0] = brick(8_000.0, FilterKind::Highpass);
    let mut config = controls(bands);
    let mut engine = Engine::new(RATE);
    let mut maximum = 0.0_f64;

    for (mode, latency) in [
        (Oversampling::Off, 0),
        (Oversampling::X2, 32),
        (Oversampling::X4, 48),
    ] {
        config.oversampling = mode;
        engine.prepare_processing(&config);
        assert_eq!(engine.latency(), latency);

        for step in 0..120 {
            config.bands[0].frequency = [30.0, 10_000.0, 23_500.0, 1_000.0][step % 4];
            config.bands[0].slope = if step % 14 == 0 {
                CutSlope::Db96
            } else {
                CutSlope::Brickwall
            };
            let mut left = std::array::from_fn::<_, 64, _>(|index| {
                (0.17 * (64 * step + index) as f64).sin() * 0.25
            });
            let mut right = left;

            nice_assert_no_alloc::assert_no_alloc(|| {
                engine.process_block_with_sidechain(&mut left, &mut right, None, &config);
            });
            assert!(left.iter().all(|sample| sample.is_finite()));
            maximum = maximum.max(left.iter().map(|sample| sample.abs()).fold(0.0, f64::max));
        }
    }

    println!("peak under steps and slope changes: {maximum:.6}");
    assert!(maximum < 1.0);
}

#[test]
fn tracked_brickwall_mapping() {
    use crate::modulation::{Lfo, Mapping, Target};

    let mut bands = flat_bands();
    bands[0] = brick(200.0, FilterKind::Highpass);
    bands[0].pitch.enabled = true;
    bands[0].pitch.algorithm = crate::pitch::DetectionMode::Mpm;
    bands[0].mapping = Mapping {
        enabled: true,
        source: Target::Frequency,
        target: Target::Frequency,
        strength: 1.0,
        reference: 200.0,
        ..Default::default()
    };
    bands[0].lfo = Lfo {
        enabled: true,
        target: Target::Frequency,
        depth: 5.0,
        sync: false,
        ..Default::default()
    };
    let config = controls(bands);
    let mut engine = Engine::new(RATE);

    for hz in [300.0_f32, 400.0, 500.0] {
        let mut left: Vec<_> = (0..48_000)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / RATE).sin() * 0.25)
            .collect();
        let mut right = left.clone();

        nice_assert_no_alloc::assert_no_alloc(|| {
            engine.process_block(&mut left, &mut right, &config);
        });
        let live = engine.modulation(0).unwrap();
        let expected = live.mapping_base * live.mapping_input / 200.0;
        assert!((live.frequency - expected).abs() < 0.05);
        assert!(
            left.iter()
                .all(|value| value.is_finite() && value.abs() < 1.0)
        );
    }

    assert_eq!(config.bands[0].frequency, 200.0);
}

#[test]
fn async_brickwall_quality() {
    let mut bands = flat_bands();
    bands[0] = brick(1_000.0, FilterKind::Highpass);
    let mut config = controls(bands);
    config.linear_phase = Some(LinearQuality::Short);
    let mut engine = Engine::new(RATE);
    engine.prepare_processing(&config);
    engine.start_fir_worker().unwrap();

    config.bands[0].frequency = 9_000.0;
    let mut left = [0.0_f64; 128];
    let mut right = left;
    nice_assert_no_alloc::assert_no_alloc(|| {
        engine.process_block_with_sidechain(&mut left, &mut right, None, &config);
    });
    config.linear_phase = Some(LinearQuality::Standard);
    config.bands[0].frequency = 2_000.0;

    for _ in 0..250 {
        nice_assert_no_alloc::assert_no_alloc(|| {
            engine.process_block_with_sidechain(&mut left, &mut right, None, &config);
        });
        if let Some([Some(taps), ..]) = engine.linear_response() {
            assert_eq!(taps.len(), LinearQuality::Standard.center() + 1);
            let dc = taps[0] + 2.0 * taps[1..].iter().sum::<f64>();
            assert!(dc.abs() < 0.0001);
            let pass: f64 = taps[0]
                + 2.0
                    * taps[1..]
                        .iter()
                        .enumerate()
                        .map(|(i, tap)| tap * (2.0 * PI * (i + 1) as f64 * 4000.0 / 48000.0).cos())
                        .sum::<f64>();
            assert!((pass - 1.0).abs() < 0.0001);
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    panic!("worker did not publish the current FIR");
}

#[test]
fn oversampled_cutoff() {
    for rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        let mut engine = Engine::new(rate);

        for kind in [FilterKind::Lowpass, FilterKind::Highpass] {
            let mut bands = flat_bands();
            bands[0] = brick(10_000.0, kind);
            let mut config = controls(bands);

            for (mode, latency) in [(Oversampling::X2, 32), (Oversampling::X4, 48)] {
                config.oversampling = mode;

                for hz in [10_000.0, rate * 0.47] {
                    config.bands[0].frequency = hz;
                    engine.prepare_processing(&config);
                    engine.reset();
                    let samples = impulse(&mut engine, &config, 8192);
                    let actual = transfer(&samples, rate, f64::from(hz));
                    let internal = rate * mode.factor() as f32;
                    let (coefficients, count) = config.bands[0].sections_for::<f64>(internal);
                    let weights =
                        crate::oversample::response_images(mode, f64::from(hz), f64::from(rate));
                    let expected: Complex64 = weights
                        .into_iter()
                        .enumerate()
                        .take(mode.factor())
                        .map(|(image, weight)| {
                            let shifted = hz + image as f32 * rate;
                            let signed = if shifted > internal * 0.5 {
                                shifted - internal
                            } else {
                                shifted
                            };
                            coefficients[..count]
                                .iter()
                                .map(|section| {
                                    section.response(f64::from(internal), f64::from(signed))
                                })
                                .product::<Complex64>()
                                * weight
                        })
                        .sum();
                    let compensate = Complex64::from_polar(
                        1.0,
                        2.0 * PI * f64::from(hz) * f64::from(latency) / f64::from(rate),
                    );

                    assert_eq!(engine.latency(), latency);
                    assert!((actual * compensate - expected).norm() < 0.00002);
                    if hz == 10_000.0 {
                        assert!((level(actual.norm()) + 3.0103).abs() < 0.04);
                    } else if rate == 48_000.0 {
                        println!(
                            "{mode:?} {kind:?} at {hz:.0} Hz: {:.3} dB",
                            level(actual.norm())
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn brickwall_time_domain() {
    use realfft::RealFftPlanner;

    const FRAMES: usize = 8192;
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(FRAMES);
    let mut random = 0x1234_5678_u32;
    let white: Vec<_> = (0..FRAMES)
        .map(|_| {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            f64::from(random) / f64::from(u32::MAX) - 0.5
        })
        .collect();
    let mut phase = 0.0;
    let sweep: Vec<_> = (0..FRAMES)
        .map(|i| {
            phase += 2.0 * PI * 20.0 * (900.0_f64).powf(i as f64 / FRAMES as f64) / 48_000.0;
            phase.sin() * 0.4
        })
        .collect();
    let mut pink_bins = fft.make_output_vec();
    fft.process(&mut white.clone(), &mut pink_bins).unwrap();
    pink_bins[0] = Complex64::default();

    for (index, bin) in pink_bins.iter_mut().enumerate().skip(1) {
        *bin /= (index as f64).sqrt() * (FRAMES as f64).sqrt();
    }

    let inverse = RealFftPlanner::<f64>::new().plan_fft_inverse(FRAMES);
    let mut pink = inverse.make_output_vec();
    inverse.process(&mut pink_bins, &mut pink).unwrap();
    let pink_peak = pink.iter().map(|sample| sample.abs()).fold(0.0, f64::max);

    for sample in &mut pink {
        *sample *= 0.4 / pink_peak;
    }

    let multisines: Vec<_> = (0..FRAMES)
        .map(|i| {
            [43.0, 335.0, 5140.0]
                .iter()
                .map(|hz| (2.0 * PI * i as f64 * hz / 48_000.0).sin() * 0.125)
                .sum()
        })
        .collect();
    let step: Vec<_> = (0..FRAMES)
        .map(|i| if i < 1024 { 0.0 } else { 0.25 })
        .collect();

    let mut engine = Engine::new(RATE);

    for kind in [FilterKind::Lowpass, FilterKind::Highpass] {
        let mut bands = flat_bands();
        bands[0] = brick(2_000.0, kind);
        let mut config = controls(bands);

        for mode in 0..3 {
            config.oversampling = if mode == 1 {
                Oversampling::X4
            } else {
                Oversampling::Off
            };
            config.linear_phase = (mode == 2).then_some(LinearQuality::Standard);
            engine.prepare_processing(&config);
            let mut peak = 0.0_f64;

            for signal in [&step, &sweep, &white, &pink, &multisines] {
                engine.reset();
                let mut left = signal.clone();
                let mut right = signal.clone();

                nice_assert_no_alloc::assert_no_alloc(|| {
                    engine.process_block_with_sidechain(&mut left, &mut right, None, &config);
                });
                assert!(left.iter().all(|value| value.is_finite()));
                assert_eq!(left, right);
                peak = peak.max(left.iter().map(|sample| sample.abs()).fold(0.0, f64::max));
            }

            let tone = if kind == FilterKind::Lowpass { 73 } else { 751 };
            let mut left: Vec<_> = (0..2 * FRAMES)
                .map(|i| (2.0 * PI * ((i * tone) % FRAMES) as f64 / FRAMES as f64).sin() * 0.25)
                .collect();
            let mut right = left.clone();
            engine.reset();
            engine.process_block_with_sidechain(&mut left, &mut right, None, &config);
            let mut spectrum = fft.make_output_vec();
            fft.process(&mut left[FRAMES..], &mut spectrum).unwrap();
            let main = spectrum[tone].norm_sqr();
            let thd = spectrum
                .iter()
                .skip(2 * tone)
                .step_by(tone)
                .map(|value| value.norm_sqr())
                .sum::<f64>()
                / main;
            let residual = spectrum
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != tone)
                .map(|(_, value)| value.norm_sqr())
                .sum::<f64>()
                / main;

            println!(
                "{kind:?} mode {mode}: peak {peak:.4}, THD {:.2} dB, residual {:.2} dB",
                10.0 * thd.log10(),
                10.0 * residual.log10()
            );
            assert!(peak < 1.0 && residual < 1e-10);
        }
    }
}

#[test]
#[ignore = "prints Brickwall frequency and callback measurements in release mode"]
fn profile_brickwall() {
    use realfft::RealFftPlanner;
    use std::time::Instant;

    const FFT: usize = 262_144;
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(FFT);
    println!(
        "fir,rate,quality,hz,kind,pdc,transition_hz,pass_min,pass_max,stop_max,cutoff_db,setup_us"
    );

    for rate in [44_100.0_f32, 48_000.0, 96_000.0, 192_000.0] {
        let mut engine = Engine::new(rate);
        let mut time = fft.make_input_vec();
        let mut bins = fft.make_output_vec();

        for quality in LinearQuality::ALL {
            for hz in [30.0_f32, 100.0, 1_000.0, 10_000.0] {
                for kind in [FilterKind::Lowpass, FilterKind::Highpass] {
                    let mut bands = flat_bands();
                    bands[0] = brick(hz, kind);
                    let mut config = controls(bands);
                    config.linear_phase = Some(quality);
                    engine.reset();
                    let start = Instant::now();
                    engine.prepare_processing(&config);
                    let setup_us = start.elapsed().as_secs_f64() * 1e6;
                    let taps = engine.linear_response().unwrap()[0].unwrap();
                    time.fill(0.0);
                    time[0] = taps[0];

                    for (offset, &tap) in taps.iter().enumerate().skip(1) {
                        time[offset] = tap;
                        time[FFT - offset] = tap;
                    }

                    fft.process(&mut time, &mut bins).unwrap();
                    let df = f64::from(rate) / FFT as f64;
                    let cutoff = (f64::from(hz) / df).round() as usize;
                    let upper = kind == FilterKind::Highpass;
                    let at = |n: usize| level(bins[n].norm());
                    let pass = if upper {
                        (cutoff..bins.len()).rev().find(|&i| at(i) < -0.05)
                    } else {
                        (0..cutoff).find(|&i| at(i) < -0.05)
                    };
                    let stop = if upper {
                        (0..=cutoff).find(|&i| at(i) > -90.0)
                    } else {
                        (cutoff..bins.len()).rev().find(|&i| at(i) > -90.0)
                    };
                    let passband =
                        pass.map_or(
                            0..0,
                            |pass| if upper { pass + 1..bins.len() } else { 0..pass },
                        );
                    let stopband =
                        stop.map_or(
                            0..0,
                            |stop| if upper { 0..stop } else { stop + 1..bins.len() },
                        );
                    let width = if passband.is_empty() || stopband.is_empty() {
                        f64::NAN
                    } else {
                        pass.unwrap().abs_diff(stop.unwrap()) as f64 * df
                    };
                    let pass_min = passband
                        .clone()
                        .map(at)
                        .reduce(f64::min)
                        .unwrap_or(f64::NAN);
                    let pass_max = passband.map(at).reduce(f64::max).unwrap_or(f64::NAN);
                    let stop_max = stopband.map(at).reduce(f64::max).unwrap_or(f64::NAN);
                    println!(
                        "fir,{rate},{quality:?},{hz},{kind:?},{},{width:.3},{pass_min:.4},{pass_max:.4},{stop_max:.2},{:.3},{setup_us:.2}",
                        engine.latency(),
                        at(cutoff),
                    );
                }
            }
        }
    }

    println!("cpu,mode,kind,bands,block,automation,mean_us,p95_us,p99_us,max_us,kernels");
    for mode in ["Zero", "Natural4x", "LinearStandard"] {
        for kind in [FilterKind::Lowpass, FilterKind::Highpass] {
            for count in [1, 8, 16, 32] {
                for block in [32, 64, 128, 512, 1024] {
                    for automate in [false, true] {
                        let mut bands = flat_bands();
                        for (i, band) in bands[..count].iter_mut().enumerate() {
                            *band = brick(2000.0 + i as f32 * 40.0, kind);
                        }
                        let mut config = controls(bands);
                        if mode == "Natural4x" {
                            config.oversampling = Oversampling::X4;
                        } else if mode == "LinearStandard" {
                            config.linear_phase = Some(LinearQuality::Standard);
                        }
                        let mut engine = Engine::new(RATE);
                        engine.prepare_processing(&config);
                        if config.linear_phase.is_some() && automate {
                            engine.start_fir_worker().unwrap();
                        }
                        let mut l = vec![0.0_f64; block];
                        let mut r = l.clone();
                        let mut times = Vec::with_capacity(320);
                        let mut revision = engine.linear_response_revision();
                        let mut kernels = 0;

                        for callback in 0..352 {
                            if automate {
                                for (i, band) in config.bands[..count].iter_mut().enumerate() {
                                    band.frequency = 1200.0
                                        + i as f32 * 40.0
                                        + 3200.0 * (callback % 69) as f32 / 69.0;
                                }
                            }
                            for (i, sample) in l.iter_mut().enumerate() {
                                *sample = (0.21 * (i + callback * block) as f64).sin() * 0.3;
                            }
                            r.copy_from_slice(&l);
                            let start = Instant::now();
                            engine.process_block_with_sidechain(&mut l, &mut r, None, &config);
                            let elapsed = start.elapsed().as_secs_f64() * 1e6;
                            if callback >= 32 {
                                times.push(elapsed);
                            }
                            let next = engine.linear_response_revision();
                            if revision != next {
                                kernels += 1;
                                revision = next;
                            }
                            if config.linear_phase.is_some() && automate && callback % 16 == 0 {
                                std::thread::sleep(Duration::from_millis(2));
                            }
                        }
                        times.sort_by(f64::total_cmp);
                        let mean = times.iter().sum::<f64>() / times.len() as f64;
                        println!(
                            "cpu,{mode},{kind:?},{count},{block},{automate},{mean:.3},{:.3},{:.3},{:.3},{kernels}",
                            times[303], times[316], times[319],
                        );
                    }
                }
            }
        }
    }
}
