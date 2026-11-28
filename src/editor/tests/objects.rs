use super::*;

#[test]
fn layout_animation_speed() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("equart-animation-{}-{nonce}", std::process::id()));
    let path = dir.join("display.json");
    editor.display_preferences = Arc::new(DisplayStore::new(Some(path.clone())));

    let layout = Pos2::new(CANVAS.x - 22.0, CANVAS.y - Layout::FOOTER * 0.5);
    click(&ctx, &mut editor, 0.0, layout);

    assert!(editor.layout.is_up());
    assert!(has_visible_text(&ctx, &mut editor, 1.0, "ANIMATION SPEED"));
    click_text(&ctx, &mut editor, 1.1, "Normal");

    for (time, label) in [(1.5, "Slow"), (1.6, "Fast"), (1.7, "Instant")] {
        assert!(has_visible_text(&ctx, &mut editor, time, label));
    }

    click_text(&ctx, &mut editor, 1.8, "Instant");
    frame(&ctx, &mut editor, 1.9, Vec::new());

    assert_eq!(
        editor.display_preferences.read().layout.animation_speed,
        AnimationSpeed::Instant,
    );
    assert_eq!(animation::speed(&ctx), AnimationSpeed::Instant);
    assert_eq!(ctx.global_style().animation_time, 0.0);
    let saved = std::fs::read_to_string(path).unwrap();
    let restored: DisplayPreferences = serde_json::from_str(&saved).unwrap();
    assert_eq!(
        restored.layout.animation_speed,
        AnimationSpeed::Instant,
    );

    editor.layout.release(2.0);
    frame(&ctx, &mut editor, 2.0, Vec::new());
    assert!(!editor.layout.is_shown());

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn heatmap_layer_order() {
    use crate::display_preferences::HeatmapMode;

    let (ctx, mut editor) = editor();
    one_bell(&mut editor);
    let params = Arc::get_mut(&mut editor.params).unwrap();
    params.regions[0].enabled = BoolParam::new("Enabled", true);

    let mut prefs = editor.display_preferences.read();
    prefs.spectrum.heatmap.mode = HeatmapMode::Full;

    let levels = [-25.0; 2_049];
    let spectrum = crate::analyzer::SpectrumFrame {
        bin_hz: 48_000.0 / 4096.0,
        output_levels: &levels,
        output_hold_levels: &levels,
        ..Default::default()
    };
    let canvas = Rect::from_min_size(Pos2::ZERO, CANVAS);
    let plot = axis_plot();

    for above_grid in [false, true] {
        prefs.spectrum.heatmap.above_grid = above_grid;
        editor.display_preferences = Arc::new(DisplayStore::with_values(prefs));

        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(canvas),
                ..Default::default()
            },
            |ui| editor.spectrum(ui, canvas, 48_000.0, &spectrum),
        );
        output.textures_delta.clear();

        let heatmap = output.shapes.iter().position(|shape| {
            matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.calc_bounds() == plot.rect)
        });
        let grid = output.shapes.iter().position(|shape| {
            matches!(&shape.shape, egui::Shape::LineSegment { points: [left, right], .. }
                if left.y == plot.y(0.0) && right.y == left.y && right.x - left.x > plot.rect.width())
        });
        let heatmap = heatmap.expect("the heatmap covers the plot");
        let grid = grid.expect("the grid shows the 0 dB line");
        assert_eq!(heatmap > grid, above_grid);

        let above = &output.shapes[heatmap + 1..];

        for width in [1.6, 1.9] {
            assert!(above.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Path(path) if path.stroke.width == width)
            }));
        }

        for label in ["Region 1", "1", "EQ dB ±12"] {
            assert!(above.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == label)
            }));
        }
    }
}

