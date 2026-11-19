use egui::{Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
use nice_plug::{context::gui::ParamSetter, prelude::*};
use std::f32::consts::{PI, TAU};

pub const RED: Color32 = Color32::from_rgb(201, 55, 65);

pub const BODY_FILL: f32 = 0.87;

pub const LABEL_GAP: f32 = 26.0;

mod glow;
mod input;

#[derive(Clone, Copy, Default)]
struct Motion {
    from: f32,
    target: f32,
    started: f64,
}

impl Motion {
    fn sample(&mut self, now: f64, active: bool) -> f32 {
        let t = ((now - self.started) / 0.95).clamp(0.0, 1.0) as f32;
        let value = self.from + (self.target - self.from) * (1.0 - (1.0 - t).powi(5));

        let target = f32::from(active);

        if target != self.target {
            self.from = value;
            self.target = target;
            self.started = now;
        }

        value
    }

    fn moving(&self, now: f64) -> bool {
        self.from != self.target && now - self.started < 0.95
    }
}

#[derive(Clone, Copy)]
struct ValueMotion {
    binding: usize,
    from: f32,
    target: f32,
    started: f64,
    switching: bool,
}

impl ValueMotion {
    fn value(&self, now: f64) -> f32 {
        let t = ((now - self.started) / 0.75).clamp(0.0, 1.0) as f32;
        self.from + (self.target - self.from) * (1.0 - (1.0 - t).powi(5))
    }

    fn update(&mut self, binding: usize, target: f32, now: f64, editing: bool) -> f32 {
        if self.binding != binding {
            // Retarget from the visible position, including during a previous transition
            self.from = self.value(now);
            self.binding = binding;
            self.started = now;
            self.switching = true;
        } else if !self.switching || editing {
            self.from = target;
            self.switching = false;
        }

        self.target = target;

        if now - self.started >= 0.75 {
            self.switching = false;
            self.from = target;
        }

        if self.switching {
            self.value(now)
        } else {
            target
        }
    }
}

fn mix(a: Color32, b: Color32, amount: f32) -> Color32 {
    let channel = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * amount) as u8;
    Color32::from_rgb(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
    )
}

pub(super) fn light_colors(accent: Color32) -> (Color32, Color32) {
    let mut luminous = egui::ecolor::Hsva::from(accent);
    luminous.v = 1.0;
    luminous.s = (luminous.s * 1.1).min(1.0);
    let glow = Color32::from(luminous);
    (glow, mix(glow, Color32::WHITE, 0.22))
}

pub fn gradient_rect(p: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();

    for (pos, color) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(mesh));
}

