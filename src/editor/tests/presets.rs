use super::*;
use crate::presets;

fn temp_store(tag: &str) -> Arc<presets::Store> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "equart-browser-{tag}-{}-{nonce}",
        std::process::id()
    ));

    Arc::new(presets::Store::at(dir))
}

fn texts(ctx: &egui::Context, editor: &mut EquartEditor, time: f64) -> Vec<(String, Rect)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
        match shape {
            egui::Shape::Text(text) => {
                let rect = Rect::from_min_size(text.pos, text.galley.size());
                out.push((text.galley.text().to_owned(), rect));
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, out)),
            _ => {}
        }
    }

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

    let mut found = Vec::new();

    for clipped in &output.shapes {
        walk(&clipped.shape, &mut found);
    }

    found
}

fn find(found: &[(String, Rect)], text: &str) -> Rect {
    found
        .iter()
        .find(|(label, _)| label == text)
        .unwrap_or_else(|| panic!("no {text} on screen: {:?}", names(found)))
        .1
}

fn names(found: &[(String, Rect)]) -> Vec<&str> {
    found.iter().map(|(label, _)| label.as_str()).collect()
}

fn editor_with_store(store: Arc<presets::Store>) -> (egui::Context, EquartEditor) {
    let ctx = egui::Context::default();
    let params = Arc::new(EquartParams::default());
    let initial = presets::seed_instance(&params, &store);

    let mut app = EquartEditor::new(
        params,
        Arc::new(DisplayStore::default()),
        Arc::new(crate::Telemetry::default()),
        Arc::new(crate::analyzer::capture::channel().1),
        store,
        Arc::new(std::sync::Mutex::new(initial)),
    );
    attach_host(&mut app);

    (ctx, app)
}

#[test]
fn default_preset_seeds_once() {
    let store = temp_store("default");
    store.set_default(Some("Factory/200-LowCut")).unwrap();

    let params = Arc::new(EquartParams::default());
    let entry = presets::seed_instance(&params, &store);
    assert_eq!(entry.as_ref().map(|e| e.name.as_str()), Some("200-LowCut"));

    let band = params.control(0);
    assert!(band.enabled, "fresh instance starts on the default preset");
    assert!((band.frequency - 200.0).abs() < 1.0, "{}", band.frequency);

    let empty_state = presets::state_of(&EquartParams::default());
    presets::restore(&params, &empty_state);
    assert!(!params.control(0).enabled);

    let ctx = egui::Context::default();
    let mut other = EquartEditor::new(
        params,
        Arc::new(DisplayStore::default()),
        Arc::new(crate::Telemetry::default()),
        Arc::new(crate::analyzer::capture::channel().1),
        store,
        Arc::new(std::sync::Mutex::new(None)),
    );
    attach_host(&mut other);
    frame(&ctx, &mut other, 0.0, Vec::new());

    assert!(
        !other.control(0).enabled,
        "opening the editor must not overwrite a restored flat session"
    );
}

#[test]
fn preset_search_filters() {
    let (ctx, mut app) = editor_with_store(temp_store("search"));

    let found = texts(&ctx, &mut app, 0.0);
    let label = find(&found, "Default");
    click(&ctx, &mut app, 0.1, label.center());

    app.preset.search = "200".to_owned();
    let found = texts(&ctx, &mut app, 2.0);
    find(&found, "200-LowCut");
    find(&found, "RESULTS");
    assert!(
        !names(&found).contains(&"Auto-LowCut"),
        "only the matching preset is listed: {:?}",
        names(&found)
    );

    app.preset.search.clear();
    let found = texts(&ctx, &mut app, 2.4);
    find(&found, "Auto-LowCut");
    find(&found, "PRESETS");
}

#[test]
fn preset_switch_replaces_state() {
    let (ctx, mut app) = editor_with_store(temp_store("switch"));

    let found = texts(&ctx, &mut app, 0.0);
    click(&ctx, &mut app, 0.1, find(&found, "Default").center());

    let found = texts(&ctx, &mut app, 2.0);
    click(&ctx, &mut app, 2.1, find(&found, "Auto-LowCut").center());
    assert!(app.control(0).enabled, "low cut is on");
    assert!(app.params.is_placed(0));

    let setter = app.gui.as_ref().map(|gui| gui.param_setter());

    if let Some(setter) = setter {
        let params = Arc::clone(&app.params);
        app.set(&setter, &params.regions[0].enabled, true);
        app.set(&setter, &params.regions[0].low, 300.0);
    }

    assert!(app.params.regions[0].enabled.value());

    let found = texts(&ctx, &mut app, 2.4);
    click(&ctx, &mut app, 2.5, find(&found, "Default").center());

    assert!(!app.control(0).enabled, "low cut is gone");
    assert!(!app.params.is_placed(0), "dot is cleared");
    assert!(!app.params.regions[0].enabled.value());
    assert!((app.params.regions[0].low.value() - 500.0).abs() < 1.0);
}

#[test]
fn preset_save_selects_entry() {
    let store = temp_store("save");
    let (ctx, mut app) = editor_with_store(store.clone());

    let found = texts(&ctx, &mut app, 0.0);
    let label = find(&found, "Default");
    click(&ctx, &mut app, 0.1, label.center());

    app.preset.save = Some(crate::editor::presets::save_draft("Mine", "Factory"));
    let found = texts(&ctx, &mut app, 2.0);
    find(&found, "SAVE PRESET");
    find(&found, "SAVE TO");

    let save = find(&found, "Save");
    click(&ctx, &mut app, 2.1, save.center());

    let entries = store.entries_in("Factory");
    assert!(
        entries.iter().any(|entry| entry.name == "Mine"),
        "{entries:?}"
    );
    assert_eq!(
        app.preset.current.as_ref().map(|entry| entry.name.as_str()),
        Some("Mine")
    );
    assert!(app.preset.save.is_none());
}

#[test]
fn preset_save_on_enter() {
    let store = temp_store("save-enter");
    let (ctx, mut app) = editor_with_store(store.clone());

    let found = texts(&ctx, &mut app, 0.0);
    click(&ctx, &mut app, 0.1, find(&found, "Default").center());

    app.preset.save = Some(crate::editor::presets::save_draft("ByEnter", "User"));
    let found = texts(&ctx, &mut app, 2.0);
    let field = find(&found, "ByEnter");
    click(&ctx, &mut app, 2.1, field.center());

    frame(
        &ctx,
        &mut app,
        2.2,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );

    assert!(store.entries_in("User").iter().any(|e| e.name == "ByEnter"));
    assert!(app.preset.save.is_none());
}

#[test]
fn preset_step_empty_folder() {
    let (ctx, mut app) = editor_with_store(temp_store("step-empty"));

    let found = texts(&ctx, &mut app, 0.0);
    click(&ctx, &mut app, 0.1, find(&found, "Default").center());

    let found = texts(&ctx, &mut app, 2.0);
    click(&ctx, &mut app, 2.1, find(&found, "User").center());

    let mid = CANVAS_H - Layout::FOOTER * 0.5;
    let right = CANVAS_W - 14.0 - 8.0 - 49.0 - 10.0;
    let next = Pos2::new(right - 8.0, mid);
    click(&ctx, &mut app, 2.3, next);

    assert_eq!(
        app.preset.current.as_ref().map(|entry| entry.name.as_str()),
        Some("200-LowCut")
    );
}
