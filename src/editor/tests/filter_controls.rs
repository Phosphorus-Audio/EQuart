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
