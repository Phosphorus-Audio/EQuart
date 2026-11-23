//! Custom Pro-Q style popup cards for footer icons

use super::*;

mod dropdown;

pub(crate) use dropdown::Dropdown;

const ROW: f32 = 26.0;
const LABEL_SIZE: f32 = 12.5;
const VALUE_SIZE: f32 = 11.0;

pub(crate) fn card(ctx: &egui::Context) -> egui::Frame {
    let panel = theme::current(ctx).panel;

    egui::Frame::new()
        .fill(panel.menu)
        .stroke(Stroke::new(1.0, panel.border))
        .corner_radius(egui::CornerRadius::same(9))
        .inner_margin(egui::Margin::same(7))
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 22,
            spread: 0,
            color: Color32::from_black_alpha(110),
        })
}

pub(crate) fn body(ui: &mut egui::Ui) {
    let spacing = &mut ui.spacing_mut();
    spacing.item_spacing.y = 1.0;
    spacing.button_padding = Vec2::new(8.0, 5.0);
}

/// small grey caps group header
pub(crate) fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(3.0);
    // don't allocate so layout won't shift
    let font = FontId::new(10.0, egui::FontFamily::Name(GLOBAL_FONT.into()));
    let anchor = ui.cursor().min + Vec2::new(0.0, 7.0);
    label_plate(ui, text, font, anchor, egui::Align2::LEFT_CENTER);
    ui.label(
        egui::RichText::new(text)
            .size(10.0)
            .color(theme::current(ui.ctx()).text.heading)
            .family(egui::FontFamily::Name(GLOBAL_FONT.into())),
    );
    ui.add_space(4.0);
}

pub(crate) fn divider(ui: &mut egui::Ui) {
    ui.add_space(5.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, theme::current(ui.ctx()).panel.border),
    );
    ui.add_space(5.0);
}

/// menu item with optional right value and check mark, returns true on click
pub(crate) fn row(
    ui: &mut egui::Ui,
    label: &str,
    value: Option<&str>,
    ticked: bool,
    accent: Color32,
) -> bool {
    row_response(ui, label, value, ticked, accent).clicked()
}

pub(super) fn row_response(
    ui: &mut egui::Ui,
    label: &str,
    value: Option<&str>,
    ticked: bool,
    accent: Color32,
) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    let painter = ui.painter();
    let palette = theme::current(ui.ctx());
    let hover = hover_amount(&response);
    if ticked {
        painter.rect_filled(rect, 5.0, palette.panel.selected);
    } else if hover > 0.0 {
        painter.rect_filled(rect, 5.0, fade(palette.panel.hover, hover));
    }

    let anchor = rect.left_center() + Vec2::new(9.0, 0.0);
    label_plate(
        ui,
        label,
        FontId::proportional(LABEL_SIZE),
        anchor,
        egui::Align2::LEFT_CENTER,
    );
    painter.text(
        anchor,
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(LABEL_SIZE),
        if ticked { palette.text.primary } else { palette.text.body },
    );

    if let Some(value) = value {
        let anchor = rect.right_center() - Vec2::new(24.0, 0.0);
        label_plate(
            ui,
            value,
            FontId::proportional(VALUE_SIZE),
            anchor,
            egui::Align2::RIGHT_CENTER,
        );
        painter.text(
            anchor,
            egui::Align2::RIGHT_CENTER,
            value,
            FontId::proportional(VALUE_SIZE),
            palette.text.value,
        );
    }

    if ticked {
        // draw tick manually, no icon font
        let tip = Pos2::new(rect.right() - 11.0, rect.center().y);
        painter.line_segment(
            [tip + Vec2::new(-4.0, 0.0), tip + Vec2::new(-1.0, 3.0)],
            Stroke::new(1.4, accent),
        );
        painter.line_segment(
            [tip + Vec2::new(-1.0, 3.0), tip + Vec2::new(4.0, -4.0)],
            Stroke::new(1.4, accent),
        );
    }

    response
}

