#[test]
fn spectrum_frame_rate() {
    use crate::analyzer::{Analyzer, AnalyzerSettings, FRAME_RATES};
    use crate::display_preferences::SpectrumPreferences;

    let params = crate::params::EquartParams::default();
    let mut analyzer = Analyzer::new(48_000.0);

    for rate in [48_000.0, 96_000.0] {
        analyzer.retune(rate);

        for (index, fps) in FRAME_RATES.into_iter().enumerate() {
            analyzer.reset();
            analyzer.apply_settings(AnalyzerSettings::read(
                &params,
                SpectrumPreferences {
                    frame_rate: index as u32,
                    ..Default::default()
                },
            ));

            let hop = rate as usize / fps as usize;
            let before = analyzer.spectrum().1[32];

            for i in 0..hop - 1 {
                let sample = (std::f32::consts::TAU * 375.0 * i as f32 / rate).sin();
                analyzer.ingest([sample; 2]);
            }

            analyzer.refresh();
            assert_eq!(analyzer.spectrum().1[32], before);

            for _ in 0..8 {
                analyzer.ingest([0.0; 2]);
            }

            analyzer.refresh();
            let first = analyzer.spectrum().1[32];
            assert!(first > before);

            for _ in 0..hop - 7 {
                analyzer.ingest([0.0; 2]);
            }

            analyzer.refresh();
            assert_ne!(analyzer.spectrum().1[32], first);
        }
    }
}

#[test]
fn analyser_follows_host_rate() {
    let rate = 96_000.0_f32;
    // Activated at 48 kHz, but the host processes at 96 kHz.
    let mut analyzer = crate::analyzer::Analyzer::new(48_000.0);

    let feed = |analyzer: &mut crate::analyzer::Analyzer, seconds: f32| {
        let block = 512usize;
        let total = (rate * seconds) as usize;
        let mut left = vec![0.0; block];
        let mut right = vec![0.0; block];

        for n in (0..total).step_by(block) {
            for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
                let t = (n + i) as f32 / rate;
                let sample = 0.5 * (std::f32::consts::TAU * 19_000.0 * t).sin();
                *l = sample;
                *r = sample;
            }

            for (&left, &right) in left.iter().zip(&right) {
                analyzer.push([left, right]);
            }
        }
    };

    let at = |analyzer: &crate::analyzer::Analyzer, hz: f32| {
        let (bin_hz, levels) = analyzer.spectrum();
        levels[(hz / bin_hz).round() as usize]
    };

    feed(&mut analyzer, 2.0);
    let before = at(&analyzer, 19_000.0);

    analyzer.retune(rate);
    feed(&mut analyzer, 2.0);
    let after = at(&analyzer, 19_000.0);
    // mapping for half rate should shift tone down, retuned puts it back
    assert!(
        after - before > 15.0,
        "after retune the 19 kHz tone should be back: {before:.1} dB -> {after:.1} dB"
    );
    assert!(
        after > -30.0,
        "after retune the 19 kHz tone should be back, got {after:.1} dB"
    );
}

#[test]
fn quiet_highs_stay_finite() {
    // loud bass used to make f32 boxcar sum round to 0 -> log10(0) = NaN
    let mut analyzer = crate::analyzer::Analyzer::new(48_000.0);

    let mut left = vec![0.0f32; 512];
    let mut right = vec![0.0f32; 512];
    let mut seed = 0x9e37_79b9u32;

    for block in 0..400 {
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let hiss = (seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * 1e-5;
            let t = (block * 512 + i) as f32 / 48_000.0;
            let sample = 0.9 * (std::f32::consts::TAU * 60.0 * t).sin() + hiss;
            *l = sample;
            *r = sample;
        }
        for (&left, &right) in left.iter().zip(&right) {
            analyzer.push([left, right]);
        }
    }

    let bad: Vec<usize> = analyzer
        .spectrum()
        .1
        .iter()
        .enumerate()
        .filter(|(_, db)| !db.is_finite())
        .map(|(i, _)| i)
        .collect();
    assert!(bad.is_empty(), "non-finite bins: {bad:?}");

    let at = |hz: f32| {
        let (bin_hz, levels) = analyzer.spectrum();
        levels[(hz / bin_hz).round() as usize]
    };
    // quiet -100 dBFS hiss shouldn't collapse to NaN in f32 sum
    assert!(
        at(19_000.0) > -130.0,
        "the quiet bins collapsed towards the floor, got {:.1} dB",
        at(19_000.0)
    );
}
