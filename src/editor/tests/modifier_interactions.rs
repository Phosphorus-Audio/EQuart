use super::*;

fn text_center(
    ctx: &egui::Context,
    editor: &mut EquartEditor,
    time: f64,
    label: &str,
) -> Option<Pos2> {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, CANVAS)),
            time: Some(time),
            focused: true,
            ..Default::default()
        },
        |ui| editor.draw(ui),
    );
    output.textures_delta.clear();

    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                let bounds = clipped.shape.visual_bounding_rect();

                clipped
                    .clip_rect
                    .contains_rect(bounds)
                    .then(|| bounds.center())
            }
            _ => None,
        })
}

fn find_text(ctx: &egui::Context, editor: &mut EquartEditor, time: f64, label: &str) -> Pos2 {
    text_center(ctx, editor, time, label)
        .unwrap_or_else(|| panic!("the requested control is not visible: {label}"))
}

#[test]
fn spectral_dynamics_panel() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    editor.place_region(1000.0);
    let params = editor.params.clone();
    let dynamics = &params.regions[0].dynamics;
    let setter = editor.gui.as_ref().unwrap().param_setter();
    editor.set(&setter, &dynamics.enabled, true);
    editor.reveal_modifier = Some((0, crate::editor::modifiers::Modifier::SpectralDynamics));

    let meter = crate::region::dynamics::Meter {
        input: -14.0,
        output: -20.0,
        reduction: -6.0,
        external_missing: false,
    };
    for tick in 0..250 {
        editor.telemetry.region_dynamics[0].write(tick * 800, 48_000.0, meter, true);
    }
    assert_eq!(editor.telemetry.region_dynamics[0].read().count(), 240);

    for i in 0..20 {
        frame(&ctx, &mut editor, f64::from(i) * 0.04, Vec::new());
    }

    let paint = |editor: &mut EquartEditor, time: f64| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, CANVAS)),
                focused: true,
                time: Some(time),
                ..Default::default()
            },
            |ui| editor.draw(ui),
        );
        output.textures_delta.clear();
        output.shapes
    };
    let label = |shapes: &[egui::epaint::ClippedShape], name: &str| {
        shapes.iter().find_map(|item| match &item.shape {
            egui::Shape::Text(text) if text.galley.text() == name => {
                let bounds = item.shape.visual_bounding_rect();
                item.clip_rect
                    .contains_rect(bounds)
                    .then(|| bounds.center())
            }
            _ => None,
        })
    };
    let chart = |shapes: &[egui::epaint::ClippedShape], width: f32| {
        shapes.iter().find_map(|item| match &item.shape {
            egui::Shape::Rect(plot)
                if (plot.rect.width() - width).abs() < 0.01
                    && (plot.rect.height() - 180.0).abs() < 0.01 =>
            {
                Some(plot.rect)
            }
            _ => None,
        })
    };
    let plot = paint(&mut editor, 1.0);
    let square = chart(&plot, 180.0).expect("square transfer graph");
    let monitor = chart(&plot, 230.0).expect("full height monitor");
    assert_eq!(square.right_top(), monitor.left_top());
    assert_eq!(square.right_bottom(), monitor.left_bottom());
    let meshes = |clip| {
        plot.iter()
            .filter(|shape| shape.clip_rect == clip && matches!(shape.shape, egui::Shape::Mesh(_)))
            .count()
    };
    assert!(meshes(square) >= 1);
    assert!(meshes(monitor) >= 2);
    let zero_line = |shapes: &[egui::epaint::ClippedShape], rect: Rect, fraction: f32| {
        let y = rect.top() + rect.height() * fraction;
        shapes.iter().any(|item| {
            matches!(&item.shape, egui::Shape::LineSegment { points, .. }
            if item.clip_rect == rect
                && (points[0].y - y).abs() < 0.01 && (points[1].y - y).abs() < 0.01
                && points[0].x == rect.left() && points[1].x == rect.right())
        })
    };
    assert!(zero_line(&plot, square, 12.0 / 60.0));
    assert!(zero_line(&plot, monitor, 12.0 / 60.0));

    let settings = label(&plot, "Settings").expect("settings button");
    click(&ctx, &mut editor, 1.1, settings);
    let choices = paint(&mut editor, 1.3);
    let expander = label(&choices, "Expander").expect("mode buttons in settings");
    assert!(choices.iter().any(|item| {
        matches!(&item.shape, egui::Shape::Rect(button) if button.rect.contains(expander)
            && (76.0..=120.0).contains(&button.rect.width())
            && (23.0..=28.0).contains(&button.rect.height())
            && button.stroke.width > 0.0)
    }));
    click(&ctx, &mut editor, 1.4, expander);
    assert_eq!(
        dynamics.mode.value(),
        crate::region::dynamics::Mode::Expander
    );
    let peak = label(&paint(&mut editor, 1.6), "Peak").expect("detector buttons in settings");
    click(&ctx, &mut editor, 1.7, peak);
    assert_eq!(
        dynamics.detector.value(),
        crate::region::dynamics::Detector::Peak
    );
    let sidechain =
        label(&paint(&mut editor, 1.9), "Sidechain").expect("source buttons in settings");
    click(&ctx, &mut editor, 2.0, sidechain);
    assert_eq!(
        dynamics.source.value(),
        crate::dynamics::DetectorInput::Sidechain
    );
    let expanded = label(&paint(&mut editor, 2.08), "-96 dB").expect("expanded graph range");
    click(&ctx, &mut editor, 2.13, expanded);
    click(&ctx, &mut editor, 2.22, square.center());
    let zoomed = paint(&mut editor, 2.3);
    assert!(zero_line(&zoomed, square, 12.0 / 108.0));
    assert!(zero_line(&zoomed, monitor, 12.0 / 108.0));

    let node = square.min + square.size() * egui::vec2(0.34, 0.74);
    click_button(&ctx, &mut editor, 2.4, node, egui::PointerButton::Secondary);
    let bank = &params.spectral_transfer_curves;
    let curve = bank.read(0).unwrap();
    assert_eq!(curve.points().len(), 3);
    let destination = node + Vec2::new(5.0, 8.0);
    drag(&ctx, &mut editor, 2.6, node, destination);
    assert!(bank.read(0).unwrap().points()[1].output < curve.points()[1].output - 2.0);

    click_button(
        &ctx,
        &mut editor,
        2.8,
        destination,
        egui::PointerButton::Secondary,
    );
    let smooth = label(&paint(&mut editor, 3.0), "Smooth").expect("segment menu");
    click(&ctx, &mut editor, 3.1, smooth);
    assert_eq!(
        bank.read(0).unwrap().points()[1].segment,
        crate::region::dynamics::Segment::Smooth
    );
    click(&ctx, &mut editor, 3.3, square.min + Vec2::new(20.0, 20.0));

    let curve = bank.read(0).unwrap();
    let middle = (curve.points()[1].input + curve.points()[2].input) * 0.5;
    let halfway = square.min
        + square.size()
            * Vec2::new(
                (middle + 96.0) / 108.0,
                (12.0 - curve.output(middle).unwrap()) / 108.0,
            );
    drag(
        &ctx,
        &mut editor,
        3.4,
        halfway,
        halfway - Vec2::new(0.0, 18.0),
    );
    assert!(bank.read(0).unwrap().points()[1].bend > 0.1);

    click_button(
        &ctx,
        &mut editor,
        3.5,
        monitor.center(),
        egui::PointerButton::Secondary,
    );
    let pause = label(&paint(&mut editor, 3.7), "Pause").expect("monitor menu");
    click(&ctx, &mut editor, 3.8, pause);
    assert!(editor.region_dynamics_paused[0]);
    let resume = label(&paint(&mut editor, 4.0), "Resume").expect("resume in monitor menu");
    click(&ctx, &mut editor, 4.1, resume);
    assert!(!editor.region_dynamics_paused[0]);

    click(&ctx, &mut editor, 4.3, square.min + Vec2::new(20.0, 20.0));
    let compact = label(&paint(&mut editor, 4.5), "Compact").expect("compact is visible");
    click(&ctx, &mut editor, 4.6, compact);
    assert!(editor.region_dynamics_compact[0]);
    let compact_view = paint(&mut editor, 4.8);
    assert!(chart(&compact_view, 230.0).is_none());
    let square = chart(&compact_view, 180.0).expect("compact transfer graph");
    let at = |x, y| square.min + square.size() * Vec2::new(x, y);
    click_button(
        &ctx,
        &mut editor,
        5.2,
        at(0.8, 0.5),
        egui::PointerButton::Secondary,
    );
    let curve = bank.read(0).unwrap();
    assert_eq!(curve.points().len(), 4);
    let [dragged, neighbor] = [curve.points()[1], curve.points()[2]];
    let from = at(
        (dragged.input + 96.0) / 108.0,
        (12.0 - dragged.output) / 108.0,
    );

    press_and_drag(&ctx, &mut editor, 5.4, from, at(0.89, 0.95));
    let crossed = bank.read(0).unwrap();
    assert_eq!(crossed.points()[1], neighbor);
    assert_eq!(crossed.points()[2].segment, dragged.segment);
    assert!(crossed.points()[2].input - crossed.points()[2].output > 72.0);
    frame(
        &ctx,
        &mut editor,
        5.46,
        vec![egui::Event::PointerMoved(at(0.93, 0.90))],
    );
    frame(
        &ctx,
        &mut editor,
        5.47,
        vec![primary(at(0.93, 0.90), false)],
    );
    let moved = bank.read(0).unwrap();
    assert_eq!(moved.points()[1], neighbor);
    assert!(moved.points()[2].input > crossed.points()[2].input);

    let mut preferences = editor.display_preferences.read();
    let accent = Color32::from_rgb(20, 140, 165);
    preferences.theme.region[0] = accent;
    editor.display_preferences = Arc::new(DisplayStore::with_values(preferences));
    params
        .ui_scale_percent
        .store(110, std::sync::atomic::Ordering::Relaxed);

    paint(&mut editor, 5.6);
    let scaled = paint(&mut editor, 5.7);
    assert!((ctx.zoom_factor() - 1.1).abs() < 0.001);
    assert!(scaled.iter().any(|item| {
        matches!(&item.shape, egui::Shape::Circle(circle) if circle.stroke.color == accent)
    }));
}

