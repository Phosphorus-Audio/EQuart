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
fn dynamic_block_independent() {
    let config = dynamic_band(true);
    let source: Vec<_> = (0..8_191).map(sine).collect();
    let mut rendered = Vec::new();
    for chunk in [17, 512] {
        let mut engine = Engine::new(RATE);
        engine.analyze = false;
        let mut left = source.clone();
        let mut right = source.clone();
        for (left, right) in left.chunks_mut(chunk).zip(right.chunks_mut(chunk)) {
            engine.process_block(left, right, &config);
        }
        rendered.push(left);
    }
    assert!(
        rendered[0]
            .iter()
            .zip(&rendered[1])
            .all(|(a, b)| (a - b).abs() < 1e-5)
    );
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
        engine.analyze = false;
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
        telemetry.publish(&engine, false);
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
