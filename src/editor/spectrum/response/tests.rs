use super::*;
use crate::{
    engine::{Controls, Engine},
    oversample::Oversampling,
    precision::Precision,
    processing_mode::LinearQuality,
};
use realfft::num_complex::Complex64;

const RATE: f32 = 48_000.0;

fn impulse_matrix(
    engine: &mut Engine,
    controls: &Controls,
    telemetry: &crate::Telemetry,
) -> [Vec<f64>; 4] {
    let mut basis = std::array::from_fn(|_| [vec![0.0; 4096], vec![0.0; 4096]]);

    for input in 0..2 {
        engine.reset();
        let [left, right] = &mut basis[input];
        [left, right][input][0] = 1.0;
        let [left, right] = &mut basis[input];

        nice_assert_no_alloc::assert_no_alloc(|| {
            engine.process_block_with_sidechain(left, right, None, controls);
            telemetry.publish(engine);
        });
    }

    let [[ll, rl], [lr, rr]] = basis;
    [ll, lr, rl, rr]
}

fn matrix_at(matrix: &[Vec<f64>; 4], hz: f32, latency: u32) -> [Complex64; 4] {
    let angle = -std::f64::consts::TAU * f64::from(hz) / f64::from(RATE);
    let rotation = Complex64::from_polar(1.0, angle);
    matrix.each_ref().map(|signal| {
        let mut phase = Complex64::from_polar(1.0, -angle * f64::from(latency));
        let mut transfer = Complex64::default();

        for sample in signal {
            transfer += phase * sample;
            phase *= rotation;
        }

        transfer
    })
}

#[test]
fn plotted_channel_responses() {
    use ChannelMode::*;
    let cases: &[&[ChannelMode]] = &[
        &[Stereo],
        &[Left],
        &[Right],
        &[Mid],
        &[Side],
        &[Left, Right],
        &[Mid, Side],
        &[Stereo, Left],
        &[Stereo, Mid],
        &[Left, Mid],
        &[Right, Side, Left],
    ];
    let canvas = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 520.0));
    let plot = Plot::new(canvas, RATE);
    let theme = crate::display_preferences::theme::Theme::default();

    for (oversampling, quality) in [
        (Oversampling::Off, None),
        (Oversampling::X4, None),
        (Oversampling::Off, Some(LinearQuality::Short)),
    ] {
        let ctx = egui::Context::default();
        let mut engine = Engine::new(RATE);
        let mut time = 0.0;

        for &placements in cases {
            let controls = Controls {
                bands: std::array::from_fn(|index| {
                    let placement = placements.get(index).copied();

                    BandControl {
                        enabled: placement.is_some(),
                        frequency: 650.0 + index as f32 * 480.0,
                        gain: [8.0, -11.0, 6.0][index % 3],
                        q: 1.2,
                        channel: placement.unwrap_or_default(),
                        ..Default::default()
                    }
                }),
                regions: Default::default(),
                output_db: 0.0,
                pan: 0.0,
                mix: 1.0,
                bypass: false,
                oversampling,
                linear_phase: quality,
                precision: Precision::Double,
            };
            let telemetry = crate::Telemetry::default();
            let mut fir = crate::telemetry::FirResponse::default();
            fir.update(&telemetry.fir);
            let measured = impulse_matrix(&mut engine, &controls, &telemetry);
            if quality.is_some() {
                telemetry.publish(&engine);
                fir.update(&telemetry.fir);
            }

            let input = egui::RawInput {
                screen_rect: Some(canvas),
                time: Some(time),
                ..Default::default()
            };
            time += 0.1;
            let design_rate = RATE * oversampling.factor() as f32;
            let center = quality.map(|quality| quality.center_at(RATE));
            let mut output = ctx.run_ui(input, |ui| {
                draw_response(
                    ui.painter(),
                    &plot,
                    canvas,
                    &mut std::array::from_fn(|_| Vec::new()),
                    ResponseInput {
                        controls: &controls.bands,
                        base: None,
                        offsets: &[0.0; BAND_COUNT],
                        regions: &controls.regions,
                        telemetry: &telemetry,
                        rate: design_rate,
                        oversampling,
                        selected: None,
                        phase_mode: Some(phase::Mode::Wrapped),
                        linear_center: center,
                        fir_response: &fir,
                        preview_hover: None,
                    },
                );
            });
            output.textures_delta.clear();

            let coupled = placements.iter().any(|mode| matches!(mode, Left | Right))
                && placements.iter().any(|mode| matches!(mode, Mid | Side));
            for mode in ChannelMode::ALL {
                let expected =
                    placements.contains(&mode) || (coupled && matches!(mode, Left | Right));
                let ink = theme.curves.channel(mode).gamma_multiply(0.95);
                let line = output.shapes.iter().find_map(|shape| {
                    let egui::Shape::Path(path) = &shape.shape else {
                        return None;
                    };
                    (path.stroke.width == 1.9
                        && path.stroke.color == egui::epaint::ColorMode::Solid(ink))
                    .then_some(path)
                });
                assert_eq!(line.is_some(), expected, "{placements:?} {mode:?}");
                let Some(line) = line else { continue };

                for hz in [550.0, 1000.0, 1440.0, 6500.0] {
                    let x = plot.x(hz);
                    let sample = line
                        .points
                        .iter()
                        .min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs()))
                        .unwrap();
                    let frequency = plot.hz_extended(sample.x);
                    let [ll, lr, rl, rr] = matrix_at(&measured, frequency, engine.latency());
                    let reference = match mode {
                        Stereo if placements == [Stereo, Left] => rr,
                        Stereo if placements == [Stereo, Mid] => (ll - lr - rl + rr) * 0.5,
                        Stereo => (ll + rr) * 0.5,
                        Left => ll,
                        Right => rr,
                        Mid => (ll + lr + rl + rr) * 0.5,
                        Side => (ll - lr - rl + rr) * 0.5,
                    };
                    let actual_db = plot.db_free(sample.y);
                    let reference_db = (20.0 * reference.norm().log10()) as f32;
                    assert!(
                        (actual_db - reference_db).abs() < 0.12,
                        "{oversampling:?} {quality:?} {placements:?} {mode:?} at {frequency}: {actual_db} vs {reference_db}"
                    );
                }
            }

            if coupled {
                for (lane, color) in [(1, theme.curves.left), (2, theme.curves.right)] {
                    let x = plot.x(1000.0);
                    let ink = color.gamma_multiply(0.7);
                    let sample = output
                        .shapes
                        .iter()
                        .filter_map(|shape| match shape.shape {
                            egui::Shape::LineSegment { points, stroke }
                                if stroke.width == 1.35 && stroke.color == ink =>
                            {
                                Some(points[0])
                            }
                            _ => None,
                        })
                        .min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs()))
                        .unwrap();

                    let hz = plot.hz_extended(sample.x);
                    let reference = matrix_at(&measured, hz, engine.latency())[lane];
                    let expected = (20.0 * reference.norm().log10()) as f32;
                    assert!((plot.db_free(sample.y) - expected).abs() < 0.12);
                }
            }

            if let Some(center) = center {
                let cached = fir.matrix(1000.0, RATE, center).unwrap();
                let actual = matrix_at(&measured, 1000.0, engine.latency());

                for (cached, actual) in cached.into_iter().zip(actual) {
                    assert!(
                        (Complex64::new(cached.re.into(), cached.im.into()) - actual).norm()
                            < 0.001
                    );
                }
            }
        }
    }
}
