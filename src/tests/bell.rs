use super::*;

// filter math vs RBJ reference

/// Log-spaced probe frequencies from 20 Hz to just under Nyquist.
fn probes(rate: f64) -> Vec<f64> {
    (0..64)
        .map(|i| 20.0 * ((rate * 0.45) / 20.0).powf(i as f64 / 63.0))
        .collect()
}

pub(super) fn ours_db(control: BandControl, rate: f32, hz: f32) -> f64 {
    let (sections, count) = control.sections(rate);
    let mut response = realfft::num_complex::Complex32::new(1.0, 0.0);
    for section in sections.iter().take(count) {
        response *= section.response(rate, hz);
    }
    20.0 * response.norm().max(1e-12).log10() as f64
}

/// single-section filters match RBJ biquad (only f32 noise in stopband)
#[test]
fn single_section_matches_rbj() {
    use crate::reference::Biquad;

    let mut worst = 0.0_f64;

    for rate in [44_100.0_f64, 48_000.0, 96_000.0] {
        for f in [60.0, 250.0, 1_000.0, 6_000.0, 15_000.0] {
            for q in [0.4, 0.70710678, 1.0, 2.5, 8.0] {
                for gain in [-18.0, -6.0, 0.0, 9.0, 18.0] {
                    let cases = [
                        (FilterKind::Peak, Biquad::peak(rate, f, gain, q)),
                        (FilterKind::LowShelf, Biquad::low_shelf(rate, f, gain, q)),
                        (FilterKind::HighShelf, Biquad::high_shelf(rate, f, gain, q)),
                        (FilterKind::Highpass, Biquad::high_pass(rate, f, q)),
                        (FilterKind::Lowpass, Biquad::low_pass(rate, f, q)),
                    ];

                    for (kind, reference) in cases {
                        let control = BandControl {
                            enabled: true,
                            frequency: f as f32,
                            gain: gain as f32,
                            q: q as f32,
                            kind,
                            slope: if kind == FilterKind::Peak {
                                // The plain biquad bell used by the reference.
                                CutSlope::Db12
                            } else {
                                CutSlope::default()
                            },
                            channel: ChannelMode::Stereo,
                            dynamics: DynamicControl::default(),
                            pitch: Default::default(),
                        };

                        for hz in probes(rate) {
                            let expected = reference.magnitude_db(rate, hz);
                            // f32 rounding dominates below -80 dB
                            if expected < -80.0 {
                                continue;
                            }

                            let deviation =
                                (ours_db(control, rate as f32, hz as f32) - expected).abs();
                            worst = worst.max(deviation);
                            assert!(
                                deviation < 0.05,
                                "{kind:?} at {rate} Hz, f={f}, q={q}, gain={gain}, \
                                 probe {hz:.0} Hz: {deviation:.3} dB off the reference"
                            );
                        }
                    }
                }
            }
        }
    }

    println!("worst deviation from the RBJ reference: {worst:.4} dB");
}

// sloped bells (>=24 dB/oct are transformed low shelves)

fn sloped_bell(slope: CutSlope, gain: f32, q: f32) -> BandControl {
    BandControl {
        slope,
        ..band(1_000.0, gain, q, FilterKind::Peak)
    }
}

fn bell_db(control: BandControl, hz: f32) -> f64 {
    ours_db(control, RATE, hz)
}

/// shelf floor must sit at exact gain so node stays on curve
#[test]
fn sloped_bell_hits_gain() {
    for slope in CutSlope::EVEN_ORDER {
        for gain in [-24.0, -12.0, -3.0, 3.0, 9.0, 18.0] {
            for q in [0.3, 0.7, 1.0, 2.0, 8.0, 24.0] {
                let centre = bell_db(sloped_bell(slope, gain, q), 1_000.0);
                assert!(
                    (centre - f64::from(gain)).abs() < 0.2,
                    "{} at {gain} dB, Q {q}: the centre reads {centre:.2} dB",
                    slope.label()
                );
            }
        }
    }
}

#[test]
fn steep_bell_q_affects_audio() {
    let hz = 1_090.0;
    let baseline = reference(hz);
    for q in [6.0, 10.0] {
        let mut bands = flat_bands();
        let bell = sloped_bell(CutSlope::Db96, -4.3, q);
        bands[0] = bell;
        let measured = f64::from(db(measure(&controls(bands), hz), baseline));
        assert!(
            (measured - bell_db(bell, hz)).abs() < 0.03,
            "Q {q}: {measured} dB"
        );
    }
}