#[test]
fn response_picker_add() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    editor.place_region(1_200.0);

    let canvas = Rect::from_min_size(Pos2::ZERO, CANVAS);
    let panel = editor.region_panel_rect(canvas, 48_000.0).unwrap();

    click(
        &ctx,
        &mut editor,
        0.0,
        Pos2::new(panel.right() - 18.0, panel.top() + panel.height() * 0.875),
    );

    let row = find_text(&ctx, &mut editor, 0.8, "Response (0/2)");

    assert!(text_center(&ctx, &mut editor, 0.81, "Response").is_none());
    assert!(text_center(&ctx, &mut editor, 0.82, "Response 2").is_none());

    click(&ctx, &mut editor, 1.0, row);
    let add = find_text(&ctx, &mut editor, 1.1, "Add");
    click(&ctx, &mut editor, 1.2, add);

    for tick in 0..15 {
        frame(&ctx, &mut editor, 1.3 + f64::from(tick) * 0.04, Vec::new());
    }

    find_text(&ctx, &mut editor, 2.0, "Response 1");
    find_text(&ctx, &mut editor, 2.01, "Region Gain");
    find_text(&ctx, &mut editor, 2.02, "Edge Softness");

    let params = editor.params.clone();
    let region = &params.regions[0];
    assert!(region.responses[0].enabled.value());
    assert_eq!(region.responses[0].strength.value(), 0.0);
    assert_eq!(
        region.responses[0].target.value(),
        crate::region::response::Value::Softness
    );
    assert!((region.responses[0].control().reference - region.gain.value()).abs() < 1e-6);
    assert!(!region.responses[1].enabled.value());
}

