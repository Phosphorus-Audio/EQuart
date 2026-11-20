use super::*;

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
            Modifier::Harmonic | Modifier::Resonance => unreachable!("this test adds Band modifiers"),
        };
        let row = find_text(&mut editor, 1.0, row_label);
        assert!(row.y < opener.y && opener.y - row.y < 170.0);
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
