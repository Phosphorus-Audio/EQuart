use super::*;
use crate::filter::FilterKind;

mod band_panel;
mod card;

pub(super) use band_panel::{
    BAND_ACTION_WIDTH, panel_actions, panel_plate, panel_power, power_icon,
};
pub(super) use card::{CardLife, Fall};

const MENU_KNOB_RADIUS: f32 = 14.0;
const MENU_KNOB_Y: f32 = 28.0;
const MENU_KNOB_BOX: f32 = 66.0;
/// extra gap so label doesn't hit tick ring
const MENU_KNOB_LABEL_GAP: f32 = knob::LABEL_GAP + 12.0;
/// extra top clip room for raised knob labels
const MENU_KNOB_HEADROOM: f32 = 8.0;
const MENU_CARD_W: f32 = 420.0;
const MENU_CARD_H: f32 = 372.0;
const MENU_CARD_MARGIN: f32 = 12.0;
const MENU_CARD_GAP: f32 = 14.0;
const MENU_SCALE_W: f32 = 142.0;
const FOOTER_BUTTON_W: f32 = 24.0;
const FOOTER_BUTTON_H: f32 = 24.0;
/// x centers: bypass, scale, options
const FOOTER_BUTTONS: [f32; 3] = [25.0, 52.0, 79.0];
const FOOTER_GLYPH: f32 = 16.0;

impl EquartEditor {
    pub(super) fn settings_card_is_up(&self) -> bool {
        self.settings.is_up()
    }

    /// true if any footer popup is open (so plot won't treat clicks as blank)
    pub(super) fn footer_menu_is_up(&self) -> bool {
        self.settings.is_up() || self.scale.is_up()
    }

    /// bottom bar with icons and sample rate
    pub(super) fn footer(&mut self, ui: &mut egui::Ui, rect: Rect, values: &MeterReadings) {
        let at = |x: f32, y: f32| rect.min + Vec2::new(x, y);
        let p = ui.painter_at(rect);
        let accent = self.accent();

        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();

        let row = |x: f32| {
            Rect::from_center_size(
                at(x, rect.height() * 0.5),
                Vec2::new(FOOTER_BUTTON_W, FOOTER_BUTTON_H),
            )
        };

        if icon_button(
            ui,
            ("footer-bypass", 0),
            row(FOOTER_BUTTONS[0]),
            icons::Glyph::Bypass,
            self.params.bypass.value(),
            RED,
        )
        .clicked()
        {
            self.set(&setter, &self.params.bypass, !self.params.bypass.value());
        }

        let scale = icon_button(
            ui,
            ("footer-scale", 0),
            row(FOOTER_BUTTONS[1]),
            icons::Glyph::Scale,
            false,
            accent,
        );
        let now = ui.input(|i| i.time);

        let mut scale_life = self.scale;
        if scale.clicked() {
            if scale_life.is_up() {
                scale_life.release(now);
            } else {
                scale_life.open(now);
            }
        }
        let dismissed = Self::animated_card(
            footer_menu(&scale, MENU_SCALE_W),
            &mut scale_life,
            now,
            None,
            |ui, _, flash, _| self.ui_scale_menu(ui, flash),
        );
        if dismissed && !scale.clicked() {
            scale_life.release(now);
        }
        self.scale = scale_life;

        let options_salt = ("footer-options", 0);
        let options = icon_button(
            ui,
            options_salt,
            row(FOOTER_BUTTONS[2]),
            icons::Glyph::Options,
            self.settings_card_is_up(),
            accent,
        );
        let mut card_life = self.settings;
        if options.clicked() {
            if card_life.is_up() {
                card_life.release(now);
            } else {
                card_life.open(now);
            }
        }
        let dismissed = Self::animated_card(
            settings_card(&options),
            &mut card_life,
            now,
            None,
            |ui, progress, flash, fall| {
                self.settings_menu(ui, values, progress, flash, fall);
                false
            },
        );
        // don't close if we just clicked the toggle button
        if dismissed && !options.clicked() {
            card_life.release(now);
        }
        if !card_life.is_shown() {
            // reset dropdown state when card closes
            ui.ctx()
                .data_mut(|d| d.insert_temp(settings_group().with("open"), 0u32));
        }
        self.settings = card_life;

        // sample rate readout on right
        p.text(
            at(rect.width() - 14.0, rect.height() * 0.5),
            egui::Align2::RIGHT_CENTER,
            format!("{:.1} kHz", values.sample_rate / 1_000.0),
            FontId::monospace(9.5),
            Color32::from_gray(132),
        );
    }