#[test]
fn response_picker_uses_next_slot() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    editor.place_region(1_200.0);

    let params = editor.params.clone();
    let region = &params.regions[0];

    {
        let setter = editor.gui.as_ref().unwrap().param_setter();
        editor.set(&setter, &region.responses[0].enabled, true);
        editor.set(&setter, &region.responses[0].strength, 0.25);
    }

    let canvas = Rect::from_min_size(Pos2::ZERO, CANVAS);
    let panel = editor.region_panel_rect(canvas, 48_000.0).unwrap();

    click(
        &ctx,
        &mut editor,
        0.0,
        Pos2::new(panel.left() + 326.0, panel.top() + panel.height() * 0.875),
    );

    let row = find_text(&ctx, &mut editor, 0.8, "Response (1/2)");
    click(&ctx, &mut editor, 1.0, row);
    let add = find_text(&ctx, &mut editor, 1.1, "Add");
    click(&ctx, &mut editor, 1.2, add);

    for tick in 0..15 {
        frame(&ctx, &mut editor, 1.3 + f64::from(tick) * 0.04, Vec::new());
    }

    let handle = find_text(&ctx, &mut editor, 2.0, "Response 2");
    find_text(&ctx, &mut editor, 2.81, "Region Gain");
    find_text(&ctx, &mut editor, 2.82, "Edge Softness");
    assert!(region.responses[1].enabled.value());
    assert_eq!(region.responses[0].strength.value(), 0.25);

    {
        let setter = editor.gui.as_ref().unwrap().param_setter();
        editor.set(&setter, &region.responses[1].strength, 0.5);
    }

    click(&ctx, &mut editor, 3.0, handle);
    click(&ctx, &mut editor, 3.1, handle);
    assert!(region.reverse_responses.value());
    assert_eq!(region.responses[0].strength.value(), 0.25);

    editor.reveal_modifier = Some((0, modifiers::Modifier::Response(1)));

    for tick in 0..10 {
        frame(&ctx, &mut editor, 3.2 + f64::from(tick) * 0.04, Vec::new());
    }

    let handle = find_text(&ctx, &mut editor, 3.7, "Response 2");
    click(&ctx, &mut editor, 3.8, handle + Vec2::new(0.0, 43.0));
    let disable = find_text(&ctx, &mut editor, 4.3, "Disable modifier");
    click(&ctx, &mut editor, 4.4, disable);
    assert!(!region.responses[1].enabled.value());
    assert_eq!(region.responses[1].strength.value(), 0.5);

    let setter = editor.gui.as_ref().unwrap().param_setter();
    editor.reset_region(&setter, 0);
    assert!(!region.responses[0].enabled.value());
    assert!(!region.reverse_responses.value());
}

