use super::*;

#[test]
fn region_boost_and_cut() {
    let mut settings = controls(flat_bands());
    settings.regions[0] = crate::region::RegionControl {
        enabled: true,
        low: 500.0,
        high: 2_000.0,
        ..Default::default()
    };
    let dry = 0.25 * std::f32::consts::FRAC_1_SQRT_2;
    for gain in [6.0, -6.0] {
        settings.regions[0].gain = gain;
        for (hz, fraction) in [
            (80.0, 0.0),
            (500.0, 0.5),
            (1_000.0, 1.0),
            (2_000.0, 0.5),
            (10_000.0, 0.0),
        ] {
            let measured = db(measure(&settings, hz), dry);
            assert!(
                (measured - gain * fraction).abs() < 0.12,
                "{hz} Hz: {measured} dB"
            );
        }
    }
    settings.regions[0].softness = 0.0;
    let hard = db(measure(&settings, 2_500.0), dry);
    settings.regions[0].softness = 100.0;
    let soft = db(measure(&settings, 2_500.0), dry);
    assert!(
        soft < hard - 0.5,
        "soft edge {soft} dB, hard edge {hard} dB"
    );

    settings.regions[0].bypass = true;
    assert!(db(measure(&settings, 1_000.0), dry).abs() < 0.02);
}

#[test]
fn region_depth_migration() {
    use nice_plug::plugin::{ParamValue, Plugin, PluginState};

    let mut state = PluginState {
        version: String::new(),
        params: [
            ("region_depth_1".into(), ParamValue::F32(9.0)),
            ("region_depth_6".into(), ParamValue::F32(5.0)),
            ("region_gain_6".into(), ParamValue::F32(3.0)),
        ]
        .into(),
        fields: Default::default(),
    };
    crate::Equart::filter_state(&mut state);
    assert!(matches!(
        state.params.get("region_gain_1"),
        Some(ParamValue::F32(-9.0))
    ));
    assert!(matches!(
        state.params.get("region_gain_6"),
        Some(ParamValue::F32(3.0))
    ));
    assert!(!state.params.contains_key("region_depth_1"));
}

#[test]
fn harmonic_region_modes() {
    use crate::region::{HarmonicControl, HarmonicMode, Region, RegionControl};

    let mut control = RegionControl {
        enabled: true,
        low: 180.0,
        high: 1_200.0,
        gain: -8.0,
        harmonic: HarmonicControl {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let measure_harmonic = |control, hz: f32, pitch| {
        let mut region = Region::new();
        let mut phase = 0.0_f32;
        let mut power = 0.0;
        let step = std::f32::consts::TAU * hz / RATE;
        for frame in 0..1500 {
            region.update(control, RATE, pitch, 0.1, 0.1);
            let mut left: [f32; 32] = std::array::from_fn(|_| {
                phase = (phase + step).rem_euclid(std::f32::consts::TAU);
                phase.sin() * 0.25
            });
            let mut right = left;
            region.process(&mut left, &mut right);
            if frame >= 750 {
                power += left.iter().map(|x| x * x).sum::<f32>();
            }
        }
        db(
            (power / 24_000.0).sqrt(),
            0.25 * std::f32::consts::FRAC_1_SQRT_2,
        )
    };

    let f0 = Some(110.0);
    assert!(measure_harmonic(control, 440.0, f0) < -6.0);
    assert!(measure_harmonic(control, 880.0, f0) < -5.0);
    assert!(measure_harmonic(control, 495.0, f0).abs() < 0.3);
    assert!(measure_harmonic(control, 990.0, f0).abs() < 0.1);
    assert!(measure_harmonic(control, 440.0, None).abs() < 0.1);
    control.gain = 8.0;
    assert!(measure_harmonic(control, 440.0, f0) > 6.0);

    control.harmonic.mode = HarmonicMode::Protect;
    assert!(measure_harmonic(control, 440.0, f0).abs() < 0.15);
    assert!(measure_harmonic(control, 495.0, f0) < -6.0);
    assert!(measure_harmonic(control, 2_000.0, f0).abs() < 0.3);
    assert!(measure_harmonic(control, 495.0, None).abs() < 0.1);
}

#[test]
fn resonance_delta_audition() {
    use crate::oversample::Oversampling;

    const FRAMES: usize = 10_240;
    const RELEASE: usize = 8_013;
    for mode in Oversampling::ALL {
        let render = |resonance: bool, audition: bool| {
            let mut engine = Engine::new(RATE);
            engine.analyze = false;
            engine.delta_request = audition.then_some(1);
            let mut settings = controls(flat_bands());
            settings.oversampling = mode;
            settings.regions[1].enabled = true;
            settings.regions[1].gain = -4.0;
            settings.regions[1].resonance.enabled = resonance;
            if audition {
                settings.regions[2].enabled = true;
                settings.regions[2].gain = 9.0;
                settings.mix = 0.0;
                settings.output_db = -12.0;
            }
            let mut left: Vec<_> = (0..FRAMES).map(|i| {
                0.35 * (std::f32::consts::TAU * 937.5 * i as f32 / RATE).sin()
            }).collect();
            let mut right: Vec<_> = left.iter().map(|x| -0.5 * x).collect();
            let input = left.clone();
            nice_assert_no_alloc::assert_no_alloc(|| {
                engine.process_block(&mut left[..RELEASE], &mut right[..RELEASE], &settings);
                engine.delta_request = None;
                engine.process_block(&mut left[RELEASE..], &mut right[RELEASE..], &settings);
            });
            if audition {
                let delay = engine.latency() as usize;
                for i in RELEASE + 512..FRAMES {
                    assert_eq!(left[i], input[i - delay]);
                }
            }
            [left, right]
        };
        let before = render(false, false);
        let after = render(true, false);
        let delta = render(true, true);
        for channel in 0..2 {
            let mut peak = 0.0_f32;
            for i in 4096..RELEASE {
                let expected = before[channel][i] - after[channel][i];
                peak = peak.max(expected.abs());
                assert!((delta[channel][i] - expected).abs() < 1e-5, "{mode:?}, frame {i}");
            }
            assert!(peak > 0.01, "expected an audible resonance cut");
        }
    }
}

#[test]
fn harmonic_tracks_dry() {
    let mut engine = Engine::new(RATE);
    engine.analyze = false;
    let mut settings = controls(flat_bands());
    settings.regions[0].enabled = true;
    settings.regions[0].harmonic.enabled = true;
    let mut left: Vec<f32> = (0..48_000)
        .map(|i| {
            let phase = std::f32::consts::TAU * 220.0 * i as f32 / RATE;
            0.4 * phase.sin() + 0.12 * (phase * 2.0).sin() + 0.1 * (phase * 3.0).sin()
        })
        .collect();
    let mut right = left.clone();
    nice_assert_no_alloc::assert_no_alloc(|| {
        engine.process_block(&mut left, &mut right, &settings)
    });
    let detected = engine
        .harmonic_hz()
        .expect("Harmonic should lock to the dry input");
    assert!((detected - 220.0).abs() < 2.0, "detected {detected} Hz");
    assert_eq!(engine.latency(), 0);
}
