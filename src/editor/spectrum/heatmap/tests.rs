use super::*;
use crate::analyzer::{Analyzer, AnalyzerSettings, RESOLUTIONS};
use crate::display_preferences::SpectrumPreferences;

fn render(
    cache: &mut Heatmap,
    plot: &Plot,
    frame: &SpectrumFrame<'_>,
    hold: Hold,
    mode: HeatmapMode,
    theme: &Theme,
) -> Option<Arc<egui::Mesh>> {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(plot.rect.expand(64.0)),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        cache.draw(
            ui.painter(),
            plot,
            frame,
            hold,
            HeatmapPreferences {
                mode,
                ..Default::default()
            },
            theme,
        );
    });
    output.textures_delta.clear();
    output
        .shapes
        .into_iter()
        .find_map(|shape| match shape.shape {
            egui::Shape::Mesh(mesh) => Some(mesh),
            _ => None,
        })
}

#[test]
fn heatmap_modes_and_cache() {
    let plot = Plot::new(
        Rect::from_min_size(Pos2::ZERO, Vec2::new(960.0, 640.0)),
        48_000.0,
    );
    let levels = [-20.0; 2_049];
    let frame = SpectrumFrame {
        bin_hz: 48_000.0 / 4096.0,
        output_levels: &levels,
        ..Default::default()
    };
    let mut cache = Heatmap::default();
    let theme = Theme::default();

    for mode in HeatmapMode::ALL {
        let mesh = render(&mut cache, &plot, &frame, Hold::Off, mode, &theme);
        if mode == HeatmapMode::Off {
            assert!(mesh.is_none());
            continue;
        }

        let mesh = mesh.unwrap();
        let bounds = mesh.calc_bounds();
        assert_eq!(bounds.x_range(), plot.rect.x_range());
        let mid = plot.rect.center().y;
        let (top, bottom) = match mode {
            HeatmapMode::Top => (plot.rect.top(), mid),
            HeatmapMode::Bottom => (mid, plot.rect.bottom()),
            _ => (plot.rect.top(), plot.rect.bottom()),
        };
        assert_eq!(bounds.y_range(), top..=bottom);
        assert!(
            mesh.indices
                .iter()
                .all(|&i| (i as usize) < mesh.vertices.len())
        );

        let cached = render(&mut cache, &plot, &frame, Hold::Off, mode, &theme).unwrap();
        assert!(Arc::ptr_eq(&mesh, &cached));
    }

    let light = crate::display_preferences::theme::presets::builtins()[1].1;
    let dark_mesh = cache.mesh.clone();
    let light_mesh = render(
        &mut cache,
        &plot,
        &frame,
        Hold::Off,
        HeatmapMode::Bottom,
        &light,
    )
    .unwrap();
    assert_ne!(dark_mesh.vertices[1].color, light_mesh.vertices[1].color);

    let resized = Plot::new(
        Rect::from_min_size(Pos2::new(11.0, 3.0), Vec2::new(726.0, 395.0)),
        48_000.0,
    );
    let mesh = render(
        &mut cache,
        &resized,
        &frame,
        Hold::Off,
        HeatmapMode::Full,
        &light,
    )
    .unwrap();
    assert_eq!(mesh.calc_bounds(), resized.rect);
}

fn feed(analyzer: &mut Analyzer, samples: usize, mut sample: impl FnMut(usize) -> f32) {
    for index in 0..samples {
        analyzer.ingest([sample(index); 2]);
    }

    analyzer.refresh();
}

fn peak_x(mesh: &egui::Mesh) -> f32 {
    mesh.vertices
        .chunks_exact(ROWS)
        .max_by_key(|column| column[1].color.a())
        .unwrap()[1]
        .pos
        .x
}