    /// ui scale popup
    pub(super) fn ui_scale_menu(&self, ui: &mut egui::Ui, flash: f32) -> bool {
        menu::LabelFlash::begin(ui, ui.max_rect(), flash);
        menu::body(ui);
        menu::section(ui, "INTERFACE SCALE");
        let accent = self.accent();
        let mut picked = false;
        for option in UI_SCALES {
            let scale = option as f32 / 100.0;
            let current = (ui.ctx().zoom_factor() - scale).abs() < 0.001;
            if menu::row(ui, &format!("{option}%"), None, current, accent) {
                self.params
                    .ui_scale_percent
                    .store(option, Ordering::Relaxed);
                ui.set_zoom_factor(scale);
                picked = true;
            }
        }
        picked
    }

    /// two-column settings card
    pub(super) fn settings_menu(
        &self,
        ui: &mut egui::Ui,
        values: &MeterReadings,
        progress: f32,
        flash: f32,
        fall: Option<Fall>,
    ) {
        let accent = self.accent();
        let t = progress.clamp(0.0, 1.0);
        let group = settings_group();
        let mut dropdowns = menu::Dropdown::new(ui.ctx(), group);
        let card = Rect::from_min_size(ui.cursor().min, Vec2::new(MENU_CARD_W, MENU_CARD_H));
        menu::LabelFlash::begin(ui, card, flash);

        {
            ui.allocate_rect(card, Sense::hover());
            card_plate(ui, card, t, accent);

            let inner = card.shrink(MENU_CARD_MARGIN);
            let column = (inner.width() - MENU_CARD_GAP) * 0.5;
            let left = Rect::from_min_size(inner.min, Vec2::new(column, inner.height()));
            let right = Rect::from_min_size(
                inner.min + Vec2::new(column + MENU_CARD_GAP, 0.0),
                Vec2::new(column, inner.height()),
            );

            let mut l = ui.new_child(egui::UiBuilder::new().max_rect(left));
            menu::body(&mut l);
            self.analysis_rows(&mut l, &mut dropdowns, group);

            let mut r = ui.new_child(egui::UiBuilder::new().max_rect(right));
            menu::body(&mut r);
            menu::section(&mut r, "LEVEL");
            self.level_controls(&mut r);
            self.level_readout(&mut r, values);
            let fade = self.meter_fade();
            if menu::switch(&mut r, group.with("meter-fade"), "Meter fade", fade, accent) {
                self.params.meter_fade.store(!fade, Ordering::Relaxed);
            }

            menu::divider(&mut r);
            dropdowns.new_group();
            menu::section(&mut r, "ENGINE");
            let modes: Vec<&str> = crate::oversample::Oversampling::ALL
                .iter()
                .map(|mode| mode.label())
                .collect();
            let current = crate::oversample::Oversampling::ALL
                .iter()
                .position(|mode| *mode == self.params.oversampling.value())
                .unwrap_or(0);
            dropdowns.combo(
                &mut r,
                menu::COMBO_OVERSAMPLING,
                "Oversampling",
                modes[current],
                &modes,
                current,
                accent,
            );
            if menu::row(&mut r, "Flat all", None, false, accent) {
                self.flatten_all();
            }
            // push version note to bottom
            menu::divider(&mut r);
            let floor = r.max_rect().bottom() - 24.0;
            r.add_space((floor - r.cursor().top()).max(0.0));
            menu::note(
                &mut r,
                &format!(
                    "{} {}   {:.1} kHz",
                    crate::NAME,
                    env!("CARGO_PKG_VERSION"),
                    values.sample_rate / 1_000.0
                ),
            );
        }

        // draw dropdowns last so they float on top
        if fall.is_none()
            && let Some((key, index)) = dropdowns.finish(ui, card, accent)
        {
            if key == menu::COMBO_OVERSAMPLING {
                if let Some(gui) = &self.gui {
                    let setter = gui.param_setter();
                    self.set(
                        &setter,
                        &self.params.oversampling,
                        crate::oversample::Oversampling::ALL[index],
                    );
                }
            } else {
                self.apply_analysis_choice(key, index);
            }
        }
    }

