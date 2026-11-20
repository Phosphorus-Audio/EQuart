mod filter_controls;
mod modifier_interactions;
mod objects;
mod selection;

use super::*;
use crate::filter::{CutSlope, FilterKind};
use std::sync::Arc;

/// run one frame, egui needs move->press->release on separate frames for a click
fn frame(ctx: &egui::Context, editor: &mut EquartEditor, time: f64, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, CANVAS)),
        time: Some(time),
        focused: true,
        events,
        ..Default::default()
    };

    let mut output = ctx.run_ui(input, |ui| editor.draw(ui));
    // no renderer in tests
    output.textures_delta.clear();
}

fn click(ctx: &egui::Context, editor: &mut EquartEditor, time: f64, pos: Pos2) {
    click_button(ctx, editor, time, pos, egui::PointerButton::Primary);
}

fn click_button(
    ctx: &egui::Context,
    editor: &mut EquartEditor,
    time: f64,
    pos: Pos2,
    button: egui::PointerButton,
) {
    let button = |pressed: bool| egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::default(),
    };

    frame(ctx, editor, time, Vec::new());
    frame(
        ctx,
        editor,
        time + 0.01,
        vec![egui::Event::PointerMoved(pos)],
    );
    frame(ctx, editor, time + 0.02, vec![button(true)]);
    frame(ctx, editor, time + 0.03, vec![button(false)]);
}

/// count drawn band dots (8px dark grey circles)
fn marker_count(ctx: &egui::Context, editor: &mut EquartEditor, time: f64) -> usize {
    fn walk(shape: &egui::Shape, count: &mut usize) {
        match shape {
            egui::Shape::Circle(circle) => {
                if (circle.radius - 8.0).abs() < 0.01 && circle.fill == Color32::from_gray(27) {
                    *count += 1;
                }
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, count)),
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
    // no renderer in tests
    output.textures_delta.clear();

    let mut count = 0;
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut count);
    }
    count
}

/// dummy host that writes param changes right back
struct TestHost;

impl GuiContextInner for TestHost {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Standalone
    }

    unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {}

    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
        // SAFETY: test pointers are valid and everything runs on the test thread
        unsafe { param._internal_set_normalized_value(normalized) };
    }

    unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {}

    fn get_state(&self) -> PluginState {
        PluginState {
            version: String::new(),
            params: Default::default(),
            fields: Default::default(),
        }
    }

    fn set_state(&self, _: PluginState) {}
    fn request_restart(&self) {}
}

fn attach_host(editor: &mut EquartEditor) {
    editor.gui = Some(GuiContext::new(Arc::new(TestHost)));
}

/// enable band 0 with given filter kind
fn one_band(editor: &mut EquartEditor, kind: FilterKind) {
    let params = Arc::get_mut(&mut editor.params).unwrap();
    params.bands[0].enabled = BoolParam::new("Enabled", true);
    params.bands[0].kind = EnumParam::new("Type", kind);
}

fn one_bell(editor: &mut EquartEditor) {
    one_band(editor, FilterKind::Peak);
}

/// hover pos and scroll wheel 1 notch
fn wheel_at(ctx: &egui::Context, editor: &mut EquartEditor, time: f64, pos: Pos2, down: bool) {
    let event = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, if down { -1.0 } else { 1.0 }),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::default(),
    };
    frame(ctx, editor, time, Vec::new());
    frame(
        ctx,
        editor,
        time + 0.01,
        vec![egui::Event::PointerMoved(pos)],
    );
    frame(ctx, editor, time + 0.02, vec![event]);
}

fn editor() -> (egui::Context, EquartEditor) {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let editor = EquartEditor::new(
        Arc::new(crate::params::EquartParams::default()),
        Arc::new(crate::Telemetry::default()),
    );
    (ctx, editor)
}

fn axis_outer() -> Rect {
    Layout::plot_outer(Rect::from_min_size(Pos2::ZERO, CANVAS), true)
}

fn axis_plot() -> spectrum::Plot {
    spectrum::Plot::new(axis_outer(), 48_000.0)
}

/// pos on keyboard strip for given hz
fn key_at(hz: f32) -> Pos2 {
    Pos2::new(
        axis_plot().x(hz),
        spectrum::keys_strip(axis_outer()).center().y,
    )
}

fn node(editor: &EquartEditor, band: usize) -> Pos2 {
    let control = editor.control(band);
    let plot = axis_plot();
    // cut handle y depends on Q, others use gain
    let y = if control.kind.is_cut() {
        spectrum::cut_handle_y(&plot, spectrum::cut_handle_q(&control))
    } else {
        plot.y(control.gain)
    };
    Pos2::new(plot.x(control.frequency), y)
}

/// button pos relative to panel top-left
fn panel_button(x: f32, y: f32) -> Pos2 {
    let canvas = Rect::from_min_size(Pos2::ZERO, CANVAS);
    Layout::panel(canvas, canvas.center().x).min + Vec2::new(x, y)
}

fn primary(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}

/// press and drag without releasing (needs 2 move frames for egui)
fn press_and_drag(ctx: &egui::Context, editor: &mut EquartEditor, time: f64, from: Pos2, to: Pos2) {
    frame(ctx, editor, time, Vec::new());
    frame(
        ctx,
        editor,
        time + 0.01,
        vec![egui::Event::PointerMoved(from)],
    );
    frame(ctx, editor, time + 0.02, vec![primary(from, true)]);
    frame(
        ctx,
        editor,
        time + 0.03,
        vec![egui::Event::PointerMoved(to)],
    );
    frame(
        ctx,
        editor,
        time + 0.04,
        vec![egui::Event::PointerMoved(to)],
    );
}

fn drag(ctx: &egui::Context, editor: &mut EquartEditor, time: f64, from: Pos2, to: Pos2) {
    press_and_drag(ctx, editor, time, from, to);
    frame(ctx, editor, time + 0.05, vec![primary(to, false)]);
}

fn three_bells(editor: &mut EquartEditor) {
    let params = Arc::get_mut(&mut editor.params).unwrap();
    for (band, hz) in [(0usize, 200.0), (1, 1_500.0), (2, 12_000.0)] {
        let band = &mut params.bands[band];
        band.enabled = BoolParam::new("Enabled", true);
        band.frequency = FloatParam::new(
            "Frequency",
            hz,
            FloatRange::Logarithmic {
                min: 10.0,
                max: 30_000.0,
            },
        );
        band.gain = FloatParam::new(
            "Gain",
            0.0,
            FloatRange::Linear {
                min: -30.0,
                max: 30.0,
            },
        );
    }
}