pub(crate) fn note(ui: &mut egui::Ui, text: &str) {
    let height = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let anchor = rect.left_center() + Vec2::new(9.0, 0.0);
    let font = FontId::proportional(10.5);
    label_plate(ui, text, font.clone(), anchor, egui::Align2::LEFT_CENTER);
    ui.painter().text(
        anchor,
        egui::Align2::LEFT_CENTER,
        text,
        font,
        theme::current(ui.ctx()).text.heading,
    );
}

/// dropdown keys shared between settings card and axis menu
pub(crate) const COMBO_RESOLUTION: u32 = 1;
pub(crate) const COMBO_SPEED: u32 = 2;
pub(crate) const COMBO_SMOOTHING: u32 = 3;
pub(crate) const COMBO_OVERSAMPLING: u32 = 4;
pub(crate) const COMBO_TILT: u32 = 5;
pub(crate) const COMBO_HOLD: u32 = 6;
pub(crate) const COMBO_PHASE: u32 = 7;
pub(crate) const COMBO_PROCESSING: u32 = 8;
pub(crate) const COMBO_LINEAR_QUALITY: u32 = 9;

const COMBO_CAPTION_H: f32 = 14.0;
pub(crate) const COMBO_BUTTON_H: f32 = 24.0;
/// gap above dropdowns (except first after section header)
const COMBO_LEAD: f32 = 10.0;
const OPTION_H: f32 = 23.0;
const SWITCH_W: f32 = 32.0;
const SWITCH_H: f32 = 17.0;