    /// OUT and MIX knobs
    fn level_controls(&self, ui: &mut egui::Ui) {
        // reserve row height manually since knobs use absolute centers
        let row = Rect::from_min_size(
            ui.cursor().min,
            Vec2::new(ui.available_width(), MENU_KNOB_BOX),
        );
        let accent = self.accent();
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(row));
        // expand clip rect upwards for raised labels
        let clip = Rect::from_min_max(row.min - Vec2::new(0.0, MENU_KNOB_HEADROOM), row.max);
        child.set_clip_rect(clip);
        if let Some(gui) = &self.gui {
            let setter = gui.param_setter();
            child.visuals_mut().selection.bg_fill = accent.gamma_multiply(0.3);
            child.visuals_mut().selection.stroke = Stroke::new(1.0, accent);
            for (param, label, across) in [
                (&self.params.output, "OUT", 0.36),
                (&self.params.mix, "MIX", 0.74),
            ] {
                let center = Pos2::new(row.left() + row.width() * across, row.top() + MENU_KNOB_Y);
                knob::knob(
                    &mut child,
                    param,
                    &setter,
                    knob::KnobAppearance {
                        center,
                        radius: MENU_KNOB_RADIUS,
                        grow: 1.0,
                        body_fill: knob::BODY_FILL,
                        label_gap: MENU_KNOB_LABEL_GAP,
                        label,
                        bipolar: true,
                    },
                    knob::KnobMode::Editable,
                );
            }
        }
        ui.allocate_rect(row, Sense::hover());
    }

    fn level_readout(&self, ui: &mut egui::Ui, values: &MeterReadings) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
        self.level_meters(
            ui.painter(),
            rect.min + Vec2::new(26.0, 7.0),
            rect.width() - 34.0,
            values,
        );
    }
}

/// borderless icon button
fn icon_button(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    glyph: icons::Glyph,
    active: bool,
    accent: Color32,
) -> Response {
    let response = ui
        .interact(rect, ui.make_persistent_id(salt), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    let hover = hover_amount(&response);
    let ink = if active {
        lighten(accent, 0.45)
    } else if response.is_pointer_button_down_on() {
        Color32::from_gray(248)
    } else {
        Color32::from_gray(165).lerp_to_gamma(Color32::from_gray(222), hover)
    };

    icons::draw(
        ui.painter(),
        ui.ctx(),
        glyph,
        Rect::from_center_size(rect.center(), Vec2::splat(FOOTER_GLYPH)),
        ink,
    );

    response
}

/// popup opening upwards from footer (forced open for close animation)
fn footer_menu<'a>(button: &'a Response, width: f32) -> egui::Popup<'a> {
    egui::Popup::menu(button)
        .open(true)
        .align(egui::emath::RectAlign::TOP_START)
        .width(width)
        .frame(menu::card())
}

fn settings_group() -> egui::Id {
    egui::Id::unique("equart-settings-dropdowns")
}

/// settings card popup (forced open for animation, keeps clicks inside)
fn settings_card<'a>(button: &'a Response) -> egui::Popup<'a> {
    egui::Popup::menu(button)
        .open(true)
        .align(egui::emath::RectAlign::TOP_START)
        .width(MENU_CARD_W)
        .gap(5.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(egui::Frame::NONE)
}

/// card background with shadow and top accent flash
fn card_plate(ui: &mut egui::Ui, rect: Rect, t: f32, accent: Color32) {
    let p = ui.painter();
    for layer in (0..14).rev() {
        let f = layer as f32 / 13.0;
        let grow = 2.0 + 16.0 * f;
        let alpha = ((1.5 + 12.0 * (1.0 - f).powi(2)) * t) as u8;
        p.rect_filled(
            rect.translate(Vec2::new(0.0, 6.0)).expand(grow),
            12.0 + grow,
            Color32::from_black_alpha(alpha),
        );
    }

    knob::gradient_rounded(p, rect, 9.0, Color32::from_gray(38), Color32::from_gray(30));
    p.rect_stroke(
        rect,
        9.0,
        Stroke::new(1.0, Color32::from_gray(76)),
        egui::StrokeKind::Inside,
    );

    let flash = (t * (1.0 - t) * 4.0).clamp(0.0, 1.0);
    p.line_segment(
        [
            Pos2::new(rect.left() + 12.0, rect.top() + 1.0),
            Pos2::new(rect.right() - 12.0, rect.top() + 1.0),
        ],
        Stroke::new(1.6, menu::fade(accent, flash * 0.9)),
    );
}