#[test]
fn heatmap_signal_alignment() {
    let params = crate::params::EquartParams::default();
    let theme = Theme::default();
    let mut cache = Heatmap::default();
    let mut analyzer = Analyzer::new(44_100.0);

    for rate in [44_100.0, 48_000.0, 96_000.0] {
        analyzer.retune(rate);
        let plot = Plot::new(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(960.0, 640.0)),
            rate,
        );

        for resolution in 0..RESOLUTIONS.len() as u32 {
            analyzer.reset();
            analyzer.apply_settings(AnalyzerSettings::read(
                &params,
                SpectrumPreferences {
                    resolution,
                    tilt: 0,
                    ..Default::default()
                },
            ));
            let bin_hz = analyzer.spectrum().0;
            let tone = (750.0 / bin_hz).round() * bin_hz;
            feed(&mut analyzer, 20_000, |i| {
                0.55 * (std::f32::consts::TAU * tone * i as f32 / rate).sin()
            });

            let frame = SpectrumFrame::from_analyzers([&analyzer, &analyzer]);
            let mesh = render(
                &mut cache,
                &plot,
                &frame,
                Hold::Off,
                HeatmapMode::Full,
                &theme,
            )
            .unwrap();
            let fft_width = plot.x(tone + bin_hz) - plot.x(tone - bin_hz);
            let column_width = plot.rect.width() / COLUMNS as f32;
            let error = (peak_x(&mesh) - plot.x(tone)).abs();
            assert!(
                error <= fft_width + column_width,
                "{rate} Hz, resolution {resolution}, peak error {error:.2} points"
            );
        }
    }

    let plot = Plot::new(
        Rect::from_min_size(Pos2::ZERO, Vec2::new(960.0, 640.0)),
        96_000.0,
    );
    let mut phase = 0.0;
    let mut peaks = Vec::new();

    for step in 0..12 {
        feed(&mut analyzer, 16_384, |i| {
            let t = (step as f32 + i as f32 / 16_384.0) / 12.0;
            phase += std::f32::consts::TAU * 100.0 * 64.0_f32.powf(t) / 96_000.0;
            phase.sin() * 0.6
        });
        let frame = SpectrumFrame::from_analyzers([&analyzer, &analyzer]);
        let mesh = render(
            &mut cache,
            &plot,
            &frame,
            Hold::Off,
            HeatmapMode::Full,
            &theme,
        )
        .unwrap();
        peaks.push(peak_x(&mesh));
    }

    assert!(peaks.windows(2).all(|pair| pair[1] > pair[0]));
}

#[test]
fn heatmap_noise_and_hold() {
    let plot = Plot::new(
        Rect::from_min_size(Pos2::ZERO, Vec2::new(960.0, 640.0)),
        48_000.0,
    );
    let theme = Theme::default();
    let mut cache = Heatmap::default();
    let mut analyzer = Analyzer::new(48_000.0);
    let mut seed = 0x91e1_0da5_u32;
    let mut white = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    };

    for pink in [false, true] {
        let mut octaves = [0.0; 9];
        analyzer.reset();
        feed(&mut analyzer, 8_192, |i| {
            if !pink {
                return white() * 0.25;
            }

            for (octave, sample) in octaves.iter_mut().enumerate() {
                if i % (1 << octave) == 0 {
                    *sample = white();
                }
            }

            (white() + octaves.iter().sum::<f32>()) * 0.045
        });
        let frame = SpectrumFrame::from_analyzers([&analyzer, &analyzer]);
        let mesh = render(
            &mut cache,
            &plot,
            &frame,
            Hold::Smooth,
            HeatmapMode::Full,
            &theme,
        )
        .unwrap();
        assert!(mesh.vertices.iter().filter(|v| v.color.a() > 0).count() > COLUMNS);
    }

    let frame = SpectrumFrame::from_analyzers([&analyzer, &analyzer]);
    let active = render(
        &mut cache,
        &plot,
        &frame,
        Hold::Smooth,
        HeatmapMode::Full,
        &theme,
    )
    .unwrap();
    analyzer.set_frozen(true);
    feed(&mut analyzer, 48_000, |_| 0.0);
    let frame = SpectrumFrame::from_analyzers([&analyzer, &analyzer]);
    let frozen = render(
        &mut cache,
        &plot,
        &frame,
        Hold::Smooth,
        HeatmapMode::Full,
        &theme,
    )
    .unwrap();
    assert_eq!(active.vertices, frozen.vertices);

    analyzer.set_frozen(false);
    for _ in 0..240 {
        feed(&mut analyzer, 800, |_| 0.0);
    }

    let frame = SpectrumFrame::from_analyzers([&analyzer, &analyzer]);
    assert!(
        render(
            &mut cache,
            &plot,
            &frame,
            Hold::Smooth,
            HeatmapMode::Full,
            &theme
        )
        .is_none()
    );

    let bad = SpectrumFrame {
        revision: frame.revision + 1,
        bin_hz: frame.bin_hz,
        output_levels: &[f32::NAN, f32::INFINITY, f32::NEG_INFINITY],
        ..Default::default()
    };
    assert!(
        render(
            &mut cache,
            &plot,
            &bad,
            Hold::Line,
            HeatmapMode::Full,
            &theme
        )
        .is_none()
    );
}
