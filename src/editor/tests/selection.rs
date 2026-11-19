use super::*;

/// dragging one picked band moves the rest too (checks armed followers since no host here)
#[test]
fn drag_box_moves_followers() {
    let (ctx, mut editor) = editor();
    three_bells(&mut editor);
    editor.selected = None;

    let left = node(&editor, 0);
    let right = node(&editor, 1);
    drag(
        &ctx,
        &mut editor,
        0.0,
        Pos2::new(left.x - 40.0, left.y - 60.0),
        Pos2::new(right.x + 40.0, right.y + 60.0),
    );

    let follower = editor.control(1);
    let grab = node(&editor, 0);
    press_and_drag(
        &ctx,
        &mut editor,
        1.0,
        grab,
        Pos2::new(grab.x + 70.0, grab.y + 35.0),
    );

    let drag = editor.drag.as_ref().expect("a point drag should be armed");
    assert_eq!(drag.band, 0, "the point under the cursor leads");
    assert_eq!(
        drag.followers.len(),
        1,
        "the other picked point rides along"
    );
    assert_eq!(drag.followers[0].0, 1);
    assert!((drag.followers[0].1.0 - follower.frequency).abs() < 0.01);
    assert!((drag.followers[0].1.1 - follower.gain).abs() < 0.01);
}

/// double click piano key adds a band at that freq (0 dB)
#[test]
fn double_click_key_adds_band() {
    let (ctx, mut editor) = editor();
    attach_host(&mut editor);
    let pos = key_at(440.0);

    click(&ctx, &mut editor, 0.0, pos);
    click(&ctx, &mut editor, 0.05, pos);

    let band = editor.params.band(0);
    assert_eq!(band.kind.value(), FilterKind::Peak);
    assert!((band.frequency.value() - 440.0).abs() < 0.5, "A4 = 440 Hz");
    assert_eq!(band.gain.value(), 0.0, "a key carries no gain");
    assert!(band.enabled.value());
    assert_eq!(editor.selected, Some(0));
    assert!(editor.params.is_placed(0));
    assert!(!editor.params.is_placed(1));
}

/// old state without placed mask still keeps ghost if param != default
#[test]
fn old_state_keeps_ghosts() {
    let (_, mut editor) = editor();
    attach_host(&mut editor);

    // simulate old restore: set value without touching placed mask
    let setter = editor.gui.as_ref().unwrap().param_setter();
    editor.set(&setter, &editor.params.band(2).gain, 3.0);

    assert_eq!(editor.params.band(2).gain.value(), 3.0, "the value landed");
    assert!(
        !editor.params.is_placed(2),
        "the mask knows nothing about it"
    );
    assert!(editor.placed(2), "the value still says the slot was used");
}