#[test]
fn response_picker_hides_when_full() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    editor.place_region(1_200.0);

    let params = editor.params.clone();
    let setter = editor.gui.as_ref().unwrap().param_setter();

    for response in &params.regions[0].responses {
        editor.set(&setter, &response.enabled, true);
    }

    let canvas = Rect::from_min_size(Pos2::ZERO, CANVAS);
    let panel = editor.region_panel_rect(canvas, 48_000.0).unwrap();

    click(
        &ctx,
        &mut editor,
        0.0,
        Pos2::new(panel.left() + 326.0, panel.top() + panel.height() * 0.875),
    );

    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(canvas),
            time: Some(0.8),
            focused: true,
            ..Default::default()
        },
        |ui| editor.draw(ui),
    );
    output.textures_delta.clear();
    assert!(
        editor
            .modifier_picker
            .as_ref()
            .is_some_and(modifiers::Picker::is_up)
    );
    assert!(output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text() == "Harmonic")
    }));
    assert!(!output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text().starts_with("Response ("))
    }));
}

#[test]
fn drag_tracked_band() {
    let (ctx, mut editor) = editor();
    one_bell(&mut editor);
    attach_host(&mut editor);
    let params = editor.params.clone();
    let band = params.band(0);
    let setter = editor.gui.as_ref().unwrap().param_setter();
    editor.set(&setter, &band.pitch.enabled, true);
    let base = band.frequency.value();
    let plot = axis_plot();
    let point = |hz| Pos2::new(plot.x(hz), plot.y(band.gain.value()));

    let first = point(900.0);
    editor.telemetry.set_tracked_frequency(0, 900.0);
    frame(
        &ctx,
        &mut editor,
        0.0,
        vec![egui::Event::PointerMoved(first)],
    );
    frame(&ctx, &mut editor, 0.1, vec![primary(first, true)]);
    editor.telemetry.set_tracked_frequency(0, 2_200.0);
    frame(&ctx, &mut editor, 0.2, vec![primary(first, false)]);
    assert_eq!(editor.selected, Some(0));

    let moved = point(2_200.0);
    drag(
        &ctx,
        &mut editor,
        0.5,
        moved - Vec2::splat(24.0),
        moved + Vec2::splat(24.0),
    );
    assert!(editor.is_picked(0));
    let gain = band.gain.value();
    drag(
        &ctx,
        &mut editor,
        1.0,
        moved,
        moved + Vec2::new(50.0, -30.0),
    );
    assert!(band.gain.value() > gain);
    assert_eq!(band.frequency.value(), base);
}

