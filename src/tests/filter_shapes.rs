use super::{bell::ours_db, *};

#[test]
fn steep_shelf_resonance() {
    for kind in [FilterKind::LowShelf, FilterKind::HighShelf] {
        for (gain, expected) in [(-0.3, 0.0..1.0), (-3.5, 3.7..4.5), (-12.0, 5.0..11.0)] {
            let control = BandControl {
                slope: CutSlope::Db96,
                ..band(383.0, gain, 40.0, kind)
            };
            let (hz, peak) = (-1000..=1000)
                .map(|i| {
                    let hz = control.frequency * (i as f32 * 0.0001).exp();
                    (hz, ours_db(control, RATE, hz))
                })
                .max_by(|(_, a), (_, b)| a.total_cmp(b))
                .unwrap();
            assert!(
                expected.contains(&peak),
                "{kind:?}, {gain} dB: peak {peak:.2} dB"
            );
            assert!(
                (ours_db(control, RATE, control.frequency) - f64::from(gain) * 0.5).abs() < 0.02
            );
            let mut bands = flat_bands();
            bands[0] = control;
            let measured = f64::from(db(measure(&controls(bands), hz), reference(hz)));
            assert!(
                (measured - peak).abs() < 0.03,
                "{kind:?}, {gain} dB: audio peak {measured:.2} vs {peak:.2}"
            );
        }
    }
}

#[test]
fn notch_bandwidth() {
    for (rate, frequency) in [
        (44_100.0_f32, 30.0_f32),
        (48_000.0, 21_600.0),
        (192_000.0, 1_000.0),
    ] {
        let warped = (std::f64::consts::PI * f64::from(frequency / rate)).tan();
        for q in [0.1_f32, 0.7, 1.0, 24.0] {
            for &slope in FilterKind::Notch.slopes() {
                let notch = BandControl {
                    slope,
                    ..band(frequency, 0.0, q, FilterKind::Notch)
                };
                assert!(ours_db(notch, rate, frequency) < -75.0);

                // normalized band-stop freq (x=1 is -3 dB point)
                for x in [0.6_f64, 1.0, 1.5, 4.0] {
                    let distance = x / f64::from(q);
                    let ratio = (distance + distance.hypot(2.0)) * 0.5;
                    let expected = -10.0 * (1.0 + x.powi(-(slope.order() as i32))).log10();
                    for r in [ratio.recip(), ratio] {
                        let hz =
                            ((warped * r).atan() * f64::from(rate) / std::f64::consts::PI) as f32;
                        let actual = ours_db(notch, rate, hz);
                        assert!(
                            (actual - expected).abs() < 0.04,
                            "{slope:?} at {frequency} Hz, {rate} Hz rate, Q {q}, probe {hz} Hz: {actual} vs {expected}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn tilt_bandpass_match() {
    let hz = 2_000.0;
    let baseline = reference(hz);
    for (kind, slope) in [
        (FilterKind::TiltShelf, CutSlope::Db6),
        (FilterKind::TiltShelf, CutSlope::Db24),
        (FilterKind::FlatTilt, CutSlope::Db12),
        (FilterKind::BandPass, CutSlope::Db24),
    ] {
        let control = BandControl {
            slope,
            ..band(1_000.0, 6.0, 1.0, kind)
        };
        let mut bands = flat_bands();
        bands[0] = control;
        let measured = f64::from(db(measure(&controls(bands), hz), baseline));
        let expected = ours_db(control, RATE, hz);
        assert!(
            (measured - expected).abs() < 0.03,
            "{kind:?} {slope:?}: {measured} vs {expected}"
        );
    }
}

/// neutral Q gives Butterworth response (-3.01 dB at cutoff)
#[test]
fn cut_flat_passband() {
    let rate = 48_000.0;
    let cutoff = 1_000.0;

    for slope in CutSlope::ALL {
        for kind in [FilterKind::Lowpass, FilterKind::Highpass] {
            let control = BandControl {
                enabled: true,
                frequency: cutoff,
                gain: 0.0,
                q: 1.0 / std::f32::consts::SQRT_2,
                kind,
                slope,
                channel: ChannelMode::Stereo,
                dynamics: DynamicControl::default(),
                ..BandControl::default()
            };

            let at_cutoff = ours_db(control, rate, cutoff);
            assert!(
                (at_cutoff + 3.0103).abs() < 0.05,
                "{slope:?} {kind:?} was {at_cutoff:.3} dB at the cutoff"
            );

            // Maximally flat means no overshoot and a monotone passband.
            let mut previous = if kind == FilterKind::Lowpass {
                0.0_f64
            } else {
                -120.0
            };
            for i in 0..64 {
                let hz = if kind == FilterKind::Lowpass {
                    20.0 * (cutoff * 0.9 / 20.0).powf(i as f32 / 63.0)
                } else {
                    (cutoff / 0.9) * ((rate * 0.45) / (cutoff / 0.9)).powf(i as f32 / 63.0)
                };

                let db = ours_db(control, rate, hz);
                assert!(
                    db <= 1e-3,
                    "{slope:?} {kind:?} overshot to {db:.4} dB at {hz:.0} Hz"
                );

                if kind == FilterKind::Lowpass {
                    assert!(
                        db <= previous + 1e-4,
                        "{slope:?} low pass bounced from {previous:.4} to {db:.4} dB at {hz:.0} Hz"
                    );
                    previous = db;
                } else {
                    assert!(
                        db >= previous - 1e-4,
                        "{slope:?} high pass bounced from {previous:.4} to {db:.4} dB at {hz:.0} Hz"
                    );
                    previous = db;
                }
            }
        }
    }
}

/// cutoff gain is 20*log10(Q) regardless of slope
#[test]
fn cut_cutoff_gain_follows_q() {
    let rate = 48_000.0;
    let cutoff = 1_000.0;

    // A first-order section has no resonance to move.
    for slope in CutSlope::ALL.into_iter().filter(|s| s.order() > 1) {
        for q in [0.5_f32, 1.0, 2.0, 4.0] {
            let control = BandControl {
                enabled: true,
                frequency: cutoff,
                gain: 0.0,
                q,
                kind: FilterKind::Lowpass,
                slope,
                channel: ChannelMode::Stereo,
                dynamics: DynamicControl::default(),
                ..BandControl::default()
            };

            let at_cutoff = ours_db(control, rate, cutoff);
            let expected = 20.0 * f64::from(q).log10();
            assert!(
                (at_cutoff - expected).abs() < 0.1,
                "{slope:?} at Q {q}: {at_cutoff:.3} dB at the cutoff, expected {expected:.3}"
            );

            // no resonance below neutral Q (max is at DC)
            if q < 1.0 {
                continue;
            }

            let mut peak = f64::NEG_INFINITY;
            for i in 0..600 {
                let hz = 400.0 * (2_500.0_f32 / 400.0).powf(i as f32 / 599.0);
                peak = peak.max(ours_db(control, rate, hz));
            }

            assert!(
                peak < expected + 2.0,
                "{slope:?} at Q {q} rang up to {peak:.2} dB, far above {expected:.2}"
            );
        }
    }
}