#[test]
fn plot_menu_bands_and_regions() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    let plot = axis_plot();
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
                    let rect = clipped.shape.visual_bounding_rect();
                    clipped.clip_rect.contains_rect(rect).then(|| rect.center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing visible label: {label}"))
    };
    let invoke = |editor: &mut EquartEditor, time, position, action| {
        click_button(&ctx, editor, time, position, egui::PointerButton::Secondary);
        let item = find_text(editor, time + 0.1, action);
        click(&ctx, editor, time + 0.2, item);
    };

    let original = editor.control(1);
    let point = Pos2::new(plot.x(1_200.0), plot.y(4.0));
    invoke(&mut editor, 0.0, point, "Add Band");
    assert_eq!(editor.selected, Some(0));
    assert!((editor.control(0).frequency - 1_200.0).abs() < 0.1);
    assert!((editor.control(0).gain - 4.0).abs() < 0.02);

    let blank = Pos2::new(plot.x(3_500.0), plot.y(5.0));
    invoke(&mut editor, 1.0, blank, "Freeze");
    assert!(editor.params.analyzer_frozen.load(Ordering::Relaxed));
    invoke(&mut editor, 2.0, blank, "Freeze");
    assert!(!editor.params.analyzer_frozen.load(Ordering::Relaxed));

    invoke(&mut editor, 2.5, blank, "Add Spectral Region");
    assert_eq!(editor.selected_region, Some(0));
    assert_eq!(editor.selected, None);
    let region = editor.params.regions[0].control();
    assert!(region.enabled && !region.bypass);
    assert!((region.high / region.low - 2.0).abs() < 0.001);
    assert_eq!(region.gain, crate::region::DEFAULT_GAIN);
    let start = find_text(&mut editor, 2.8, "Range Start");
    let end = find_text(&mut editor, 2.81, "Range End");
    let softness = find_text(&mut editor, 2.82, "Edge Softness");
    let gain = find_text(&mut editor, 2.83, "Gain");
    assert!(start.x < end.x && end.x < softness.x && softness.x < gain.x);
    assert!((start.y - gain.y).abs() < 2.0);
    let panel = editor
        .region_panel_rect(Rect::from_min_size(Pos2::ZERO, CANVAS), 0.0)
        .unwrap();
    let bypass = panel.min + Vec2::new(24.0, 18.0);
    click(&ctx, &mut editor, 2.9, bypass);
    assert!(editor.params.regions[0].bypass.value());
    let action = |y| Pos2::new(panel.right() - 18.0, panel.top() + y);
    let add_modifier = action(108.5);
    click(&ctx, &mut editor, 3.1, add_modifier);
    assert!(
        editor
            .modifier_picker
            .as_ref()
            .is_some_and(modifiers::Picker::is_up)
    );
    let harmonic = find_text(&mut editor, 4.1, "Harmonic");
    click(&ctx, &mut editor, 4.2, harmonic);
    let add = find_text(&mut editor, 4.3, "Add");
    click(&ctx, &mut editor, 4.4, add);
    let harmonic = editor.params.regions[0].harmonic.control();
    assert!(harmonic.enabled);
    assert_eq!((harmonic.first, harmonic.last), (2, 8));
    let protect = find_text(&mut editor, 4.9, "Protect");
    click(&ctx, &mut editor, 5.0, protect);
    assert_eq!(
        editor.params.regions[0].harmonic.mode.value(),
        crate::region::HarmonicMode::Protect
    );

    let tracked_panel = editor
        .region_panel_rect(Rect::from_min_size(Pos2::ZERO, CANVAS), 0.0)
        .unwrap();

    let target = find_text(&mut editor, 5.03, "Auto");

    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, CANVAS)),
            time: Some(5.04),
            focused: true,
            ..Default::default()
        },
        |ui| editor.draw(ui),
    );

    let target_button = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if shape.rect.contains(target) && shape.rect.height() < 50.0 => {
                Some(shape.rect)
            }
            _ => None,
        })
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("Target selector draws a button");

    assert!(
        tracked_panel.contains_rect(target_button),
        "Target button {target_button:?} overflows Tracked panel {tracked_panel:?}"
    );

    click(&ctx, &mut editor, 5.05, target);

    let reference = find_text(&mut editor, 5.12, "Reference");

    click(&ctx, &mut editor, 5.13, reference);
    assert_eq!(
        editor.params.regions[0].harmonic.detection.target.value(),
        crate::pitch::TargetMode::Reference
    );

    let source = find_text(&mut editor, 5.20, "Tracked");
    click(&ctx, &mut editor, 5.22, source);

    let fixed = find_text(&mut editor, 5.29, "Fixed");
    click(&ctx, &mut editor, 5.31, fixed);

    assert_eq!(
        editor.params.regions[0].harmonic.source.value(),
        crate::region::FundamentalSource::Fixed
    );

    let expanded = editor
        .region_panel_rect(Rect::from_min_size(Pos2::ZERO, CANVAS), 0.0)
        .unwrap();
    assert!(
        expanded.width() < tracked_panel.width(),
        "Fixed should size the panel to its two visible columns"
    );

    let action = |y| Pos2::new(expanded.left() + panel.width() - 18.0, expanded.top() + y);
    click(&ctx, &mut editor, 5.4, action(15.5));
    assert_eq!(editor.selected_region, None);
    assert!(editor.params.regions[0].enabled.value());
    let chip = find_text(&mut editor, 5.5, "Region 1");
    click(&ctx, &mut editor, 5.6, chip);
    assert_eq!(editor.selected_region, Some(0));
    delete_click(&ctx, &mut editor, 6.0, action(15.5));
    assert!(!editor.params.regions[0].enabled.value());
    assert!(!editor.params.regions[0].harmonic.enabled.value());
    for _ in 0..crate::region::REGION_COUNT {
        editor.place_region(880.0);
    }
    assert!(
        editor
            .params
            .regions
            .iter()
            .all(|region| region.enabled.value())
    );
    assert_eq!(
        editor.selected_region,
        Some(crate::region::REGION_COUNT - 1)
    );

    editor.set_band_enabled(1, true);
    let setter = editor.gui.as_ref().unwrap().param_setter();
    editor.set(&setter, &editor.params.band(1).pitch.enabled, true);
    editor.set_band_enabled(1, false);
    assert!(editor.band_exists(1));
    invoke(&mut editor, 6.6, blank, "Delete all");
    assert_eq!(editor.selected, None);
    assert_eq!(editor.selected_region, None);
    assert!(
        editor
            .params
            .regions
            .iter()
            .all(|region| !region.enabled.value())
    );
    assert_eq!(marker_count(&ctx, &mut editor, 7.0), 0);
    assert_eq!(editor.control(1), original);
    assert!((0..BAND_COUNT).all(|index| !editor.params.is_placed(index)));
}