#[allow(clippy::too_many_arguments)]
fn paint_combo_button(
    ui: &egui::Ui,
    response: &Response,
    value: &str,
    open: bool,
    available: bool,
    accent: Color32,
    flash: bool,
    colors: &crate::display_preferences::theme::DropdownColors,
) {
    let rect = response.rect;
    let p = ui.painter();
    let hover = hover_amount(response);

    let fill = if !available {
        colors.background.gamma_multiply(0.6)
    } else if open {
        colors.selected
    } else {
        colors.background.lerp_to_gamma(colors.hover, hover)
    };

    p.rect_filled(rect, 6.0, fill);
    p.rect_stroke(
        rect,
        6.0,
        Stroke::new(
            1.0,
            if open {
                accent.gamma_multiply(0.85)
            } else {
                colors.border.gamma_multiply(0.6)
            },
        ),
        egui::StrokeKind::Inside,
    );

    let anchor = rect.left_center() + Vec2::new(10.0, 0.0);
    let font = FontId::proportional(11.0);

    if flash {
        label_plate(ui, value, font.clone(), anchor, egui::Align2::LEFT_CENTER);
    }

    p.text(
        anchor,
        egui::Align2::LEFT_CENTER,
        value,
        font,
        if available {
            colors.text
        } else {
            colors.text.gamma_multiply(0.55)
        },
    );

    if available {
        let tip = Pos2::new(rect.right() - 14.0, rect.center().y);
        let dir = if open { -1.0 } else { 1.0 };

        p.line_segment(
            [tip + Vec2::new(-4.0, -2.0 * dir), tip],
            Stroke::new(1.2, colors.arrow),
        );
        p.line_segment(
            [tip, tip + Vec2::new(4.0, -2.0 * dir)],
            Stroke::new(1.2, colors.arrow),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_option(
    p: &egui::Painter,
    rect: Rect,
    label: &str,
    chosen: bool,
    hover: f32,
    accent: Color32,
    opacity: f32,
    flash: Option<LabelFlash>,
    colors: &crate::display_preferences::theme::DropdownColors,
) {
    if chosen {
        p.rect_filled(rect, 5.0, fade(colors.selected, opacity));
    } else if hover > 0.0 {
        p.rect_filled(rect, 5.0, fade(colors.hover, opacity * hover));
    }

    let anchor = rect.left_center() + Vec2::new(9.0, 0.0);
    let font = FontId::proportional(11.5);

    if let Some(flash) = flash {
        flash.paint(p, label, font.clone(), anchor, egui::Align2::LEFT_CENTER);
    }

    p.text(
        anchor,
        egui::Align2::LEFT_CENTER,
        label,
        font,
        fade(colors.text, opacity),
    );

    if chosen {
        let tip = Pos2::new(rect.right() - 11.0, rect.center().y);
        let stroke = Stroke::new(1.4, fade(accent, opacity));

        p.line_segment(
            [tip + Vec2::new(-4.0, 0.0), tip + Vec2::new(-1.0, 3.0)],
            stroke,
        );
        p.line_segment(
            [tip + Vec2::new(-1.0, 3.0), tip + Vec2::new(4.0, -4.0)],
            stroke,
        );
    }
}

pub(super) fn wheel_choice<T: Copy + PartialEq>(
    response: &Response,
    closed: bool,
    current: T,
    choices: &[T],
) -> Option<T> {
    let current = choices.iter().position(|choice| *choice == current);
    let index = dropdown::wheel_index(response, closed, current, choices.len())?;

    Some(choices[index])
}

/// value button with popup
#[allow(clippy::too_many_arguments)]
pub(crate) fn combo_popup<T: Copy + PartialEq, L: AsRef<str>>(
    ui: &mut egui::Ui,
    id: egui::Id,
    rect: Rect,
    current: T,
    choices: &[T],
    label: impl Fn(T) -> L,
    accent: Color32,
    life: &mut controls::CardLife,
) -> Option<T> {
    let available = !choices.is_empty();
    let sense = if available {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    let colors = theme::current(ui.ctx()).panel_dropdown;
    let value = label(current);

    if !available {
        life.shut();
        paint_combo_button(ui, &response, value.as_ref(), false, false, accent, false, &colors);
        return None;
    }

    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let now = ui.input(|i| i.time);
    let stepped = wheel_choice(
        &response,
        !life.is_shown() && !response.clicked(),
        current,
        choices,
    );

    if response.clicked() {
        if life.is_up() {
            life.release(now);
        } else {
            life.open(now);
        }
    }

    paint_combo_button(
        ui,
        &response,
        value.as_ref(),
        life.is_up(),
        true,
        accent,
        false,
        &colors,
    );

    let mut frame = card(ui.ctx())
        .fill(colors.popup)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(8)
        .inner_margin(egui::Margin::same(5));
    frame.shadow.blur = 16;

    let popup = egui::Popup::menu(&response)
        .open(true)
        .gap(4.0)
        .frame(frame)
        .width(rect.width());
    let mut picked = None;

    let dismissed = EquartEditor::animated_card(popup, life, now, None, |ui, _, flash, fall| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let flash = fall
            .is_none()
            .then(|| LabelFlash::new(ui.max_rect(), flash));

        for &choice in choices {
            let (row, response) = ui.allocate_exact_size(
                Vec2::new(rect.width() - 12.0, OPTION_H - 1.0),
                Sense::click(),
            );
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

            paint_option(
                ui.painter(),
                row,
                label(choice).as_ref(),
                choice == current,
                hover_amount(&response),
                accent,
                1.0,
                flash,
                &colors,
            );

            if fall.is_none() && response.clicked() {
                picked = Some(choice);
            }
        }

        picked.is_some()
    });
    if dismissed && !response.clicked() {
        life.release(now);
        ui.ctx().request_repaint();
    }
    picked.or(stepped)
}

/// fade premultiplied color
pub(crate) fn fade(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha.clamp(0.0, 1.0))
}

/// white reveal plates behind menu text when opening
#[derive(Clone, Copy)]
pub(crate) struct LabelFlash {
    t: f32,
    top: f32,
    height: f32,
    mid_x: f32,
}

impl LabelFlash {
    fn id(layer: egui::LayerId) -> egui::Id {
        layer.id.with(("equart-label-flash", layer.order))
    }

    fn new(card: Rect, t: f32) -> Self {
        Self {
            t,
            top: card.top(),
            height: card.height(),
            mid_x: card.center().x,
        }
    }

    pub(crate) fn begin(ui: &egui::Ui, card: Rect, t: f32) {
        ui.data_mut(|d| d.insert_temp(Self::id(ui.layer_id()), Self::new(card, t)));
    }

    /// plate alpha, cascades down and slightly right-lagged
    fn alpha(self, anchor: Pos2) -> f32 {
        let along = ((anchor.y - self.top) / self.height.max(1.0)).clamp(0.0, 1.0);
        let column_lag = if anchor.x > self.mid_x { 0.10 } else { 0.0 };
        let local = ((self.t - along * 0.34 - column_lag) / 0.56).clamp(0.0, 1.0);
        // smoothstep fade
        1.0 - local * local * (3.0 - 2.0 * local)
    }

    fn paint(self, p: &egui::Painter, text: &str, font: FontId, anchor: Pos2, align: egui::Align2) {
        let alpha = self.alpha(anchor);
        if alpha <= 0.003 {
            return;
        }
        let size = p
            .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
            .size();
        let plate = align.anchor_size(anchor, size).expand2(Vec2::new(3.0, 1.5));
        p.rect_filled(plate, 3.0, Color32::from_white_alpha((alpha * 255.0) as u8));
    }
}

/// draw white reveal plate behind text (call before drawing text)
pub(crate) fn label_plate(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    anchor: Pos2,
    align: egui::Align2,
) {
    label_plate_on(ui.painter(), text, font, anchor, align);
}

pub(crate) fn label_plate_on(
    p: &egui::Painter,
    text: &str,
    font: FontId,
    anchor: Pos2,
    align: egui::Align2,
) {
    let Some(flash) = p
        .ctx()
        .data(|d| d.get_temp::<LabelFlash>(LabelFlash::id(p.layer_id())))
    else {
        return;
    };
    flash.paint(p, text, font, anchor, align);
}

/// animated toggle switch
pub(crate) fn switch(
    ui: &mut egui::Ui,
    id: egui::Id,
    label: &str,
    value: bool,
    accent: Color32,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t =
        ui.ctx()
            .animate_bool_with_time_and_easing(id, value, 0.22, egui::emath::easing::cubic_out);

    let p = ui.painter();
    let palette = theme::current(ui.ctx());
    let hover = hover_amount(&response);
    if hover > 0.0 {
        p.rect_filled(rect, 5.0, fade(palette.panel.hover, hover));
    }
    let anchor = rect.left_center() + Vec2::new(9.0, 0.0);
    let font = FontId::proportional(LABEL_SIZE);
    label_plate(ui, label, font.clone(), anchor, egui::Align2::LEFT_CENTER);
    p.text(anchor, egui::Align2::LEFT_CENTER, label, font, palette.text.primary);

    let switch = palette.switch;
    let track_on = if switch.follow_accent {
        accent.gamma_multiply(0.85)
    } else {
        switch.colors.track_on
    };

    let track = Rect::from_center_size(
        rect.right_center() - Vec2::new(SWITCH_W * 0.5 + 9.0, 0.0),
        Vec2::new(SWITCH_W, SWITCH_H),
    );

    if t > 0.01 {
        let glow = Rect::from_center_size(track.center(), track.size() + Vec2::splat(6.0 * t));
        p.rect_filled(glow, SWITCH_H * 0.5, fade(track_on, 0.12 * t));
    }

    p.rect_filled(track, SWITCH_H * 0.5, switch.colors.track_off);
    p.rect_filled(track, SWITCH_H * 0.5, fade(track_on, t));

    let radius = SWITCH_H * 0.5 - 2.5;
    let pad = radius + 2.5;
    let x = track.left() + pad + t * (track.width() - 2.0 * pad);
    p.circle_filled(Pos2::new(x, track.center().y), radius, switch.colors.thumb);

    response.clicked()
}
