use super::*;

#[test]
fn meter_peak_resets() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    editor.telemetry.record_output_max(0.5);
    editor.telemetry.record_output_max(0.25);

    let readout = text_center(&ctx, &mut editor, 0.0, "-6.0").unwrap();
    assert!(readout.x > CANVAS.x - Layout::METER_W);
    click(&ctx, &mut editor, 0.1, readout);

    assert_eq!(editor.telemetry.meters().output_max, 0.0);
    assert!(text_center(&ctx, &mut editor, 0.2, "-INF").is_some());

    editor.telemetry.record_output_max(0.25);
    assert!(text_center(&ctx, &mut editor, 0.3, "-12").is_some());
}

#[test]
fn meter_current_smooths() {
    let (ctx, mut editor) = editor();
    let mut prefs = editor.display_preferences.read();
    prefs.layout.meter_peak_hold = false;
    editor.display_preferences = Arc::new(DisplayStore::with_values(prefs));

    let mut engine = crate::engine::Engine::new(48_000.0);
    engine.output_peak = 0.5;
    editor.telemetry.publish(&engine);

    frame(&ctx, &mut editor, 0.0, vec![]);
    let first = editor.meter_smoothed_peak;
    assert!(first > 0.0 && first < 0.5);

    frame(&ctx, &mut editor, 0.02, vec![]);
    let second = editor.meter_smoothed_peak;
    assert!(second > first && second < 0.5);

    engine.output_peak = 0.0;
    editor.telemetry.publish(&engine);
    frame(&ctx, &mut editor, 0.04, vec![]);
    assert!(editor.meter_smoothed_peak > 0.0 && editor.meter_smoothed_peak < second);
}