fn disc(p: &egui::Painter, center: Pos2, radius: f32, top: Color32, bottom: Color32) {
    let scale = p.pixels_per_point();
    let radius = (radius * scale).round() / scale;
    let feather = 0.5 / scale;
    let segments = (8.0 * (radius * scale).sqrt()).ceil() as u32;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, mix(top, bottom, 0.5));

    for i in 0..=segments {
        let direction = Vec2::angled(i as f32 / segments as f32 * TAU);
        let light = (direction.y + 1.0) * 0.5;
        mesh.colored_vertex(
            center + direction * (radius - feather),
            mix(top, bottom, light),
        );
        mesh.colored_vertex(
            center + direction * (radius + feather),
            Color32::TRANSPARENT,
        );

        if i > 0 {
            let previous = 1 + (i - 1) * 2;
            let current = 1 + i * 2;
            mesh.add_triangle(0, previous, current);
            // keeping raw meshes antialiased
            mesh.add_triangle(previous, previous + 1, current);
            mesh.add_triangle(previous + 1, current + 1, current);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

fn arc(p: &egui::Painter, center: Pos2, radius: f32, from: f32, to: f32, stroke: Stroke) {
    if from == to {
        return;
    }

    let points = (0..=64)
        .map(|i| {
            let value = from + (to - from) * i as f32 / 64.0;
            center + Vec2::angled(PI * (0.75 + 1.5 * value)) * radius
        })
        .collect();
    p.add(egui::Shape::line(points, stroke));
}

// copy from somewhere
pub(crate) fn gradient_rounded(
    p: &egui::Painter,
    rect: Rect,
    radius: f32,
    top: Color32,
    bottom: Color32,
) {
    let at = |y: f32| {
        let t = ((y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
        mix(top, bottom, t)
    };

    let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), at(rect.center().y));

    let corners = [
        (Pos2::new(rect.left() + r, rect.top() + r), PI),
        (Pos2::new(rect.right() - r, rect.top() + r), 1.5 * PI),
        (Pos2::new(rect.right() - r, rect.bottom() - r), 0.0),
        (Pos2::new(rect.left() + r, rect.bottom() - r), 0.5 * PI),
    ];
    let mut outline = Vec::with_capacity(28);
    for (center, start) in corners {
        for step in 0..=6 {
            let angle = start + 0.5 * PI * step as f32 / 6.0;
            outline.push(center + Vec2::angled(angle) * r);
        }
    }

    for &point in &outline {
        mesh.colored_vertex(point, at(point.y));
    }

    let n = outline.len() as u32;
    for i in 1..n {
        mesh.add_triangle(0, i, i + 1);
    }
    mesh.add_triangle(0, n, 1);

    p.add(egui::Shape::mesh(mesh));
}

pub(crate) struct KnobMeter {
    pub level: Option<f32>,
    pub gain: f32,
}

pub(crate) enum KnobMode {
    Editable,
    Metered(KnobMeter),
    GainOffset(f32),
    ReadOnly(f32),
}

pub(crate) struct KnobAppearance<'a> {
    pub center: Pos2,
    pub radius: f32,
    pub grow: f32,
    pub body_fill: f32,
    pub label_gap: f32,
    pub label: &'a str,
    pub bipolar: bool,
}

pub(crate) fn knob(
    ui: &mut Ui,
    param: &FloatParam,
    setter: &ParamSetter<'_>,
    appearance: KnobAppearance<'_>,
    mode: KnobMode,
) -> Response {
    let KnobAppearance {
        center,
        radius,
        grow,
        body_fill,
        label_gap,
        label,
        bipolar,
    } = appearance;
    let id = ui.make_persistent_id(("knob", label));
    let read_only = matches!(mode, KnobMode::ReadOnly(_));

    let scale = radius / 35.0;
    let grown = radius * grow;
    let gscale = grown / 35.0;

    let rect = Rect::from_center_size(center, Vec2::splat((grown + 15.0 * gscale) * 2.0));

    let response = if read_only {
        let response = ui.interact(rect, id, Sense::hover());
        response.surrender_focus();
        response
    } else {
        ui.interact(rect, id, Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Grab)
    };

    // i dont want to support the wayland or x11 and i have not mac
    #[cfg(target_os = "windows")]
    if !read_only && response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down())
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    }

    if !read_only && response.clicked() {
        response.request_focus();
    }

    if !read_only && response.drag_started_by(egui::PointerButton::Primary) {
        setter.begin_set_parameter(param);
        ui.data_mut(|data| data.insert_temp(id, param.unmodulated_normalized_value()));
    }

    if !read_only && response.dragged_by(egui::PointerButton::Primary) {
        let sensitivity = if ui.input(|i| i.modifiers.shift) {
            0.0004
        } else {
            0.004
        };

        let delta = response.drag_delta();
        let previous = ui
            .data(|data| data.get_temp::<f32>(id))
            .expect("Knob drag started without a parameter gesture");
        let value = (previous + (delta.x - delta.y) * sensitivity).clamp(0.0, 1.0);
        ui.data_mut(|data| data.insert_temp(id, value));
        setter.set_parameter(param, param.preview_plain(value));
    }

    if (read_only || response.drag_stopped_by(egui::PointerButton::Primary))
        && ui.data_mut(|data| data.remove_temp::<f32>(id)).is_some()
    {
        setter.end_set_parameter(param);
    }

    if !read_only
        && (response.double_clicked() || (response.clicked() && ui.input(|i| i.modifiers.command)))
    {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, param.default_plain_value());
        setter.end_set_parameter(param);
    }

    let wheel = if !read_only && response.hovered() {
        ui.input(|i| i.smooth_scroll_delta.y)
    } else {
        0.0
    };

    let key = if !read_only && response.has_focus() {
        ui.input(|i| {
            f32::from(i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight))
                - f32::from(
                    i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft),
                )
        })
    } else {
        0.0
    };

    if wheel != 0.0 || key != 0.0 {
        let fine = if ui.input(|i| i.modifiers.shift) {
            0.1
        } else {
            1.0
        };

        let value = (param.unmodulated_normalized_value() + (wheel * 0.001 + key * 0.01) * fine)
            .clamp(0.0, 1.0);
        setter.begin_set_parameter(param);
        setter.set_parameter(param, param.preview_plain(value));
        setter.end_set_parameter(param);
    }

    let entered_value = input::edit_value(ui, &response, param, setter, label, read_only);
    let target = match mode {
        KnobMode::ReadOnly(hz) => param.preview_normalized(hz),
        _ => param.modulated_normalized_value(),
    };
    let now = ui.input(|i| i.time);
    let binding = param as *const FloatParam as usize;
    let editing = entered_value
        || response.dragged()
        || response.double_clicked()
        || (response.clicked() && ui.input(|i| i.modifiers.command))
        || wheel != 0.0
        || key != 0.0;

    let (animated_value, switching) = ui.data_mut(|data| {
        let motion_id = id.with("value-transition");
        let mut motion = data
            .get_temp::<ValueMotion>(motion_id)
            .unwrap_or(ValueMotion {
                binding,
                from: target,
                target,
                started: now,
                switching: false,
            });

        let value = motion.update(binding, target, now, editing || read_only);
        data.insert_temp(motion_id, motion);
        (value, motion.switching)
    });
    let value = if read_only { target } else { animated_value };

    //lets lighter
    let pointer = if ui.is_enabled() {
        Color32::from_gray(238)
    } else {
        Color32::from_gray(96)
    };

    let accent = if ui.is_enabled() {
        ui.visuals().selection.stroke.color
    } else {
        Color32::from_gray(163)
    };

    let (hover, press, moving) = ui.data_mut(|data| {
        let motion_id = id.with("motion");
        let mut motion = data.get_temp::<[Motion; 2]>(motion_id).unwrap_or_default();
        let hover = motion[0].sample(now, !read_only && ui.is_enabled() && response.hovered());
        let press = motion[1].sample(
            now,
            !read_only && ui.is_enabled() && response.is_pointer_button_down_on(),
        );
        let moving = motion.iter().any(|m| m.moving(now));
        data.insert_temp(motion_id, motion);
        (hover, press, moving)
    });

    if moving || switching {
        ui.ctx().request_repaint();
    }

    let p = ui.painter();
    let body_radius = grown * body_fill;
    let body_center = center + Vec2::new(0.0, press * 0.8 * gscale);
    let ring = body_radius + 2.2 * gscale;
    let blur = ((11.0 + hover * 7.0) * (1.0 - press) + 4.0 * press) * gscale;

    for i in (0..16).rev() {
        let t = i as f32 / 15.0;
        p.circle_filled(
            body_center
                + Vec2::new(1.5, (5.0 + hover * 2.0) * (1.0 - press) + 2.0 * press) * gscale,
            body_radius + t * blur,
            Color32::from_black_alpha((2.0 + 6.0 * (1.0 - t).powi(2)) as u8),
        );
    }

    let caption = center - Vec2::new(0.0, radius + label_gap * scale);
    let font = FontId::proportional((15.0 * scale).max(9.0));
    crate::editor::menu::label_plate(
        ui,
        label,
        font.clone(),
        caption,
        egui::Align2::CENTER_CENTER,
    );

    p.text(
        caption,
        egui::Align2::CENTER_CENTER,
        label,
        font,
        Color32::from_gray(158),
    );

    for i in 0..=10 {
        let angle = PI * (0.75 + 1.5 * i as f32 / 10.0);
        let direction = Vec2::angled(angle);
        let inner = grown + 11.0 * gscale;
        let outer = grown + if i % 5 == 0 { 16.0 } else { 13.5 } * gscale;

        p.line_segment(
            [center + direction * inner, center + direction * outer],
            Stroke::new(1.0, Color32::from_gray(112)),
        );
    }

    let start = if bipolar {
        param.preview_normalized(0.0)
    } else {
        0.0
    };

    let (glow, core) = light_colors(accent);

    let ring_end = match &mode {
        KnobMode::Metered(meter) => meter.level.map(|level| param.preview_normalized(level)),
        _ => Some(value),
    };


    if let Some(end) = ring_end {
        if press > 0.001 {
            glow::arc(p, body_center, ring, [start, end], gscale, glow, press);
        }
        arc(
            p,
            body_center,
            ring,
            start,
            end,
            Stroke::new(3.0 * gscale, mix(accent, core, press)),
        );
    }
    let gain_end = match mode {
        KnobMode::Metered(meter) => Some(value + meter.gain / 48.0),
        KnobMode::GainOffset(offset) if offset != 0.0 => {
            Some(param.preview_normalized(param.preview_plain(value) + offset))
        }
        _ => None,
    };
    if let Some(end) = gain_end {
        arc(
            p,
            body_center,
            ring + 3.25 * gscale,
            value,
            end.clamp(0.0, 1.0),
            Stroke::new(2.5 * gscale, Color32::from_gray(215)),
        );
    }

    let dim = if ui.is_enabled() { 0.0 } else { 0.45 };
    let metal = |gray: u8| {
        let lit = mix(
            Color32::from_gray(gray),
            Color32::from_gray(74),
            hover * 0.30,
        );
        let pressed = mix(lit, Color32::from_gray(0), press * 0.35);
        mix(pressed, Color32::from_gray(30), dim)
    };

    // lay
    disc(p, body_center, body_radius + gscale, metal(30), metal(6));
    disc(
        p,
        body_center,
        body_radius - 0.5 * gscale,
        metal(8),
        metal(26),
    );
    disc(
        p,
        body_center,
        body_radius - 2.0 * gscale,
        metal(26),
        metal(11),
    );
    disc(
        p,
        body_center - Vec2::new(0.0, 0.8 * gscale),
        body_radius - 4.0 * gscale,
        metal(25),
        metal(13),
    );
    // out

    let direction = Vec2::angled(PI * (0.75 + 1.5 * value));
    let a = body_center + direction * (body_radius * 0.53);
    let b = body_center + direction * (body_radius * 0.82);

    p.line_segment(
        [
            a + Vec2::new(0.7, 0.8) * gscale,
            b + Vec2::new(0.7, 0.8) * gscale,
        ],
        Stroke::new(4.6 * gscale, Color32::from_gray(0)),
    );
    p.line_segment([a, b], Stroke::new(3.6 * gscale, pointer));

    if response.has_focus() {
        p.circle_stroke(
            center,
            grown + 21.0 * gscale,
            Stroke::new(1.0, accent.gamma_multiply(0.6)),
        );
    }

    let display = input::value_text(param, value);
    let readout = center + Vec2::new(0.0, radius + 27.0 * scale);
    let font = FontId::monospace((13.0 * scale).max(9.0));
    crate::editor::menu::label_plate(
        ui,
        &display,
        font.clone(),
        readout,
        egui::Align2::CENTER_CENTER,
    );
    p.text(
        readout,
        egui::Align2::CENTER_CENTER,
        display,
        font,
        if ui.is_enabled() {
            Color32::from_gray(128)
        } else {
            Color32::from_gray(151)
        },
    );

    response
}