#[test]
fn panel_converts_band_region() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    let params = editor.params.clone();
    let band = params.band(0);
    let setter = editor.gui.as_ref().unwrap().param_setter();
    let center = axis_plot().hz(CANVAS.x * 0.5);
    editor.set_band_enabled(0, true);
    editor.set(&setter, &band.kind, FilterKind::Peak);
    editor.set(&setter, &band.frequency, center);
    editor.set(&setter, &band.gain, 4.25);
    editor.set(&setter, &band.q, 1.3);
    editor.set(&setter, &band.slope, CutSlope::Db48);
    editor.set_band_enabled(0, false);
    editor.pick_only(0);

    let convert = panel_button(Layout::PANEL_W - 18.0, 77.5);
    click(&ctx, &mut editor, 0.0, convert);
    assert!(editor.conversion_dialog.is_some());
    assert!(editor.band_exists(0));
    click_text(&ctx, &mut editor, 0.1, "Continue");
    assert_eq!(editor.selected_region, Some(0));
    assert!(!editor.band_exists(0));
    frame(&ctx, &mut editor, 0.2, vec![]);
    assert_eq!(editor.selected_region, Some(0));

    assert!(has_visible_text(
        &ctx,
        &mut editor,
        0.3,
        "Spectral Region 1"
    ));

    let region = params.regions[0].control();
    assert!(region.enabled && region.bypass);
    assert_eq!(region.gain, 4.25);
    assert!(region.low < center && region.high > center);

    let panel = editor
        .region_panel_rect(Rect::from_min_size(Pos2::ZERO, CANVAS), 0.0)
        .unwrap();
    let convert = Pos2::new(panel.right() - 18.0, panel.top() + 77.5);
    click(&ctx, &mut editor, 0.5, convert);
    assert_eq!(editor.selected, Some(0));
    assert!(!params.regions[0].enabled.value());
    assert!(editor.band_exists(0));
    let band = editor.control(0);
    assert!(!band.enabled);
    assert!((band.frequency - center).abs() < 0.02);
    assert!((band.q - 1.3).abs() < 0.01);
    assert_eq!(band.gain, 4.25);
    assert_eq!(band.slope, CutSlope::Db48);

    for _ in 0..crate::region::REGION_COUNT {
        editor.place_region(880.0);
    }
    editor.pick_only(0);
    click(
        &ctx,
        &mut editor,
        1.0,
        panel_button(Layout::PANEL_W - 18.0, 77.5),
    );
    assert_eq!(editor.selected, Some(0));
    assert_eq!(editor.control(0), band);
}

