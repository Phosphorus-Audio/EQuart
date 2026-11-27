use super::*;

/// wheel on cut changes slope, not Q
#[test]
fn wheel_steps_cut_slope() {
    let (ctx, mut editor) = editor();
    one_band(&mut editor, FilterKind::Lowpass);
    attach_host(&mut editor);
    let q = editor.params.band(0).q.value();
    let position = node(&editor, 0);

    wheel_at(&ctx, &mut editor, 0.0, position, false);
    assert_eq!(
        editor.params.band(0).slope.value(),
        CutSlope::Db18,
        "one notch up should steepen the slope"
    );
    assert_eq!(editor.params.band(0).q.value(), q, "Q must stay put");
    assert_eq!(editor.selected, Some(0), "the wheel focuses the band");

    wheel_at(&ctx, &mut editor, 1.0, position, true);
    assert_eq!(editor.params.band(0).slope.value(), CutSlope::Db12);
}

#[test]
fn mapping_preview_ignores_wheel() {
    let (ctx, mut editor) = editor();
    one_bell(&mut editor);
    attach_host(&mut editor);

    {
        let setter = editor.gui.as_ref().unwrap().param_setter();
        editor.set(&setter, &editor.params.band(0).mapping.enabled, true);
    }

    editor.pick_only(0);
    let control = editor.control(0);
    let input = format!(
        "Input  {}",
        crate::modulation::Target::Frequency.display(control.frequency)
    );
    let graph = text_center(&ctx, &mut editor, 0.0, &input).unwrap() - Vec2::new(0.0, 34.0);

    wheel_at(&ctx, &mut editor, 0.1, graph, false);
    assert_eq!(editor.control(0).q, control.q);

    let band = node(&editor, 0);
    wheel_at(&ctx, &mut editor, 0.5, band, false);
    assert!(editor.control(0).q > control.q);
}

#[test]
fn panel_close_click_preserves_point_and_control_click_deletes_it() {
    let (ctx, mut editor) = editor();
    one_bell(&mut editor);
    attach_host(&mut editor);

    {
        let setter = editor.gui.as_ref().unwrap().param_setter();
        editor.set(
            &setter,
            &editor.params.band(0).frequency,
            axis_plot().hz(CANVAS.x * 0.5),
        );
    }

    editor.pick_only(0);
    let close = panel_button(Layout::PANEL_W - 18.0, 24.0);
    click(&ctx, &mut editor, 0.0, close);
    assert_eq!(editor.selected, None);
    assert!(editor.band_exists(0));

    editor.pick_only(0);
    delete_click(&ctx, &mut editor, 1.0, close);
    assert_eq!(editor.selected, None);
    assert!(!editor.band_exists(0));
}
