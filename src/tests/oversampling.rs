use super::*;

/// block kernel and per-sample path must match
#[test]
fn block_vs_sample_match() {
    let mut bands = flat_bands();
    bands[1] = BandControl {
        slope: CutSlope::Db96,
        ..band(700.0, 0.0, 0.7071, FilterKind::Lowpass)
    };
    bands[4] = band(2_500.0, 7.5, 2.0, FilterKind::Peak);
    bands[5] = BandControl {
        channel: ChannelMode::Mid,
        ..band(3_000.0, -4.0, 1.0, FilterKind::Peak)
    };
    bands[6] = BandControl {
        channel: ChannelMode::Side,
        ..band(9_000.0, 5.0, 0.8, FilterKind::HighShelf)
    };
    let controls = controls(bands);

    let total = 8_192;
    let input: Vec<f32> = (0..total)
        .map(|i| {
            let phase = std::f32::consts::TAU * 431.0 * i as f32 / RATE;
            phase.sin() * 0.3
        })
        .collect();

    // 1 frame per call (per-sample path)
    let mut single = Engine::new(RATE);
    single.analyze = false;
    let mut expected = Vec::new();
    for sample in &input {
        let out = single.process_frame([*sample, *sample * 0.8], &controls);
        expected.push(out);
    }

    // odd block size 97 (not aligned to block/coeff intervals)
    let mut blocked = Engine::new(RATE);
    blocked.analyze = false;
    let mut actual = Vec::new();
    let mut left = vec![0.0_f32; 97];
    let mut right = vec![0.0_f32; 97];
    let mut offset = 0;
    while offset < total {
        let len = (total - offset).min(97);
        for i in 0..len {
            left[i] = input[offset + i];
            right[i] = input[offset + i] * 0.8;
        }
        blocked.process_block(&mut left[..len], &mut right[..len], &controls);
        for i in 0..len {
            actual.push([left[i], right[i]]);
        }
        offset += len;
    }

    let worst = expected
        .iter()
        .zip(&actual)
        .map(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()))
        .fold(0.0_f32, f32::max);

    assert!(worst < 1e-5, "blocked path drifted {worst} from per-sample");
}

// bilinear warping baseline

/// Magnitude of the analog prototype the digital filter is trying to realise.
fn analog_peak_db(hz: f64, fc: f64, q: f64, gain: f64) -> f64 {
    let a = 10.0_f64.powf(gain / 40.0);
    let w0 = std::f64::consts::TAU * fc;
    let w = std::f64::consts::TAU * hz;
    let num = realfft::num_complex::Complex64::new(w0 * w0 - w * w, w * w0 * a / q);
    let den = realfft::num_complex::Complex64::new(w0 * w0 - w * w, w * w0 / (a * q));
    20.0 * (num / den).norm().log10()
}

// oversampling

/// steady-state RMS of 1s sine
fn measure_with(controls: &Controls, hz: f32) -> f32 {
    const BLOCK: usize = 64;

    let mut engine = Engine::new(RATE);
    let total = RATE as usize;
    let settle = (RATE * 0.6) as usize;
    let mut sum = 0.0;
    let mut count = 0;

    let mut left = [0.0_f32; BLOCK];
    let mut right = [0.0_f32; BLOCK];

    let mut offset = 0;
    while offset < total {
        for (k, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let value = (std::f32::consts::TAU * hz * (offset + k) as f32 / RATE).sin() * 0.25;
            *l = value;
            *r = value;
        }

        engine.process_block(&mut left, &mut right, controls);

        if offset >= settle {
            for value in left.iter().chain(right.iter()) {
                sum += value * value;
                count += 1;
            }
        }

        offset += BLOCK;
    }

    (sum / count as f32).sqrt()
}

/// higher sample rate shrinks bilinear warping error at high freqs
#[test]
fn oversampling_reduces_warping() {
    use crate::oversample::Oversampling;

    let centre = 6_000.0;
    let gain = 12.0;
    let probe = 12_000.0;

    let expected = analog_peak_db(probe as f64, centre as f64, 1.0, gain as f64);
    let mut errors = Vec::new();

    for mode in Oversampling::ALL {
        let mut bands = flat_bands();
        bands[3] = band(centre, gain, 1.0, FilterKind::Peak);

        let mut shaped = controls(bands);
        shaped.oversampling = mode;

        let mut flat = controls(flat_bands());
        flat.oversampling = mode;

        let measured = db(measure_with(&shaped, probe), measure_with(&flat, probe));
        let error = (measured as f64 - expected).abs();
        println!("{mode:?}: {measured:6.2} dB at {probe} Hz, {error:5.2} dB off the prototype");
        errors.push(error);
    }

    // 2x should halve 1x error, 4x beats 2x
    assert!(errors[0] > 0.4, "1x baseline looks wrong: {errors:?}");
    assert!(
        errors[1] < errors[0] * 0.5,
        "2x only improved to {errors:?}"
    );
    assert!(
        errors[2] < errors[1],
        "4x did not improve on 2x: {errors:?}"
    );
    assert!(errors[2] < 0.1, "4x left {errors:?}");
}

/// changing oversampling updates latency and keeps audio finite
#[test]
fn oversampling_switch_safe() {
    use crate::oversample::{Oversampling, TAPS};

    let mut bands = flat_bands();
    bands[4] = band(2_000.0, 9.0, 1.5, FilterKind::Peak);
    bands[6] = BandControl {
        slope: CutSlope::Db48,
        ..band(11_000.0, 0.0, 0.7071, FilterKind::Highpass)
    };

    let mut engine = Engine::new(RATE);
    engine.analyze = false;

    let mut peak = 0.0_f32;
    assert_eq!(engine.latency(), 0);
    let mut left = [0.0_f32; 128];
    let mut right = [0.0_f32; 128];

    for block in 0..400 {
        let mut controls = controls(bands);
        let (mode, expected_latency) = match (block / 40) % 3 {
            0 => (Oversampling::Off, 0),
            1 => (Oversampling::X2, TAPS as u32 / 2),
            _ => (Oversampling::X4, TAPS as u32 * 3 / 4),
        };
        controls.oversampling = mode;

        for (k, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let t = (block * 128 + k) as f32;
            let value = (std::f32::consts::TAU * 440.0 * t / RATE).sin() * 0.25;
            *l = value;
            *r = value * 0.8;
        }

        engine.process_block(&mut left, &mut right, &controls);

        assert_eq!(engine.latency(), expected_latency, "{mode:?}");
        for value in left.iter().chain(right.iter()) {
            assert!(value.is_finite(), "output went non-finite at block {block}");
            peak = peak.max(value.abs());
        }
    }

    // A +9 dB bell on a 0.25 amplitude sine must stay well under clipping.
    assert!(peak > 0.1, "signal never came through: {peak}");
    assert!(peak < 1.5, "switching oversampling rang up to {peak}");
}