#[test]
fn edited_band_conversion_can_be_cancelled() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    let params = editor.params.clone();
    editor.set_band_enabled(0, true);
    let setter = editor.gui.as_ref().unwrap().param_setter();
    editor.set(
        &setter,
        &params.band(0).frequency,
        axis_plot().hz(CANVAS.x * 0.5),
    );
    editor.band_conversion_baselines[0] = Some(editor.control(0));
    editor.set(&setter, &params.band(0).gain, 4.0);
    editor.pick_only(0);
    let original = editor.control(0);

    let convert = panel_button(Layout::PANEL_W - 18.0, 77.5);
    click(&ctx, &mut editor, 0.0, convert);
    assert!(matches!(
        editor.conversion_dialog,
        Some(regions::ConversionDialog::BandToRegion { source: 0, .. })
    ));
    assert!(editor.band_exists(0));
    assert!(!params.regions[0].enabled.value());
    assert!(has_visible_text(
        &ctx,
        &mut editor,
        0.05,
        "Switch to Spectral Region?"
    ));

    click_text(&ctx, &mut editor, 0.1, "Cancel");
    assert!(editor.conversion_dialog.is_none());
    assert_eq!(editor.selected, Some(0));
    assert_eq!(editor.control(0), original);
    assert!(!params.regions[0].enabled.value());

    click(&ctx, &mut editor, 0.3, convert);
    click_text(&ctx, &mut editor, 0.35, "Continue");
    assert_eq!(editor.selected_region, Some(0));
    assert!(!editor.band_exists(0));
}

#[test]
fn band_modifier_conversion_requires_confirmation() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    let params = editor.params.clone();
    editor.set_band_enabled(0, true);
    let setter = editor.gui.as_ref().unwrap().param_setter();
    editor.set(
        &setter,
        &params.band(0).frequency,
        axis_plot().hz(CANVAS.x * 0.5),
    );
    editor.band_conversion_baselines[0] = Some(editor.control(0));
    editor.set(&setter, &params.band(0).pitch.enabled, true);
    editor.pick_only(0);

    let canvas = Rect::from_min_size(Pos2::ZERO, CANVAS);
    let panel = Layout::panel(canvas, canvas.center().x);
    let width = (Layout::PANEL_W + modifiers::Modifier::Pitch.width()).min(canvas.width() - 24.0);
    let left = panel.left().min(canvas.right() - 12.0 - width);
    let convert = Pos2::new(left + Layout::PANEL_W - 18.0, panel.top() + 77.5);
    click(&ctx, &mut editor, 0.0, convert);
    assert!(editor.conversion_dialog.is_some());
    click_text(&ctx, &mut editor, 0.1, "Continue");
    assert_eq!(editor.selected_region, Some(0));
    assert!(!params.band(0).pitch.enabled.value());
}

#[test]
fn edited_region_conversion_can_be_cancelled() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    let params = editor.params.clone();
    editor.place_region(1_000.0);
    let setter = editor.gui.as_ref().unwrap().param_setter();
    editor.set(&setter, &params.regions[0].gain, -2.5);
    let original = params.regions[0].control();
    let panel = editor
        .region_panel_rect(Rect::from_min_size(Pos2::ZERO, CANVAS), 48_000.0)
        .unwrap();
    let convert = Pos2::new(panel.right() - 18.0, panel.top() + 77.5);

    click(&ctx, &mut editor, 0.0, convert);
    assert!(matches!(
        editor.conversion_dialog,
        Some(regions::ConversionDialog::RegionToBand { source: 0, .. })
    ));
    assert!(params.regions[0].enabled.value());
    assert!(!editor.band_exists(0));
    assert!(has_visible_text(&ctx, &mut editor, 0.05, "Switch to Band?"));

    click_text(&ctx, &mut editor, 0.1, "Cancel");
    assert!(editor.conversion_dialog.is_none());
    assert_eq!(editor.selected_region, Some(0));
    assert!(params.regions[0].control() == original);
    assert!(!editor.band_exists(0));

    click(&ctx, &mut editor, 0.3, convert);
    click_text(&ctx, &mut editor, 0.35, "Continue");
    assert_eq!(editor.selected, Some(0));
    assert!(!params.regions[0].enabled.value());
    assert!(editor.band_exists(0));
}

#[test]
fn region_modifier_conversion_requires_confirmation() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    let params = editor.params.clone();
    editor.place_region(1_000.0);
    let setter = editor.gui.as_ref().unwrap().param_setter();
    editor.set(&setter, &params.regions[0].harmonic.enabled, true);
    let panel = editor
        .region_panel_rect(Rect::from_min_size(Pos2::ZERO, CANVAS), 48_000.0)
        .unwrap();
    let convert = Pos2::new(panel.left() + 326.0, panel.top() + 77.5);

    click(&ctx, &mut editor, 0.0, convert);
    assert!(editor.conversion_dialog.is_some());
    click_text(&ctx, &mut editor, 0.1, "Continue");
    assert_eq!(editor.selected, Some(0));
    assert!(!params.regions[0].harmonic.enabled.value());
}