#[test]
fn modifier_picker_add() {
    use modifiers::Modifier;

    for (modifier, double_click) in Modifier::BAND
        .into_iter()
        .flat_map(|modifier| [(modifier, false), (modifier, true)])
    {
        let (ctx, mut editor) = editor();
        attach_host(&mut editor);
        let params = editor.params.clone();
        let setter = editor.gui.as_ref().unwrap().param_setter();
        editor.set_band_enabled(0, true);
        if modifier == Modifier::Pitch {
            editor.set(&setter, &params.band(0).kind, FilterKind::Notch);
        }
        editor.set(
            &setter,
            &params.band(0).frequency,
            axis_plot().hz(CANVAS.x * 0.5),
        );
        editor.pick_only(0);
        let opener = panel_button(Layout::PANEL_W - 18.0, 108.5);
        click(&ctx, &mut editor, 0.0, opener);
        assert!(editor.modifier_picker.is_some());

        let find_text = |editor: &mut EquartEditor, time, label| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, CANVAS)),
                    time: Some(time),
                    focused: true,
                    ..Default::default()
                },
                |ui| editor.draw(ui),
            );
            output.textures_delta.clear();
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => {
                        let bounds = clipped.shape.visual_bounding_rect();
                        if label == "THRESHOLD" {
                            assert!(clipped.clip_rect.contains_rect(bounds));
                        }
                        if matches!(label, "Lock reference" | "Sync" | "Phase & timing") {
                            assert_eq!(text.galley.rows.len(), 1, "{label} wrapped");
                            assert!(
                                clipped.clip_rect.contains_rect(bounds),
                                "{label} was clipped"
                            );
                        }
                        Some(bounds.center())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing text: {label}"))
        };
        frame(&ctx, &mut editor, 0.08, Vec::new());
        let row_label = match modifier {
            Modifier::Dynamic => "Dynamic",
            Modifier::Pitch => "f0lock",
            Modifier::Mapping => "Mapping",
            Modifier::Lfo => "LFO",
            Modifier::Harmonic
            | Modifier::Resonance
            | Modifier::Response(_)
            | Modifier::SpectralDynamics => {
                unreachable!("this test adds Band modifiers")
            }
        };
        let row = find_text(&mut editor, 1.0, row_label);
        assert!(row.y < opener.y && opener.y - row.y < 220.0);
        assert!(row.x < opener.x && opener.x - row.x < 210.0);
        click(&ctx, &mut editor, 1.1, row);
        if double_click {
            click(&ctx, &mut editor, 1.2, row);
        } else {
            let add = find_text(&mut editor, 1.2, "Add");
            click(&ctx, &mut editor, 1.3, add);
        }
        assert!(
            modifier.attached(params.band(0)),
            "{row_label}: double_click={double_click}, picker open={}, row at {row:?}",
            editor.modifier_picker.is_some(),
        );
        assert!(
            editor
                .modifier_picker
                .as_ref()
                .is_some_and(|picker| !picker.is_up())
        );
        for tick in 0..12 {
            frame(&ctx, &mut editor, 1.4 + f64::from(tick) * 0.04, Vec::new());
        }
        assert!(editor.modifier_picker.is_none());
        assert!(editor.reveal_modifier.is_none());
        if modifier == Modifier::Mapping {
            let band = params.band(0);
            assert_eq!(band.mapping.reference_hz.value(), band.frequency.value());
            find_text(&mut editor, 1.95, "Strength");
            find_text(&mut editor, 1.96, "Lock reference");
            continue;
        }
        if modifier == Modifier::Lfo {
            find_text(&mut editor, 1.95, "Phase & timing");
            find_text(&mut editor, 1.96, "Sync");
            continue;
        }
        if modifier == Modifier::Pitch {
            find_text(&mut editor, 1.95, "Multiplier");
            let algorithm = find_text(&mut editor, 2.0, "F0LOCK Ψ");
            click(&ctx, &mut editor, 2.1, algorithm);
            let pyin = find_text(&mut editor, 2.15, "pYIN");
            click(&ctx, &mut editor, 2.2, pyin);
            assert_eq!(
                params.band(0).pitch.detection.algorithm.value(),
                crate::pitch::DetectionMode::Pyin
            );
            assert!(!params.band(0).dynamics.enabled.value());
            continue;
        }
        assert_eq!(
            params.band(0).dynamics.detect.mode.value(),
            crate::dynamics::DetectorMode::Follow
        );
        let threshold = find_text(&mut editor, 2.0, "THRESHOLD");
        assert!(threshold.x < CANVAS.x);
        if !double_click {
            let pointer = Pos2::new(CANVAS.x * 0.5, panel_button(0.0, 108.0).y);
            frame(
                &ctx,
                &mut editor,
                2.1,
                vec![egui::Event::PointerMoved(pointer)],
            );
            frame(
                &ctx,
                &mut editor,
                2.12,
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(180.0, 0.0),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: egui::Modifiers::default(),
                }],
            );
            for tick in 0..12 {
                frame(&ctx, &mut editor, 2.14 + f64::from(tick) * 0.03, Vec::new());
            }
            let shifted = find_text(&mut editor, 2.6, "THRESHOLD");
            assert!(
                shifted.x > threshold.x + 5.0,
                "horizontal scroll: {threshold:?} -> {shifted:?}"
            );
        } else {
            let threshold_param = &params.band(0).dynamics.threshold;
            let before = threshold_param.value();
            wheel_at(
                &ctx,
                &mut editor,
                2.2,
                threshold + egui::vec2(0.0, 40.0),
                false,
            );
            assert!(threshold_param.value() > before);
        }
    }
}
