use super::*;

#[test]
fn drag_history() {
    let (ctx, mut editor) = editor();
    one_bell(&mut editor);
    attach_host(&mut editor);

    let before = editor.control(0);
    let start = node(&editor, 0);
    let end = start + Vec2::new(75.0, -58.0);
    press_and_drag(&ctx, &mut editor, 0.0, start, start.lerp(end, 0.5));
    frame(&ctx, &mut editor, 0.2, vec![egui::Event::PointerMoved(end)]);
    frame(
        &ctx,
        &mut editor,
        0.3,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let after = editor.control(0);
    assert!((after.gain - before.gain).abs() > 0.5);

    let setter = editor.history_host.as_ref().unwrap().param_setter();
    editor.set(&setter, &editor.params.global.output, -4.0);

    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    let modifiers = egui::Modifiers {
        ctrl: !mac,
        mac_cmd: mac,
        command: true,
        ..Default::default()
    };
    frame(
        &ctx,
        &mut editor,
        0.4,
        vec![egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }],
    );
    assert!((editor.control(0).gain - before.gain).abs() < 0.001);
    assert!((editor.control(0).frequency - before.frequency).abs() < 0.01);
    assert!((editor.params.global.output.value() + 4.0).abs() < 0.001);

    click(
        &ctx,
        &mut editor,
        0.6,
        Pos2::new(52.0, CANVAS.y - Layout::FOOTER * 0.5),
    );
    assert!((editor.control(0).gain - after.gain).abs() < 0.001);
    assert!((editor.control(0).frequency - after.frequency).abs() < 0.01);
}

#[test]
fn placement_history() {
    let (ctx, mut editor) = editor();
    one_bell(&mut editor);
    Arc::get_mut(&mut editor.params).unwrap().mark_placed(0);
    attach_host(&mut editor);

    {
        let setter = editor.history_host.as_ref().unwrap().param_setter();
        editor.set(
            &setter,
            &editor.params.band(0).frequency,
            axis_plot().hz(CANVAS.x * 0.5),
        );
    }

    editor.pick_only(0);
    let close = panel_button(Layout::PANEL_W - 18.0, 24.0);
    delete_click(&ctx, &mut editor, 0.1, close);
    assert!(!editor.band_exists(0));

    click(
        &ctx,
        &mut editor,
        0.5,
        Pos2::new(25.0, CANVAS.y - Layout::FOOTER * 0.5),
    );
    assert!(editor.control(0).enabled);
    assert!(editor.params.is_placed(0));

    click(
        &ctx,
        &mut editor,
        0.8,
        Pos2::new(52.0, CANVAS.y - Layout::FOOTER * 0.5),
    );
    assert!(!editor.band_exists(0));
}
